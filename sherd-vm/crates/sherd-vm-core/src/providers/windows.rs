use async_trait::async_trait;

use crate::{
    config::OsKind,
    error::{VmError, VmResult},
    providers::{
        CreateVmOpts, ExecOutput, FsEntry, VmHealth, VmProvider, VmSession, VmState, VmStatus,
    },
};

/// Windows provider — pluggable stub mirroring `sherd-platform-linux` pattern.
/// For MVP, returns `NotImplemented` for all operations. Replace with real
/// Hyper-V / Windows Sandbox / cloud Windows API (Azure, Paperspace) when
/// available, or use Wine fallback on Linux provider for `.exe` demo.
///
/// This keeps the `VmProvider` trait uniform across OS kinds and allows
/// `VmManager` to select provider by `OsKind` without branching on `Option`.
pub struct WindowsProvider {
    // Future: hold Windows-specific client (e.g., Hyper-V API, cloud SDK)
    // For now, stateless stub.
}

impl WindowsProvider {
    pub fn new() -> Self {
        Self {}
    }

    fn not_implemented(op: &str) -> VmError {
        VmError::NotImplemented(format!(
            "Windows provider not yet implemented for '{}' — use Linux provider with Wine for .exe demo, or configure a Windows cloud backend (Hyper-V/Azure/Paperspace)",
            op
        ))
    }
}

impl Default for WindowsProvider {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl VmProvider for WindowsProvider {
    fn kind(&self) -> OsKind { OsKind::Windows }
    fn name(&self) -> &'static str { "windows-stub" }

    async fn create(&self, _opts: CreateVmOpts) -> VmResult<VmSession> {
        Err(Self::not_implemented("create"))
    }
    async fn get(&self, _id: &str) -> VmResult<VmSession> {
        Err(Self::not_implemented("get"))
    }
    async fn destroy(&self, _id: &str) -> VmResult<()> {
        Err(Self::not_implemented("destroy"))
    }
    async fn health(&self, _id: &str) -> VmResult<VmHealth> {
        Err(Self::not_implemented("health"))
    }
    async fn pause(&self, _id: &str) -> VmResult<()> {
        Err(Self::not_implemented("pause"))
    }
    async fn resume(&self, _id: &str) -> VmResult<VmSession> {
        Err(Self::not_implemented("resume"))
    }
    async fn set_timeout(&self, _id: &str, _timeout_ms: u64) -> VmResult<()> {
        Err(Self::not_implemented("set_timeout"))
    }
    async fn exec(&self, _id: &str, _cmd: &str, _args: Vec<String>) -> VmResult<ExecOutput> {
        Err(Self::not_implemented("exec"))
    }
    async fn fs_write(&self, _id: &str, _path: &str, _content: &[u8]) -> VmResult<()> {
        Err(Self::not_implemented("fs_write"))
    }
    async fn fs_read(&self, _id: &str, _path: &str) -> VmResult<Vec<u8>> {
        Err(Self::not_implemented("fs_read"))
    }
    async fn fs_list(&self, _id: &str, _path: &str) -> VmResult<Vec<FsEntry>> {
        Err(Self::not_implemented("fs_list"))
    }
    async fn screenshot(&self, _id: &str, _format: &str, _quality: Option<u8>) -> VmResult<Vec<u8>> {
        Err(Self::not_implemented("screenshot"))
    }
    async fn stream_url(&self, _id: &str) -> VmResult<String> {
        Err(Self::not_implemented("stream_url"))
    }
    async fn mouse_move(&self, _id: &str, _x: u32, _y: u32, _humanize: bool) -> VmResult<()> {
        Err(Self::not_implemented("mouse_move"))
    }
    async fn mouse_click(&self, _id: &str, _x: u32, _y: u32, _button: &str, _humanize: bool) -> VmResult<()> {
        Err(Self::not_implemented("mouse_click"))
    }
    async fn keyboard_type(&self, _id: &str, _text: &str) -> VmResult<()> {
        Err(Self::not_implemented("keyboard_type"))
    }
    async fn keyboard_press(&self, _id: &str, _keys: Vec<String>) -> VmResult<()> {
        Err(Self::not_implemented("keyboard_press"))
    }
    async fn display_set(&self, _id: &str, _width: u32, _height: u32) -> VmResult<()> {
        Err(Self::not_implemented("display_set"))
    }
    async fn open_app(&self, _id: &str, _app: &str, _args: Vec<String>) -> VmResult<String> {
        Err(Self::not_implemented("open_app"))
    }
    async fn clipboard_set(&self, _id: &str, _text: &str) -> VmResult<()> {
        Err(Self::not_implemented("clipboard_set"))
    }
    async fn clipboard_get(&self, _id: &str) -> VmResult<String> {
        Err(Self::not_implemented("clipboard_get"))
    }
    async fn process_list(&self, _id: &str) -> VmResult<serde_json::Value> {
        Err(Self::not_implemented("process_list"))
    }
    async fn process_kill(&self, _id: &str, _pid: &str) -> VmResult<()> {
        Err(Self::not_implemented("process_kill"))
    }
    async fn snapshot(&self, _id: &str, _name: Option<String>) -> VmResult<String> {
        Err(Self::not_implemented("snapshot"))
    }
    async fn revert(&self, _id: &str, _snapshot_id: &str) -> VmResult<()> {
        Err(Self::not_implemented("revert"))
    }
}

/// Helper to build a VmStatus for Windows stub errors, for IPC responses.
pub fn stub_status(session_id: &str, detail: String) -> VmStatus {
    VmStatus {
        session_id: session_id.to_string(),
        state: VmState::Error,
        detail,
        stream_url: None,
        os: OsKind::Windows,
    }
}
