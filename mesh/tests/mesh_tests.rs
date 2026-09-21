use std::time::Duration;
use tokio::time::sleep;

use mesh::{Keypair, NetworkEvent, NetworkNode, NodeConfig, PeerState};
use wire::{
    HandshakePayload, MessagePayload, Packet, TaskAnnouncePayload, TaskId,
    PROTOCOL_VERSION,
};

fn test_config() -> NodeConfig {
    NodeConfig {
        heartbeat_interval: Duration::from_millis(50),
        stale_timeout: Duration::from_millis(300),
        disconnect_timeout: Duration::from_millis(600),
        gossip_max_hops: 4,
        max_peers_announce: 8,
        dedup_capacity: 1000,
        dedup_ttl: Duration::from_secs(10),
    }
}

#[tokio::test]
async fn test_node_handshake_and_ping() {
    let kp_a = Keypair::generate();
    let kp_b = Keypair::generate();

    let node_a = NetworkNode::bind("127.0.0.1:0".parse().unwrap(), kp_a, Some(test_config()))
        .await
        .expect("Bind node A");
    let node_b = NetworkNode::bind("127.0.0.1:0".parse().unwrap(), kp_b, Some(test_config()))
        .await
        .expect("Bind node B");

    let mut rx_a = node_a.subscribe();
    let mut rx_b = node_b.subscribe();

    // Node A connects to Node B
    node_a.connect_peer(node_b.local_addr()).await.expect("Connect peer");

    // Wait for mutual PeerConnected events
    let mut b_connected_to_a = false;
    let mut a_connected_to_b = false;

    let timeout = sleep(Duration::from_secs(2));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            _ = &mut timeout => break,
            Ok(evt) = rx_b.recv() => {
                if let NetworkEvent::PeerConnected { node_id, .. } = evt {
                    if node_id == node_a.node_id() {
                        b_connected_to_a = true;
                    }
                }
            }
            Ok(evt) = rx_a.recv() => {
                if let NetworkEvent::PeerConnected { node_id, .. } = evt {
                    if node_id == node_b.node_id() {
                        a_connected_to_b = true;
                    }
                }
            }
        }
        if a_connected_to_b && b_connected_to_a {
            break;
        }
    }

    assert!(a_connected_to_b, "Node A must confirm Node B connected");
    assert!(b_connected_to_a, "Node B must confirm Node A connected");

    // Check peer table state
    let table_a = node_a.peer_table().read().await;
    let peer_b = table_a.get_peer(&node_b.node_id()).expect("Peer B in table A");
    assert_eq!(peer_b.state, PeerState::Connected);

    // Let heartbeat run for ping/pong exchange
    sleep(Duration::from_millis(150)).await;

    node_a.stop();
    node_b.stop();
}

#[tokio::test]
async fn test_handshake_rejects_invalid_signature() {
    let kp_a = Keypair::generate();
    let kp_b = Keypair::generate();
    let kp_impostor = Keypair::generate();

    let node_a = NetworkNode::bind("127.0.0.1:0".parse().unwrap(), kp_a, Some(test_config()))
        .await
        .expect("Bind node A");
    let node_b = NetworkNode::bind("127.0.0.1:0".parse().unwrap(), kp_b, Some(test_config()))
        .await
        .expect("Bind node B");

    let mut rx_b = node_b.subscribe();

    // Node A crafts a packet claiming to be impostor's NodeId but signed with A's key
    let challenge = [0x99; 32];
    let timestamp = 1700000000;
    let signing_data = HandshakePayload::challenge_signing_data(
        &kp_impostor.node_id(),
        node_a.local_addr().port(),
        PROTOCOL_VERSION,
        timestamp,
        &challenge,
    );
    let fake_signature = node_a.keypair().sign(&signing_data);

    let forged_payload = HandshakePayload {
        node_id: kp_impostor.node_id(),
        listen_port: node_a.local_addr().port(),
        version: PROTOCOL_VERSION,
        timestamp,
        challenge,
        signature: fake_signature,
    };

    let forged_packet = Packet::new(0, 1234, MessagePayload::Handshake(forged_payload));
    node_a.send_to_addr(node_b.local_addr(), forged_packet.payload)
        .await
        .expect("Send forged packet");

    // Wait briefly and verify Node B did NOT connect to impostor
    let timeout = sleep(Duration::from_millis(300));
    tokio::pin!(timeout);

    let mut connected = false;
    loop {
        tokio::select! {
            _ = &mut timeout => break,
            Ok(evt) = rx_b.recv() => {
                if let NetworkEvent::PeerConnected { node_id, .. } = evt {
                    if node_id == kp_impostor.node_id() {
                        connected = true;
                    }
                }
            }
        }
    }

    assert!(!connected, "Node B must reject forged handshake");
    let table_b = node_b.peer_table().read().await;
    assert!(table_b.get_peer(&kp_impostor.node_id()).is_none());

    node_a.stop();
    node_b.stop();
}

#[tokio::test]
async fn test_gossip_mesh_propagation_and_deduplication() {
    // Topology:
    //      A
    //     / \
    //    B   C
    //     \ /
    //      D
    let kp_a = Keypair::generate();
    let kp_b = Keypair::generate();
    let kp_c = Keypair::generate();
    let kp_d = Keypair::generate();

    let node_a = NetworkNode::bind("127.0.0.1:0".parse().unwrap(), kp_a, Some(test_config()))
        .await
        .unwrap();
    let node_b = NetworkNode::bind("127.0.0.1:0".parse().unwrap(), kp_b, Some(test_config()))
        .await
        .unwrap();
    let node_c = NetworkNode::bind("127.0.0.1:0".parse().unwrap(), kp_c, Some(test_config()))
        .await
        .unwrap();
    let node_d = NetworkNode::bind("127.0.0.1:0".parse().unwrap(), kp_d, Some(test_config()))
        .await
        .unwrap();

    // Connect A <-> B
    node_a.connect_peer(node_b.local_addr()).await.unwrap();
    // Connect A <-> C
    node_a.connect_peer(node_c.local_addr()).await.unwrap();
    // Connect B <-> D
    node_b.connect_peer(node_d.local_addr()).await.unwrap();
    // Connect C <-> D
    node_c.connect_peer(node_d.local_addr()).await.unwrap();

    // Wait for all connections to be established
    sleep(Duration::from_millis(300)).await;

    // Verify D is connected to B and C
    {
        let table_d = node_d.peer_table().read().await;
        assert_eq!(table_d.connected_peers().len(), 2, "D should have 2 connected peers (B and C)");
    }

    let mut rx_d = node_d.subscribe();

    // A broadcasts a TaskAnnounce via gossip
    let task_id = TaskId::new([0x77; 32]);
    let announce = TaskAnnouncePayload {
        task_id,
        author: node_a.node_id(),
        priority: 10,
        created_at: 1000,
        payload_size: 64,
    };

    let sent_count = node_a.broadcast(MessagePayload::TaskAnnounce(announce)).await.unwrap();
    assert_eq!(sent_count, 2, "A should broadcast to both B and C");

    // D should receive the announced task
    let timeout = sleep(Duration::from_secs(2));
    tokio::pin!(timeout);

    let mut received_at_d = 0;
    loop {
        tokio::select! {
            _ = &mut timeout => break,
            Ok(evt) = rx_d.recv() => {
                if let NetworkEvent::PacketReceived { packet, .. } = evt {
                    if let MessagePayload::TaskAnnounce(task) = packet.payload {
                        if task.task_id == task_id {
                            received_at_d += 1;
                        }
                    }
                }
            }
        }
    }

    // Node D should have received the message exactly ONCE despite two paths (A->B->D and A->C->D)
    assert_eq!(
        received_at_d, 1,
        "D must receive the gossip message exactly once due to deduplication"
    );

    node_a.stop();
    node_b.stop();
    node_c.stop();
    node_d.stop();
}

#[tokio::test]
async fn test_stale_peer_detection() {
    let kp_a = Keypair::generate();
    let kp_b = Keypair::generate();

    let config = NodeConfig {
        heartbeat_interval: Duration::from_millis(30),
        stale_timeout: Duration::from_millis(100),
        disconnect_timeout: Duration::from_millis(250),
        gossip_max_hops: 4,
        max_peers_announce: 8,
        dedup_capacity: 1000,
        dedup_ttl: Duration::from_secs(5),
    };

    let node_a = NetworkNode::bind("127.0.0.1:0".parse().unwrap(), kp_a, Some(config.clone()))
        .await
        .unwrap();
    let node_b = NetworkNode::bind("127.0.0.1:0".parse().unwrap(), kp_b, Some(config))
        .await
        .unwrap();

    node_a.connect_peer(node_b.local_addr()).await.unwrap();
    sleep(Duration::from_millis(100)).await;

    {
        let table_a = node_a.peer_table().read().await;
        let peer = table_a.get_peer(&node_b.node_id()).expect("Peer B found");
        assert_eq!(peer.state, PeerState::Connected);
    }

    // Stop node B completely
    node_b.stop();

    // Wait past stale_timeout and disconnect_timeout
    sleep(Duration::from_millis(350)).await;

    {
        let table_a = node_a.peer_table().read().await;
        let peer = table_a.get_peer(&node_b.node_id()).expect("Peer B found");
        assert_eq!(peer.state, PeerState::Disconnected, "Peer B should be marked Disconnected");
    }

    node_a.stop();
}
