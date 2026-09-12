//! Thin wrapper around shelling out to `netsh`. One place to run the
//! command and decode its output so every module handles it the same way.

use tokio::process::Command;

pub(crate) struct CommandOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Run `netsh <args>` and capture its output.
///
/// `netsh` writes its console output in the OEM codepage rather than UTF-8;
/// `from_utf8_lossy` will mangle non-ASCII characters (accented locale
/// strings) but every keyword this crate looks for is plain ASCII, so
/// parsing still works even when the surrounding text doesn't decode
/// perfectly.
pub(crate) async fn run_netsh(args: &[&str]) -> std::io::Result<CommandOutput> {
    let output = Command::new("netsh").args(args).output().await?;
    Ok(CommandOutput {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Value after the first `:` on a `key : value` style netsh output line,
/// trimmed. Case is preserved (SSIDs are case-sensitive) — callers that want
/// a case-insensitive comparison (e.g. matching "connected"/"started")
/// should lowercase the result themselves.
pub(crate) fn value_after_colon(line: &str) -> Option<String> {
    line.splitn(2, ':').nth(1).map(|v| v.trim().to_string())
}
