use thiserror::Error;

#[derive(Debug, Error)]
pub enum VmError {
    #[error("missing SOLARI_API_KEY")]
    MissingApiKey,

    #[error("invalid api key format: expected slr_live_...")]
    InvalidApiKeyFormat,

    #[error("solari error: {0}")]
    Solari(#[from] solari_desktop_rs::SolariError),

    #[error("platform error: {0}")]
    Platform(String),

    #[error("not implemented on this provider: {0}")]
    NotImplemented(String),

    #[error("unsupported: {0}")]
    Unsupported(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("concurrency limit exceeded: {0}")]
    ConcurrencyLimit(String),

    #[error("vm not ready: {0}")]
    NotReady(String),

    #[error("timeout after {0}ms: {1}")]
    Timeout(u64, String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("http error: {0}")]
    Http(String),

    #[error("auth error: {0}")]
    Auth(String),

    #[error("ipc error: {0}")]
    Ipc(String),

    #[error("{0}")]
    Other(String),
}

impl From<anyhow::Error> for VmError {
    fn from(e: anyhow::Error) -> Self {
        VmError::Other(e.to_string())
    }
}

impl From<reqwest::Error> for VmError {
    fn from(e: reqwest::Error) -> Self {
        VmError::Http(e.to_string())
    }
}

pub type VmResult<T> = Result<T, VmError>;
