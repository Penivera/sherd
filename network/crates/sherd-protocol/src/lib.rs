//! Wire types shared by the sherd daemon and every client (the CLI today;
//! a GUI or a third-party app tomorrow). Deliberately dependency-light —
//! anything that links this crate should NOT also have to pull in
//! `rusqlite`, `windows`, or any other platform/storage dependency.
//!
//! Framing: each message is one line of JSON (newline-delimited JSON) sent
//! over the local IPC socket. [`encode_line`]/[`decode_line`] are the only
//! framing logic; both the daemon and clients use them so the format only
//! has one implementation.

use serde::{Deserialize, Serialize};
use sherd_platform::{CapabilityReport, LinkStatus};

/// Fixed local-socket name both the daemon and every client use. Works
/// unmodified on Windows (named pipe) and Linux (abstract Unix socket) via
/// `interprocess`'s `GenericNamespaced` — no per-OS socket path needed.
pub const SOCKET_NAME: &str = "sherd-ipc.sock";

/// A request a client sends to the daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum Request {
    /// The one command most users need: join a nearby sherd network if one
    /// is visible, otherwise host a new one (falling back to a warning if
    /// this device can only ever be a station and none was found). The
    /// daemon also runs this automatically on startup.
    Auto,
    /// Re-run (or fetch the cached result of) the Wi-Fi capability check.
    Capability,
    /// Full status snapshot: capability + hotspot + station link state.
    Status,
    HotspotStart { ssid: String, key: String },
    HotspotStop,
    StationConnect { ssid: String, key: String },
    StationDisconnect,
    /// Reserved shape for the next milestone (mesh relay). The daemon
    /// accepts this today and answers with
    /// [`Response::NotYetImplemented`] so the protocol surface won't
    /// change when messaging actually lands.
    SendMessage { to: String, body: String },
    /// Reserved shape for file transfer, same status as `SendMessage`.
    SendFile { to: String, path: String },
}

/// The daemon's reply to a [`Request`]. Sent as the direct response to the
/// request that triggered it (this pass has no request IDs — one request in
/// flight per client connection at a time).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum Response {
    Auto(AutoOutcome),
    Capability(CapabilityReport),
    Status(StatusReport),
    Ok,
    Error { message: String },
    /// The request was understood but its feature isn't built yet
    /// (currently `SendMessage`/`SendFile` — see the mesh-relay milestone).
    NotYetImplemented { feature: String },
}

/// What the auto-connect flow ended up doing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum AutoOutcome {
    /// This device is hosting its own sherd network. Unconditional whenever
    /// the adapter is capable of it -- not just a fallback for when no
    /// other network was found -- so the mesh keeps a hotspot at every
    /// capable node. `uplink` is set when this device *also* joined another
    /// sherd network as a station at the same time, relaying that
    /// network's reach through its own hotspot rather than just hosting an
    /// island of its own.
    Hosting { ssid: String, uplink: Option<String> },
    /// This device can't host (station-only adapter); joined an existing
    /// sherd network instead.
    Joined { ssid: String },
    /// Neither worked: nothing to join, and this device can't host
    /// (station-only adapter) or has no usable Wi-Fi at all.
    Unavailable { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusReport {
    pub capability: CapabilityReport,
    pub hotspot: Option<LinkStatus>,
    pub station: Option<LinkStatus>,
}

/// Something the daemon pushes to clients without being asked — a link or
/// capability change. Distinguished from [`Response`] by [`ServerMessage`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum Event {
    CapabilityChanged(CapabilityReport),
    HotspotStatus(LinkStatus),
    StationStatus(LinkStatus),
    /// Pushed whenever the auto-connect flow runs (including the daemon's
    /// own startup attempt), so a client that wasn't the one asking still
    /// learns the outcome.
    AutoResult(AutoOutcome),
}

/// Everything the daemon can write to a client connection: either the
/// answer to a request, or an unsolicited event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum ServerMessage {
    Response(Response),
    Event(Event),
}

/// Serialize one message as a single line (no embedded newlines, newline
/// terminator included) for the newline-delimited-JSON transport.
pub fn encode_line<T: Serialize>(value: &T) -> serde_json::Result<String> {
    let mut line = serde_json::to_string(value)?;
    line.push('\n');
    Ok(line)
}

/// Parse one line (without its trailing newline) back into a message.
pub fn decode_line<T: for<'de> Deserialize<'de>>(line: &str) -> serde_json::Result<T> {
    serde_json::from_str(line.trim_end())
}
