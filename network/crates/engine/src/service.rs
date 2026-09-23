use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use platform::{CapabilityLevel, CapabilityReport, LinkState, PlatformBackend};
use protocol::{AutoOutcome, Event, PeerSummary, ReceivedAttachment, StatusReport};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;

use crate::config::SherdConfig;
use crate::identity::DeviceIdentity;
use crate::mailbox::{self, Frame, MailboxError, PeerRegistry};
use crate::models::MessageDirection;
use crate::storage::Storage;

/// How many times (500ms apart) to poll station status after a `connect()`
/// call before giving up on confirming the link actually came up. `netsh
/// wlan connect` returns as soon as the request is accepted, not once
/// negotiation finishes, so a real answer needs a short poll.
const CONNECT_POLL_ATTEMPTS: u32 = 10;
const CONNECT_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Everything that can go wrong bringing a [`SherdService`] up: setting up
/// this device's persistent identity, or opening its local message store.
#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("could not set up this device's identity: {0}")]
    Identity(#[from] std::io::Error),
    #[error("could not open local message storage: {0}")]
    Storage(#[from] sea_orm::DbErr),
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
}

impl SherdService {
    /// Sets up this device's persistent identity (generating one on first
    /// run) and opens its local message store. Both live under
    /// `identity::default_data_dir()`.
    pub async fn new(backend: PlatformBackend, config: SherdConfig) -> Result<Self, InitError> {
        let (events_tx, _) = broadcast::channel(64);
        let data_dir = crate::identity::default_data_dir();
        let identity = DeviceIdentity::load_or_create(
            &crate::identity::default_identity_path(),
            config.display_name.clone(),
        )?;
        let storage = Storage::open(data_dir.join("sherd.db")).await?;

        Ok(Self {
            backend,
            config,
            events_tx,
            identity,
            storage,
            peers: PeerRegistry::new(),
            files_dir: data_dir.join("received"),
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
                detail: format!("capability check failed: {e}"),
                checked_via: "error".to_string(),
            },
        };
        let hotspot = self.backend.hotspot.status().await.ok();
        let station = self.backend.station.status().await.ok();
        StatusReport { capability, hotspot, station }
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

    /// The one flow most users need. **Hosting is never optional on a
    /// capable device**: a sherd mesh's range comes from having a hotspot
    /// running at every node that can run one, not just at whichever single
    /// device happened to start first — so on a
    /// [`CapabilityLevel::FullMeshCapable`] adapter this always (re)starts
    /// this device's own hotspot, unconditionally, regardless of whether any
    /// other sherd network is visible. It *also* tries to join a visible
    /// sherd network as a station at the same time, best-effort, because a
    /// capable adapter can run both roles at once ("full mesh capable" =
    /// concurrent AP + station on one radio) — joining as well as hosting is
    /// what turns this device into a repeater that extends someone else's
    /// hotspot further, rather than just its own separate island. That join
    /// is attempted *before* starting the hotspot so that, when it
    /// succeeds, the hotspot has an actual internet/uplink connection to
    /// tether from (Windows' Mobile Hotspot shares an existing connection —
    /// it doesn't conjure one).
    ///
    /// A station-only device obviously can't host, so for it this is a
    /// plain join-if-visible-else-warn. Also run automatically by the
    /// daemon on startup, and by its watchdog (`sherd_daemon::supervise`)
    /// any time the hotspot (on a capable device) or the station link (on a
    /// station-only device) isn't up.
    pub async fn auto_connect(&self) -> AutoOutcome {
        let capability = match self.capability().await {
            Ok(report) => report,
            Err(e) => {
                return self.finish_auto(AutoOutcome::Unavailable {
                    reason: format!("capability check failed: {e}"),
                });
            }
        };

        if !matches!(capability.level, CapabilityLevel::FullMeshCapable) {
            return match self.join_uplink().await {
                Some(ssid) => self.finish_auto(AutoOutcome::Joined { ssid }),
                None => self.finish_auto(AutoOutcome::Unavailable {
                    reason: "no sherd network is visible nearby, and this device's Wi-Fi \
                             adapter can only join networks, not host one"
                        .to_string(),
                }),
            };
        }

        let uplink = self.join_uplink().await;
        let ssid = self.config.device_ssid.clone();
        match self.hotspot_start(&ssid, &self.config.shared_key).await {
            Ok(()) => self.finish_auto(AutoOutcome::Hosting { ssid, uplink }),
            Err(e) => self.finish_auto(AutoOutcome::Unavailable {
                reason: format!("could not host a network: {e}"),
            }),
        }
    }

    /// Best-effort: try every visible sherd network, strongest signal
    /// first, and join the first one that actually comes up -- a single
    /// candidate failing to join (gone by the time we connect, rejected us,
    /// etc.) shouldn't stop us from trying the next one. Returns the SSID
    /// joined, or `None` if none were visible or none worked.
    async fn join_uplink(&self) -> Option<String> {
        let scan = self.backend.station.scan().await.unwrap_or_else(|e| {
            tracing::warn!("scan for nearby sherd networks failed: {e}");
            Vec::new()
        });

        let mut candidates: Vec<_> = scan
            .iter()
            .filter(|result| result.ssid.starts_with(&self.config.network_prefix))
            // Never try to join the network we ourselves are about to host.
            .filter(|result| result.ssid != self.config.device_ssid)
            .collect();
        candidates.sort_by_key(|result| std::cmp::Reverse(result.signal_percent.unwrap_or(0)));

        for candidate in candidates {
            match self.station_connect(&candidate.ssid, &self.config.shared_key).await {
                Ok(()) if self.wait_for_station_up().await => {
                    return Some(candidate.ssid.clone());
                }
                Ok(()) => {
                    tracing::warn!(
                        "connect to {} was accepted but the link never came up; trying the next one",
                        candidate.ssid
                    );
                }
                Err(e) => tracing::warn!("failed to join {}: {e}; trying the next one", candidate.ssid),
            }
        }
        None
    }

    fn finish_auto(&self, outcome: AutoOutcome) -> AutoOutcome {
        self.publish(Event::AutoResult(outcome.clone()));
        outcome
    }

    async fn wait_for_station_up(&self) -> bool {
        for _ in 0..CONNECT_POLL_ATTEMPTS {
            if let Ok(status) = self.backend.station.status().await {
                if status.state == LinkState::Up {
                    return true;
                }
            }
            tokio::time::sleep(CONNECT_POLL_INTERVAL).await;
        }
        false
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
        if let Err(e) = self
            .record_message(&peer.device_id, &peer.display_name, MessageDirection::Outgoing, Some(body), None, "sent")
            .await
        {
            tracing::warn!("sent message to {} but failed to record it in history: {e}", peer.device_id);
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
            tracing::warn!("sent file to {} but failed to record it in history: {e}", peer.device_id);
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

    /// Runs this device's contribution to messaging for as long as the
    /// daemon lives: broadcasting its presence and accepting incoming
    /// connections. Meant to be spawned once from `daemon/main.rs`
    /// alongside `supervise`.
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
                    "could not bind discovery socket on UDP port {}: {e}; this device won't discover or be discovered by peers",
                    mailbox::DISCOVERY_PORT
                );
                return;
            }
        };
        if let Err(e) = socket.set_broadcast(true) {
            tracing::warn!("could not enable UDP broadcast for discovery: {e}");
            return;
        }
        let socket = Arc::new(socket);

        {
            let socket = Arc::clone(&socket);
            let this = Arc::clone(&self);
            tokio::spawn(async move {
                let announce = mailbox::Announce {
                    device_id: this.identity.device_id().to_string(),
                    display_name: this.identity.display_name().to_string(),
                    mailbox_port: mailbox::MAILBOX_PORT,
                };
                let Ok(payload) = serde_json::to_vec(&announce) else { return };
                let mut ticker = tokio::time::interval(mailbox::DISCOVERY_INTERVAL);
                loop {
                    ticker.tick().await;
                    let dest: SocketAddr = (std::net::Ipv4Addr::BROADCAST, mailbox::DISCOVERY_PORT).into();
                    if let Err(e) = socket.send_to(&payload, dest).await {
                        tracing::debug!("discovery broadcast failed: {e}");
                    }
                    this.peers.prune_stale(mailbox::PEER_STALE_AFTER);
                }
            });
        }

        tracing::info!(port = mailbox::DISCOVERY_PORT, "broadcasting presence for peer discovery");
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
            let addr = SocketAddr::new(from.ip(), announce.mailbox_port);
            self.peers.upsert(announce.device_id, announce.display_name, addr);
        }
    }

    async fn run_mailbox_server(self: Arc<Self>) {
        let listener = match TcpListener::bind(("0.0.0.0", mailbox::MAILBOX_PORT)).await {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!(
                    "could not bind mailbox listener on TCP port {}: {e}; this device cannot receive messages or files",
                    mailbox::MAILBOX_PORT
                );
                return;
            }
        };
        tracing::info!(port = mailbox::MAILBOX_PORT, "listening for incoming messages and files");

        loop {
            let (stream, addr) = match listener.accept().await {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("mailbox accept error: {e}");
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
                let Some(sig_bytes) = Frame::decode_signature(&signature) else {
                    tracing::warn!(%addr, "mailbox connection sent a malformed signature; dropping");
                    return;
                };
                if !DeviceIdentity::verify(&device_id, device_id.as_bytes(), &sig_bytes) {
                    tracing::warn!(%addr, %device_id, "mailbox connection's Hello failed identity verification; dropping");
                    return;
                }
                (device_id, display_name)
            }
            Ok(_) => {
                tracing::warn!(%addr, "mailbox connection's first frame wasn't Hello; dropping");
                return;
            }
            Err(e) => {
                tracing::warn!(%addr, "failed to read Hello frame: {e}");
                return;
            }
        };

        self.peers.upsert(device_id.clone(), display_name.clone(), SocketAddr::new(addr.ip(), mailbox::MAILBOX_PORT));

        let (body, attachment) = match mailbox::read_frame(&mut stream).await {
            Ok(Frame::Text { body }) => (Some(body), None),
            Ok(Frame::File { name, data_base64 }) => match Frame::decode_file_data(&data_base64) {
                Ok(data) => match self.save_incoming_file(&name, &data).await {
                    Ok(path) => (None, Some((name, path, data.len() as u64))),
                    Err(e) => {
                        tracing::warn!("could not save incoming file {name:?} from {device_id}: {e}");
                        return;
                    }
                },
                Err(e) => {
                    tracing::warn!("bad file payload from {device_id}: {e}");
                    return;
                }
            },
            Ok(Frame::Hello { .. }) => {
                tracing::warn!(%addr, "got a second Hello instead of a payload; dropping");
                return;
            }
            Err(e) => {
                tracing::warn!(%addr, %device_id, "failed to read message payload: {e}");
                return;
            }
        };

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
            tracing::warn!("failed to persist incoming message from {device_id}: {e}");
        }

        tracing::info!(from = %display_name, has_text = body.is_some(), has_file = attachment.is_some(), "message received");
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
