//! Domain model for the SMS-like layer. Nothing populates these yet this
//! pass (see the plan's "Deferred" section — mesh relay is next) but the
//! schema is settled now so `storage.rs` doesn't have to change shape later.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contact {
    pub id: i64,
    pub display_name: String,
    /// Stable identifier for the other device. Opaque for now; will likely
    /// become a persisted per-install identity once the mesh transport
    /// lands and devices need to recognize each other across reconnects.
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub id: i64,
    pub conversation_id: i64,
    pub body: String,
    pub status: MessageStatus,
    pub created_at_unix: i64,
}

/// A record of having seen/joined a particular contact's network — the
/// beginning of "who's nearby" bookkeeping for the mesh milestone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerLink {
    pub id: i64,
    pub contact_id: i64,
    pub ssid: String,
    pub last_seen_unix: i64,
}
