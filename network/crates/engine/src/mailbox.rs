//! Wire format and peer bookkeeping for device-to-device messaging over the
//! Wi-Fi network `network`'s own daemon joins/hosts. Deliberately separate
//! from `wire`/`mesh` (whose protocol is built around node handshakes and
//! *compute task* distribution, with no text/file message type) -- these two
//! transports are meant to stay parallel stacks for now, per the project's
//! current direction.
//!
//! Two channels:
//! - **Discovery** (UDP broadcast on [`DISCOVERY_PORT`]): every device
//!   periodically shouts "I'm here" (an [`Announce`]) so others on the same
//!   Wi-Fi network learn its `device_id` and address without needing to
//!   know it in advance. Feeds [`PeerRegistry`].
//! - **Mailbox** (TCP on [`MAILBOX_PORT`]): to actually send something, a
//!   device opens a short-lived connection to a peer's announced address,
//!   sends a [`Frame::Hello`] (proving it controls the private key for its
//!   claimed `device_id`) followed by exactly one payload frame
//!   ([`Frame::Text`] or [`Frame::File`]), then closes. One message per
//!   connection -- simple, no connection-pool/keepalive state to manage.
//!
//! Known limitations, deliberately deferred: no offline queueing (the
//! target must be in the [`PeerRegistry`] right now), no delivery
//! acknowledgement, and a whole file is read into memory and sent as one
//! frame rather than streamed/chunked -- fine for chat-sized files on a LAN,
//! not meant for huge transfers yet.

use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// TCP port a device listens on for incoming messages/files.
pub const MAILBOX_PORT: u16 = 7420;
/// UDP port devices broadcast their presence on.
pub const DISCOVERY_PORT: u16 = 7421;
/// How often a device re-broadcasts its presence.
pub const DISCOVERY_INTERVAL: Duration = Duration::from_secs(5);
/// A peer not heard from in this long is dropped from the registry (treated
/// as no longer reachable -- e.g. it left the Wi-Fi network).
pub const PEER_STALE_AFTER: Duration = Duration::from_secs(20);
/// Sanity cap on a single frame's size, so a corrupt/hostile length prefix
/// can't make a peer try to allocate gigabytes.
const FRAME_MAX_BYTES: u32 = 64 * 1024 * 1024; // 64 MiB

/// One presence broadcast on the discovery channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Announce {
    pub device_id: String,
    pub display_name: String,
    pub mailbox_port: u16,
    /// The Sherd hotspot this device is running right now, if any.
    #[serde(default)]
    pub hosting_ssid: Option<String>,
    /// The network this device is connected to as a client right now, if
    /// any. Together with `hosting_ssid`, this is what lets a device avoid
    /// connecting "backwards" into its own clients' hotspots (a loop): if
    /// a peer's `uplink_ssid` is *my* hotspot, then that peer's
    /// `hosting_ssid` is downstream of me and must never become my uplink.
    #[serde(default)]
    pub uplink_ssid: Option<String>,
}

/// One message on the mailbox (TCP) channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Frame {
    /// Always the first frame on a mailbox connection. `signature` is the
    /// sender's private key signing its own `device_id` bytes -- proof it
    /// actually controls that identity, not just a claim. (A static
    /// self-signature, not a fresh per-connection challenge, so this stops
    /// trivial ID spoofing but isn't replay-proof against an active
    /// man-in-the-middle -- consistent with `SherdConfig`'s documented
    /// "no strong pairing yet" security posture.)
    Hello { device_id: String, display_name: String, signature: String },
    Text { body: String },
    /// `data_base64` rather than raw bytes so this stays valid, readable
    /// JSON instead of serde_json's inefficient array-of-numbers encoding
    /// for `Vec<u8>`.
    File { name: String, data_base64: String },
}

impl Frame {
    pub fn hello(device_id: String, display_name: String, signature: &[u8; 64]) -> Self {
        Frame::Hello { device_id, display_name, signature: BASE64.encode(signature) }
    }

    /// Decode a `Hello` frame's base64 `signature` field back to raw bytes,
    /// for [`crate::identity::DeviceIdentity::verify`].
    pub fn decode_signature(signature: &str) -> Option<[u8; 64]> {
        BASE64.decode(signature).ok()?.try_into().ok()
    }

    pub fn encode_file(name: String, data: &[u8]) -> Self {
        Frame::File { name, data_base64: BASE64.encode(data) }
    }

    pub fn decode_file_data(data_base64: &str) -> Result<Vec<u8>, MailboxError> {
        BASE64.decode(data_base64).map_err(|e| MailboxError::Protocol(format!("bad base64 in file frame: {e}")))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MailboxError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("{0} is not currently reachable (not seen on the network recently)")]
    PeerUnreachable(String),
}

/// Write one frame as a 4-byte big-endian length prefix followed by its JSON
/// encoding.
pub async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, frame: &Frame) -> Result<(), MailboxError> {
    let json = serde_json::to_vec(frame).map_err(|e| MailboxError::Protocol(format!("could not encode frame: {e}")))?;
    let len = u32::try_from(json.len())
        .map_err(|_| MailboxError::Protocol("frame too large to send".to_string()))?;
    writer.write_all(&len.to_be_bytes()).await?;
    writer.write_all(&json).await?;
    writer.flush().await?;
    Ok(())
}

/// Read one frame written by [`write_frame`].
pub async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Frame, MailboxError> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf);
    if len > FRAME_MAX_BYTES {
        return Err(MailboxError::Protocol(format!("frame of {len} bytes exceeds the {FRAME_MAX_BYTES}-byte limit")));
    }
    let mut buf = vec![0u8; len as usize];
    reader.read_exact(&mut buf).await?;
    serde_json::from_slice(&buf).map_err(|e| MailboxError::Protocol(format!("could not decode frame: {e}")))
}

/// A peer this device has recently heard a discovery [`Announce`] from.
#[derive(Debug, Clone)]
pub struct PeerRecord {
    pub device_id: String,
    pub display_name: String,
    pub addr: SocketAddr,
    pub last_seen_unix: i64,
}

/// Thread-safe table of currently-reachable peers, fed by the discovery
/// beacon and consulted whenever this device wants to send something.
#[derive(Clone, Default)]
pub struct PeerRegistry {
    peers: Arc<Mutex<HashMap<String, PeerRecord>>>,
}

impl PeerRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record (or refresh) a peer. Returns `true` if it wasn't already
    /// known -- i.e. it just became reachable -- so callers can announce
    /// that once rather than on every 5-second beacon.
    pub fn upsert(&self, device_id: String, display_name: String, addr: SocketAddr) -> bool {
        let mut peers = self.peers.lock().expect("peer registry lock poisoned");
        peers
            .insert(
                device_id.clone(),
                PeerRecord { device_id, display_name, addr, last_seen_unix: now_unix() },
            )
            .is_none()
    }

    pub fn get(&self, device_id: &str) -> Option<PeerRecord> {
        self.peers.lock().expect("peer registry lock poisoned").get(device_id).cloned()
    }

    /// Resolve an exact `device_id` or an unambiguous prefix of one to the
    /// matching peer. A full ID is 64 hex characters -- too long to type by
    /// hand -- so every CLI-facing lookup (`send`, `send-file`) goes
    /// through this rather than requiring the exact value `peers` prints in
    /// short form. Returns `None` if nothing matches, or if the prefix
    /// matches more than one currently-known peer.
    pub fn resolve(&self, id_or_prefix: &str) -> Option<PeerRecord> {
        let peers = self.peers.lock().expect("peer registry lock poisoned");
        if let Some(exact) = peers.get(id_or_prefix) {
            return Some(exact.clone());
        }
        let mut matches = peers.values().filter(|p| p.device_id.starts_with(id_or_prefix));
        let first = matches.next()?.clone();
        if matches.next().is_some() {
            return None; // ambiguous prefix -- don't guess which one was meant
        }
        Some(first)
    }

    pub fn list(&self) -> Vec<PeerRecord> {
        self.peers.lock().expect("peer registry lock poisoned").values().cloned().collect()
    }

    /// Drop peers not heard from in `max_age` -- they're presumably no
    /// longer on the network. Returns the ones dropped.
    pub fn prune_stale(&self, max_age: Duration) -> Vec<PeerRecord> {
        let cutoff = now_unix() - max_age.as_secs() as i64;
        let mut peers = self.peers.lock().expect("peer registry lock poisoned");
        let stale: Vec<String> =
            peers.values().filter(|p| p.last_seen_unix < cutoff).map(|p| p.device_id.clone()).collect();
        stale.iter().filter_map(|id| peers.remove(id)).collect()
    }

    pub fn len(&self) -> usize {
        self.peers.lock().expect("peer registry lock poisoned").len()
    }
}

fn now_unix() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn frame_round_trips_over_a_pipe() {
        let (mut a, mut b) = tokio::io::duplex(4096);
        let frame = Frame::Hello {
            device_id: "abcd".to_string(),
            display_name: "test device".to_string(),
            signature: "sig".to_string(),
        };
        write_frame(&mut a, &frame).await.expect("write");
        let read_back = read_frame(&mut b).await.expect("read");
        match read_back {
            Frame::Hello { device_id, .. } => assert_eq!(device_id, "abcd"),
            other => panic!("unexpected frame: {other:?}"),
        }
    }

    #[test]
    fn file_frame_round_trips_bytes() {
        let data = b"not actually a file, just some bytes\x00\x01\x02";
        let frame = Frame::encode_file("note.txt".to_string(), data);
        let Frame::File { data_base64, .. } = &frame else { unreachable!() };
        let decoded = Frame::decode_file_data(data_base64).expect("decode");
        assert_eq!(decoded, data);
    }

    #[test]
    fn registry_forgets_stale_peers() {
        let registry = PeerRegistry::new();
        registry.upsert("dev1".to_string(), "Dev One".to_string(), "127.0.0.1:1".parse().unwrap());
        assert_eq!(registry.list().len(), 1);
        registry.prune_stale(Duration::from_secs(0));
        // last_seen_unix == now, and cutoff == now - 0, so it should survive
        // a zero-age prune; use a very small negative window instead to
        // force everything to look stale without a real sleep.
        {
            let mut peers = registry.peers.lock().unwrap();
            for p in peers.values_mut() {
                p.last_seen_unix -= 100;
            }
        }
        registry.prune_stale(Duration::from_secs(10));
        assert!(registry.list().is_empty());
    }
}
