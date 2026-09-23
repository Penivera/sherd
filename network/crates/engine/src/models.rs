//! Domain model for the SMS-like layer. Populated by `service.rs`'s mailbox
//! (see `mailbox.rs`) as messages are sent and received.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contact {
    pub id: i64,
    pub display_name: String,
    /// Stable identifier for the other device: its Ed25519 public key,
    /// hex-encoded. See `identity::DeviceIdentity`.
    pub device_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversation {
    pub id: i64,
    pub contact_id: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageStatus {
    Queued,
    Sent,
    Delivered,
    Failed,
}

impl MessageStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            MessageStatus::Queued => "queued",
            MessageStatus::Sent => "sent",
            MessageStatus::Delivered => "delivered",
            MessageStatus::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageDirection {
    Outgoing,
    Incoming,
}

impl MessageDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            MessageDirection::Outgoing => "outgoing",
            MessageDirection::Incoming => "incoming",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub id: i64,
    pub conversation_id: i64,
    pub direction: MessageDirection,
    /// `None` for a file-only message.
    pub body: Option<String>,
    pub attachment_name: Option<String>,
    pub attachment_path: Option<String>,
    pub status: MessageStatus,
    pub created_at_unix: i64,
}

/// A record of having seen/joined a particular contact's network — history
/// of "who's nearby, and on which SSID," kept alongside the live in-memory
/// `mailbox::PeerRegistry` used for actually addressing a currently-online
/// peer. Not yet written to (see `service.rs`) -- a fuller "who have I seen
/// before, even offline" view is a natural follow-up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerLink {
    pub id: i64,
    pub contact_id: i64,
    pub ssid: String,
    pub last_seen_unix: i64,
}
