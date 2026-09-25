use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum OsKind {
    Linux,
    Windows,
}

impl std::str::FromStr for OsKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "linux" | "ubuntu" => Ok(OsKind::Linux),
            "windows" | "win" => Ok(OsKind::Windows),
            _ => Err(format!("unknown os kind: {s} (expected linux|windows)")),
        }
    }
}

impl std::fmt::Display for OsKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OsKind::Linux => write!(f, "linux"),
            OsKind::Windows => write!(f, "windows"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SolariConfig {
    pub api_key: String,
    pub base_url: String,
    pub region: String,
    pub timeout_ms: u64,
    pub max_retries: usize,
    pub default_template: String,
    pub default_resolution: String,
    pub default_cpu: u8,
    pub default_mem_mb: u32,
    pub default_timeout_ms: u64,
    pub default_lifecycle: String, // "pause" | "kill"
}

impl SolariConfig {
    pub fn from_env() -> Result<Self, crate::error::VmError> {
        let api_key = std::env::var("SOLARI_API_KEY")
            .map_err(|_| crate::error::VmError::MissingApiKey)?;
        if api_key.is_empty() {
            return Err(crate::error::VmError::MissingApiKey);
        }
        let base_url = std::env::var("SOLARI_BASE_URL")
            .unwrap_or_else(|_| "https://api.getsolari.com".into());
        let region = std::env::var("SOLARI_REGION").unwrap_or_else(|_| "us-west".into());
        let default_template = std::env::var("SOLARI_TEMPLATE").unwrap_or_else(|_| "default".into());
        let default_resolution = std::env::var("SOLARI_RESOLUTION").unwrap_or_else(|_| "1280x720".into());
        Ok(Self {
            api_key,
            base_url: base_url.trim_end_matches('/').to_string(),
            region,
            timeout_ms: 90_000,
            max_retries: 5,
            default_template,
            default_resolution,
            default_cpu: 2,
            default_mem_mb: 4096,
            default_timeout_ms: 15 * 60 * 1000,
            default_lifecycle: "pause".into(),
        })
    }

    pub fn with_template(mut self, t: impl Into<String>) -> Self {
        self.default_template = t.into();
        self
    }
}

#[derive(Debug, Clone)]
pub struct SherdVmConfig {
    pub solari: SolariConfig,
    pub auth_url: String,
    pub ipc_socket: String,
    pub vm_socket: String,
    pub watchdog_interval: Duration,
}

impl SherdVmConfig {
    pub fn from_env() -> Result<Self, crate::error::VmError> {
        let solari = SolariConfig::from_env()?;
        let auth_url = std::env::var("SHERD_AUTH_URL")
            .or_else(|_| std::env::var("VITE_API_BASE_URL"))
            .unwrap_or_else(|_| "http://localhost:8000".into());
        let ipc_socket = std::env::var("SHERD_IPC_SOCKET").unwrap_or_else(|_| "sherd-ipc.sock".into());
        let vm_socket = std::env::var("SHERD_VM_SOCKET").unwrap_or_else(|_| "sherd-vm.sock".into());
        Ok(Self {
            solari,
            auth_url: auth_url.trim_end_matches('/').to_string(),
            ipc_socket,
            vm_socket,
            watchdog_interval: Duration::from_secs(15),
        })
    }

    pub fn auth_url(&self) -> &str {
        &self.auth_url
    }
}
