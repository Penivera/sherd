use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Maximum transmission unit limit for safe UDP datagrams without IP fragmentation.
pub const MAX_PACKET_SIZE: usize = 1280;

/// Fixed magic bytes "SHRD" at the start of every packet.
pub const MAGIC_BYTES: [u8; 4] = [0x53, 0x48, 0x52, 0x44];

/// Current protocol wire version.
pub const PROTOCOL_VERSION: u8 = 1;

/// Fixed header size in bytes:
/// Magic (4) + Version (1) + MsgType (1) + Flags (2) + MsgId (8) + PayloadLen (2) = 18 bytes.
pub const HEADER_SIZE: usize = 18;

/// Maximum payload size fitting in a single unfragmented packet.
pub const MAX_PAYLOAD_SIZE: usize = MAX_PACKET_SIZE - HEADER_SIZE;

/// Flag mask indicating a message intended for gossip propagation.
pub const FLAG_GOSSIP: u16 = 0x0001;

/// Extract hop count from header flags (stored in the upper 8 bits).
pub fn get_hop_count(flags: u16) -> u8 {
    (flags >> 8) as u8
}

/// Set hop count in header flags (stored in the upper 8 bits).
pub fn set_hop_count(flags: u16, hop_count: u8) -> u16 {
    (flags & 0x00FF) | ((hop_count as u16) << 8)
}

#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum WireError {
    #[error("Packet length {0} is smaller than minimum header size {HEADER_SIZE}")]
    PacketTooShort(usize),
    #[error("Packet length {0} exceeds maximum packet size {MAX_PACKET_SIZE}")]
    PacketTooLarge(usize),
    #[error("Invalid magic bytes: expected {expected:?}, got {actual:?}")]
    InvalidMagic { expected: [u8; 4], actual: [u8; 4] },
    #[error("Unsupported protocol version {0}, expected {PROTOCOL_VERSION}")]
    UnsupportedVersion(u8),
    #[error("Unknown message type code: {0:#04x}")]
    InvalidMessageType(u8),
    #[error("Header payload length {header_len} does not match packet body length {actual_len}")]
    PayloadLengthMismatch { header_len: usize, actual_len: usize },
    #[error("Payload exceeds maximum permitted size {max}: {actual}")]
    PayloadTooLarge { max: usize, actual: usize },
    #[error("Malformed payload: {0}")]
    MalformedPayload(String),
    #[error("Buffer too small: required {required}, available {available}")]
    BufferTooSmall { required: usize, available: usize },
}

/// Cryptographically derived stable 32-byte node identifier (e.g. Ed25519 public key).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct NodeId(pub [u8; 32]);

impl NodeId {
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn from_slice(slice: &[u8]) -> Result<Self, WireError> {
        if slice.len() != 32 {
            return Err(WireError::MalformedPayload(format!(
                "NodeId must be 32 bytes, got {}",
                slice.len()
            )));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(slice);
        Ok(Self(arr))
    }
}

impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NodeId({})", &hex_encode(&self.0)[..8])
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex_encode(&self.0))
    }
}

/// Content-addressed 32-byte task identifier (SHA-256 hash of canonical task representation).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct TaskId(pub [u8; 32]);

impl TaskId {
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Compute canonical TaskId from author, priority, created_at, and payload.
    pub fn compute(author: &NodeId, priority: u32, created_at: u64, payload: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"SHRD-TASK-V1:");
        hasher.update(author.as_bytes());
        hasher.update(&priority.to_be_bytes());
        hasher.update(&created_at.to_be_bytes());
        hasher.update(payload);
        let result = hasher.finalize();
        let mut id = [0u8; 32];
        id.copy_from_slice(&result);
        Self(id)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn from_slice(slice: &[u8]) -> Result<Self, WireError> {
        if slice.len() != 32 {
            return Err(WireError::MalformedPayload(format!(
                "TaskId must be 32 bytes, got {}",
                slice.len()
            )));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(slice);
        Ok(Self(arr))
    }
}

impl fmt::Debug for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TaskId({})", &hex_encode(&self.0)[..8])
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex_encode(&self.0))
    }
}

/// Single-byte wire message identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum MessageType {
    Handshake = 0x01,
    HandshakeAck = 0x02,
    Ping = 0x03,
    Pong = 0x04,
    PeerAnnounce = 0x05,
    PeerRequest = 0x06,
    TaskAnnounce = 0x10,
    TaskRequest = 0x11,
    TaskData = 0x12,
    TaskClaim = 0x13,
    TaskClaimAck = 0x14,
    TaskResult = 0x15,
}

impl MessageType {
    pub fn from_u8(val: u8) -> Result<Self, WireError> {
        match val {
            0x01 => Ok(Self::Handshake),
            0x02 => Ok(Self::HandshakeAck),
            0x03 => Ok(Self::Ping),
            0x04 => Ok(Self::Pong),
            0x05 => Ok(Self::PeerAnnounce),
            0x06 => Ok(Self::PeerRequest),
            0x10 => Ok(Self::TaskAnnounce),
            0x11 => Ok(Self::TaskRequest),
            0x12 => Ok(Self::TaskData),
            0x13 => Ok(Self::TaskClaim),
            0x14 => Ok(Self::TaskClaimAck),
            0x15 => Ok(Self::TaskResult),
            other => Err(WireError::InvalidMessageType(other)),
        }
    }

    pub fn to_u8(self) -> u8 {
        self as u8
    }
}

/// Binary packet header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub magic: [u8; 4],
    pub version: u8,
    pub msg_type: MessageType,
    pub flags: u16,
    pub msg_id: u64,
    pub payload_len: u16,
}

impl Header {
    pub fn new(msg_type: MessageType, flags: u16, msg_id: u64, payload_len: u16) -> Self {
        Self {
            magic: MAGIC_BYTES,
            version: PROTOCOL_VERSION,
            msg_type,
            flags,
            msg_id,
            payload_len,
        }
    }

    pub fn encode(&self, out: &mut [u8]) -> Result<(), WireError> {
        if out.len() < HEADER_SIZE {
            return Err(WireError::BufferTooSmall {
                required: HEADER_SIZE,
                available: out.len(),
            });
        }
        out[0..4].copy_from_slice(&self.magic);
        out[4] = self.version;
        out[5] = self.msg_type.to_u8();
        out[6..8].copy_from_slice(&self.flags.to_be_bytes());
        out[8..16].copy_from_slice(&self.msg_id.to_be_bytes());
        out[16..18].copy_from_slice(&self.payload_len.to_be_bytes());
        Ok(())
    }

    pub fn decode(input: &[u8]) -> Result<Self, WireError> {
        if input.len() < HEADER_SIZE {
            return Err(WireError::PacketTooShort(input.len()));
        }
        let mut magic = [0u8; 4];
        magic.copy_from_slice(&input[0..4]);
        if magic != MAGIC_BYTES {
            return Err(WireError::InvalidMagic {
                expected: MAGIC_BYTES,
                actual: magic,
            });
        }
        let version = input[4];
        if version != PROTOCOL_VERSION {
            return Err(WireError::UnsupportedVersion(version));
        }
        let msg_type = MessageType::from_u8(input[5])?;
        let flags = u16::from_be_bytes([input[6], input[7]]);
        let msg_id = u64::from_be_bytes(input[8..16].try_into().unwrap());
        let payload_len = u16::from_be_bytes([input[16], input[17]]);

        Ok(Self {
            magic,
            version,
            msg_type,
            flags,
            msg_id,
            payload_len,
        })
    }
}

// ----------------------------------------------------------------------------
// Typed Message Payloads
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandshakePayload {
    pub node_id: NodeId,
    pub listen_port: u16,
    pub version: u8,
    pub timestamp: u64,
    pub challenge: [u8; 32],
    pub signature: [u8; 64],
}

impl HandshakePayload {
    pub const SIZE: usize = 32 + 2 + 1 + 8 + 32 + 64; // 139 bytes

    pub fn challenge_signing_data(node_id: &NodeId, listen_port: u16, version: u8, timestamp: u64, challenge: &[u8; 32]) -> Vec<u8> {
        let mut out = Vec::with_capacity(32 + 2 + 1 + 8 + 32 + 16);
        out.extend_from_slice(b"SHRD-HANDSHAKE:");
        out.extend_from_slice(node_id.as_bytes());
        out.extend_from_slice(&listen_port.to_be_bytes());
        out.push(version);
        out.extend_from_slice(&timestamp.to_be_bytes());
        out.extend_from_slice(challenge);
        out
    }

    pub fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(self.node_id.as_bytes());
        out.extend_from_slice(&self.listen_port.to_be_bytes());
        out.push(self.version);
        out.extend_from_slice(&self.timestamp.to_be_bytes());
        out.extend_from_slice(&self.challenge);
        out.extend_from_slice(&self.signature);
    }

    pub fn decode(input: &[u8]) -> Result<Self, WireError> {
        if input.len() < Self::SIZE {
            return Err(WireError::MalformedPayload(format!(
                "Handshake expected at least {} bytes, got {}",
                Self::SIZE,
                input.len()
            )));
        }
        let node_id = NodeId::from_slice(&input[0..32])?;
        let listen_port = u16::from_be_bytes([input[32], input[33]]);
        let version = input[34];
        let timestamp = u64::from_be_bytes(input[35..43].try_into().unwrap());
        let mut challenge = [0u8; 32];
        challenge.copy_from_slice(&input[43..75]);
        let mut signature = [0u8; 64];
        signature.copy_from_slice(&input[75..139]);

        Ok(Self {
            node_id,
            listen_port,
            version,
            timestamp,
            challenge,
            signature,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandshakeAckPayload {
    pub node_id: NodeId,
    pub status: u8, // 0 = ok, 1 = version mismatch, 2 = auth failed, 3 = rejected
    pub timestamp: u64,
    pub challenge: [u8; 32],
    pub signature: [u8; 64],
}

impl HandshakeAckPayload {
    pub const SIZE: usize = 32 + 1 + 8 + 32 + 64; // 137 bytes

    pub fn ack_signing_data(node_id: &NodeId, status: u8, timestamp: u64, challenge: &[u8; 32]) -> Vec<u8> {
        let mut out = Vec::with_capacity(32 + 1 + 8 + 32 + 20);
        out.extend_from_slice(b"SHRD-HANDSHAKE-ACK:");
        out.extend_from_slice(node_id.as_bytes());
        out.push(status);
        out.extend_from_slice(&timestamp.to_be_bytes());
        out.extend_from_slice(challenge);
        out
    }

    pub fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(self.node_id.as_bytes());
        out.push(self.status);
        out.extend_from_slice(&self.timestamp.to_be_bytes());
        out.extend_from_slice(&self.challenge);
        out.extend_from_slice(&self.signature);
    }

    pub fn decode(input: &[u8]) -> Result<Self, WireError> {
        if input.len() < Self::SIZE {
            return Err(WireError::MalformedPayload(format!(
                "HandshakeAck expected at least {} bytes, got {}",
                Self::SIZE,
                input.len()
            )));
        }
        let node_id = NodeId::from_slice(&input[0..32])?;
        let status = input[32];
        let timestamp = u64::from_be_bytes(input[33..41].try_into().unwrap());
        let mut challenge = [0u8; 32];
        challenge.copy_from_slice(&input[41..73]);
        let mut signature = [0u8; 64];
        signature.copy_from_slice(&input[73..137]);

        Ok(Self {
            node_id,
            status,
            timestamp,
            challenge,
            signature,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PingPayload {
    pub nonce: u64,
}

impl PingPayload {
    pub const SIZE: usize = 8;

    pub fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.nonce.to_be_bytes());
    }

    pub fn decode(input: &[u8]) -> Result<Self, WireError> {
        if input.len() < Self::SIZE {
            return Err(WireError::MalformedPayload("Ping payload too short".into()));
        }
        let nonce = u64::from_be_bytes(input[0..8].try_into().unwrap());
        Ok(Self { nonce })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PongPayload {
    pub nonce: u64,
}

impl PongPayload {
    pub const SIZE: usize = 8;

    pub fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.nonce.to_be_bytes());
    }

    pub fn decode(input: &[u8]) -> Result<Self, WireError> {
        if input.len() < Self::SIZE {
            return Err(WireError::MalformedPayload("Pong payload too short".into()));
        }
        let nonce = u64::from_be_bytes(input[0..8].try_into().unwrap());
        Ok(Self { nonce })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerInfoWire {
    pub node_id: NodeId,
    pub addr: SocketAddr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerAnnouncePayload {
    pub peers: Vec<PeerInfoWire>,
}

impl PeerAnnouncePayload {
    pub fn encode(&self, out: &mut Vec<u8>) {
        let count = self.peers.len() as u16;
        out.extend_from_slice(&count.to_be_bytes());
        for peer in &self.peers {
            out.extend_from_slice(peer.node_id.as_bytes());
            encode_socket_addr(&peer.addr, out);
        }
    }

    pub fn decode(input: &[u8]) -> Result<Self, WireError> {
        if input.len() < 2 {
            return Err(WireError::MalformedPayload("PeerAnnounce payload too short".into()));
        }
        let count = u16::from_be_bytes([input[0], input[1]]) as usize;
        let mut offset = 2;
        let mut peers = Vec::with_capacity(count);

        for _ in 0..count {
            if offset + 32 > input.len() {
                return Err(WireError::MalformedPayload("PeerAnnounce truncated node_id".into()));
            }
            let node_id = NodeId::from_slice(&input[offset..offset + 32])?;
            offset += 32;
            let (addr, consumed) = decode_socket_addr(&input[offset..])?;
            offset += consumed;
            peers.push(PeerInfoWire { node_id, addr });
        }

        Ok(Self { peers })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerRequestPayload {
    pub max_peers: u16,
}

impl PeerRequestPayload {
    pub const SIZE: usize = 2;

    pub fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.max_peers.to_be_bytes());
    }

    pub fn decode(input: &[u8]) -> Result<Self, WireError> {
        if input.len() < Self::SIZE {
            return Err(WireError::MalformedPayload("PeerRequest payload too short".into()));
        }
        let max_peers = u16::from_be_bytes([input[0], input[1]]);
        Ok(Self { max_peers })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskAnnouncePayload {
    pub task_id: TaskId,
    pub author: NodeId,
    pub priority: u32,
    pub created_at: u64,
    pub payload_size: u32,
}

impl TaskAnnouncePayload {
    pub const SIZE: usize = 32 + 32 + 4 + 8 + 4; // 80 bytes

    pub fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(self.task_id.as_bytes());
        out.extend_from_slice(self.author.as_bytes());
        out.extend_from_slice(&self.priority.to_be_bytes());
        out.extend_from_slice(&self.created_at.to_be_bytes());
        out.extend_from_slice(&self.payload_size.to_be_bytes());
    }

    pub fn decode(input: &[u8]) -> Result<Self, WireError> {
        if input.len() < Self::SIZE {
            return Err(WireError::MalformedPayload("TaskAnnounce payload too short".into()));
        }
        let task_id = TaskId::from_slice(&input[0..32])?;
        let author = NodeId::from_slice(&input[32..64])?;
        let priority = u32::from_be_bytes(input[64..68].try_into().unwrap());
        let created_at = u64::from_be_bytes(input[68..76].try_into().unwrap());
        let payload_size = u32::from_be_bytes(input[76..80].try_into().unwrap());

        Ok(Self {
            task_id,
            author,
            priority,
            created_at,
            payload_size,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRequestPayload {
    pub task_id: TaskId,
}

impl TaskRequestPayload {
    pub const SIZE: usize = 32;

    pub fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(self.task_id.as_bytes());
    }

    pub fn decode(input: &[u8]) -> Result<Self, WireError> {
        if input.len() < Self::SIZE {
            return Err(WireError::MalformedPayload("TaskRequest payload too short".into()));
        }
        let task_id = TaskId::from_slice(&input[0..32])?;
        Ok(Self { task_id })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskDataPayload {
    pub task_id: TaskId,
    pub author: NodeId,
    pub priority: u32,
    pub created_at: u64,
    pub payload: Vec<u8>,
}

impl TaskDataPayload {
    pub fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(self.task_id.as_bytes());
        out.extend_from_slice(self.author.as_bytes());
        out.extend_from_slice(&self.priority.to_be_bytes());
        out.extend_from_slice(&self.created_at.to_be_bytes());
        let len = self.payload.len() as u32;
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&self.payload);
    }

    pub fn decode(input: &[u8]) -> Result<Self, WireError> {
        if input.len() < 32 + 32 + 4 + 8 + 4 {
            return Err(WireError::MalformedPayload("TaskData payload header too short".into()));
        }
        let task_id = TaskId::from_slice(&input[0..32])?;
        let author = NodeId::from_slice(&input[32..64])?;
        let priority = u32::from_be_bytes(input[64..68].try_into().unwrap());
        let created_at = u64::from_be_bytes(input[68..76].try_into().unwrap());
        let payload_len = u32::from_be_bytes(input[76..80].try_into().unwrap()) as usize;
        let data_start = 80;
        let data_end = data_start + payload_len;

        if input.len() < data_end {
            return Err(WireError::MalformedPayload(format!(
                "TaskData claims payload length {}, available {}",
                payload_len,
                input.len() - data_start
            )));
        }

        let payload = input[data_start..data_end].to_vec();
        Ok(Self {
            task_id,
            author,
            priority,
            created_at,
            payload,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskClaimPayload {
    pub task_id: TaskId,
    pub claimant: NodeId,
    pub claimed_at: u64,
    pub signature: [u8; 64],
}

impl TaskClaimPayload {
    pub const SIZE: usize = 32 + 32 + 8 + 64; // 136 bytes

    pub fn claim_signing_data(task_id: &TaskId, claimant: &NodeId, claimed_at: u64) -> Vec<u8> {
        let mut out = Vec::with_capacity(32 + 32 + 8 + 16);
        out.extend_from_slice(b"SHRD-TASK-CLAIM:");
        out.extend_from_slice(task_id.as_bytes());
        out.extend_from_slice(claimant.as_bytes());
        out.extend_from_slice(&claimed_at.to_be_bytes());
        out
    }

    pub fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(self.task_id.as_bytes());
        out.extend_from_slice(self.claimant.as_bytes());
        out.extend_from_slice(&self.claimed_at.to_be_bytes());
        out.extend_from_slice(&self.signature);
    }

    pub fn decode(input: &[u8]) -> Result<Self, WireError> {
        if input.len() < Self::SIZE {
            return Err(WireError::MalformedPayload("TaskClaim payload too short".into()));
        }
        let task_id = TaskId::from_slice(&input[0..32])?;
        let claimant = NodeId::from_slice(&input[32..64])?;
        let claimed_at = u64::from_be_bytes(input[64..72].try_into().unwrap());
        let mut signature = [0u8; 64];
        signature.copy_from_slice(&input[72..136]);

        Ok(Self {
            task_id,
            claimant,
            claimed_at,
            signature,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskClaimAckPayload {
    pub task_id: TaskId,
    pub claimant: NodeId,
    pub accepted: bool,
    pub reason: u8, // 0 = ok, 1 = already claimed, 2 = not found, 3 = invalid signature
    pub owner: NodeId,
    pub signature: [u8; 64],
}

impl TaskClaimAckPayload {
    pub const SIZE: usize = 32 + 32 + 1 + 1 + 32 + 64; // 162 bytes

    pub fn ack_signing_data(task_id: &TaskId, claimant: &NodeId, accepted: bool, reason: u8, owner: &NodeId) -> Vec<u8> {
        let mut out = Vec::with_capacity(32 + 32 + 1 + 1 + 32 + 20);
        out.extend_from_slice(b"SHRD-TASK-CLAIM-ACK:");
        out.extend_from_slice(task_id.as_bytes());
        out.extend_from_slice(claimant.as_bytes());
        out.push(if accepted { 1 } else { 0 });
        out.push(reason);
        out.extend_from_slice(owner.as_bytes());
        out
    }

    pub fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(self.task_id.as_bytes());
        out.extend_from_slice(self.claimant.as_bytes());
        out.push(if self.accepted { 1 } else { 0 });
        out.push(self.reason);
        out.extend_from_slice(self.owner.as_bytes());
        out.extend_from_slice(&self.signature);
    }

    pub fn decode(input: &[u8]) -> Result<Self, WireError> {
        if input.len() < Self::SIZE {
            return Err(WireError::MalformedPayload("TaskClaimAck payload too short".into()));
        }
        let task_id = TaskId::from_slice(&input[0..32])?;
        let claimant = NodeId::from_slice(&input[32..64])?;
        let accepted = input[64] != 0;
        let reason = input[65];
        let owner = NodeId::from_slice(&input[66..98])?;
        let mut signature = [0u8; 64];
        signature.copy_from_slice(&input[98..162]);

        Ok(Self {
            task_id,
            claimant,
            accepted,
            reason,
            owner,
            signature,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskResultPayload {
    pub task_id: TaskId,
    pub worker: NodeId,
    pub success: bool,
    pub completed_at: u64,
    pub result_data: Vec<u8>,
}

impl TaskResultPayload {
    pub fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(self.task_id.as_bytes());
        out.extend_from_slice(self.worker.as_bytes());
        out.push(if self.success { 1 } else { 0 });
        out.extend_from_slice(&self.completed_at.to_be_bytes());
        let len = self.result_data.len() as u32;
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&self.result_data);
    }

    pub fn decode(input: &[u8]) -> Result<Self, WireError> {
        if input.len() < 32 + 32 + 1 + 8 + 4 {
            return Err(WireError::MalformedPayload("TaskResult payload too short".into()));
        }
        let task_id = TaskId::from_slice(&input[0..32])?;
        let worker = NodeId::from_slice(&input[32..64])?;
        let success = input[64] != 0;
        let completed_at = u64::from_be_bytes(input[65..73].try_into().unwrap());
        let data_len = u32::from_be_bytes(input[73..77].try_into().unwrap()) as usize;
        let data_start = 77;
        let data_end = data_start + data_len;

        if input.len() < data_end {
            return Err(WireError::MalformedPayload(format!(
                "TaskResult claims length {}, available {}",
                data_len,
                input.len() - data_start
            )));
        }

        let result_data = input[data_start..data_end].to_vec();
        Ok(Self {
            task_id,
            worker,
            success,
            completed_at,
            result_data,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessagePayload {
    Handshake(HandshakePayload),
    HandshakeAck(HandshakeAckPayload),
    Ping(PingPayload),
    Pong(PongPayload),
    PeerAnnounce(PeerAnnouncePayload),
    PeerRequest(PeerRequestPayload),
    TaskAnnounce(TaskAnnouncePayload),
    TaskRequest(TaskRequestPayload),
    TaskData(TaskDataPayload),
    TaskClaim(TaskClaimPayload),
    TaskClaimAck(TaskClaimAckPayload),
    TaskResult(TaskResultPayload),
}

impl MessagePayload {
    pub fn message_type(&self) -> MessageType {
        match self {
            Self::Handshake(_) => MessageType::Handshake,
            Self::HandshakeAck(_) => MessageType::HandshakeAck,
            Self::Ping(_) => MessageType::Ping,
            Self::Pong(_) => MessageType::Pong,
            Self::PeerAnnounce(_) => MessageType::PeerAnnounce,
            Self::PeerRequest(_) => MessageType::PeerRequest,
            Self::TaskAnnounce(_) => MessageType::TaskAnnounce,
            Self::TaskRequest(_) => MessageType::TaskRequest,
            Self::TaskData(_) => MessageType::TaskData,
            Self::TaskClaim(_) => MessageType::TaskClaim,
            Self::TaskClaimAck(_) => MessageType::TaskClaimAck,
            Self::TaskResult(_) => MessageType::TaskResult,
        }
    }

    pub fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Self::Handshake(p) => p.encode(out),
            Self::HandshakeAck(p) => p.encode(out),
            Self::Ping(p) => p.encode(out),
            Self::Pong(p) => p.encode(out),
            Self::PeerAnnounce(p) => p.encode(out),
            Self::PeerRequest(p) => p.encode(out),
            Self::TaskAnnounce(p) => p.encode(out),
            Self::TaskRequest(p) => p.encode(out),
            Self::TaskData(p) => p.encode(out),
            Self::TaskClaim(p) => p.encode(out),
            Self::TaskClaimAck(p) => p.encode(out),
            Self::TaskResult(p) => p.encode(out),
        }
    }

    pub fn decode(msg_type: MessageType, input: &[u8]) -> Result<Self, WireError> {
        match msg_type {
            MessageType::Handshake => Ok(Self::Handshake(HandshakePayload::decode(input)?)),
            MessageType::HandshakeAck => Ok(Self::HandshakeAck(HandshakeAckPayload::decode(input)?)),
            MessageType::Ping => Ok(Self::Ping(PingPayload::decode(input)?)),
            MessageType::Pong => Ok(Self::Pong(PongPayload::decode(input)?)),
            MessageType::PeerAnnounce => Ok(Self::PeerAnnounce(PeerAnnouncePayload::decode(input)?)),
            MessageType::PeerRequest => Ok(Self::PeerRequest(PeerRequestPayload::decode(input)?)),
            MessageType::TaskAnnounce => Ok(Self::TaskAnnounce(TaskAnnouncePayload::decode(input)?)),
            MessageType::TaskRequest => Ok(Self::TaskRequest(TaskRequestPayload::decode(input)?)),
            MessageType::TaskData => Ok(Self::TaskData(TaskDataPayload::decode(input)?)),
            MessageType::TaskClaim => Ok(Self::TaskClaim(TaskClaimPayload::decode(input)?)),
            MessageType::TaskClaimAck => Ok(Self::TaskClaimAck(TaskClaimAckPayload::decode(input)?)),
            MessageType::TaskResult => Ok(Self::TaskResult(TaskResultPayload::decode(input)?)),
        }
    }
}

/// A fully formed wire packet containing a header and typed payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    pub header: Header,
    pub payload: MessagePayload,
}

impl Packet {
    pub fn new(flags: u16, msg_id: u64, payload: MessagePayload) -> Self {
        let msg_type = payload.message_type();
        let mut temp = Vec::new();
        payload.encode(&mut temp);
        let payload_len = temp.len() as u16;

        Self {
            header: Header::new(msg_type, flags, msg_id, payload_len),
            payload,
        }
    }

    pub fn encode(&self) -> Result<Vec<u8>, WireError> {
        let mut payload_bytes = Vec::new();
        self.payload.encode(&mut payload_bytes);

        if payload_bytes.len() > MAX_PAYLOAD_SIZE {
            return Err(WireError::PayloadTooLarge {
                max: MAX_PAYLOAD_SIZE,
                actual: payload_bytes.len(),
            });
        }

        let total_size = HEADER_SIZE + payload_bytes.len();
        if total_size > MAX_PACKET_SIZE {
            return Err(WireError::PacketTooLarge(total_size));
        }

        let mut out = vec![0u8; total_size];
        let mut header = self.header.clone();
        header.payload_len = payload_bytes.len() as u16;
        header.encode(&mut out[0..HEADER_SIZE])?;
        out[HEADER_SIZE..].copy_from_slice(&payload_bytes);
        Ok(out)
    }

    pub fn decode(input: &[u8]) -> Result<Self, WireError> {
        if input.len() < HEADER_SIZE {
            return Err(WireError::PacketTooShort(input.len()));
        }
        if input.len() > MAX_PACKET_SIZE {
            return Err(WireError::PacketTooLarge(input.len()));
        }

        let header = Header::decode(&input[0..HEADER_SIZE])?;
        let expected_payload_len = header.payload_len as usize;
        let actual_payload_len = input.len() - HEADER_SIZE;

        if expected_payload_len != actual_payload_len {
            return Err(WireError::PayloadLengthMismatch {
                header_len: expected_payload_len,
                actual_len: actual_payload_len,
            });
        }

        let payload = MessagePayload::decode(header.msg_type, &input[HEADER_SIZE..])?;
        Ok(Self { header, payload })
    }
}

// ----------------------------------------------------------------------------
// Helper Utilities
// ----------------------------------------------------------------------------

fn encode_socket_addr(addr: &SocketAddr, out: &mut Vec<u8>) {
    match addr {
        SocketAddr::V4(v4) => {
            out.push(4); // Family tag
            out.extend_from_slice(&v4.ip().octets());
            out.extend_from_slice(&v4.port().to_be_bytes());
        }
        SocketAddr::V6(v6) => {
            out.push(6); // Family tag
            out.extend_from_slice(&v6.ip().octets());
            out.extend_from_slice(&v6.port().to_be_bytes());
        }
    }
}

fn decode_socket_addr(input: &[u8]) -> Result<(SocketAddr, usize), WireError> {
    if input.is_empty() {
        return Err(WireError::MalformedPayload("Empty address data".into()));
    }
    match input[0] {
        4 => {
            if input.len() < 7 {
                return Err(WireError::MalformedPayload("IPv4 address data truncated".into()));
            }
            let octets = [input[1], input[2], input[3], input[4]];
            let port = u16::from_be_bytes([input[5], input[6]]);
            Ok((SocketAddr::new(IpAddr::V4(Ipv4Addr::from(octets)), port), 7))
        }
        6 => {
            if input.len() < 19 {
                return Err(WireError::MalformedPayload("IPv6 address data truncated".into()));
            }
            let mut octets = [0u8; 16];
            octets.copy_from_slice(&input[1..17]);
            let port = u16::from_be_bytes([input[17], input[18]]);
            Ok((SocketAddr::new(IpAddr::V6(Ipv6Addr::from(octets)), port), 19))
        }
        other => Err(WireError::MalformedPayload(format!("Invalid IP family tag {other}"))),
    }
}

fn hex_encode(data: &[u8]) -> String {
    let mut s = String::with_capacity(data.len() * 2);
    for &b in data {
        use std::fmt::Write;
        write!(&mut s, "{:02x}", b).unwrap();
    }
    s
}
