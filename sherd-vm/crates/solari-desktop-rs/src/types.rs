use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Lifecycle {
    #[serde(rename = "onTimeout", skip_serializing_if = "Option::is_none")]
    pub on_timeout: Option<String>, // "pause" | "kill"
}

impl Lifecycle {
    pub fn pause() -> Self {
        Self { on_timeout: Some("pause".into()) }
    }
    pub fn kill() -> Self {
        Self { on_timeout: Some("kill".into()) }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CreateDesktopOpts {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<u8>,
    #[serde(rename = "memMb", skip_serializing_if = "Option::is_none")]
    pub mem_mb: Option<u32>,
    #[serde(rename = "timeoutMs", skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lifecycle: Option<Lifecycle>,
    #[serde(rename = "fromSnapshot", skip_serializing_if = "Option::is_none")]
    pub from_snapshot: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volumes: Option<Vec<VolumeMount>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeMount {
    #[serde(rename = "volumeId")]
    pub volume_id: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesktopSession {
    #[serde(alias = "sessionId", alias = "desktopId", alias = "vmId", alias = "id")]
    pub session_id: String,
    #[serde(alias = "streamUrl", alias = "stream_url")]
    pub stream_url: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub template: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Health {
    pub ready: bool,
    #[serde(default)]
    pub display: Option<DisplayInfo>,
    #[serde(default)]
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
pub struct ExecRequest {
    pub cmd: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecResponse {
    #[serde(rename = "exitCode", alias = "exit_code")]
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsWriteRequest {
    pub path: String,
    pub content: String, // base64 or plain text depending on endpoint
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsReadResponse {
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsListEntry {
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub is_dir: bool,
    #[serde(default)]
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenshotOpts {
    #[serde(default = "default_format")]
    pub format: String, // "png" | "jpeg"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<u8>,
}

fn default_format() -> String { "png".into() }

impl Default for ScreenshotOpts {
    fn default() -> Self {
        Self { format: "png".into(), quality: None }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MouseOpts {
    pub x: u32,
    pub y: u32,
    #[serde(default)]
    pub humanize: bool,
    #[serde(default)]
    pub button: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardOpts {
    pub text: Option<String>,
    pub keys: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamInfo {
    #[serde(alias = "streamUrl", alias = "url")]
    pub stream_url: String,
    #[serde(default)]
    pub token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorBody {
    pub error: Option<String>,
    pub message: Option<String>,
    pub code: Option<String>,
    pub retryable: Option<bool>,
    pub plan: Option<String>,
    pub cap: Option<u32>,
    pub feature: Option<String>,
    pub detail: Option<String>,
}

impl ErrorBody {
    pub fn message_or_error(&self) -> String {
        self.error.clone()
            .or_else(|| self.message.clone())
            .unwrap_or_else(|| "unknown error".into())
    }
}
