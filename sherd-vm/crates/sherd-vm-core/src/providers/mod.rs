pub mod solari_linux;
pub mod windows;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{config::OsKind, error::VmResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateVmOpts {
    pub os: OsKind,
    pub template: Option<String>,
    pub resolution: Option<String>,
    pub cpu: Option<u8>,
    pub mem_mb: Option<u32>,
    pub timeout_ms: Option<u64>,
    pub lifecycle: Option<String>, // "pause" | "kill"
    pub from_snapshot: Option<String>,
    pub volumes: Option<Vec<VolumeMount>>,
}

impl Default for CreateVmOpts {
    fn default() -> Self {
        Self {
            os: OsKind::Linux,
            template: None,
            resolution: None,
            cpu: None,
            mem_mb: None,
            timeout_ms: None,
            lifecycle: None,
            from_snapshot: None,
            volumes: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeMount {
    pub volume_id: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmSession {
    pub session_id: String,
    pub stream_url: Option<String>,
    pub status: Option<String>,
    pub template: Option<String>,
    pub os: OsKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmHealth {
    pub ready: bool,
    pub display: Option<DisplayInfo>,
    pub vnc: Option<VncInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayInfo {
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VncInfo {
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmStatus {
    pub session_id: String,
    pub state: VmState,
    pub detail: String,
    pub stream_url: Option<String>,
    pub os: OsKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VmState {
    Starting,
    Up,
    Down,
    Paused,
    Error,
}

impl VmState {
    pub fn as_str(self) -> &'static str {
        match self {
            VmState::Starting => "starting",
            VmState::Up => "up",
            VmState::Down => "down",
            VmState::Paused => "paused",
            VmState::Error => "error",
        }
    }
}

#[async_trait]
pub trait VmProvider: Send + Sync {
    fn kind(&self) -> OsKind;
    fn name(&self) -> &'static str;

    async fn create(&self, opts: CreateVmOpts) -> VmResult<VmSession>;
    async fn get(&self, id: &str) -> VmResult<VmSession>;
    async fn destroy(&self, id: &str) -> VmResult<()>;
    async fn health(&self, id: &str) -> VmResult<VmHealth>;
    async fn pause(&self, id: &str) -> VmResult<()>;
    async fn resume(&self, id: &str) -> VmResult<VmSession>;
    async fn set_timeout(&self, id: &str, timeout_ms: u64) -> VmResult<()>;

    async fn exec(&self, id: &str, cmd: &str, args: Vec<String>) -> VmResult<ExecOutput>;
    async fn fs_write(&self, id: &str, path: &str, content: &[u8]) -> VmResult<()>;
    async fn fs_read(&self, id: &str, path: &str) -> VmResult<Vec<u8>>;
    async fn fs_list(&self, id: &str, path: &str) -> VmResult<Vec<FsEntry>>;
    async fn screenshot(&self, id: &str, format: &str, quality: Option<u8>) -> VmResult<Vec<u8>>;
    async fn stream_url(&self, id: &str) -> VmResult<String>;

    async fn mouse_move(&self, id: &str, x: u32, y: u32, humanize: bool) -> VmResult<()>;
    async fn mouse_click(&self, id: &str, x: u32, y: u32, button: &str, humanize: bool) -> VmResult<()>;
    async fn keyboard_type(&self, id: &str, text: &str) -> VmResult<()>;
    async fn keyboard_press(&self, id: &str, keys: Vec<String>) -> VmResult<()>;

    async fn display_set(&self, id: &str, width: u32, height: u32) -> VmResult<()>;
    async fn open_app(&self, id: &str, app: &str, args: Vec<String>) -> VmResult<String>;
    async fn clipboard_set(&self, id: &str, text: &str) -> VmResult<()>;
    async fn clipboard_get(&self, id: &str) -> VmResult<String>;
    async fn process_list(&self, id: &str) -> VmResult<serde_json::Value>;
    async fn process_kill(&self, id: &str, pid: &str) -> VmResult<()>;
    async fn snapshot(&self, id: &str, name: Option<String>) -> VmResult<String>;
    async fn revert(&self, id: &str, snapshot_id: &str) -> VmResult<()>;
}
