use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use crate::{
    error::VmResult,
    providers::{CreateVmOpts, VmSession, VmStatus},
    vm::VmManager,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum VmEvent {
    Created(VmSession),
    StatusChanged(VmStatus),
    Destroyed { session_id: String },
    Error { session_id: String, message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmStatusReport {
    pub session: Option<VmSession>,
    pub status: Option<VmStatus>,
    pub detail: String,
}

pub struct VmService {
    manager: Arc<VmManager>,
    events_tx: broadcast::Sender<VmEvent>,
}

impl VmService {
    pub fn new(manager: Arc<VmManager>) -> Self {
        let (events_tx, _) = broadcast::channel(64);
        Self { manager, events_tx }
    }

    pub fn manager(&self) -> &Arc<VmManager> {
        &self.manager
    }

    pub fn subscribe(&self) -> broadcast::Receiver<VmEvent> {
        self.events_tx.subscribe()
    }

    fn publish(&self, event: VmEvent) {
        let _ = self.events_tx.send(event);
    }

    pub async fn create(&self, opts: CreateVmOpts) -> VmResult<VmSession> {
        let session = self.manager.provision(opts).await?;
        self.publish(VmEvent::Created(session.clone()));
        Ok(session)
    }

    pub async fn get(&self, id: &str) -> VmResult<VmSession> {
        self.manager.get(id, None).await
    }

    pub async fn status(&self, id: &str) -> VmResult<VmStatus> {
        let status = self.manager.status(id).await?;
        self.publish(VmEvent::StatusChanged(status.clone()));
        Ok(status)
    }

    pub async fn destroy(&self, id: &str) -> VmResult<()> {
        self.manager.destroy(id).await?;
        self.publish(VmEvent::Destroyed { session_id: id.to_string() });
        Ok(())
    }

    pub async fn pause(&self, id: &str) -> VmResult<()> {
        self.manager.pause(id).await
    }

    pub async fn resume(&self, id: &str) -> VmResult<VmSession> {
        let sess = self.manager.resume(id).await?;
        self.publish(VmEvent::Created(sess.clone()));
        Ok(sess)
    }

    pub async fn exec(&self, id: &str, cmd: &str, args: Vec<String>) -> VmResult<crate::providers::ExecOutput> {
        self.manager.exec(id, cmd, args).await
    }

    pub async fn upload(&self, id: &str, path: &str, content: &[u8]) -> VmResult<()> {
        self.manager.fs_write(id, path, content).await
    }

    pub async fn stream_url(&self, id: &str) -> VmResult<String> {
        self.manager.stream_url(id).await
    }

    pub async fn screenshot(&self, id: &str, format: &str, quality: Option<u8>) -> VmResult<Vec<u8>> {
        self.manager.screenshot(id, format, quality).await
    }

    pub async fn send_input(&self, id: &str, event: crate::stream::InputEvent) -> VmResult<()> {
        let bridge = crate::stream::StreamBridge::new(Arc::clone(&self.manager));
        bridge.send_input(id, event).await
    }

    pub async fn list(&self) -> Vec<VmSession> {
        self.manager.list_sessions().await
    }

    pub async fn health(&self, id: &str) -> VmResult<crate::providers::VmHealth> {
        self.manager.health(id).await
    }
}
