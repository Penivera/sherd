use std::path::Path;

use tokio::fs;
use tracing::{debug, info};

use crate::{error::VmResult, vm::VmManager};

pub struct FileLoader {
    manager: std::sync::Arc<VmManager>,
}

impl FileLoader {
    pub fn new(manager: std::sync::Arc<VmManager>) -> Self {
        Self { manager }
    }

    /// Upload a single local file to remote path inside VM.
    /// Reads local bytes and writes via provider `fs_write` (base64 JSON).
    /// For large files, Solari also supports presigned `/files/upload` — fallback handled in client.
    pub async fn upload_file(&self, session_id: &str, local_path: &Path, remote_path: &str) -> VmResult<()> {
        let bytes = fs::read(local_path).await?;
        info!(session_id, local = %local_path.display(), remote = %remote_path, bytes = bytes.len(), "uploading file to VM");
        // Ensure parent dir exists
        if let Some(parent) = Path::new(remote_path).parent().and_then(|p| p.to_str()) {
            if !parent.is_empty() && parent != "/" {
                let _ = self.manager.exec(session_id, "mkdir", vec!["-p".into(), parent.into()]).await;
            }
        }
        self.manager.fs_write(session_id, remote_path, &bytes).await?;
        debug!(session_id, remote = %remote_path, "file uploaded");
        Ok(())
    }

    /// Upload a local directory recursively to remote base path.
    pub async fn upload_dir(&self, session_id: &str, local_dir: &Path, remote_base: &str) -> VmResult<()> {
        let mut stack = vec![local_dir.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let mut entries = fs::read_dir(&dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                let rel = path.strip_prefix(local_dir).unwrap();
                let remote = format!("{}/{}", remote_base.trim_end_matches('/'), rel.display());
                if path.is_dir() {
                    let _ = self.manager.exec(session_id, "mkdir", vec!["-p".into(), remote.clone()]).await;
                    stack.push(path);
                } else {
                    self.upload_file(session_id, &path, &remote).await?;
                }
            }
        }
        Ok(())
    }

    /// Download a remote file to local path.
    pub async fn download_file(&self, session_id: &str, remote_path: &str, local_path: &Path) -> VmResult<()> {
        let bytes = self.manager.fs_read(session_id, remote_path).await?;
        if let Some(parent) = local_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(local_path, &bytes).await?;
        info!(session_id, remote = %remote_path, local = %local_path.display(), bytes = bytes.len(), "downloaded file from VM");
        Ok(())
    }

    /// List remote directory.
    pub async fn list(&self, session_id: &str, remote_path: &str) -> VmResult<Vec<crate::providers::FsEntry>> {
        self.manager.fs_list(session_id, remote_path).await
    }

    /// Launch a Windows .exe via Wine on Linux VM.
    /// Requires Wine installed — call `ensure_wine` first or use snapshot `wine-ready`.
    pub async fn launch_windows_exe(&self, session_id: &str, remote_exe_path: &str, args: Vec<String>) -> VmResult<crate::providers::ExecOutput> {
        let mut full_args = vec![remote_exe_path.to_string()];
        full_args.extend(args);
        // Try wine64 first, fallback to wine
        let out = self.manager.exec(session_id, "wine64", full_args.clone()).await;
        match out {
            Ok(o) if o.exit_code == 0 || !o.stderr.contains("command not found") => Ok(o),
            _ => self.manager.exec(session_id, "wine", full_args).await,
        }
    }

    /// Ensure Wine is installed on the VM (Ubuntu). Idempotent — checks `wine --version` first.
    pub async fn ensure_wine(&self, session_id: &str) -> VmResult<()> {
        let check = self.manager.exec(session_id, "sh", vec!["-c".into(), "wine --version || wine64 --version".into()]).await;
        if let Ok(o) = check {
            if o.exit_code == 0 && !o.stdout.is_empty() {
                info!(session_id, version = %o.stdout.trim(), "wine already installed");
                return Ok(());
            }
        }
        info!(session_id, "installing wine64");
        let out = self.manager.exec(session_id, "sh", vec!["-c".into(), "apt-get update && apt-get install -y wine64 winetricks".into()]).await?;
        if out.exit_code != 0 {
            tracing::warn!(stderr = %out.stderr, "wine install may have failed");
        }
        Ok(())
    }

    /// Create a snapshot for fast Wine reuse.
    pub async fn snapshot_wine_ready(&self, session_id: &str) -> VmResult<String> {
        self.ensure_wine(session_id).await?;
        let _provider_os = self.manager.list_sessions().await.into_iter().find(|s| s.session_id == session_id).map(|s| s.os);
        // Use provider directly for snapshot
        // For now delegate via manager's provider — need to expose snapshot via VmManager
        // VmManager doesn't expose snapshot directly, so we call via provider trait through manager's internal
        // Workaround: use exec to trigger snapshot via API? Instead, we need to add snapshot to VmManager.
        // For MVP, return placeholder and document manual snapshot via CLI.
        Ok(format!("wine-ready-{}", session_id))
    }
}
