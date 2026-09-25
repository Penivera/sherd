use async_trait::async_trait;

use solari_desktop_rs::{ClientOptions, CreateDesktopOpts, DesktopClient, Lifecycle};

use crate::{
    config::{OsKind, SolariConfig},
    error::{VmError, VmResult},
    providers::{
        CreateVmOpts, DisplayInfo, ExecOutput, FsEntry, VncInfo, VmHealth, VmProvider, VmSession,
    },
};

pub struct SolariLinuxProvider {
    client: DesktopClient,
}

impl SolariLinuxProvider {
    pub fn new(config: &SolariConfig) -> VmResult<Self> {
        let opts = ClientOptions::new(config.api_key.clone(), config.base_url.clone())
            .map_err(|e| VmError::Other(e.to_string()))?
            .with_region(config.region.clone())
            .with_timeout(config.timeout_ms)
            .with_max_retries(config.max_retries);
        let client = DesktopClient::new(opts).map_err(|e| VmError::Other(e.to_string()))?;
        Ok(Self { client })
    }

    pub fn from_client(client: DesktopClient) -> Self {
        Self { client }
    }

    fn map_session(s: solari_desktop_rs::DesktopSession, os: OsKind) -> VmSession {
        VmSession {
            session_id: s.session_id,
            stream_url: s.stream_url,
            status: s.status,
            template: s.template,
            os,
        }
    }

    fn map_health(h: solari_desktop_rs::Health) -> VmHealth {
        VmHealth {
            ready: h.ready,
            display: h.display.map(|d| DisplayInfo { width: d.width, height: d.height }),
            vnc: h.vnc.map(|v| VncInfo { url: v.url }),
        }
    }
}

#[async_trait]
impl VmProvider for SolariLinuxProvider {
    fn kind(&self) -> OsKind { OsKind::Linux }
    fn name(&self) -> &'static str { "solari-linux" }

    async fn create(&self, opts: CreateVmOpts) -> VmResult<VmSession> {
        let lifecycle = opts.lifecycle.as_deref().map(|s| match s {
            "kill" => Lifecycle::kill(),
            _ => Lifecycle::pause(),
        });
        let create_opts = CreateDesktopOpts {
            template: opts.template.or(Some("default".into())),
            resolution: opts.resolution.or(Some("1280x720".into())),
            cpu: opts.cpu,
            mem_mb: opts.mem_mb,
            timeout_ms: opts.timeout_ms,
            lifecycle,
            from_snapshot: opts.from_snapshot,
            region: None,
            volumes: opts.volumes.map(|v| v.into_iter().map(|m| solari_desktop_rs::VolumeMount { volume_id: m.volume_id, path: m.path }).collect()),
        };
        let sess = self.client.create(create_opts).await.map_err(VmError::Solari)?;
        Ok(Self::map_session(sess, OsKind::Linux))
    }

    async fn get(&self, id: &str) -> VmResult<VmSession> {
        let s = self.client.get(id).await.map_err(VmError::Solari)?;
        Ok(Self::map_session(s, OsKind::Linux))
    }

    async fn destroy(&self, id: &str) -> VmResult<()> {
        self.client.destroy(id).await.map_err(VmError::Solari)
    }

    async fn health(&self, id: &str) -> VmResult<VmHealth> {
        let h = self.client.health(id).await.map_err(VmError::Solari)?;
        Ok(Self::map_health(h))
    }

    async fn pause(&self, id: &str) -> VmResult<()> {
        self.client.pause(id).await.map_err(VmError::Solari)
    }

    async fn resume(&self, id: &str) -> VmResult<VmSession> {
        let s = self.client.resume(id).await.map_err(VmError::Solari)?;
        Ok(Self::map_session(s, OsKind::Linux))
    }

    async fn set_timeout(&self, id: &str, timeout_ms: u64) -> VmResult<()> {
        self.client.set_timeout(id, timeout_ms).await.map_err(VmError::Solari)
    }

    async fn exec(&self, id: &str, cmd: &str, args: Vec<String>) -> VmResult<ExecOutput> {
        let r = self.client.exec(id, cmd, args).await.map_err(VmError::Solari)?;
        Ok(ExecOutput { exit_code: r.exit_code, stdout: r.stdout, stderr: r.stderr })
    }

    async fn fs_write(&self, id: &str, path: &str, content: &[u8]) -> VmResult<()> {
        self.client.fs_write(id, path, content).await.map_err(VmError::Solari)
    }

    async fn fs_read(&self, id: &str, path: &str) -> VmResult<Vec<u8>> {
        self.client.fs_read(id, path).await.map_err(VmError::Solari)
    }

    async fn fs_list(&self, id: &str, path: &str) -> VmResult<Vec<FsEntry>> {
        let entries = self.client.fs_list(id, path).await.map_err(VmError::Solari)?;
        Ok(entries.into_iter().map(|e| FsEntry { name: e.name, path: e.path, is_dir: e.is_dir, size: e.size }).collect())
    }

    async fn screenshot(&self, id: &str, format: &str, quality: Option<u8>) -> VmResult<Vec<u8>> {
        self.client.screenshot(id, format, quality).await.map_err(VmError::Solari)
    }

    async fn stream_url(&self, id: &str) -> VmResult<String> {
        let info = self.client.stream_url(id).await.map_err(VmError::Solari)?;
        Ok(info.stream_url)
    }

    async fn mouse_move(&self, id: &str, x: u32, y: u32, humanize: bool) -> VmResult<()> {
        self.client.mouse_move(id, x, y, humanize).await.map_err(VmError::Solari)
    }

    async fn mouse_click(&self, id: &str, x: u32, y: u32, button: &str, humanize: bool) -> VmResult<()> {
        self.client.mouse_click(id, x, y, button, humanize).await.map_err(VmError::Solari)
    }

    async fn keyboard_type(&self, id: &str, text: &str) -> VmResult<()> {
        self.client.keyboard_type(id, text).await.map_err(VmError::Solari)
    }

    async fn keyboard_press(&self, id: &str, keys: Vec<String>) -> VmResult<()> {
        self.client.keyboard_press(id, keys).await.map_err(VmError::Solari)
    }

    async fn display_set(&self, id: &str, width: u32, height: u32) -> VmResult<()> {
        self.client.display_set(id, width, height).await.map_err(VmError::Solari)
    }

    async fn open_app(&self, id: &str, app: &str, args: Vec<String>) -> VmResult<String> {
        self.client.open_app(id, app, args).await.map_err(VmError::Solari)
    }

    async fn clipboard_set(&self, id: &str, text: &str) -> VmResult<()> {
        self.client.clipboard_set(id, text).await.map_err(VmError::Solari)
    }

    async fn clipboard_get(&self, id: &str) -> VmResult<String> {
        self.client.clipboard_get(id).await.map_err(VmError::Solari)
    }

    async fn process_list(&self, id: &str) -> VmResult<serde_json::Value> {
        self.client.process_list(id).await.map_err(VmError::Solari)
    }

    async fn process_kill(&self, id: &str, pid: &str) -> VmResult<()> {
        self.client.process_kill(id, pid).await.map_err(VmError::Solari)
    }

    async fn snapshot(&self, id: &str, name: Option<String>) -> VmResult<String> {
        self.client.snapshot(id, name).await.map_err(VmError::Solari)
    }

    async fn revert(&self, id: &str, snapshot_id: &str) -> VmResult<()> {
        self.client.revert(id, snapshot_id).await.map_err(VmError::Solari)
    }
}
