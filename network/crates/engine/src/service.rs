use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use platform::{CapabilityLevel, CapabilityReport, LinkState, LinkStatus, PlatformBackend, ScanResult};
use protocol::{AutoOutcome, Event, PeerSummary, ReceivedAttachment, StatusReport};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio::time::timeout;

use crate::config::SherdConfig;
use crate::identity::{short_id, DeviceIdentity};
use crate::mailbox::{self, Frame, MailboxError, PeerRegistry};
use crate::models::MessageDirection;
use crate::storage::Storage;

/// How many times (500ms apart) to poll station status after a `connect()`
/// call before giving up on confirming the link actually came up. `netsh
/// wlan connect` returns as soon as the request is accepted, not once
/// negotiation finishes, so a real answer needs a short poll.
const CONNECT_POLL_ATTEMPTS: u32 = 10;
const CONNECT_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Upper bounds on OS calls that have been seen to hang (a watchdog cycle
/// once stalled for over four minutes). Past these, give up on this attempt
/// and let the watchdog retry later, rather than freezing everything.
const HOTSPOT_START_TIMEOUT: Duration = Duration::from_secs(45);
const SCAN_TIMEOUT: Duration = Duration::from_secs(20);
const JOIN_TIMEOUT: Duration = Duration::from_secs(20);

/// How long to remember that a Sherd network is hosted by one of *this*
/// device's own clients (and so must never become this device's uplink --
/// that would be a loop). Longer than peer staleness on purpose: the very
/// moment this matters is right after our own upstream drops, when those
/// clients may have briefly gone quiet too.
const DOWNSTREAM_MEMORY: Duration = Duration::from_secs(300);

/// Move to a different Sherd network only when it's this many signal
/// points stronger than the current one. Without a margin, two networks at
/// similar strength would have the device hopping back and forth -- and
/// every hop briefly cuts off everything connected through it.
const ROAM_MARGIN: u8 = 30;

/// When two devices first come into range of each other, both would
/// otherwise try to join the other's hotspot at the same moment -- a loop.
/// So the device with the lower ID holds back this long, while the other
/// joins first. By the time this runs out, the lower-ID device has heard
/// that the other is now connected through it, so won't join it at all.
/// Long enough for a watchdog check, a Wi-Fi connection, and a few
/// discovery broadcasts.
const JOIN_GRACE: Duration = Duration::from_secs(45);

/// How long to give Wi-Fi to come back after switching the radio on,
/// before scanning.
const RADIO_WARMUP: Duration = Duration::from_secs(4);

/// What the station (Wi-Fi client) side should do, per the "always be
/// connected to the nearest Sherd network" policy. See
/// [`SherdService::plan_station`].
#[derive(Debug)]
enum StationPlan {
    /// Connected to a suitable Sherd network; leave it.
    Stay(String),
    /// No usable Sherd network in range: nothing to connect to, so the Wi-Fi
    /// is left as it is.
    NothingToJoin,
    /// A Sherd network is in range, but this device is deliberately letting
    /// the other device join first (see [`JOIN_GRACE`]).
    Waiting,
    /// Should be on one of these Sherd networks (best first); `problem`
    /// says why, in plain English.
    Join { candidates: Vec<String>, problem: String },
    /// Connected to a network in a loop and must leave it.
    LeaveLoop(String),
}

/// Result of [`SherdService::check`]: what, if anything, needs fixing.
#[derive(Debug, Clone)]
pub struct Health {
    pub can_host: bool,
    /// Plain-English descriptions, e.g. "The hotspot was renamed to \"X\"".
    /// Empty when everything is as it should be.
    pub problems: Vec<String>,
}

/// Everything that can go wrong bringing a [`SherdService`] up: setting up
/// this device's persistent identity, or opening its local message store.
#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("could not set up this device's identity: {0}")]
    Identity(#[from] std::io::Error),
    #[error("could not open local message storage: {0}")]
    Storage(#[from] sea_orm::DbErr),
}

/// What this device is doing on the network right now, as last observed.
/// Broadcast in every discovery beacon so neighbours can avoid loops.
#[derive(Debug, Default, Clone)]
struct LinkSnapshot {
    hosting: Option<String>,
    uplink: Option<String>,
}

/// Orchestrates the platform backend behind a stable, OS-agnostic API.
/// `sherd-daemon` is the only thing that constructs one of these (choosing
/// the right [`PlatformBackend`] for the current OS) and the only thing that
/// translates [`protocol::Request`]s into calls on it.
pub struct SherdService {
    backend: PlatformBackend,
    config: SherdConfig,
    events_tx: broadcast::Sender<Event>,
    identity: DeviceIdentity,
    storage: Storage,
    /// Other sherd devices currently reachable on this Wi-Fi network, kept
    /// live by `run_discovery_beacon`. Separate from the persisted
    /// `contacts` table: this is "who can I reach *right now*," not history.
    peers: PeerRegistry,
    /// Where incoming files get saved.
    files_dir: PathBuf,
    links: Mutex<LinkSnapshot>,
    /// Sherd hotspots run by devices that are connected to *our* hotspot,
    /// with when we last heard so. See [`DOWNSTREAM_MEMORY`].
    downstream: Mutex<HashMap<String, Instant>>,
    /// The internet connection we last warned the user about sharing, so
    /// the warning is shown once per connection rather than every cycle.
    warned_sharing: Mutex<Option<String>>,
    /// When each currently-visible Sherd network was first seen. Used for
    /// [`JOIN_GRACE`].
    first_seen: Mutex<HashMap<String, Instant>>,
    /// Held for the whole of every hotspot on/off operation, so they take
    /// turns. Without it, closing the daemon while the watchdog was part
    /// way through restarting the hotspot (to undo a rename, say -- which
    /// takes Windows ~10 seconds) sent "off" while "on" was still in
    /// flight, and "on" won: the hotspot was left running after exit (seen
    /// live).
    hotspot_op: tokio::sync::Mutex<()>,
    shutting_down: AtomicBool,
}

impl SherdService {
    /// Sets up this device's persistent identity (generating one on first
    /// run) and opens its local message store. Both live under
    /// `identity::default_data_dir()`.
    pub async fn new(backend: PlatformBackend, mut config: SherdConfig) -> Result<Self, InitError> {
        let (events_tx, _) = broadcast::channel(64);
        let data_dir = crate::identity::default_data_dir();
        let identity = DeviceIdentity::load_or_create(
            &crate::identity::default_identity_path(),
            config.display_name.clone(),
        )?;
        let storage = Storage::open(data_dir.join("sherd.db")).await?;

        // A stable hotspot name derived from the permanent identity, rather
        // than a fresh random one every run: peers' saved Wi-Fi profiles
        // keep matching, and it's recognizably "the same device".
        config.device_ssid =
            format!("{}-{}", config.network_prefix, identity.device_id()[..6].to_uppercase());

        Ok(Self {
            backend,
            config,
            events_tx,
            identity,
            storage,
            peers: PeerRegistry::new(),
            files_dir: data_dir.join("received"),
            links: Mutex::new(LinkSnapshot::default()),
            downstream: Mutex::new(HashMap::new()),
            warned_sharing: Mutex::new(None),
            first_seen: Mutex::new(HashMap::new()),
            hotspot_op: tokio::sync::Mutex::new(()),
            shutting_down: AtomicBool::new(false),
        })
    }

    pub fn config(&self) -> &SherdConfig {
        &self.config
    }

    /// Subscribe to link/capability/auto-connect/message events. Every
    /// daemon client connection gets its own receiver.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events_tx.subscribe()
    }

    fn publish(&self, event: Event) {
        // No receivers (e.g. no clients connected yet) is not an error.
        let _ = self.events_tx.send(event);
    }

    fn is_sherd_network(&self, ssid: &str) -> bool {
        ssid.starts_with(&self.config.network_prefix)
    }

    pub async fn capability(&self) -> platform::PlatformResult<CapabilityReport> {
        let report = self.backend.capability.check().await?;
        self.publish(Event::CapabilityChanged(report.clone()));
        Ok(report)
    }

    pub async fn status(&self) -> StatusReport {
        let capability = match self.capability().await {
            Ok(report) => report,
            Err(e) => CapabilityReport {
                level: CapabilityLevel::Unsupported,
                detail: format!("capability check failed: {}", e.reason()),
                checked_via: "error".to_string(),
            },
        };
        let hotspot = self.backend.hotspot.status().await.ok();
        let station = self.backend.station.status().await.ok();
        self.remember_links(hotspot.as_ref(), station.as_ref());

        let hosting = hotspot.as_ref().is_some_and(|h| h.state == LinkState::Up);
        let sharing_internet_from = if hosting { self.sharing_source().await } else { None };

        StatusReport {
            capability,
            hotspot,
            station,
            device_name: self.identity.display_name().to_string(),
            device_id: self.identity.device_id().to_string(),
            hotspot_name: self.config.device_ssid.clone(),
            sharing_internet_from,
            peer_count: self.peers.len(),
        }
    }

    /// Is this device doing everything it should for the mesh? Read-only
    /// apart from bookkeeping; [`Self::auto_connect`] is what fixes things.
    /// The daemon's watchdog runs this every check and only calls
    /// `auto_connect` when something's listed here.
    ///
    /// What "should" means:
    /// - A device that can host is always hosting, under Sherd's own name
    ///   and password (someone renaming it or changing the password cuts
    ///   every other device off, so that counts as broken too).
    /// - Wi-Fi is switched on.
    /// - Whenever a Sherd network is in range, the Wi-Fi is connected to the
    ///   nearest one (see [`Self::plan_station`]) -- not to some other
    ///   network someone picked by hand, which would leave this device
    ///   unreachable.
    pub async fn check(&self) -> Health {
        let can_host = matches!(
            self.capability().await.map(|c| c.level),
            Ok(CapabilityLevel::FullMeshCapable)
        );
        let mut problems = Vec::new();

        let hotspot = self.backend.hotspot.status().await.ok();
        if can_host {
            match &hotspot {
                Some(h) if h.state == LinkState::Up => match self.backend.hotspot.configured().await {
                    Some((ssid, _)) if ssid != self.config.device_ssid => {
                        problems.push(format!("The hotspot was renamed to \"{ssid}\""))
                    }
                    Some((_, key)) if key != self.config.shared_key => {
                        problems.push("The hotspot's password was changed".to_string())
                    }
                    _ => {}
                },
                Some(h) if h.state == LinkState::Starting => {} // mid-switch; look again next time
                _ => problems.push("The hotspot is off".to_string()),
            }
        }

        if self.backend.station.is_radio_on().await == Some(false) {
            problems.push("Wi-Fi is turned off".to_string());
        } else {
            let station = self.backend.station.status().await.ok();
            self.remember_links(hotspot.as_ref(), station.as_ref());
            let current = station.filter(|s| s.state == LinkState::Up).and_then(|s| s.ssid);
            let scan = self.scan().await;
            match self.plan_station(current.as_deref(), &scan) {
                StationPlan::Join { problem, .. } => problems.push(problem),
                StationPlan::LeaveLoop(ssid) => {
                    problems.push(format!("This device and \"{ssid}\" are connected to each other in a loop"))
                }
                StationPlan::NothingToJoin
                    if !can_host && !current.as_deref().is_some_and(|s| self.is_sherd_network(s)) =>
                {
                    problems.push("No Sherd network is in range yet".to_string())
                }
                StationPlan::Stay(_) | StationPlan::NothingToJoin | StationPlan::Waiting => {}
            }
        }

        Health { can_host, problems }
    }

    async fn scan(&self) -> Vec<ScanResult> {
        match timeout(SCAN_TIMEOUT, self.backend.station.scan()).await {
            Ok(Ok(results)) => results,
            Ok(Err(e)) => {
                tracing::debug!("Wi-Fi scan failed: {}", e.reason());
                Vec::new()
            }
            Err(_) => {
                tracing::debug!("Wi-Fi scan timed out");
                Vec::new()
            }
        }
    }

    /// See [`plan_station`]. Supplies it with this device's settings and
    /// its memory of the network, and updates when each Sherd network was
    /// first seen.
    fn plan_station(&self, current: Option<&str>, scan: &[ScanResult]) -> StationPlan {
        let downstream = self.downstream_ssids();
        let first_seen: HashMap<String, Instant> = {
            let mut first_seen = self.first_seen.lock().expect("first-seen lock poisoned");
            let visible: HashSet<&str> =
                scan.iter().map(|r| r.ssid.as_str()).filter(|s| self.is_sherd_network(s)).collect();
            first_seen.retain(|ssid, _| visible.contains(ssid.as_str()));
            let now = Instant::now();
            for ssid in visible {
                first_seen.entry(ssid.to_string()).or_insert(now);
            }
            first_seen.clone()
        };
        plan_station(
            &self.config.network_prefix,
            &self.config.device_ssid,
            current,
            scan,
            &downstream,
            &first_seen,
        )
    }

    pub async fn hotspot_start(&self, ssid: &str, key: &str) -> platform::PlatformResult<()> {
        {
            let _turn = self.hotspot_op.lock().await;
            if self.is_shutting_down() {
                return Err(platform::PlatformError::CommandFailed("Sherd is shutting down".to_string()));
            }
            self.backend.hotspot.start(ssid, key).await?;
        }
        if let Ok(status) = self.backend.hotspot.status().await {
            self.publish(Event::HotspotStatus(status));
        }
        Ok(())
    }

    pub async fn hotspot_stop(&self) -> platform::PlatformResult<()> {
        {
            let _turn = self.hotspot_op.lock().await;
            self.backend.hotspot.stop().await?;
        }
        if let Ok(status) = self.backend.hotspot.status().await {
            self.publish(Event::HotspotStatus(status));
        }
        Ok(())
    }

    pub async fn station_connect(&self, ssid: &str, key: &str) -> platform::PlatformResult<()> {
        self.backend.station.connect(ssid, key).await?;
        if let Ok(status) = self.backend.station.status().await {
            self.publish(Event::StationStatus(status));
        }
        Ok(())
    }

    pub async fn station_disconnect(&self) -> platform::PlatformResult<()> {
        self.backend.station.disconnect().await?;
        if let Ok(status) = self.backend.station.status().await {
            self.publish(Event::StationStatus(status));
        }
        Ok(())
    }

    /// Make this device do everything [`Self::check`] expects: Wi-Fi on,
    /// connected to the nearest Sherd network when one's in range, and --
    /// on a device that can host -- the hotspot on under Sherd's own name
    /// and password. Safe to call any time: anything already right is left
    /// alone. Run on startup, by the watchdog whenever `check` finds a
    /// problem, and by `sherd auto`.
    ///
    /// **On a device that can host, hosting is never optional**: a Sherd
    /// mesh's range comes from having a hotspot running at every node that
    /// can run one.
    ///
    /// **The daemon is in charge of the Wi-Fi.** If someone connects it to a
    /// different network by hand, or disconnects it, or switches Wi-Fi off,
    /// that's put back: otherwise the device silently drops out of the mesh
    /// and becomes unreachable. The Wi-Fi is only left alone when there's no
    /// Sherd network in range to connect to. The station side is sorted out
    /// *before* hosting, so the hotspot has a connection to share (Windows'
    /// Mobile Hotspot shares an existing connection; it can't conjure one).
    pub async fn auto_connect(&self) -> AutoOutcome {
        if self.is_shutting_down() {
            return AutoOutcome::Unavailable { reason: "sherd is shutting down".to_string() };
        }

        let capability = match self.capability().await {
            Ok(report) => report,
            Err(e) => {
                return self.finish_auto(AutoOutcome::Unavailable {
                    reason: format!("couldn't check this device's Wi-Fi: {}", e.reason()),
                });
            }
        };
        let can_host = matches!(capability.level, CapabilityLevel::FullMeshCapable);

        if self.backend.station.is_radio_on().await == Some(false) {
            match self.backend.station.turn_radio_on().await {
                Ok(()) => {
                    tracing::info!("Wi-Fi was turned off -- turned it back on.");
                    tokio::time::sleep(RADIO_WARMUP).await;
                }
                Err(e) => {
                    return self.finish_auto(AutoOutcome::Unavailable {
                        reason: format!("Wi-Fi is turned off and couldn't be turned back on: {}", e.reason()),
                    });
                }
            }
        }

        let station = self.backend.station.status().await.ok();
        let connected_to = station.filter(|s| s.state == LinkState::Up).and_then(|s| s.ssid);
        let scan = self.scan().await;

        let mut waiting = false;
        let uplink = match self.plan_station(connected_to.as_deref(), &scan) {
            StationPlan::Stay(ssid) => Some(ssid),
            StationPlan::NothingToJoin => None,
            StationPlan::Waiting => {
                waiting = true;
                None
            }
            StationPlan::LeaveLoop(ssid) => {
                tracing::warn!(
                    "This device and \"{ssid}\" were connected to each other in a loop -- disconnecting from \
                     \"{ssid}\" to break it."
                );
                if let Err(e) = self.station_disconnect().await {
                    tracing::warn!("Couldn't disconnect from \"{ssid}\": {}", e.reason());
                }
                None
            }
            StationPlan::Join { candidates, .. } => self
                .join_first_working(&candidates)
                .await
                .or_else(|| connected_to.clone().filter(|s| self.is_sherd_network(s))),
        };
        // What the Wi-Fi is on now, Sherd network or not.
        let connected_now = uplink.clone().or_else(|| {
            connected_to.clone().filter(|s| !self.is_sherd_network(s))
        });

        if !can_host {
            self.set_links(None, connected_now);
            return match uplink {
                Some(ssid) => self.finish_auto(AutoOutcome::Joined { ssid }),
                None if waiting => self.finish_auto(AutoOutcome::Unavailable {
                    reason: "a Sherd network is in range; waiting a few seconds so this device and its \
                             owner don't try to join each other at the same time"
                        .to_string(),
                }),
                None => self.finish_auto(AutoOutcome::Unavailable {
                    reason: "this device's Wi-Fi can only join networks, not host one, and no Sherd \
                             network is in range yet"
                        .to_string(),
                }),
            };
        }
        let connected_to = connected_now;

        let ssid = self.config.device_ssid.clone();
        let failure = match timeout(HOTSPOT_START_TIMEOUT, self.hotspot_start(&ssid, &self.config.shared_key)).await {
            Ok(Ok(())) => {
                self.set_links(Some(ssid.clone()), uplink.clone().or(connected_to));
                self.warn_if_sharing_internet().await;
                return self.finish_auto(AutoOutcome::Hosting { ssid, uplink });
            }
            Ok(Err(e)) => e.reason(),
            Err(_) => format!(
                "Windows didn't respond within {} seconds",
                HOTSPOT_START_TIMEOUT.as_secs()
            ),
        };

        self.set_links(None, uplink.clone().or(connected_to));
        match uplink {
            // Still part of the mesh as a client, just not extending it.
            Some(uplink) => {
                tracing::warn!(
                    "Couldn't turn on this device's hotspot ({failure}). It's still connected to the \
                     Sherd network \"{uplink}\", so it can send and receive, but it isn't extending \
                     the mesh's range."
                );
                self.finish_auto(AutoOutcome::Joined { ssid: uplink })
            }
            None => self.finish_auto(AutoOutcome::Unavailable {
                reason: format!("couldn't turn on the hotspot: {failure}"),
            }),
        }
    }

    /// Try each network in order (best first) and stop at the first one
    /// that actually comes up -- one failing (gone by the time we connect,
    /// rejected us, etc.) shouldn't stop us from trying the next.
    async fn join_first_working(&self, candidates: &[String]) -> Option<String> {
        for ssid in candidates {
            match timeout(JOIN_TIMEOUT, self.station_connect(ssid, &self.config.shared_key)).await {
                Ok(Ok(())) if self.wait_for_station_up(ssid).await => {
                    tracing::info!("Joined the Sherd network \"{ssid}\".");
                    return Some(ssid.clone());
                }
                Ok(Ok(())) => tracing::warn!("Tried to join \"{ssid}\", but the connection never came up."),
                Ok(Err(e)) => tracing::warn!("Couldn't join \"{ssid}\": {}.", e.reason()),
                Err(_) => tracing::warn!("Joining \"{ssid}\" timed out."),
            }
        }
        None
    }

    fn finish_auto(&self, outcome: AutoOutcome) -> AutoOutcome {
        self.publish(Event::AutoResult(outcome.clone()));
        outcome
    }

    async fn wait_for_station_up(&self, ssid: &str) -> bool {
        for _ in 0..CONNECT_POLL_ATTEMPTS {
            if let Ok(status) = self.backend.station.status().await {
                if status.state == LinkState::Up && status.ssid.as_deref() == Some(ssid) {
                    return true;
                }
            }
            tokio::time::sleep(CONNECT_POLL_INTERVAL).await;
        }
        false
    }

    fn set_links(&self, hosting: Option<String>, uplink: Option<String>) {
        *self.links.lock().expect("links lock poisoned") = LinkSnapshot { hosting, uplink };
    }

    fn remember_links(&self, hotspot: Option<&LinkStatus>, station: Option<&LinkStatus>) {
        let up = |l: Option<&LinkStatus>| l.filter(|l| l.state == LinkState::Up).and_then(|l| l.ssid.clone());
        self.set_links(up(hotspot), up(station));
    }

    fn downstream_ssids(&self) -> HashSet<String> {
        let mut downstream = self.downstream.lock().expect("downstream lock poisoned");
        downstream.retain(|_, seen| seen.elapsed() < DOWNSTREAM_MEMORY);
        downstream.keys().cloned().collect()
    }

    /// The connection the hotspot is sharing onward, when that's *not*
    /// another Sherd network (relaying a Sherd network is the point; sharing
    /// someone's home internet is worth telling them about).
    async fn sharing_source(&self) -> Option<String> {
        let source = match self.backend.hotspot.upstream_name().await {
            Some(name) => Some(name),
            None => self.links.lock().expect("links lock poisoned").uplink.clone(),
        };
        source.filter(|name| !self.is_sherd_network(name))
    }

    async fn warn_if_sharing_internet(&self) {
        let source = self.sharing_source().await;
        let mut warned = self.warned_sharing.lock().expect("warned lock poisoned");
        if let Some(source) = &source {
            if warned.as_deref() != Some(source.as_str()) {
                tracing::warn!(
                    "Heads up: this hotspot is sharing this PC's internet connection (\"{source}\") with \
                     every device that joins the mesh -- and since every Sherd install uses the same \
                     built-in password, that's anyone nearby running Sherd. Close the daemon to stop sharing."
                );
            }
        }
        *warned = source;
    }

    // ---- Shutdown -----------------------------------------------------

    pub fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::SeqCst)
    }

    /// Turn this device's hotspot off (and put the user's own hotspot
    /// settings back) as the daemon exits. Also stops the watchdog from
    /// turning it straight back on.
    ///
    /// Asks Windows to stop *first*, with no status check beforehand:
    /// closing the window leaves only ~5 seconds before Windows kills the
    /// process, and switching the hotspot off itself can take that long.
    /// Once asked, Windows finishes the job even if this process is gone
    /// (seen live: still "switching" as the daemon exited, off moments
    /// later), so `grace` running out isn't a failure.
    pub async fn shutdown(&self, grace: Duration) {
        self.shutting_down.store(true, Ordering::SeqCst);

        // Waits out any hotspot operation already in progress (no new one
        // can start once `shutting_down` is set), then turns it off.
        let stop = async {
            let _turn = self.hotspot_op.lock().await;
            self.backend.hotspot.stop().await
        };
        match timeout(grace, stop).await {
            Ok(Ok(())) => tracing::info!("Hotspot turned off."),
            Ok(Err(e)) => tracing::warn!(
                "Couldn't turn the hotspot off ({}). Turn it off in Settings > Network & internet > Mobile hotspot.",
                e.reason()
            ),
            Err(_) => tracing::info!("Hotspot is turning off (Windows finishes that in the background)."),
        }
    }

    // ---- Identity ---------------------------------------------------

    pub fn device_id(&self) -> &str {
        self.identity.device_id()
    }

    pub fn display_name(&self) -> &str {
        self.identity.display_name()
    }

    // ---- Messaging ----------------------------------------------------
    //
    // See `mailbox.rs` for the wire format. Summary: devices broadcast
    // themselves on the network (`run_discovery_beacon`) so others learn
    // their address; sending opens a short-lived connection to a known
    // peer's address (`run_mailbox_server` on their end accepts it).

    /// Other sherd devices currently reachable on this Wi-Fi network.
    pub fn list_peers(&self) -> Vec<PeerSummary> {
        self.peers
            .list()
            .into_iter()
            .map(|p| PeerSummary {
                device_id: p.device_id,
                display_name: p.display_name,
                addr: p.addr.to_string(),
                last_seen_unix: p.last_seen_unix,
            })
            .collect()
    }

    /// Full message history with one contact, oldest first.
    pub async fn history(&self, device_id: &str) -> Result<Vec<protocol::HistoryEntry>, sea_orm::DbErr> {
        let rows = self.storage.messages_for_device(device_id).await?;
        Ok(rows
            .into_iter()
            .map(|m| protocol::HistoryEntry {
                direction: m.direction,
                body: m.body,
                attachment_name: m.attachment_name,
                attachment_path: m.attachment_path,
                status: m.status,
                created_at_unix: m.created_at_unix,
            })
            .collect())
    }

    /// Send a text message to a device currently in the peer table. Fails
    /// immediately (no queueing) if `to` isn't currently reachable.
    pub async fn send_message(&self, to: &str, body: &str) -> Result<(), MailboxError> {
        let peer = self.send_frame(to, Frame::Text { body: body.to_string() }).await?;
        tracing::info!("Sent to {} ({}): {body}", peer.display_name, short_id(&peer.device_id));
        if let Err(e) = self
            .record_message(&peer.device_id, &peer.display_name, MessageDirection::Outgoing, Some(body), None, "sent")
            .await
        {
            tracing::warn!("Message sent, but couldn't save it to history: {e}");
        }
        Ok(())
    }

    /// Send a file to a device currently in the peer table. The whole file
    /// is read into memory and sent as one frame -- fine for chat-sized
    /// files on a LAN, not a streaming/chunked transfer yet.
    pub async fn send_file(&self, to: &str, path: &str) -> Result<(), MailboxError> {
        let data = tokio::fs::read(path).await?;
        let name = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();
        let peer = self.send_frame(to, Frame::encode_file(name.clone(), &data)).await?;
        tracing::info!(
            "Sent file \"{name}\" ({}) to {} ({}).",
            format_size(data.len() as u64),
            peer.display_name,
            short_id(&peer.device_id)
        );
        if let Err(e) = self
            .record_message(
                &peer.device_id,
                &peer.display_name,
                MessageDirection::Outgoing,
                None,
                Some((&name, path)),
                "sent",
            )
            .await
        {
            tracing::warn!("File sent, but couldn't save it to history: {e}");
        }
        Ok(())
    }

    async fn send_frame(&self, to: &str, frame: Frame) -> Result<mailbox::PeerRecord, MailboxError> {
        let peer = self.peers.resolve(to).ok_or_else(|| MailboxError::PeerUnreachable(to.to_string()))?;
        let mut stream = TcpStream::connect(peer.addr).await?;
        let signature = self.identity.sign(self.identity.device_id().as_bytes());
        let hello = Frame::hello(self.identity.device_id().to_string(), self.identity.display_name().to_string(), &signature);
        mailbox::write_frame(&mut stream, &hello).await?;
        mailbox::write_frame(&mut stream, &frame).await?;
        Ok(peer)
    }

    #[allow(clippy::too_many_arguments)]
    async fn record_message(
        &self,
        device_id: &str,
        display_name: &str,
        direction: MessageDirection,
        body: Option<&str>,
        attachment: Option<(&str, &str)>,
        status: &str,
    ) -> Result<(), sea_orm::DbErr> {
        let contact_id = self.storage.find_or_create_contact(device_id, display_name).await?;
        let conversation_id = self.storage.find_or_create_conversation(contact_id).await?;
        self.storage
            .insert_message(
                conversation_id,
                direction.as_str(),
                body,
                attachment.map(|(name, _)| name),
                attachment.map(|(_, path)| path),
                status,
                now_unix(),
            )
            .await
    }

    async fn save_incoming_file(&self, name: &str, data: &[u8]) -> std::io::Result<String> {
        tokio::fs::create_dir_all(&self.files_dir).await?;
        let safe_name = sanitize_filename(name);
        let mut path = self.files_dir.join(&safe_name);
        let mut counter = 1u32;
        while tokio::fs::try_exists(&path).await.unwrap_or(false) {
            let stem = std::path::Path::new(&safe_name).file_stem().and_then(|s| s.to_str()).unwrap_or("file");
            path = match std::path::Path::new(&safe_name).extension().and_then(|e| e.to_str()) {
                Some(ext) => self.files_dir.join(format!("{stem}-{counter}.{ext}")),
                None => self.files_dir.join(format!("{stem}-{counter}")),
            };
            counter += 1;
        }
        tokio::fs::write(&path, data).await?;
        Ok(path.display().to_string())
    }

    /// Record a peer as reachable, announcing it (log + event) only the
    /// first time it's seen rather than on every beacon. Returns whether it
    /// was new.
    fn note_peer(&self, device_id: String, display_name: String, addr: SocketAddr) -> bool {
        let is_new = self.peers.upsert(device_id.clone(), display_name.clone(), addr);
        if is_new {
            tracing::info!("{display_name} ({}) is now reachable.", short_id(&device_id));
            self.publish(Event::PeerJoined { device_id, display_name });
        }
        is_new
    }

    /// Runs this device's contribution to messaging for as long as the
    /// daemon lives: broadcasting its presence and accepting incoming
    /// connections. Meant to be spawned once from `daemon/main.rs`
    /// alongside the watchdog.
    pub async fn run_messaging(self: Arc<Self>) {
        let beacon = tokio::spawn(Arc::clone(&self).run_discovery_beacon());
        let mailbox_server = tokio::spawn(Arc::clone(&self).run_mailbox_server());
        let _ = tokio::join!(beacon, mailbox_server);
    }

    async fn run_discovery_beacon(self: Arc<Self>) {
        let socket = match tokio::net::UdpSocket::bind(("0.0.0.0", mailbox::DISCOVERY_PORT)).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(
                    "Couldn't start device discovery (UDP port {} is in use or blocked: {e}). This device \
                     won't find other devices, or be found by them.",
                    mailbox::DISCOVERY_PORT
                );
                return;
            }
        };
        if let Err(e) = socket.set_broadcast(true) {
            tracing::warn!("Couldn't start device discovery (broadcast not allowed: {e}).");
            return;
        }
        let socket = Arc::new(socket);

        {
            let socket = Arc::clone(&socket);
            let this = Arc::clone(&self);
            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(mailbox::DISCOVERY_INTERVAL);
                loop {
                    ticker.tick().await;
                    if let Ok(payload) = serde_json::to_vec(&this.current_announce()) {
                        // Announce on every network this device is on. A
                        // plain 255.255.255.255 broadcast only goes out one
                        // of them: a PC that hosts a hotspot while also on
                        // home Wi-Fi would announce itself on the home
                        // network only, so devices on its own hotspot never
                        // heard of it (they couldn't find it in `peers`,
                        // or send to it).
                        let mut targets = this.backend.interfaces.ipv4_broadcast_addresses().await;
                        targets.push(std::net::Ipv4Addr::BROADCAST);
                        for target in targets {
                            let dest = SocketAddr::from((target, mailbox::DISCOVERY_PORT));
                            if let Err(e) = socket.send_to(&payload, dest).await {
                                tracing::debug!("discovery broadcast to {dest} failed: {e}");
                            }
                        }
                    }
                    for gone in this.peers.prune_stale(mailbox::PEER_STALE_AFTER) {
                        tracing::info!("{} ({}) is no longer reachable.", gone.display_name, short_id(&gone.device_id));
                        this.publish(Event::PeerLeft { device_id: gone.device_id, display_name: gone.display_name });
                    }
                }
            });
        }

        let mut buf = [0u8; 2048];
        loop {
            let (len, from) = match socket.recv_from(&mut buf).await {
                Ok(v) => v,
                Err(e) => {
                    tracing::debug!("discovery recv error: {e}");
                    continue;
                }
            };
            let Ok(announce) = serde_json::from_slice::<mailbox::Announce>(&buf[..len]) else { continue };
            if announce.device_id == self.identity.device_id() {
                continue; // heard our own broadcast
            }

            // A device whose uplink is *our* hotspot is downstream of us;
            // its own hotspot must never become our uplink.
            if let (Some(uplink), Some(hosting)) = (&announce.uplink_ssid, &announce.hosting_ssid) {
                if *uplink == self.config.device_ssid {
                    self.downstream
                        .lock()
                        .expect("downstream lock poisoned")
                        .insert(hosting.clone(), Instant::now());
                }
            }

            let addr = SocketAddr::new(from.ip(), announce.mailbox_port);
            if self.note_peer(announce.device_id, announce.display_name, addr) {
                // Answer a newly-seen device directly, so it learns about
                // this one straight away -- even if this device's own
                // broadcasts aren't reaching it for some reason.
                if let Ok(payload) = serde_json::to_vec(&self.current_announce()) {
                    let _ = socket.send_to(&payload, SocketAddr::new(from.ip(), mailbox::DISCOVERY_PORT)).await;
                }
            }
        }
    }

    fn current_announce(&self) -> mailbox::Announce {
        let links = self.links.lock().expect("links lock poisoned").clone();
        mailbox::Announce {
            device_id: self.identity.device_id().to_string(),
            display_name: self.identity.display_name().to_string(),
            mailbox_port: mailbox::MAILBOX_PORT,
            hosting_ssid: links.hosting,
            uplink_ssid: links.uplink,
        }
    }

    async fn run_mailbox_server(self: Arc<Self>) {
        let listener = match TcpListener::bind(("0.0.0.0", mailbox::MAILBOX_PORT)).await {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!(
                    "Couldn't start receiving messages (TCP port {} is in use or blocked: {e}). This device \
                     won't be able to receive messages or files.",
                    mailbox::MAILBOX_PORT
                );
                return;
            }
        };

        loop {
            let (stream, addr) = match listener.accept().await {
                Ok(v) => v,
                Err(e) => {
                    tracing::debug!("mailbox accept error: {e}");
                    continue;
                }
            };
            let this = Arc::clone(&self);
            tokio::spawn(async move { this.handle_mailbox_connection(stream, addr).await });
        }
    }

    async fn handle_mailbox_connection(&self, mut stream: TcpStream, addr: SocketAddr) {
        let (device_id, display_name) = match mailbox::read_frame(&mut stream).await {
            Ok(Frame::Hello { device_id, display_name, signature }) => {
                let verified = Frame::decode_signature(&signature)
                    .is_some_and(|sig| DeviceIdentity::verify(&device_id, device_id.as_bytes(), &sig));
                if !verified {
                    tracing::warn!(
                        "Ignored a message from {addr} claiming to be \"{display_name}\": it couldn't prove \
                         that identity."
                    );
                    return;
                }
                (device_id, display_name)
            }
            Ok(_) => {
                tracing::debug!("{addr} didn't start with a Hello; dropping");
                return;
            }
            Err(e) => {
                tracing::debug!("{addr}: failed to read Hello: {e}");
                return;
            }
        };

        self.note_peer(device_id.clone(), display_name.clone(), SocketAddr::new(addr.ip(), mailbox::MAILBOX_PORT));
        let who = format!("{display_name} ({})", short_id(&device_id));

        let (body, attachment) = match mailbox::read_frame(&mut stream).await {
            Ok(Frame::Text { body }) => (Some(body), None),
            Ok(Frame::File { name, data_base64 }) => match Frame::decode_file_data(&data_base64) {
                Ok(data) => match self.save_incoming_file(&name, &data).await {
                    Ok(path) => (None, Some((name, path, data.len() as u64))),
                    Err(e) => {
                        tracing::warn!("{who} sent a file (\"{name}\"), but it couldn't be saved: {e}");
                        return;
                    }
                },
                Err(e) => {
                    tracing::warn!("{who} sent a file, but it arrived damaged: {e}");
                    return;
                }
            },
            Ok(Frame::Hello { .. }) => {
                tracing::debug!("{who}: got a second Hello instead of a payload; dropping");
                return;
            }
            Err(e) => {
                tracing::warn!("A message from {who} was cut off before it finished arriving ({e}).");
                return;
            }
        };

        match (&body, &attachment) {
            (Some(body), _) => tracing::info!("Message from {who}: {body}"),
            (None, Some((name, path, size))) => {
                tracing::info!("File from {who}: \"{name}\" ({}), saved to {path}", format_size(*size))
            }
            (None, None) => {}
        }

        if let Err(e) = self
            .record_message(
                &device_id,
                &display_name,
                MessageDirection::Incoming,
                body.as_deref(),
                attachment.as_ref().map(|(name, path, _)| (name.as_str(), path.as_str())),
                "delivered",
            )
            .await
        {
            tracing::warn!("Received a message from {who}, but couldn't save it to history: {e}");
        }

        self.publish(Event::MessageReceived {
            from_device_id: device_id,
            from_display_name: display_name,
            body,
            attachment: attachment.map(|(name, path, size_bytes)| ReceivedAttachment { name, path, size_bytes }),
        });
    }
}

/// The owner-ID part of a Sherd hotspot name (`Sherd-1B13A2` -> `1B13A2`);
/// every device's hotspot name is built from its ID.
fn owner_tag<'a>(prefix: &str, ssid: &'a str) -> Option<&'a str> {
    ssid.strip_prefix(prefix)?.strip_prefix('-')
}

/// Tie-break between this device and the owner of `ssid`, used to stop two
/// devices joining each other at once: the device with the higher ID is the
/// one that joins. True if this device should go ahead and join that
/// network straight away.
fn joins_first(prefix: &str, own_ssid: &str, ssid: &str) -> bool {
    match (owner_tag(prefix, ssid), owner_tag(prefix, own_ssid)) {
        (Some(theirs), Some(mine)) => mine > theirs,
        _ => true,
    }
}

/// Decide what the Wi-Fi should be connected to, given what it's on now
/// (`current`) and what's in range (`scan`). The rule is "always be on the
/// nearest Sherd network", with three refinements:
/// - Never this device's own hotspot (`own_ssid`), nor one run by a device
///   that's connected through this one (`downstream` -- a loop, see
///   [`DOWNSTREAM_MEMORY`]).
/// - Don't hop between Sherd networks for small signal differences
///   ([`ROAM_MARGIN`]).
/// - When two devices first see each other, the lower-ID one waits
///   ([`JOIN_GRACE`], measured from `first_seen`) so they don't join each
///   other at the same moment.
fn plan_station(
    prefix: &str,
    own_ssid: &str,
    current: Option<&str>,
    scan: &[ScanResult],
    downstream: &HashSet<String>,
    first_seen: &HashMap<String, Instant>,
) -> StationPlan {
    let is_sherd = |ssid: &str| ssid.starts_with(prefix);
    let mut candidates: Vec<&ScanResult> = scan
        .iter()
        .filter(|r| is_sherd(&r.ssid) && r.ssid != own_ssid && !downstream.contains(&r.ssid))
        .collect();
    candidates.sort_by_key(|r| std::cmp::Reverse(r.signal_percent.unwrap_or(0)));
    let may_join = |ssid: &str| {
        joins_first(prefix, own_ssid, ssid)
            || first_seen.get(ssid).is_some_and(|seen| seen.elapsed() >= JOIN_GRACE)
    };

    if let Some(current) = current.filter(|s| is_sherd(s)) {
        if downstream.contains(current) && !joins_first(prefix, own_ssid, current) {
            return StationPlan::LeaveLoop(current.to_string());
        }
        let current_signal = scan.iter().find(|r| r.ssid == current).and_then(|r| r.signal_percent);
        if let (Some(best), Some(current_signal)) = (candidates.first(), current_signal) {
            let best_signal = best.signal_percent.unwrap_or(0);
            if best.ssid != current
                && best_signal >= current_signal.saturating_add(ROAM_MARGIN)
                && may_join(&best.ssid)
            {
                return StationPlan::Join {
                    candidates: vec![best.ssid.clone()],
                    problem: format!(
                        "A much closer Sherd network is in range (\"{}\" at {best_signal}% signal, vs \
                         {current_signal}% for \"{current}\")",
                        best.ssid
                    ),
                };
            }
        }
        return StationPlan::Stay(current.to_string());
    }

    if candidates.is_empty() {
        return StationPlan::NothingToJoin;
    }
    let allowed: Vec<String> = candidates.iter().filter(|r| may_join(&r.ssid)).map(|r| r.ssid.clone()).collect();
    if allowed.is_empty() {
        return StationPlan::Waiting;
    }
    let problem = match current {
        Some(other) => format!("Wi-Fi is connected to \"{other}\" instead of a Sherd network"),
        None => "Wi-Fi isn't connected, but a Sherd network is in range".to_string(),
    };
    StationPlan::Join { candidates: allowed, problem }
}

fn now_unix() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// "1.4 MB" rather than "1468006 bytes".
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["bytes", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} bytes")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

/// Keep filenames from another device to a single path component with a
/// conservative character set -- no directory separators or `..`, so a
/// malicious/buggy sender can't write outside `files_dir`.
fn sanitize_filename(name: &str) -> String {
    let base = std::path::Path::new(name).file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let cleaned: String = base
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' })
        .collect();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        "file".to_string()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_sizes_readably() {
        assert_eq!(format_size(512), "512 bytes");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(5 * 1024 * 1024), "5.0 MB");
    }

    // ---- plan_station -------------------------------------------------
    // This device is "Sherd-500000": higher ID than "Sherd-100000", lower
    // than "Sherd-900000".

    const ME: &str = "Sherd-500000";

    fn net(ssid: &str, signal: u8) -> ScanResult {
        ScanResult { ssid: ssid.to_string(), signal_percent: Some(signal) }
    }

    fn plan(current: Option<&str>, scan: &[ScanResult], downstream: &[&str], seen_long_ago: &[&str]) -> StationPlan {
        let downstream = downstream.iter().map(|s| s.to_string()).collect();
        let long_ago = Instant::now() - JOIN_GRACE - Duration::from_secs(1);
        let first_seen = scan
            .iter()
            .map(|r| {
                let when = if seen_long_ago.contains(&r.ssid.as_str()) { long_ago } else { Instant::now() };
                (r.ssid.clone(), when)
            })
            .collect();
        plan_station("Sherd", ME, current, scan, &downstream, &first_seen)
    }

    #[test]
    fn leaves_a_hand_picked_network_for_the_nearest_sherd_one() {
        let scan = [net("Home", 90), net("Sherd-100000", 40), net("Sherd-200000", 70)];
        match plan(Some("Home"), &scan, &[], &[]) {
            StationPlan::Join { candidates, problem } => {
                assert_eq!(candidates, ["Sherd-200000", "Sherd-100000"]); // strongest first
                assert!(problem.contains("\"Home\""));
            }
            other => panic!("expected Join, got {other:?}"),
        }
    }

    #[test]
    fn leaves_wifi_alone_when_no_sherd_network_is_in_range() {
        let scan = [net("Home", 90), net(ME, 99)]; // own hotspot doesn't count
        assert!(matches!(plan(Some("Home"), &scan, &[], &[]), StationPlan::NothingToJoin));
    }

    #[test]
    fn never_joins_a_network_that_is_connected_through_this_device() {
        let scan = [net("Sherd-100000", 90)];
        assert!(matches!(plan(None, &scan, &["Sherd-100000"], &[]), StationPlan::NothingToJoin));
    }

    #[test]
    fn lower_id_device_gives_the_other_a_head_start() {
        let scan = [net("Sherd-900000", 80)];
        assert!(matches!(plan(None, &scan, &[], &[]), StationPlan::Waiting));
        // ...but joins once the head start has run out and it still isn't
        // connected through us.
        assert!(matches!(plan(None, &scan, &[], &["Sherd-900000"]), StationPlan::Join { .. }));
    }

    #[test]
    fn higher_id_device_joins_straight_away() {
        let scan = [net("Sherd-100000", 80)];
        assert!(matches!(plan(None, &scan, &[], &[]), StationPlan::Join { .. }));
    }

    #[test]
    fn only_roams_for_a_much_stronger_network() {
        let small_gain = [net("Sherd-100000", 50), net("Sherd-200000", 60)];
        assert!(matches!(plan(Some("Sherd-100000"), &small_gain, &[], &[]), StationPlan::Stay(_)));

        let big_gain = [net("Sherd-100000", 30), net("Sherd-200000", 80)];
        match plan(Some("Sherd-100000"), &big_gain, &[], &[]) {
            StationPlan::Join { candidates, .. } => assert_eq!(candidates, ["Sherd-200000"]),
            other => panic!("expected Join, got {other:?}"),
        }
    }

    #[test]
    fn breaks_a_loop_from_the_lower_id_side_only() {
        // We're on 900000's hotspot while it's on ours: we're the lower ID,
        // so we're the one to leave.
        let scan = [net("Sherd-900000", 80)];
        assert!(matches!(plan(Some("Sherd-900000"), &scan, &["Sherd-900000"], &[]), StationPlan::LeaveLoop(_)));
        // Same loop seen from the higher-ID side: stay; the other one leaves.
        let scan = [net("Sherd-100000", 80)];
        assert!(matches!(plan(Some("Sherd-100000"), &scan, &["Sherd-100000"], &[]), StationPlan::Stay(_)));
    }

    #[test]
    fn sanitizes_hostile_filenames() {
        assert_eq!(sanitize_filename("../../evil.exe"), "evil.exe");
        assert_eq!(sanitize_filename("holiday photo (1).jpg"), "holiday_photo__1_.jpg");
        assert_eq!(sanitize_filename(".."), "file");
    }
}
