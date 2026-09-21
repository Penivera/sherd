use serde::{Deserialize, Serialize};

pub const SOCKET_NAME: &str = "sherd-ipc.sock";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum Request {
    Auto,
    Capability,
    Status,
    HotspotStart { ssid: String, key: String },
    HotspotStop,
    StationConnect { ssid: String, key: String },
    StationDisconnect,
    SendMessage { to: String, body: String },
    SendFile { to: String, path: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityReport {
    pub level: String,
    pub detail: String,
    pub checked_via: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkStatus {
    pub state: String,
    pub ssid: Option<String>,
    pub interface: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusReport {
    pub capability: CapabilityReport,
    pub hotspot: Option<LinkStatus>,
    pub station: Option<LinkStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum AutoOutcome {
    Hosting { ssid: String, uplink: Option<String> },
    Joined { ssid: String },
    Unavailable { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum Response {
    Auto(AutoOutcome),
    Capability(CapabilityReport),
    Status(StatusReport),
    Ok,
    Error { message: String },
    NotYetImplemented { feature: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum Event {
    CapabilityChanged(CapabilityReport),
    HotspotStatus(LinkStatus),
    StationStatus(LinkStatus),
    AutoResult(AutoOutcome),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum ServerMessage {
    Response(Response),
    Event(Event),
}

pub fn encode_line<T: Serialize>(value: &T) -> serde_json::Result<String> {
    let mut line = serde_json::to_string(value)?;
    line.push('\n');
    Ok(line)
}

pub fn decode_line<T: for<'de> Deserialize<'de>>(line: &str) -> serde_json::Result<T> {
    serde_json::from_str(line.trim_end())
}
