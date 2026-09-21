use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{oneshot, Mutex};
use tracing::{debug, trace, warn};

use mesh::{verify_signature, NetworkEvent, NetworkNode};
use wire::{
    MessagePayload, Packet, TaskAnnouncePayload, TaskClaimAckPayload, TaskClaimPayload,
    TaskDataPayload, TaskId, TaskRequestPayload, TaskResultPayload,
};

use crate::error::MempoolError;
use crate::pool::Mempool;
use crate::task::{Task, TaskState};

pub struct DistributedMempool {
    mempool: Arc<Mempool>,
    network: Arc<NetworkNode>,
    pending_claims: Arc<Mutex<HashMap<TaskId, oneshot::Sender<Result<(), MempoolError>>>>>,
    pending_requests: Arc<Mutex<HashMap<TaskId, Vec<std::net::SocketAddr>>>>,
    running: Arc<AtomicBool>,
}

impl DistributedMempool {
    pub fn new(mempool: Arc<Mempool>, network: Arc<NetworkNode>) -> Arc<Self> {
        let dist = Arc::new(Self {
            mempool,
            network,
            pending_claims: Arc::new(Mutex::new(HashMap::new())),
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
            running: Arc::new(AtomicBool::new(true)),
        });

        let dist_loop = Arc::clone(&dist);
        tokio::spawn(async move {
            dist_loop.run_event_loop().await;
        });

        dist
    }

    pub fn mempool(&self) -> &Arc<Mempool> {
        &self.mempool
    }

    pub fn network(&self) -> &Arc<NetworkNode> {
        &self.network
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Submit a task locally and announce it to the mesh via gossip.
    pub async fn submit_and_announce(
        &self,
        priority: u32,
        payload: Vec<u8>,
    ) -> Result<TaskId, MempoolError> {
        let now = current_timestamp();
        let author = self.network.node_id();
        let task = Task::new_available(author, priority, now, payload.clone());
        let task_id = self.mempool.submit(task)?;

        let announce = TaskAnnouncePayload {
            task_id,
            author,
            priority,
            created_at: now,
            payload_size: payload.len() as u32,
        };

        self.network
            .broadcast(MessagePayload::TaskAnnounce(announce))
            .await?;

        debug!(target: "mempool", "Submitted and announced task {}", task_id);
        Ok(task_id)
    }

    /// Claim a task across the mesh.
    /// If the local node authored the task, claims immediately.
    /// If another node authored the task, sends a signed TASK_CLAIM to the author and awaits ACK.
    pub async fn claim_task(
        &self,
        task_id: &TaskId,
        timeout_dur: Duration,
    ) -> Result<(), MempoolError> {
        let task = self
            .mempool
            .get(task_id)
            .ok_or(MempoolError::TaskNotFound(*task_id))?;

        let now = current_timestamp();

        // Local ownership path
        if task.author == self.network.node_id() {
            self.mempool.claim(task_id, self.network.node_id(), now)?;

            // Broadcast claim ack so all peers reflect the claim
            let ack = TaskClaimAckPayload {
                task_id: *task_id,
                claimant: self.network.node_id(),
                accepted: true,
                reason: 0,
                owner: self.network.node_id(),
                signature: [0u8; 64],
            };
            let _ = self.network.broadcast(MessagePayload::TaskClaimAck(ack)).await;
            return Ok(());
        }

        // Remote claim path: register one-shot response channel
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending_claims.lock().await;
            pending.insert(*task_id, tx);
        }

        let signing_data = TaskClaimPayload::claim_signing_data(
            task_id,
            &self.network.node_id(),
            now,
        );
        let signature = self.network.keypair().sign(&signing_data);

        let claim_payload = TaskClaimPayload {
            task_id: *task_id,
            claimant: self.network.node_id(),
            claimed_at: now,
            signature,
        };

        if let Err(e) = self.network.send_to(&task.author, MessagePayload::TaskClaim(claim_payload)).await {
            let mut pending = self.pending_claims.lock().await;
            pending.remove(task_id);
            return Err(MempoolError::Network(e));
        }

        tokio::select! {
            result = rx => {
                match result {
                    Ok(claim_res) => claim_res,
                    Err(_) => Err(MempoolError::Timeout),
                }
            }
            _ = tokio::time::sleep(timeout_dur) => {
                let mut pending = self.pending_claims.lock().await;
                pending.remove(task_id);
                Err(MempoolError::Timeout)
            }
        }
    }

    /// Mark a task completed locally and propagate the completion through the mesh.
    pub async fn complete_task(
        &self,
        task_id: &TaskId,
        result_data: Vec<u8>,
    ) -> Result<(), MempoolError> {
        let task = self
            .mempool
            .get(task_id)
            .ok_or(MempoolError::TaskNotFound(*task_id))?;

        let now = current_timestamp();
        self.mempool.complete(task_id, result_data.clone(), now)?;

        let result_payload = TaskResultPayload {
            task_id: *task_id,
            worker: self.network.node_id(),
            success: true,
            completed_at: now,
            result_data,
        };

        // Notify author directly if remote
        if task.author != self.network.node_id() {
            let _ = self
                .network
                .send_to(&task.author, MessagePayload::TaskResult(result_payload.clone()))
                .await;
        }

        // Broadcast to mesh
        let _ = self
            .network
            .broadcast(MessagePayload::TaskResult(result_payload))
            .await;

        Ok(())
    }

    async fn run_event_loop(&self) {
        let mut rx = self.network.subscribe();

        while self.running.load(Ordering::SeqCst) {
            match rx.recv().await {
                Ok(NetworkEvent::PacketReceived { from_addr, packet }) => {
                    self.handle_packet(packet, from_addr).await;
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    warn!(target: "mempool", "Mempool event subscriber lagged");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    break;
                }
            }
        }
    }

    async fn handle_packet(&self, packet: Packet, from_addr: std::net::SocketAddr) {
        match packet.payload {
            MessagePayload::TaskAnnounce(announce) => {
                // If we don't have this task, register it as pending and request payload
                if !self.mempool.contains(&announce.task_id) {
                    let pending_task = Task::new_pending(
                        announce.task_id,
                        announce.author,
                        announce.priority,
                        announce.created_at,
                    );
                    let _ = self.mempool.submit(pending_task);

                    // Send TaskRequest back to announcer
                    let req = TaskRequestPayload {
                        task_id: announce.task_id,
                    };
                    let _ = self
                        .network
                        .send_to_addr(from_addr, MessagePayload::TaskRequest(req))
                        .await;

                    trace!(target: "mempool", "Requested task data for {}", announce.task_id);
                }
            }
            MessagePayload::TaskRequest(req) => {
                if let Some(task) = self.mempool.get(&req.task_id) {
                    if !task.payload.is_empty() {
                        let data = TaskDataPayload {
                            task_id: task.id,
                            author: task.author,
                            priority: task.priority,
                            created_at: task.created_at,
                            payload: task.payload,
                        };
                        let _ = self
                            .network
                            .send_to_addr(from_addr, MessagePayload::TaskData(data))
                            .await;

                        trace!(target: "mempool", "Sent task data for {}", req.task_id);
                    } else {
                        // Task is pending: queue this requester to be notified when payload arrives
                        let mut reqs = self.pending_requests.lock().await;
                        reqs.entry(req.task_id).or_default().push(from_addr);
                    }
                }
            }
            MessagePayload::TaskData(data) => {
                let resolved = if let Some(task) = self.mempool.get(&data.task_id) {
                    if task.state == TaskState::Pending {
                        let _ = self.mempool.set_payload(&data.task_id, data.payload.clone());
                        debug!(target: "mempool", "Resolved pending task {} to Available", data.task_id);
                        true
                    } else {
                        false
                    }
                } else {
                    // Direct insertion if not seen before
                    let task = Task::new_available(
                        data.author,
                        data.priority,
                        data.created_at,
                        data.payload.clone(),
                    );
                    if task.id == data.task_id {
                        let _ = self.mempool.submit(task);
                        true
                    } else {
                        false
                    }
                };

                if resolved {
                    // Fulfill any queued downstream requests
                    let mut waiting_addrs = Vec::new();
                    {
                        let mut reqs = self.pending_requests.lock().await;
                        if let Some(addrs) = reqs.remove(&data.task_id) {
                            waiting_addrs = addrs;
                        }
                    }
                    for addr in waiting_addrs {
                        let reply = TaskDataPayload {
                            task_id: data.task_id,
                            author: data.author,
                            priority: data.priority,
                            created_at: data.created_at,
                            payload: data.payload.clone(),
                        };
                        let _ = self
                            .network
                            .send_to_addr(addr, MessagePayload::TaskData(reply))
                            .await;
                    }
                }
            }
            MessagePayload::TaskClaim(claim) => {
                // Verify signature
                let signing_data = TaskClaimPayload::claim_signing_data(
                    &claim.task_id,
                    &claim.claimant,
                    claim.claimed_at,
                );
                if verify_signature(&claim.claimant, &signing_data, &claim.signature).is_err() {
                    warn!(target: "mempool", "Invalid claim signature from {}", claim.claimant);
                    return;
                }

                // Check if this node is the task author/owner
                if let Some(task) = self.mempool.get(&claim.task_id) {
                    if task.author == self.network.node_id() {
                        let claim_result = self.mempool.claim(
                            &claim.task_id,
                            claim.claimant,
                            claim.claimed_at,
                        );

                        let (accepted, reason) = match claim_result {
                            Ok(()) => (true, 0),
                            Err(MempoolError::AlreadyClaimed { .. }) => (false, 1),
                            Err(_) => (false, 2),
                        };

                        let ack = TaskClaimAckPayload {
                            task_id: claim.task_id,
                            claimant: claim.claimant,
                            accepted,
                            reason,
                            owner: self.network.node_id(),
                            signature: [0u8; 64],
                        };

                        // Send direct reply to claimant
                        let _ = self
                            .network
                            .send_to_addr(from_addr, MessagePayload::TaskClaimAck(ack.clone()))
                            .await;

                        // If accepted, broadcast to mesh so all nodes mark Claimed
                        if accepted {
                            let _ = self.network.broadcast(MessagePayload::TaskClaimAck(ack)).await;
                        }
                    }
                }
            }
            MessagePayload::TaskClaimAck(ack) => {
                // Handle pending remote claim on claimant node
                let mut pending = self.pending_claims.lock().await;
                if let Some(tx) = pending.remove(&ack.task_id) {
                    if ack.accepted {
                        let _ = self.mempool.claim(
                            &ack.task_id,
                            ack.claimant,
                            current_timestamp(),
                        );
                        let _ = tx.send(Ok(()));
                    } else {
                        let _ = tx.send(Err(MempoolError::ClaimRejected(format!(
                            "Owner rejected claim with code {}",
                            ack.reason
                        ))));
                    }
                } else if ack.accepted {
                    // Update peer mempool state from broadcast
                    let _ = self.mempool.claim(
                        &ack.task_id,
                        ack.claimant,
                        current_timestamp(),
                    );
                }
            }
            MessagePayload::TaskResult(result) => {
                if let Some(_) = self.mempool.get(&result.task_id) {
                    if result.success {
                        let _ = self.mempool.complete(
                            &result.task_id,
                            result.result_data,
                            result.completed_at,
                        );
                    } else {
                        let _ = self.mempool.fail(
                            &result.task_id,
                            "Execution failed".into(),
                            result.completed_at,
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
