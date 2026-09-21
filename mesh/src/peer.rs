use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use wire::NodeId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerState {
    Discovered,
    Handshaking,
    Connected,
    Stale,
    Disconnected,
}

#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub node_id: NodeId,
    pub addr: SocketAddr,
    pub state: PeerState,
    pub last_seen: Instant,
    pub last_ping_nonce: Option<u64>,
    pub latency_ms: Option<u64>,
}

pub struct PeerTable {
    peers: HashMap<NodeId, PeerInfo>,
}

impl PeerTable {
    pub fn new() -> Self {
        Self {
            peers: HashMap::new(),
        }
    }

    pub fn upsert_discovered(&mut self, node_id: NodeId, addr: SocketAddr) {
        self.peers
            .entry(node_id)
            .and_modify(|p| {
                p.addr = addr;
            })
            .or_insert_with(|| PeerInfo {
                node_id,
                addr,
                state: PeerState::Discovered,
                last_seen: Instant::now(),
                last_ping_nonce: None,
                latency_ms: None,
            });
    }

    pub fn mark_handshaking(&mut self, node_id: NodeId, addr: SocketAddr) {
        self.peers
            .entry(node_id)
            .and_modify(|p| {
                p.addr = addr;
                p.state = PeerState::Handshaking;
                p.last_seen = Instant::now();
            })
            .or_insert_with(|| PeerInfo {
                node_id,
                addr,
                state: PeerState::Handshaking,
                last_seen: Instant::now(),
                last_ping_nonce: None,
                latency_ms: None,
            });
    }

    pub fn mark_connected(&mut self, node_id: NodeId, addr: SocketAddr) {
        self.peers
            .entry(node_id)
            .and_modify(|p| {
                p.addr = addr;
                p.state = PeerState::Connected;
                p.last_seen = Instant::now();
            })
            .or_insert_with(|| PeerInfo {
                node_id,
                addr,
                state: PeerState::Connected,
                last_seen: Instant::now(),
                last_ping_nonce: None,
                latency_ms: None,
            });
    }

    pub fn update_last_seen(&mut self, node_id: &NodeId) {
        if let Some(peer) = self.peers.get_mut(node_id) {
            peer.last_seen = Instant::now();
            if peer.state == PeerState::Stale {
                peer.state = PeerState::Connected;
            }
        }
    }

    pub fn record_ping_sent(&mut self, node_id: &NodeId, nonce: u64) {
        if let Some(peer) = self.peers.get_mut(node_id) {
            peer.last_ping_nonce = Some(nonce);
        }
    }

    pub fn record_pong(&mut self, node_id: &NodeId, nonce: u64, latency_ms: u64) {
        if let Some(peer) = self.peers.get_mut(node_id) {
            if peer.last_ping_nonce == Some(nonce) {
                peer.latency_ms = Some(latency_ms);
            }
            peer.last_seen = Instant::now();
            if peer.state == PeerState::Stale {
                peer.state = PeerState::Connected;
            }
        }
    }

    pub fn get_peer(&self, node_id: &NodeId) -> Option<&PeerInfo> {
        self.peers.get(node_id)
    }

    pub fn get_addr(&self, node_id: &NodeId) -> Option<SocketAddr> {
        self.peers.get(node_id).map(|p| p.addr)
    }

    pub fn connected_peers(&self) -> Vec<PeerInfo> {
        self.peers
            .values()
            .filter(|p| p.state == PeerState::Connected)
            .cloned()
            .collect()
    }

    pub fn all_peers(&self) -> Vec<PeerInfo> {
        self.peers.values().cloned().collect()
    }

    pub fn prune_stale(&mut self, stale_after: Duration, disconnect_after: Duration) {
        let now = Instant::now();
        for peer in self.peers.values_mut() {
            let elapsed = now.duration_since(peer.last_seen);
            if elapsed >= disconnect_after {
                peer.state = PeerState::Disconnected;
            } else if elapsed >= stale_after && peer.state == PeerState::Connected {
                peer.state = PeerState::Stale;
            }
        }
    }

    pub fn remove(&mut self, node_id: &NodeId) {
        self.peers.remove(node_id);
    }
}
