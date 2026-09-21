use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use rand::Rng;
use tokio::sync::{broadcast, Mutex, RwLock};
use tracing::{debug, trace, warn};

use wire::{
    HandshakeAckPayload, HandshakePayload, MessagePayload, NodeId, Packet,
    PeerAnnouncePayload, PeerInfoWire, PeerRequestPayload, PingPayload, PongPayload,
    FLAG_GOSSIP, PROTOCOL_VERSION,
};

use crate::dedup::DedupFilter;
use crate::error::NetworkError;
use crate::gossip::forward_gossip;
use crate::identity::{verify_signature, Keypair};
use crate::peer::PeerTable;
use crate::transport::UdpTransport;

#[derive(Debug, Clone)]
pub struct NodeConfig {
    pub heartbeat_interval: Duration,
    pub stale_timeout: Duration,
    pub disconnect_timeout: Duration,
    pub gossip_max_hops: u8,
    pub max_peers_announce: u16,
    pub dedup_capacity: usize,
    pub dedup_ttl: Duration,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval: Duration::from_millis(500),
            stale_timeout: Duration::from_secs(3),
            disconnect_timeout: Duration::from_secs(6),
            gossip_max_hops: 5,
            max_peers_announce: 16,
            dedup_capacity: 10_000,
            dedup_ttl: Duration::from_secs(60),
        }
    }
}

#[derive(Debug, Clone)]
pub enum NetworkEvent {
    PeerConnected { node_id: NodeId, addr: SocketAddr },
    PeerDisconnected { node_id: NodeId },
    PeersDiscovered(Vec<PeerInfoWire>),
    PacketReceived { from_addr: SocketAddr, packet: Packet },
}

pub struct NetworkNode {
    keypair: Arc<Keypair>,
    transport: Arc<UdpTransport>,
    peer_table: Arc<RwLock<PeerTable>>,
    dedup: Arc<Mutex<DedupFilter>>,
    event_tx: broadcast::Sender<NetworkEvent>,
    is_running: Arc<AtomicBool>,
    next_msg_id: AtomicU64,
    config: NodeConfig,
    local_addr: SocketAddr,
}

impl NetworkNode {
    pub async fn bind(
        addr: SocketAddr,
        keypair: Keypair,
        config: Option<NodeConfig>,
    ) -> Result<Arc<Self>, NetworkError> {
        let config = config.unwrap_or_default();
        let transport = Arc::new(UdpTransport::bind(addr).await?);
        let local_addr = transport.local_addr()?;
        let peer_table = Arc::new(RwLock::new(PeerTable::new()));
        let dedup = Arc::new(Mutex::new(DedupFilter::new(
            config.dedup_capacity,
            config.dedup_ttl,
        )));
        let (event_tx, _) = broadcast::channel(1024);
        let is_running = Arc::new(AtomicBool::new(true));

        let mut rng = rand::thread_rng();
        let initial_msg_id = rng.gen::<u64>();

        let node = Arc::new(Self {
            keypair: Arc::new(keypair),
            transport,
            peer_table,
            dedup,
            event_tx,
            is_running,
            next_msg_id: AtomicU64::new(initial_msg_id),
            config,
            local_addr,
        });

        // Spawn background receive loop
        let node_recv = Arc::clone(&node);
        tokio::spawn(async move {
            node_recv.run_recv_loop().await;
        });

        // Spawn background heartbeat and maintenance loop
        let node_heartbeat = Arc::clone(&node);
        tokio::spawn(async move {
            node_heartbeat.run_heartbeat_loop().await;
        });

        Ok(node)
    }

    pub fn node_id(&self) -> NodeId {
        self.keypair.node_id()
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn keypair(&self) -> &Keypair {
        &self.keypair
    }

    pub fn peer_table(&self) -> &Arc<RwLock<PeerTable>> {
        &self.peer_table
    }

    pub fn subscribe(&self) -> broadcast::Receiver<NetworkEvent> {
        self.event_tx.subscribe()
    }

    pub fn stop(&self) {
        self.is_running.store(false, Ordering::SeqCst);
    }

    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::SeqCst)
    }

    pub fn next_message_id(&self) -> u64 {
        self.next_msg_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Initiate handshake to a target socket address.
    pub async fn connect_peer(&self, addr: SocketAddr) -> Result<(), NetworkError> {
        let timestamp = current_timestamp();
        let mut challenge = [0u8; 32];
        rand::thread_rng().fill(&mut challenge);

        let signing_data = HandshakePayload::challenge_signing_data(
            &self.node_id(),
            self.local_addr.port(),
            PROTOCOL_VERSION,
            timestamp,
            &challenge,
        );
        let signature = self.keypair.sign(&signing_data);

        let payload = HandshakePayload {
            node_id: self.node_id(),
            listen_port: self.local_addr.port(),
            version: PROTOCOL_VERSION,
            timestamp,
            challenge,
            signature,
        };

        let msg_id = self.next_message_id();
        let packet = Packet::new(0, msg_id, MessagePayload::Handshake(payload));
        self.transport.send_packet(&packet, addr).await?;

        trace!(target: "mesh", "Sent handshake to {}", addr);
        Ok(())
    }

    /// Send a directed payload to a specific known NodeId.
    pub async fn send_to(&self, target: &NodeId, payload: MessagePayload) -> Result<(), NetworkError> {
        let addr = {
            let table = self.peer_table.read().await;
            table.get_addr(target).ok_or(NetworkError::PeerNotFound(*target))?
        };
        self.send_to_addr(addr, payload).await
    }

    /// Send a directed payload to a specific socket address.
    pub async fn send_to_addr(&self, addr: SocketAddr, payload: MessagePayload) -> Result<(), NetworkError> {
        let msg_id = self.next_message_id();
        let packet = Packet::new(0, msg_id, payload);
        self.transport.send_packet(&packet, addr).await?;
        Ok(())
    }

    /// Broadcast a payload to all connected peers as a gossip packet.
    pub async fn broadcast(&self, payload: MessagePayload) -> Result<usize, NetworkError> {
        let msg_id = self.next_message_id();
        {
            let mut dedup = self.dedup.lock().await;
            dedup.contains_or_insert(msg_id);
        }

        let packet = Packet::new(FLAG_GOSSIP, msg_id, payload);
        let peers = {
            let table = self.peer_table.read().await;
            table.connected_peers()
        };

        let mut sent = 0;
        for peer in peers {
            if let Ok(_) = self.transport.send_packet(&packet, peer.addr).await {
                sent += 1;
            }
        }
        Ok(sent)
    }

    // ------------------------------------------------------------------------
    // Internal Event Loops
    // ------------------------------------------------------------------------

    async fn run_recv_loop(&self) {
        let mut buf = [0u8; 2048];

        while self.is_running() {
            match self.transport.recv_from(&mut buf).await {
                Ok((len, from_addr)) => {
                    if let Ok(packet) = Packet::decode(&buf[..len]) {
                        self.handle_incoming_packet(packet, from_addr).await;
                    } else {
                        trace!(target: "mesh", "Rejected malformed packet from {}", from_addr);
                    }
                }
                Err(err) => {
                    if !self.is_running() {
                        break;
                    }
                    warn!(target: "mesh", "UDP recv error: {}", err);
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
        }
    }

    async fn handle_incoming_packet(&self, packet: Packet, from_addr: SocketAddr) {
        let msg_id = packet.header.msg_id;
        let is_gossip = (packet.header.flags & FLAG_GOSSIP) != 0;

        match packet.payload {
            MessagePayload::Handshake(ref handshake) => {
                self.handle_handshake(handshake, from_addr).await;
            }
            MessagePayload::HandshakeAck(ref ack) => {
                self.handle_handshake_ack(ack, from_addr).await;
            }
            MessagePayload::Ping(ref ping) => {
                self.handle_ping(ping, from_addr).await;
            }
            MessagePayload::Pong(ref pong) => {
                self.handle_pong(pong, from_addr).await;
            }
            MessagePayload::PeerAnnounce(ref announce) => {
                self.handle_peer_announce(announce, from_addr, msg_id, is_gossip, &packet).await;
            }
            MessagePayload::PeerRequest(ref req) => {
                self.handle_peer_request(req, from_addr).await;
            }
            MessagePayload::TaskAnnounce(_)
            | MessagePayload::TaskRequest(_)
            | MessagePayload::TaskData(_)
            | MessagePayload::TaskClaim(_)
            | MessagePayload::TaskClaimAck(_)
            | MessagePayload::TaskResult(_) => {
                // If it is gossip, check dedup filter first
                if is_gossip {
                    let is_dup = {
                        let mut dedup = self.dedup.lock().await;
                        dedup.contains_or_insert(msg_id)
                    };
                    if is_dup {
                        trace!(target: "mesh", "Dropping duplicate message id {}", msg_id);
                        return;
                    }

                    // Forward gossip packet to other connected peers
                    let _ = forward_gossip(
                        &self.transport,
                        &self.peer_table,
                        &packet,
                        Some(from_addr),
                        self.config.gossip_max_hops,
                    )
                    .await;
                }

                // Notify subscribers of the received message packet
                let _ = self.event_tx.send(NetworkEvent::PacketReceived {
                    from_addr,
                    packet,
                });
            }
        }
    }

    async fn handle_handshake(&self, handshake: &HandshakePayload, from_addr: SocketAddr) {
        // Reject self-handshakes
        if handshake.node_id == self.node_id() {
            return;
        }

        // Validate protocol version
        if handshake.version != PROTOCOL_VERSION {
            debug!(target: "mesh", "Handshake version mismatch from {}", from_addr);
            return;
        }

        // Verify cryptographic signature
        let signing_data = HandshakePayload::challenge_signing_data(
            &handshake.node_id,
            handshake.listen_port,
            handshake.version,
            handshake.timestamp,
            &handshake.challenge,
        );

        if verify_signature(&handshake.node_id, &signing_data, &handshake.signature).is_err() {
            warn!(target: "mesh", "Invalid handshake signature from {}", from_addr);
            return;
        }

        // Signature is valid. Mark peer as connected.
        {
            let mut table = self.peer_table.write().await;
            table.mark_connected(handshake.node_id, from_addr);
        }

        let _ = self.event_tx.send(NetworkEvent::PeerConnected {
            node_id: handshake.node_id,
            addr: from_addr,
        });

        // Send HandshakeAck with echoed challenge signed by us
        let timestamp = current_timestamp();
        let ack_signing_data = HandshakeAckPayload::ack_signing_data(
            &self.node_id(),
            0,
            timestamp,
            &handshake.challenge,
        );
        let signature = self.keypair.sign(&ack_signing_data);

        let ack_payload = HandshakeAckPayload {
            node_id: self.node_id(),
            status: 0,
            timestamp,
            challenge: handshake.challenge,
            signature,
        };

        let msg_id = self.next_message_id();
        let ack_pkt = Packet::new(0, msg_id, MessagePayload::HandshakeAck(ack_payload));
        let _ = self.transport.send_packet(&ack_pkt, from_addr).await;
        debug!(target: "mesh", "Accepted handshake from {} ({})", handshake.node_id, from_addr);
    }

    async fn handle_handshake_ack(&self, ack: &HandshakeAckPayload, from_addr: SocketAddr) {
        if ack.node_id == self.node_id() {
            return;
        }

        if ack.status != 0 {
            warn!(target: "mesh", "Handshake rejected by {}: status {}", from_addr, ack.status);
            return;
        }

        let signing_data = HandshakeAckPayload::ack_signing_data(
            &ack.node_id,
            ack.status,
            ack.timestamp,
            &ack.challenge,
        );

        if verify_signature(&ack.node_id, &signing_data, &ack.signature).is_err() {
            warn!(target: "mesh", "Invalid handshake ack signature from {}", from_addr);
            return;
        }

        // Mark connected
        {
            let mut table = self.peer_table.write().await;
            table.mark_connected(ack.node_id, from_addr);
        }

        let _ = self.event_tx.send(NetworkEvent::PeerConnected {
            node_id: ack.node_id,
            addr: from_addr,
        });

        debug!(target: "mesh", "Handshake ack verified from {} ({})", ack.node_id, from_addr);
    }

    async fn handle_ping(&self, ping: &PingPayload, from_addr: SocketAddr) {
        let pong = PongPayload { nonce: ping.nonce };
        let msg_id = self.next_message_id();
        let pkt = Packet::new(0, msg_id, MessagePayload::Pong(pong));
        let _ = self.transport.send_packet(&pkt, from_addr).await;
    }

    async fn handle_pong(&self, pong: &PongPayload, from_addr: SocketAddr) {
        let mut table = self.peer_table.write().await;
        let mut matching_node_id = None;
        for peer in table.all_peers() {
            if peer.addr == from_addr {
                matching_node_id = Some(peer.node_id);
                break;
            }
        }
        if let Some(id) = matching_node_id {
            table.record_pong(&id, pong.nonce, 1);
        }
    }

    async fn handle_peer_announce(
        &self,
        announce: &PeerAnnouncePayload,
        from_addr: SocketAddr,
        msg_id: u64,
        is_gossip: bool,
        packet: &Packet,
    ) {
        if is_gossip {
            let is_dup = {
                let mut dedup = self.dedup.lock().await;
                dedup.contains_or_insert(msg_id)
            };
            if is_dup {
                return;
            }

            let _ = forward_gossip(
                &self.transport,
                &self.peer_table,
                packet,
                Some(from_addr),
                self.config.gossip_max_hops,
            )
            .await;
        }

        let mut newly_discovered = Vec::new();
        {
            let mut table = self.peer_table.write().await;
            for peer in &announce.peers {
                if peer.node_id != self.node_id() {
                    if table.get_peer(&peer.node_id).is_none() {
                        newly_discovered.push(peer.clone());
                    }
                    table.upsert_discovered(peer.node_id, peer.addr);
                }
            }
        }

        if !newly_discovered.is_empty() {
            let _ = self.event_tx.send(NetworkEvent::PeersDiscovered(newly_discovered));
        }
    }

    async fn handle_peer_request(&self, req: &PeerRequestPayload, from_addr: SocketAddr) {
        let peers = {
            let table = self.peer_table.read().await;
            table
                .connected_peers()
                .into_iter()
                .take(req.max_peers as usize)
                .map(|p| PeerInfoWire {
                    node_id: p.node_id,
                    addr: p.addr,
                })
                .collect()
        };

        let announce = PeerAnnouncePayload { peers };
        let msg_id = self.next_message_id();
        let pkt = Packet::new(0, msg_id, MessagePayload::PeerAnnounce(announce));
        let _ = self.transport.send_packet(&pkt, from_addr).await;
    }

    async fn run_heartbeat_loop(&self) {
        let mut interval = tokio::time::interval(self.config.heartbeat_interval);
        let mut announce_tick: u32 = 0;

        while self.is_running() {
            interval.tick().await;
            announce_tick = announce_tick.wrapping_add(1);

            let connected_peers = {
                let mut table = self.peer_table.write().await;
                table.prune_stale(self.config.stale_timeout, self.config.disconnect_timeout);
                table.connected_peers()
            };

            // Send Ping to all connected peers
            for peer in &connected_peers {
                let nonce = rand::thread_rng().gen::<u64>();
                {
                    let mut table = self.peer_table.write().await;
                    table.record_ping_sent(&peer.node_id, nonce);
                }
                let ping = PingPayload { nonce };
                let msg_id = self.next_message_id();
                let pkt = Packet::new(0, msg_id, MessagePayload::Ping(ping));
                let _ = self.transport.send_packet(&pkt, peer.addr).await;
            }

            // Periodically announce known peers to mesh
            if announce_tick % 5 == 0 && !connected_peers.is_empty() {
                let wire_peers: Vec<PeerInfoWire> = connected_peers
                    .iter()
                    .take(self.config.max_peers_announce as usize)
                    .map(|p| PeerInfoWire {
                        node_id: p.node_id,
                        addr: p.addr,
                    })
                    .collect();

                let announce = PeerAnnouncePayload { peers: wire_peers };
                let _ = self.broadcast(MessagePayload::PeerAnnounce(announce)).await;
            }
        }
    }
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
