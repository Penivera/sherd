use thiserror::Error;

#[derive(Debug, Error)]
pub enum SolariError {
    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("not entitled: {0}")]
    NotEntitled(String),

    #[error("insufficient credit: {0}")]
    InsufficientCredit(String),

    #[error("concurrency limit exceeded (cap {cap} on plan {plan}): {message}")]
    ConcurrencyLimitExceeded { message: String, plan: String, cap: u32 },

    #[error("template not ready: {0}")]
    TemplateNotReady(String),

    #[error("template kind mismatch: {0}")]
    TemplateKindMismatch(String),

    #[error("invalid session id: {0}")]
    InvalidSessionId(String),

    #[error("resource conflict (409): {code}: {message}")]
    Conflict { code: String, message: String },

    #[error("bad request (400): {code}: {message}")]
    BadRequest { code: String, message: String },

    #[error("not found (404): {0}")]
    NotFound(String),

    #[error("plan limit exceeded: {0}")]
    PlanLimitExceeded(String),

    #[error("feature requires plan {feature} on {plan}: {message}")]
    FeatureRequiresPlan { feature: String, plan: String, message: String },

    #[error("transient error ({status}): {message} (retryable={retryable})")]
    Transient { status: u16, message: String, retryable: bool },

    #[error("http error {status}: {message} (code={code:?})")]
    Http { status: u16, code: Option<String>, message: String, retryable: bool },

    #[error("transport error: {0}")]
    Transport(String),

    #[error("timeout after {0}ms")]
    Timeout(u64),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("reqwest error: {0}")]
    Reqwest(String),

    #[error("websocket error: {0}")]
    WebSocket(String),

    #[error("missing api key")]
    MissingApiKey,

    #[error("invalid api key format: expected slr_live_...")]
    InvalidApiKeyFormat,

    #[error("vm not ready: {0}")]
    NotReady(String),

    #[error("{0}")]
    Other(String),
}

impl SolariError {
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            SolariError::Transient { retryable: true, .. }
                | SolariError::Http { retryable: true, .. }
        )
    }

    pub fn is_concurrency_limit(&self) -> bool {
        matches!(self, SolariError::ConcurrencyLimitExceeded { .. })
    }
}

impl From<reqwest::Error> for SolariError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            SolariError::Timeout(0)
        } else if e.is_connect() || e.is_request() {
            SolariError::Transport(e.to_string())
        } else {
            SolariError::Reqwest(e.to_string())
        }
    }
}
