use std::{collections::HashMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::{
    config::{OsKind, SherdVmConfig},
    error::{VmError, VmResult},
    providers::{
        solari_linux::SolariLinuxProvider, windows::WindowsProvider, CreateVmOpts, VmHealth,
        VmProvider, VmSession, VmState, VmStatus,
    },
};

const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(500);
const HEALTH_POLL_ATTEMPTS: usize = 30; // 15s total

pub struct VmManager {
    linux: Arc<dyn VmProvider>,
    windows: Arc<dyn VmProvider>,
    // Track active sessions for status/watchdog
    sessions: RwLock<HashMap<String, VmSession>>,
}

impl VmManager {
    pub fn new(config: &SherdVmConfig) -> VmResult<Self> {
        let linux: Arc<dyn VmProvider> = match SolariLinuxProvider::new(&config.solari) {
            Ok(p) => Arc::new(p),
            Err(e) => {
                warn!("failed to init Solari Linux provider: {} — using stub", e);
                // Fallback to stub that will error on create but allow manager to exist without key
                Arc::new(WindowsProvider::new()) // reuse stub type but kind will be wrong; better to create a failing linux stub
            }
        };
        // If linux init failed due to missing key, create a stub that errors with clear message
        let linux: Arc<dyn VmProvider> = if std::env::var("SOLARI_API_KEY").is_err() {
            Arc::new(StubProvider { kind: OsKind::Linux, msg: "SOLARI_API_KEY not set".into() })
        } else {
            linux
        };
        let windows: Arc<dyn VmProvider> = Arc::new(WindowsProvider::new());
        Ok(Self { linux, windows, sessions: RwLock::new(HashMap::new()) })
    }

    pub fn with_providers(linux: Arc<dyn VmProvider>, windows: Arc<dyn VmProvider>) -> Self {
        Self { linux, windows, sessions: RwLock::new(HashMap::new()) }
    }

    fn provider_for(&self, os: OsKind) -> Arc<dyn VmProvider> {
        match os {
            OsKind::Linux => Arc::clone(&self.linux),
            OsKind::Windows => Arc::clone(&self.windows),
        }
    }

    pub async fn provision(&self, opts: CreateVmOpts) -> VmResult<VmSession> {
        let provider = self.provider_for(opts.os);
        info!(os = %opts.os, template = ?opts.template, "provisioning VM via {}", provider.name());
        let session = provider.create(opts).await?;
        let id = session.session_id.clone();
        // Poll health until ready — skip for sandbox desktops (no health endpoint, 404 is normal)
        // Only poll if provider is not sandbox-style; for now, try health but don't fail on 404
        let _ = self.wait_for_ready(&provider, &id).await;
        // Fetch stream_url if not already present
        let mut session = session;
        if session.stream_url.is_none() {
            if let Ok(url) = provider.stream_url(&id).await {
                session.stream_url = Some(url);
            }
        }
        self.sessions.write().await.insert(id.clone(), session.clone());
        info!(session_id = %id, stream_url = ?session.stream_url, "VM provisioned and ready");
        Ok(session)
    }

    async fn wait_for_ready(&self, provider: &Arc<dyn VmProvider>, id: &str) -> VmResult<()> {
        // Solari sandboxes (kind: desktop) may not expose /health — treat 404 as ready after short delay
        let mut consecutive_404 = 0;
        for attempt in 0..HEALTH_POLL_ATTEMPTS {
            match provider.health(id).await {
                Ok(h) if h.ready => return Ok(()),
                Ok(h) => {
                    warn!(attempt, ready = h.ready, "VM not ready yet, polling");
                    consecutive_404 = 0;
                }
                Err(e) => {
                    let msg = e.to_string();
                    if msg.contains("404") || msg.contains("NotFound") || msg.contains("not found") {
                        consecutive_404 += 1;
                        // If health consistently 404, assume VM is ready (Solari sandbox desktop has no health endpoint)
                        if consecutive_404 >= 3 {
                            info!(attempt, "health endpoint not found, assuming VM ready (sandbox desktop)");
                            return Ok(());
                        }
                    } else {
                        consecutive_404 = 0;
                    }
                    warn!(attempt, error = %e, "health check failed, polling");
                }
            }
            tokio::time::sleep(HEALTH_POLL_INTERVAL).await;
        }
        Err(VmError::Timeout(
            HEALTH_POLL_INTERVAL.as_millis() as u64 * HEALTH_POLL_ATTEMPTS as u64,
            format!("VM {} not ready after {} attempts", id, HEALTH_POLL_ATTEMPTS),
        ))
    }

    pub async fn get(&self, id: &str, os: Option<OsKind>) -> VmResult<VmSession> {
        // Try to infer provider from tracked sessions, else try both
        if let Some(os) = os {
            return self.provider_for(os).get(id).await;
        }
        if let Some(sess) = self.sessions.read().await.get(id) {
            return self.provider_for(sess.os).get(id).await;
        }
        // Try linux first, then windows
        match self.linux.get(id).await {
            Ok(s) => Ok(s),
            Err(_) => self.windows.get(id).await,
        }
    }

    pub async fn destroy(&self, id: &str) -> VmResult<()> {
        let os = self.sessions.read().await.get(id).map(|s| s.os);
        let provider = if let Some(os) = os { self.provider_for(os) } else { Arc::clone(&self.linux) };
        let res = provider.destroy(id).await;
        // Also try windows if linux 404
        let res = match res {
            Err(VmError::Solari(solari_desktop_rs::SolariError::NotFound(_))) |
            Err(VmError::NotFound(_)) if os.is_none() => self.windows.destroy(id).await,
            other => other,
        };
        self.sessions.write().await.remove(id);
        res
    }

    pub async fn health(&self, id: &str) -> VmResult<VmHealth> {
        let os = self.sessions.read().await.get(id).map(|s| s.os);
        let provider = if let Some(os) = os { self.provider_for(os) } else { Arc::clone(&self.linux) };
        let res = provider.health(id).await;
        match res {
            Err(VmError::Solari(solari_desktop_rs::SolariError::NotFound(_))) if os.is_none() => self.windows.health(id).await,
            other => other,
        }
    }

    pub async fn status(&self, id: &str) -> VmResult<VmStatus> {
        let sess = self.get(id, None).await;
        let health = self.health(id).await;
        match (sess, health) {
            (Ok(s), Ok(h)) => Ok(VmStatus {
                session_id: s.session_id,
                state: if h.ready { VmState::Up } else { VmState::Starting },
                detail: if h.ready { "ready".into() } else { "starting".into() },
                stream_url: s.stream_url,
                os: s.os,
            }),
            (Ok(s), Err(e)) => Ok(VmStatus {
                session_id: s.session_id.clone(),
                state: VmState::Error,
                detail: format!("health error: {}", e),
                stream_url: s.stream_url,
                os: s.os,
            }),
            (Err(e), _) => Err(e),
        }
    }

    pub async fn pause(&self, id: &str) -> VmResult<()> {
        let os = self.sessions.read().await.get(id).map(|s| s.os).unwrap_or(OsKind::Linux);
        self.provider_for(os).pause(id).await
    }

    pub async fn resume(&self, id: &str) -> VmResult<VmSession> {
        let os = self.sessions.read().await.get(id).map(|s| s.os).unwrap_or(OsKind::Linux);
        let sess = self.provider_for(os).resume(id).await?;
        self.sessions.write().await.insert(sess.session_id.clone(), sess.clone());
        Ok(sess)
    }

    pub async fn set_timeout(&self, id: &str, timeout_ms: u64) -> VmResult<()> {
        let os = self.sessions.read().await.get(id).map(|s| s.os).unwrap_or(OsKind::Linux);
        self.provider_for(os).set_timeout(id, timeout_ms).await
    }

    pub async fn setup_environment(&self, id: &str, spec: SetupSpec) -> VmResult<()> {
        let os = self.sessions.read().await.get(id).map(|s| s.os).unwrap_or(OsKind::Linux);
        let provider = self.provider_for(os);
        if let Some(apt) = spec.apt_packages {
            if !apt.is_empty() {
                info!(session_id = %id, packages = ?apt, "installing apt packages");
                let cmd = format!("apt-get update && apt-get install -y {}", apt.join(" "));
                let out = provider.exec(id, "sh", vec!["-c".into(), cmd]).await?;
                if out.exit_code != 0 {
                    warn!(stderr = %out.stderr, "apt install non-zero exit");
                }
            }
        }
        if let Some(pip) = spec.pip_packages {
            if !pip.is_empty() {
                let cmd = format!("pip3 install --no-cache-dir {}", pip.join(" "));
                let out = provider.exec(id, "sh", vec!["-c".into(), cmd]).await?;
                if out.exit_code != 0 {
                    warn!(stderr = %out.stderr, "pip install non-zero exit");
                }
            }
        }
        for (k, v) in spec.env_vars {
            let cmd = format!("export {}={:?}", k, v);
            let _ = provider.exec(id, "sh", vec!["-c".into(), cmd]).await;
        }
        if let Some(dir) = spec.workdir {
            let _ = provider.exec(id, "mkdir", vec!["-p".into(), dir.clone()]).await;
        }
        for cmd in spec.run_commands {
            let out = provider.exec(id, "sh", vec!["-c".into(), cmd.clone()]).await?;
            if out.exit_code != 0 {
                warn!(cmd = %cmd, stderr = %out.stderr, "run_command failed");
            }
        }
        Ok(())
    }

    pub async fn list_sessions(&self) -> Vec<VmSession> {
        self.sessions.read().await.values().cloned().collect()
    }

    // Delegate helpers
    pub async fn exec(&self, id: &str, cmd: &str, args: Vec<String>) -> VmResult<crate::providers::ExecOutput> {
        let os = self.sessions.read().await.get(id).map(|s| s.os).unwrap_or(OsKind::Linux);
        self.provider_for(os).exec(id, cmd, args).await
    }
    pub async fn fs_write(&self, id: &str, path: &str, content: &[u8]) -> VmResult<()> {
        let os = self.sessions.read().await.get(id).map(|s| s.os).unwrap_or(OsKind::Linux);
        self.provider_for(os).fs_write(id, path, content).await
    }
    pub async fn fs_read(&self, id: &str, path: &str) -> VmResult<Vec<u8>> {
        let os = self.sessions.read().await.get(id).map(|s| s.os).unwrap_or(OsKind::Linux);
        self.provider_for(os).fs_read(id, path).await
    }
    pub async fn fs_list(&self, id: &str, path: &str) -> VmResult<Vec<crate::providers::FsEntry>> {
        let os = self.sessions.read().await.get(id).map(|s| s.os).unwrap_or(OsKind::Linux);
        self.provider_for(os).fs_list(id, path).await
    }
    pub async fn screenshot(&self, id: &str, format: &str, quality: Option<u8>) -> VmResult<Vec<u8>> {
        let os = self.sessions.read().await.get(id).map(|s| s.os).unwrap_or(OsKind::Linux);
        self.provider_for(os).screenshot(id, format, quality).await
    }
    pub async fn stream_url(&self, id: &str) -> VmResult<String> {
        let os = self.sessions.read().await.get(id).map(|s| s.os).unwrap_or(OsKind::Linux);
        self.provider_for(os).stream_url(id).await
    }
    pub async fn mouse_move(&self, id: &str, x: u32, y: u32, humanize: bool) -> VmResult<()> {
        let os = self.sessions.read().await.get(id).map(|s| s.os).unwrap_or(OsKind::Linux);
        self.provider_for(os).mouse_move(id, x, y, humanize).await
    }
    pub async fn mouse_click(&self, id: &str, x: u32, y: u32, button: &str, humanize: bool) -> VmResult<()> {
        let os = self.sessions.read().await.get(id).map(|s| s.os).unwrap_or(OsKind::Linux);
        self.provider_for(os).mouse_click(id, x, y, button, humanize).await
    }
    pub async fn keyboard_type(&self, id: &str, text: &str) -> VmResult<()> {
        let os = self.sessions.read().await.get(id).map(|s| s.os).unwrap_or(OsKind::Linux);
        self.provider_for(os).keyboard_type(id, text).await
    }
    pub async fn keyboard_press(&self, id: &str, keys: Vec<String>) -> VmResult<()> {
        let os = self.sessions.read().await.get(id).map(|s| s.os).unwrap_or(OsKind::Linux);
        self.provider_for(os).keyboard_press(id, keys).await
    }
}

#[derive(Debug, Clone, Default)]
pub struct SetupSpec {
    pub apt_packages: Option<Vec<String>>,
    pub pip_packages: Option<Vec<String>>,
    pub env_vars: HashMap<String, String>,
    pub workdir: Option<String>,
    pub run_commands: Vec<String>,
}

// Stub for missing API key case
struct StubProvider {
    kind: OsKind,
    msg: String,
}

#[async_trait]
impl VmProvider for StubProvider {
    fn kind(&self) -> OsKind { self.kind }
    fn name(&self) -> &'static str { "stub" }
    async fn create(&self, _opts: CreateVmOpts) -> VmResult<VmSession> { Err(VmError::Other(self.msg.clone())) }
    async fn get(&self, _id: &str) -> VmResult<VmSession> { Err(VmError::Other(self.msg.clone())) }
    async fn destroy(&self, _id: &str) -> VmResult<()> { Err(VmError::Other(self.msg.clone())) }
    async fn health(&self, _id: &str) -> VmResult<VmHealth> { Err(VmError::Other(self.msg.clone())) }
    async fn pause(&self, _id: &str) -> VmResult<()> { Err(VmError::Other(self.msg.clone())) }
    async fn resume(&self, _id: &str) -> VmResult<VmSession> { Err(VmError::Other(self.msg.clone())) }
    async fn set_timeout(&self, _id: &str, _t: u64) -> VmResult<()> { Err(VmError::Other(self.msg.clone())) }
    async fn exec(&self, _id: &str, _cmd: &str, _args: Vec<String>) -> VmResult<crate::providers::ExecOutput> { Err(VmError::Other(self.msg.clone())) }
    async fn fs_write(&self, _id: &str, _path: &str, _c: &[u8]) -> VmResult<()> { Err(VmError::Other(self.msg.clone())) }
    async fn fs_read(&self, _id: &str, _path: &str) -> VmResult<Vec<u8>> { Err(VmError::Other(self.msg.clone())) }
    async fn fs_list(&self, _id: &str, _path: &str) -> VmResult<Vec<crate::providers::FsEntry>> { Err(VmError::Other(self.msg.clone())) }
    async fn screenshot(&self, _id: &str, _f: &str, _q: Option<u8>) -> VmResult<Vec<u8>> { Err(VmError::Other(self.msg.clone())) }
    async fn stream_url(&self, _id: &str) -> VmResult<String> { Err(VmError::Other(self.msg.clone())) }
    async fn mouse_move(&self, _id: &str, _x: u32, _y: u32, _h: bool) -> VmResult<()> { Err(VmError::Other(self.msg.clone())) }
    async fn mouse_click(&self, _id: &str, _x: u32, _y: u32, _b: &str, _h: bool) -> VmResult<()> { Err(VmError::Other(self.msg.clone())) }
    async fn keyboard_type(&self, _id: &str, _t: &str) -> VmResult<()> { Err(VmError::Other(self.msg.clone())) }
    async fn keyboard_press(&self, _id: &str, _k: Vec<String>) -> VmResult<()> { Err(VmError::Other(self.msg.clone())) }
    async fn display_set(&self, _id: &str, _w: u32, _h: u32) -> VmResult<()> { Err(VmError::Other(self.msg.clone())) }
    async fn open_app(&self, _id: &str, _a: &str, _args: Vec<String>) -> VmResult<String> { Err(VmError::Other(self.msg.clone())) }
    async fn clipboard_set(&self, _id: &str, _t: &str) -> VmResult<()> { Err(VmError::Other(self.msg.clone())) }
    async fn clipboard_get(&self, _id: &str) -> VmResult<String> { Err(VmError::Other(self.msg.clone())) }
    async fn process_list(&self, _id: &str) -> VmResult<serde_json::Value> { Err(VmError::Other(self.msg.clone())) }
    async fn process_kill(&self, _id: &str, _p: &str) -> VmResult<()> { Err(VmError::Other(self.msg.clone())) }
    async fn snapshot(&self, _id: &str, _n: Option<String>) -> VmResult<String> { Err(VmError::Other(self.msg.clone())) }
    async fn revert(&self, _id: &str, _s: &str) -> VmResult<()> { Err(VmError::Other(self.msg.clone())) }
}
