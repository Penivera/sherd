use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use platform::{CapabilityLevel, CapabilityReport, LinkState, LinkStatus, PlatformBackend};
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

    /// Whether this device is doing what it should be for the mesh, given a
    /// fresh [`StatusReport`]: a device that can host must be hosting
    /// (hosting is never optional -- it's what extends the mesh's range),
    /// and one that can't must at least be connected to a Sherd network.
    pub fn is_healthy(&self, status: &StatusReport) -> bool {
        let up = |link: &Option<LinkStatus>| link.as_ref().filter(|l| l.state == LinkState::Up).cloned();
        if matches!(status.capability.level, CapabilityLevel::FullMeshCapable) {
            up(&status.hotspot).is_some()
        } else {
            up(&status.station).and_then(|s| s.ssid).is_some_and(|ssid| self.is_sherd_network(&ssid))
        }
    }

    pub async fn hotspot_start(&self, ssid: &str, key: &str) -> platform::PlatformResult<()> {
        self.backend.hotspot.start(ssid, key).await?;
        if let Ok(status) = self.backend.hotspot.status().await {
            self.publish(Event::HotspotStatus(status));
        }
        Ok(())
    }

    pub async fn hotspot_stop(&self) -> platform::PlatformResult<()> {
        self.backend.hotspot.stop().await?;
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

    /// The one flow most users need. Run on startup, and by the daemon's
    /// watchdog whenever [`Self::is_healthy`] says something's down.
    ///
    /// **On a device that can host, hosting is never optional**: a Sherd
    /// mesh's range comes from having a hotspot running at every node that
    /// can run one. So this always (re)starts this device's hotspot.
    ///
    /// It also joins a Sherd network as an uplink first -- turning this
    /// device into a repeater that extends that network's range -- but
    /// **only if the device isn't already connected to something**. An
    /// existing connection (the user's home Wi-Fi, or a Sherd network it
    /// already joined) is never dropped: kicking someone off their own
    /// Wi-Fi without asking would be hostile, and hopping between Sherd
    /// networks would just cause churn. The join happens *before* hosting so
    /// the hotspot has a connection to share (Windows' Mobile Hotspot shares
    /// an existing connection; it can't conjure one).
    ///
    /// A device that can't host has no other way to be part of the mesh, so
    /// it does leave a non-Sherd network to join a Sherd one when one is in
    /// range -- and says so in the log.
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

        let station = self.backend.station.status().await.ok();
        let connected_to = station.filter(|s| s.state == LinkState::Up).and_then(|s| s.ssid);

        if !matches!(capability.level, CapabilityLevel::FullMeshCapable) {
            if let Some(ssid) = connected_to.as_ref().filter(|s| self.is_sherd_network(s)) {
                self.set_links(None, Some(ssid.clone()));
                return self.finish_auto(AutoOutcome::Joined { ssid: ssid.clone() });
            }
            return match self.join_uplink(connected_to.as_deref()).await {
                Some(ssid) => {
                    self.set_links(None, Some(ssid.clone()));
                    self.finish_auto(AutoOutcome::Joined { ssid })
                }
                None => self.finish_auto(AutoOutcome::Unavailable {
                    reason: "this device's Wi-Fi can only join networks, not host one, and no Sherd \
                             network is in range yet"
                        .to_string(),
                }),
            };
        }

        let uplink = match &connected_to {
            Some(ssid) => Some(ssid.clone()).filter(|s| self.is_sherd_network(s)),
            None => self.join_uplink(None).await,
        };

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

    /// Best-effort: try every visible Sherd network, strongest signal
    /// first, and join the first one that actually comes up -- a single
    /// candidate failing (gone by the time we connect, rejected us, etc.)
    /// shouldn't stop us from trying the next one. Returns the SSID joined.
    ///
    /// Never joins this device's own hotspot, nor one hosted by a device
    /// that is itself connected through *this* device (see
    /// [`DOWNSTREAM_MEMORY`]) -- either would be a loop: traffic going round
    /// in a circle, with no route to anywhere else. `leaving` is the
    /// non-Sherd network the device is currently on, if any, so the user is
    /// told before it's dropped.
    async fn join_uplink(&self, leaving: Option<&str>) -> Option<String> {
        let scan = match timeout(SCAN_TIMEOUT, self.backend.station.scan()).await {
            Ok(Ok(results)) => results,
            Ok(Err(e)) => {
                tracing::debug!("Wi-Fi scan failed: {}", e.reason());
                Vec::new()
            }
            Err(_) => {
                tracing::debug!("Wi-Fi scan timed out");
                Vec::new()
            }
        };

        let downstream = self.downstream_ssids();
        let mut candidates: Vec<_> = scan
            .iter()
            .filter(|r| self.is_sherd_network(&r.ssid) && r.ssid != self.config.device_ssid)
            .filter(|r| {
                let is_loop = downstream.contains(&r.ssid);
                if is_loop {
                    tracing::debug!(
                        "Not joining \"{}\": it's run by a device connected through this one, so it would be a loop.",
                        r.ssid
                    );
                }
                !is_loop
            })
            .collect();
        candidates.sort_by_key(|r| std::cmp::Reverse(r.signal_percent.unwrap_or(0)));

        if let (Some(leaving), Some(first)) = (leaving, candidates.first()) {
            tracing::warn!(
                "Disconnecting from \"{leaving}\" to join the Sherd network \"{}\": this device can't \
                 host a hotspot, so joining one is the only way for it to be part of the mesh.",
                first.ssid
            );
        }

        for candidate in candidates {
            match timeout(JOIN_TIMEOUT, self.station_connect(&candidate.ssid, &self.config.shared_key)).await {
                Ok(Ok(())) if self.wait_for_station_up(&candidate.ssid).await => {
                    tracing::info!("Joined the Sherd network \"{}\".", candidate.ssid);
                    return Some(candidate.ssid.clone());
                }
                Ok(Ok(())) => tracing::warn!(
                    "Tried to join \"{}\" but the connection never came up; trying the next one.",
                    candidate.ssid
                ),
                Ok(Err(e)) => tracing::warn!("Couldn't join \"{}\": {}.", candidate.ssid, e.reason()),
                Err(_) => tracing::warn!("Joining \"{}\" timed out.", candidate.ssid),
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
    /// settings back) as the daemon exits. Only touches the hotspot if it's
    /// Sherd's -- if someone switched it to their own settings meanwhile,
    /// it's theirs now and is left alone. Also stops the watchdog from
    /// turning it straight back on.
    pub async fn shutdown(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);

        let status = self.backend.hotspot.status().await.ok();
        let was_on = status.as_ref().is_some_and(|s| s.state == LinkState::Up);
        let is_ours = status
            .as_ref()
            .and_then(|s| s.ssid.as_deref())
            .map_or(true, |ssid| ssid == self.config.device_ssid);

        if was_on && !is_ours {
            tracing::info!("Leaving the hotspot on: it's been switched to settings that aren't Sherd's.");
            return;
        }
        match self.backend.hotspot.stop().await {
            Ok(()) if was_on => tracing::info!("Hotspot turned off."),
            Ok(()) => {}
            Err(e) => tracing::warn!(
                "Couldn't turn the hotspot off ({}). Turn it off in Settings > Network & internet > Mobile hotspot.",
                e.reason()
            ),
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
    /// first time it's seen rather than on every beacon.
    fn note_peer(&self, device_id: String, display_name: String, addr: SocketAddr) {
        if self.peers.upsert(device_id.clone(), display_name.clone(), addr) {
            tracing::info!("{display_name} ({}) is now reachable.", short_id(&device_id));
            self.publish(Event::PeerJoined { device_id, display_name });
        }
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
                    let links = this.links.lock().expect("links lock poisoned").clone();
                    let announce = mailbox::Announce {
                        device_id: this.identity.device_id().to_string(),
                        display_name: this.identity.display_name().to_string(),
                        mailbox_port: mailbox::MAILBOX_PORT,
                        hosting_ssid: links.hosting,
                        uplink_ssid: links.uplink,
                    };
                    if let Ok(payload) = serde_json::to_vec(&announce) {
                        let dest: SocketAddr = (std::net::Ipv4Addr::BROADCAST, mailbox::DISCOVERY_PORT).into();
                        if let Err(e) = socket.send_to(&payload, dest).await {
                            tracing::debug!("discovery broadcast failed: {e}");
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
            self.note_peer(announce.device_id, announce.display_name, addr);
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

    #[test]
    fn sanitizes_hostile_filenames() {
        assert_eq!(sanitize_filename("../../evil.exe"), "evil.exe");
        assert_eq!(sanitize_filename("holiday photo (1).jpg"), "holiday_photo__1_.jpg");
        assert_eq!(sanitize_filename(".."), "file");
    }
}
