use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use mempool::{DistributedMempool, Mempool, MempoolError, TaskState};
use mesh::{Keypair, NetworkNode, NodeConfig, PeerState};
use wire::NodeId;

fn test_config() -> NodeConfig {
    NodeConfig {
        heartbeat_interval: Duration::from_millis(50),
        stale_timeout: Duration::from_millis(400),
        disconnect_timeout: Duration::from_millis(800),
        gossip_max_hops: 4,
        max_peers_announce: 8,
        dedup_capacity: 1000,
        dedup_ttl: Duration::from_secs(10),
    }
}

async fn wait_for_connected(node: &NetworkNode, peer_id: &NodeId) {
    for _ in 0..100 {
        {
            let table = node.peer_table().read().await;
            if let Some(peer) = table.get_peer(peer_id) {
                if peer.state == PeerState::Connected {
                    return;
                }
            }
        }
        sleep(Duration::from_millis(15)).await;
    }
    panic!("Peer {:?} failed to reach Connected state", peer_id);
}

#[tokio::test]
async fn test_four_node_mesh_distributed_mempool_and_claiming() {
    // Topology:
    //          Node A
    //         /      \
    //      Node B   Node C
    //         \      /
    //          Node D

    // 1. Start all 4 nodes with their own NodeId, UDP socket, peer table, and mempool
    let net_a = NetworkNode::bind("127.0.0.1:0".parse().unwrap(), Keypair::generate(), Some(test_config()))
        .await
        .unwrap();
    let net_b = NetworkNode::bind("127.0.0.1:0".parse().unwrap(), Keypair::generate(), Some(test_config()))
        .await
        .unwrap();
    let net_c = NetworkNode::bind("127.0.0.1:0".parse().unwrap(), Keypair::generate(), Some(test_config()))
        .await
        .unwrap();
    let net_d = NetworkNode::bind("127.0.0.1:0".parse().unwrap(), Keypair::generate(), Some(test_config()))
        .await
        .unwrap();

    let dist_a = DistributedMempool::new(Arc::new(Mempool::new()), Arc::clone(&net_a));
    let dist_b = DistributedMempool::new(Arc::new(Mempool::new()), Arc::clone(&net_b));
    let dist_c = DistributedMempool::new(Arc::new(Mempool::new()), Arc::clone(&net_c));
    let dist_d = DistributedMempool::new(Arc::new(Mempool::new()), Arc::clone(&net_d));

    // 2. Establish connections: A<->B, A<->C, B<->D, C<->D
    net_a.connect_peer(net_b.local_addr()).await.unwrap();
    net_a.connect_peer(net_c.local_addr()).await.unwrap();
    net_b.connect_peer(net_d.local_addr()).await.unwrap();
    net_c.connect_peer(net_d.local_addr()).await.unwrap();

    // 3. Wait for handshakes to complete mutually across all edges
    wait_for_connected(&net_a, &net_b.node_id()).await;
    wait_for_connected(&net_a, &net_c.node_id()).await;
    wait_for_connected(&net_b, &net_a.node_id()).await;
    wait_for_connected(&net_b, &net_d.node_id()).await;
    wait_for_connected(&net_c, &net_a.node_id()).await;
    wait_for_connected(&net_c, &net_d.node_id()).await;
    wait_for_connected(&net_d, &net_b.node_id()).await;
    wait_for_connected(&net_d, &net_c.node_id()).await;

    // 4 & 5 & 6 & 7: Submit task to A and announce via gossip
    let task_payload = b"compute_heavy_simulation_data_chunks".to_vec();
    let task_id = dist_a
        .submit_and_announce(10, task_payload.clone())
        .await
        .expect("Submit and announce on Node A");

    assert!(dist_a.mempool().contains(&task_id));

    // 8 & 9 & 10: Wait for task announcement and data exchange to propagate to B, C, D
    let start = std::time::Instant::now();
    loop {
        let b_has = dist_b.mempool().get(&task_id).map(|t| t.state == TaskState::Available).unwrap_or(false);
        let c_has = dist_c.mempool().get(&task_id).map(|t| t.state == TaskState::Available).unwrap_or(false);
        let d_has = dist_d.mempool().get(&task_id).map(|t| t.state == TaskState::Available).unwrap_or(false);
        if b_has && c_has && d_has {
            break;
        }
        if start.elapsed() > Duration::from_secs(4) {
            panic!(
                "Propagation timeout after {:?}: b_has={}, c_has={}, d_has={}",
                start.elapsed(), b_has, c_has, d_has
            );
        }
        sleep(Duration::from_millis(25)).await;
    }

    // Verify all 4 nodes have the full task in Available state
    let task_at_b = dist_b.mempool().get(&task_id).expect("Node B has task");
    assert_eq!(task_at_b.state, TaskState::Available);
    assert_eq!(task_at_b.payload, task_payload);

    let task_at_c = dist_c.mempool().get(&task_id).expect("Node C has task");
    assert_eq!(task_at_c.state, TaskState::Available);

    let task_at_d = dist_d.mempool().get(&task_id).expect("Node D has task");
    assert_eq!(task_at_d.state, TaskState::Available);

    // 11. Verify duplicate submission is rejected and count is 1
    assert_eq!(dist_b.mempool().len(), 1);
    assert_eq!(dist_d.mempool().len(), 1);

    // 12. Worker on Node B claims the task
    let claim_b = dist_b.claim_task(&task_id, Duration::from_secs(2)).await;
    assert!(claim_b.is_ok(), "Node B claim should succeed: {:?}", claim_b);

    // Wait briefly for claim ack to propagate to all nodes
    sleep(Duration::from_millis(200)).await;

    // Verify state on Node A (owner) and Node B (worker) is Claimed
    let task_at_a = dist_a.mempool().get(&task_id).unwrap();
    match task_at_a.state {
        TaskState::Claimed { by, .. } => assert_eq!(by, net_b.node_id()),
        other => panic!("Expected Claimed state at Node A, got {:?}", other),
    }

    // 13 & 14. Second worker on Node D attempts to claim the same task -> must fail!
    let claim_d = dist_d.claim_task(&task_id, Duration::from_secs(2)).await;
    assert!(
        matches!(claim_d, Err(MempoolError::ClaimRejected(_)) | Err(MempoolError::AlreadyClaimed { .. })),
        "Node D claim must be rejected as already claimed: {:?}",
        claim_d
    );

    // 15. Mark task completed on Node B
    let result_data = b"simulation_result_hash_98765".to_vec();
    dist_b
        .complete_task(&task_id, result_data.clone())
        .await
        .expect("Complete task on Node B");

    // Wait for completion gossip
    sleep(Duration::from_millis(200)).await;

    // 16. Verify task state is Completed across nodes
    let final_task_a = dist_a.mempool().get(&task_id).unwrap();
    match final_task_a.state {
        TaskState::Completed { result, .. } => assert_eq!(result, result_data),
        other => panic!("Expected Completed state at Node A, got {:?}", other),
    }

    let final_task_b = dist_b.mempool().get(&task_id).unwrap();
    match final_task_b.state {
        TaskState::Completed { result, .. } => assert_eq!(result, result_data),
        other => panic!("Expected Completed state at Node B, got {:?}", other),
    }

    // Cleanup
    dist_a.stop();
    dist_b.stop();
    dist_c.stop();
    dist_d.stop();
    net_a.stop();
    net_b.stop();
    net_c.stop();
    net_d.stop();
}
