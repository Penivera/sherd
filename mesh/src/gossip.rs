use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use wire::{get_hop_count, set_hop_count, Packet, FLAG_GOSSIP};
use crate::error::NetworkError;
use crate::peer::PeerTable;
use crate::transport::UdpTransport;

/// Forward a gossip packet to all connected peers except the excluded address.
pub async fn forward_gossip(
    transport: &Arc<UdpTransport>,
    peer_table: &Arc<RwLock<PeerTable>>,
    packet: &Packet,
    exclude_addr: Option<SocketAddr>,
    max_hops: u8,
) -> Result<usize, NetworkError> {
    let current_hop = get_hop_count(packet.header.flags);
    if current_hop >= max_hops {
        return Ok(0);
    }

    let mut forwarded_packet = packet.clone();
    let new_flags = set_hop_count(packet.header.flags | FLAG_GOSSIP, current_hop + 1);
    forwarded_packet.header.flags = new_flags;

    let peers = {
        let table = peer_table.read().await;
        table.connected_peers()
    };

    let mut count = 0;
    for peer in peers {
        if Some(peer.addr) == exclude_addr {
            continue;
        }
        if let Ok(_) = transport.send_packet(&forwarded_packet, peer.addr).await {
            count += 1;
        }
    }

    Ok(count)
}
