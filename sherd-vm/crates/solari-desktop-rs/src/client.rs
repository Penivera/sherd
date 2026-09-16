use std::time::Duration;

use reqwest::{header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE}, StatusCode};
use serde::de::DeserializeOwned;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::{
    error::SolariError,
    types::{CreateDesktopOpts, DesktopSession, ErrorBody, ExecRequest, ExecResponse, Health, StreamInfo},
};

const DEFAULT_BASE_URL: &str = "https://api.getsolari.com";
const DEFAULT_TIMEOUT_MS: u64 = 90_000;
const DEFAULT_MAX_RETRIES: usize = 5;
const INITIAL_BACKOFF_MS: u64 = 150;
const MAX_BACKOFF_MS: u64 = 8_000;

#[derive(Debug, Clone)]
pub struct ClientOptions {
    pub api_key: String,
    pub base_url: String,
    pub region: Option<String>,
    pub timeout_ms: u64,
    pub max_retries: usize,
}

impl ClientOptions {
    pub fn new(api_key: impl Into<String>, base_url: impl Into<String>) -> Result<Self, SolariError> {
        let key = api_key.into();
        if key.is_empty() {
            return Err(SolariError::MissingApiKey);
        }
        if !key.starts_with("slr_live_") {
            // Warn but allow — some test keys may differ
            warn!("api key does not start with slr_live_ — proceeding anyway");
        }
        let base = base_url.into();
        let base = if base.is_empty() { DEFAULT_BASE_URL.to_string() } else { base.trim_end_matches('/').to_string() };
        Ok(Self {
            api_key: key,
            base_url: base,
            region: Some("us-west".into()),
            timeout_ms: DEFAULT_TIMEOUT_MS,
            max_retries: DEFAULT_MAX_RETRIES,
        })
    }

    pub fn with_region(mut self, region: impl Into<String>) -> Self {
        self.region = Some(region.into());
        self
    }

    pub fn with_timeout(mut self, ms: u64) -> Self {
        self.timeout_ms = ms;
        self
    }

    pub fn with_max_retries(mut self, n: usize) -> Self {
        self.max_retries = n;
        self
    }
}

#[derive(Debug, Clone)]
pub struct DesktopClient {
    http: reqwest::Client,
    opts: ClientOptions,
}

impl DesktopClient {
    pub fn new(opts: ClientOptions) -> Result<Self, SolariError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_millis(opts.timeout_ms))
            .user_agent("solari-desktop-rs/0.1.0")
            .build()
            .map_err(|e| SolariError::Other(e.to_string()))?;
        Ok(Self { http, opts })
    }

    pub fn from_env() -> Result<Self, SolariError> {
        let key = std::env::var("SOLARI_API_KEY").map_err(|_| SolariError::MissingApiKey)?;
        let base = std::env::var("SOLARI_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        let opts = ClientOptions::new(key, base)?;
        Self::new(opts)
    }

    fn auth_header(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        let val = format!("Bearer {}", self.opts.api_key);
        headers.insert(AUTHORIZATION, HeaderValue::from_str(&val).unwrap());
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers
    }

    fn idempotency_key() -> String {
        Uuid::new_v4().to_string()
    }

    fn encode_id(id: &str) -> String {
        // URL-encode sessionId which contains : and .
        urlencoding::encode(id).to_owned()
    }

    async fn parse_error(&self, status: StatusCode, body: bytes::Bytes) -> SolariError {
        let text = String::from_utf8_lossy(&body).to_string();
        let parsed: Result<ErrorBody, _> = serde_json::from_slice(&body);
        let (code, retryable, message) = if let Ok(ref b) = parsed {
            (b.code.clone(), b.retryable.unwrap_or(false), b.message_or_error())
        } else {
            (None, false, text.clone())
        };
        let status_u16 = status.as_u16();
        match status_u16 {
            401 => SolariError::Unauthorized(message),
            402 => {
                if code.as_deref() == Some("InsufficientCredit") {
                    SolariError::InsufficientCredit(message)
                } else {
                    SolariError::Http { status: status_u16, code, message, retryable }
                }
            }
            403 => {
                if code.as_deref() == Some("NotEntitled") {
                    SolariError::NotEntitled(message)
                } else {
                    SolariError::Http { status: status_u16, code, message, retryable }
                }
            }
            404 => {
                if code.as_deref() == Some("InvalidSessionId") {
                    SolariError::InvalidSessionId(message)
                } else {
                    SolariError::NotFound(message)
                }
            }
            409 => {
                if code.as_deref() == Some("TemplateNotReady") {
                    SolariError::TemplateNotReady(message)
                } else if code.as_deref() == Some("TemplateKindMismatch") {
                    SolariError::TemplateKindMismatch(message)
                } else {
                    SolariError::Conflict { code: code.unwrap_or_else(|| "Conflict".into()), message }
                }
            }
            429 => {
                // Try to parse plan/cap from body
                let plan = parsed.as_ref().ok().and_then(|b| b.plan.clone()).unwrap_or_else(|| "unknown".into());
                let cap = parsed.as_ref().ok().and_then(|b| b.cap).unwrap_or(0);
                SolariError::ConcurrencyLimitExceeded { message, plan, cap }
            }
            400 => SolariError::BadRequest { code: code.unwrap_or_else(|| "BadRequest".into()), message },
            502 | 503 | 504 => SolariError::Transient { status: status_u16, message, retryable: true },
            _ if status.is_server_error() => SolariError::Transient { status: status_u16, message, retryable },
            _ => SolariError::Http { status: status_u16, code, message, retryable },
        }
    }

    async fn request_with_retry<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        url: &str,
        body: Option<serde_json::Value>,
        idempotency_key: Option<String>,
    ) -> Result<T, SolariError> {
        let mut attempt = 0usize;
        let mut backoff = INITIAL_BACKOFF_MS;
        loop {
            attempt += 1;
            let mut req = self.http.request(method.clone(), url).headers(self.auth_header());
            if let Some(ref key) = idempotency_key {
                req = req.header("Idempotency-Key", key);
            }
            if let Some(ref b) = body {
                req = req.json(b);
            }
            debug!(attempt, method = %method, url, "sending request");
            let resp = req.send().await;
            match resp {
                Ok(r) => {
                    let status = r.status();
                    let headers = r.headers().clone();
                    let bytes = r.bytes().await.map_err(|e| SolariError::Transport(e.to_string()))?;
                    if status.is_success() {
                        if headers.get("Idempotent-Replayed").is_some() {
                            debug!("idempotent replay detected");
                        }
                        // Handle empty body for DELETE
                        if bytes.is_empty() {
                            // Try to deserialize empty as unit or default
                            let val = serde_json::from_str::<T>("null").map_err(SolariError::Json)?;
                            return Ok(val);
                        }
                        let parsed = serde_json::from_slice::<T>(&bytes).map_err(SolariError::Json)?;
                        return Ok(parsed);
                    } else {
                        let err = self.parse_error(status, bytes).await;
                        if err.is_retryable() && attempt <= self.opts.max_retries {
                            let jitter = (rand_jitter() % 100) as u64;
                            let sleep = backoff + jitter;
                            warn!(attempt, status = %status, backoff_ms = sleep, "retryable error, backing off");
                            tokio::time::sleep(Duration::from_millis(sleep)).await;
                            backoff = (backoff * 2).min(MAX_BACKOFF_MS);
                            continue;
                        }
                        // 429 never retried
                        if err.is_concurrency_limit() {
                            return Err(err);
                        }
                        if attempt <= self.opts.max_retries && matches!(err, SolariError::Transient { .. }) {
                            let jitter = (rand_jitter() % 100) as u64;
                            let sleep = backoff + jitter;
                            warn!(attempt, backoff_ms = sleep, "transient, retrying");
                            tokio::time::sleep(Duration::from_millis(sleep)).await;
                            backoff = (backoff * 2).min(MAX_BACKOFF_MS);
                            continue;
                        }
                        return Err(err);
                    }
                }
                Err(e) => {
                    let err: SolariError = e.into();
                    if err.is_retryable() && attempt <= self.opts.max_retries {
                        let jitter = (rand_jitter() % 100) as u64;
                        let sleep = backoff + jitter;
                        warn!(attempt, backoff_ms = sleep, "transport retryable, backing off: {}", err);
                        tokio::time::sleep(Duration::from_millis(sleep)).await;
                        backoff = (backoff * 2).min(MAX_BACKOFF_MS);
                        continue;
                    }
                    // Transport errors are retryable up to max_retries
                    if attempt <= self.opts.max_retries && matches!(err, SolariError::Transport(_)) {
                        let jitter = (rand_jitter() % 100) as u64;
                        let sleep = backoff + jitter;
                        warn!(attempt, backoff_ms = sleep, "transport error, retrying: {}", err);
                        tokio::time::sleep(Duration::from_millis(sleep)).await;
                        backoff = (backoff * 2).min(MAX_BACKOFF_MS);
                        continue;
                    }
                    return Err(err);
                }
            }
        }
    }

    // --- Desktop lifecycle ---

    pub async fn create(&self, opts: CreateDesktopOpts) -> Result<DesktopSession, SolariError> {
        let url = format!("{}/desktops", self.opts.base_url);
        let body = serde_json::to_value(&opts).map_err(SolariError::Json)?;
        let key = Self::idempotency_key();
        // Try /desktops first, fallback to /sandboxes (Solari lists desktops under /sandboxes with kind: desktop), then /vms
        let res: Result<DesktopSession, SolariError> = self.request_with_retry(reqwest::Method::POST, &url, Some(body.clone()), Some(key.clone())).await;
        match res {
            Ok(s) => Ok(s),
            Err(SolariError::NotFound(_)) | Err(SolariError::Http { status: 404, .. }) => {
                let alt_url = format!("{}/sandboxes", self.opts.base_url);
                let res2: Result<DesktopSession, SolariError> = self.request_with_retry(reqwest::Method::POST, &alt_url, Some(body.clone()), Some(key.clone())).await;
                match res2 {
                    Ok(s) => Ok(s),
                    Err(SolariError::NotFound(_)) | Err(SolariError::Http { status: 404, .. }) => {
                        let alt_url2 = format!("{}/vms", self.opts.base_url);
                        self.request_with_retry(reqwest::Method::POST, &alt_url2, Some(body), Some(key)).await
                    }
                    Err(e) => Err(e),
                }
            }
            Err(e) => Err(e),
        }
    }

    pub async fn list(&self) -> Result<Vec<DesktopSession>, SolariError> {
        // Solari lists desktops under /sandboxes (kind: desktop) — try there first
        let url = format!("{}/sandboxes", self.opts.base_url);
        let res: Result<serde_json::Value, SolariError> = self.request_with_retry(reqwest::Method::GET, &url, None, None).await;
        match res {
            Ok(v) => {
                if let Some(arr) = v.get("sandboxes").and_then(|x| x.as_array()) {
                    let sessions: Vec<DesktopSession> = serde_json::from_value(serde_json::Value::Array(arr.clone())).unwrap_or_default();
                    return Ok(sessions);
                }
                if let Some(arr) = v.as_array() {
                    let sessions: Vec<DesktopSession> = serde_json::from_value(v).unwrap_or_default();
                    return Ok(sessions);
                }
                Ok(vec![])
            }
            Err(e) => Err(e),
        }
    }

    pub async fn get(&self, id: &str) -> Result<DesktopSession, SolariError> {
        let enc = Self::encode_id(id);
        let url = format!("{}/desktops/{}", self.opts.base_url, enc);
        let res: Result<DesktopSession, SolariError> = self.request_with_retry(reqwest::Method::GET, &url, None, None).await;
        match res {
            Ok(s) => Ok(s),
            Err(SolariError::NotFound(_)) | Err(SolariError::Http { status: 404, .. }) => {
                let alt = format!("{}/vms/{}", self.opts.base_url, enc);
                self.request_with_retry(reqwest::Method::GET, &alt, None, None).await
            }
            Err(e) => Err(e),
        }
    }

    pub async fn destroy(&self, id: &str) -> Result<(), SolariError> {
        let enc = Self::encode_id(id);
        let url = format!("{}/desktops/{}", self.opts.base_url, enc);
        let res: Result<serde_json::Value, SolariError> = self.request_with_retry(reqwest::Method::DELETE, &url, None, None).await;
        match res {
            Ok(_) => Ok(()),
            Err(SolariError::NotFound(_)) | Err(SolariError::Http { status: 404, .. }) => {
                let alt = format!("{}/sandboxes/{}", self.opts.base_url, enc);
                let alt_res: Result<serde_json::Value, SolariError> = self.request_with_retry(reqwest::Method::DELETE, &alt, None, None).await;
                match alt_res {
                    Ok(_) => Ok(()),
                    Err(SolariError::NotFound(_)) | Err(SolariError::Http { status: 404, .. }) => {
                        let alt2 = format!("{}/vms/{}", self.opts.base_url, enc);
                        let alt_res2: Result<serde_json::Value, SolariError> = self.request_with_retry(reqwest::Method::DELETE, &alt2, None, None).await;
                        match alt_res2 {
                            Ok(_) => Ok(()),
                            Err(SolariError::NotFound(_)) | Err(SolariError::Http { status: 404, .. }) => Ok(()),
                            Err(e) => Err(e),
                        }
                    }
                    Err(e) => Err(e),
                }
            }
            Err(e) => Err(e),
        }
    }

    pub async fn health(&self, id: &str) -> Result<Health, SolariError> {
        let enc = Self::encode_id(id);
        let url = format!("{}/desktops/{}/health", self.opts.base_url, enc);
        let res: Result<Health, SolariError> = self.request_with_retry(reqwest::Method::GET, &url, None, None).await;
        match res {
            Ok(h) => Ok(h),
            Err(SolariError::NotFound(_)) | Err(SolariError::Http { status: 404, .. }) => {
                let alt = format!("{}/sandboxes/{}/health", self.opts.base_url, enc);
                let res2: Result<Health, SolariError> = self.request_with_retry(reqwest::Method::GET, &alt, None, None).await;
                match res2 {
                    Ok(h) => Ok(h),
                    Err(SolariError::NotFound(_)) | Err(SolariError::Http { status: 404, .. }) => {
                        let alt2 = format!("{}/vms/{}/health", self.opts.base_url, enc);
                        self.request_with_retry(reqwest::Method::GET, &alt2, None, None).await
                    }
                    Err(e) => Err(e),
                }
            }
            Err(e) => Err(e),
        }
    }

    pub async fn pause(&self, id: &str) -> Result<(), SolariError> {
        let enc = Self::encode_id(id);
        let url = format!("{}/desktops/{}/pause", self.opts.base_url, enc);
        let res: Result<serde_json::Value, SolariError> = self.request_with_retry(reqwest::Method::POST, &url, Some(serde_json::json!({})), None).await;
        match res {
            Ok(_) => Ok(()),
            Err(SolariError::NotFound(_)) | Err(SolariError::Http { status: 404, .. }) => {
                let alt = format!("{}/vms/{}/pause", self.opts.base_url, enc);
                self.request_with_retry::<serde_json::Value>(reqwest::Method::POST, &alt, Some(serde_json::json!({})), None).await.map(|_| ())
            }
            Err(e) => Err(e),
        }
    }

    pub async fn resume(&self, id: &str) -> Result<DesktopSession, SolariError> {
        let enc = Self::encode_id(id);
        let url = format!("{}/desktops/{}/resume", self.opts.base_url, enc);
        let res: Result<DesktopSession, SolariError> = self.request_with_retry(reqwest::Method::POST, &url, Some(serde_json::json!({})), None).await;
        match res {
            Ok(s) => Ok(s),
            Err(SolariError::NotFound(_)) | Err(SolariError::Http { status: 404, .. }) => {
                let alt = format!("{}/vms/{}/resume", self.opts.base_url, enc);
                self.request_with_retry(reqwest::Method::POST, &alt, Some(serde_json::json!({})), None).await
            }
            Err(e) => Err(e),
        }
    }

    pub async fn set_timeout(&self, id: &str, timeout_ms: u64) -> Result<(), SolariError> {
        let enc = Self::encode_id(id);
        let url = format!("{}/desktops/{}/timeout", self.opts.base_url, enc);
        let body = serde_json::json!({ "timeoutMs": timeout_ms });
        let res: Result<serde_json::Value, SolariError> = self.request_with_retry(reqwest::Method::POST, &url, Some(body.clone()), None).await;
        match res {
            Ok(_) => Ok(()),
            Err(SolariError::NotFound(_)) | Err(SolariError::Http { status: 404, .. }) => {
                let alt = format!("{}/vms/{}/timeout", self.opts.base_url, enc);
                self.request_with_retry::<serde_json::Value>(reqwest::Method::POST, &alt, Some(body), None).await.map(|_| ())
            }
            Err(e) => Err(e),
        }
    }

    // --- Control plane ---

    pub async fn exec(&self, id: &str, cmd: &str, args: Vec<String>) -> Result<ExecResponse, SolariError> {
        let enc = Self::encode_id(id);
        let url = format!("{}/desktops/{}/exec", self.opts.base_url, enc);
        let body = serde_json::to_value(ExecRequest { cmd: cmd.into(), args, cwd: None, env: None }).unwrap();
        let res: Result<ExecResponse, SolariError> = self.request_with_retry(reqwest::Method::POST, &url, Some(body.clone()), None).await;
        match res {
            Ok(r) => Ok(r),
            Err(SolariError::NotFound(_)) | Err(SolariError::Http { status: 404, .. }) => {
                let alt = format!("{}/vms/{}/exec", self.opts.base_url, enc);
                self.request_with_retry(reqwest::Method::POST, &alt, Some(body), None).await
            }
            Err(e) => Err(e),
        }
    }

    pub async fn fs_write(&self, id: &str, path: &str, content: &[u8]) -> Result<(), SolariError> {
        let enc = Self::encode_id(id);
        // Try JSON base64 first
        let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, content);
        let url = format!("{}/desktops/{}/fs/write", self.opts.base_url, enc);
        let body = serde_json::json!({ "path": path, "content": b64, "encoding": "base64" });
        let res: Result<serde_json::Value, SolariError> = self.request_with_retry(reqwest::Method::POST, &url, Some(body.clone()), None).await;
        match res {
            Ok(_) => Ok(()),
            Err(SolariError::NotFound(_)) | Err(SolariError::Http { status: 404, .. }) => {
                // Fallback to raw octet-stream upload via presigned URL if available
                // For now try alt path
                let alt = format!("{}/vms/{}/fs/write", self.opts.base_url, enc);
                self.request_with_retry::<serde_json::Value>(reqwest::Method::POST, &alt, Some(body), None).await.map(|_| ())
            }
            Err(e) => Err(e),
        }
    }

    pub async fn fs_read(&self, id: &str, path: &str) -> Result<Vec<u8>, SolariError> {
        let enc = Self::encode_id(id);
        let url = format!("{}/desktops/{}/fs/read", self.opts.base_url, enc);
        let body = serde_json::json!({ "path": path });
        let res: Result<serde_json::Value, SolariError> = self.request_with_retry(reqwest::Method::POST, &url, Some(body.clone()), None).await;
        let val = match res {
            Ok(v) => v,
            Err(SolariError::NotFound(_)) | Err(SolariError::Http { status: 404, .. }) => {
                let alt = format!("{}/vms/{}/fs/read", self.opts.base_url, enc);
                self.request_with_retry::<serde_json::Value>(reqwest::Method::POST, &alt, Some(body), None).await?
            }
            Err(e) => return Err(e),
        };
        // Try to extract content field
        if let Some(s) = val.get("content").and_then(|v| v.as_str()) {
            // Try base64 decode, fallback to raw
            if let Ok(bytes) = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, s) {
                return Ok(bytes);
            }
            return Ok(s.as_bytes().to_vec());
        }
        if let Some(s) = val.get("data").and_then(|v| v.as_str()) {
            if let Ok(bytes) = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, s) {
                return Ok(bytes);
            }
            return Ok(s.as_bytes().to_vec());
        }
        Err(SolariError::Other(format!("unexpected fs_read response: {}", val)))
    }

    pub async fn fs_list(&self, id: &str, path: &str) -> Result<Vec<crate::types::FsListEntry>, SolariError> {
        let enc = Self::encode_id(id);
        let url = format!("{}/desktops/{}/fs/list", self.opts.base_url, enc);
        let body = serde_json::json!({ "path": path });
        let res: Result<serde_json::Value, SolariError> = self.request_with_retry(reqwest::Method::POST, &url, Some(body.clone()), None).await;
        let val = match res {
            Ok(v) => v,
            Err(SolariError::NotFound(_)) | Err(SolariError::Http { status: 404, .. }) => {
                let alt = format!("{}/vms/{}/fs/list", self.opts.base_url, enc);
                self.request_with_retry::<serde_json::Value>(reqwest::Method::POST, &alt, Some(body), None).await?
            }
            Err(e) => return Err(e),
        };
        // Response may be {entries: [...]} or direct array
        if let Some(arr) = val.get("entries").and_then(|v| v.as_array()) {
            let entries: Vec<crate::types::FsListEntry> = serde_json::from_value(serde_json::Value::Array(arr.clone())).map_err(SolariError::Json)?;
            return Ok(entries);
        }
        if let Some(arr) = val.as_array() {
            let entries: Vec<crate::types::FsListEntry> = serde_json::from_value(serde_json::Value::Array(arr.clone())).map_err(SolariError::Json)?;
            return Ok(entries);
        }
        Err(SolariError::Other(format!("unexpected fs_list response: {}", val)))
    }

    pub async fn screenshot(&self, id: &str, format: &str, quality: Option<u8>) -> Result<Vec<u8>, SolariError> {
        let enc = Self::encode_id(id);
        let mut url = format!("{}/desktops/{}/screenshot?format={}", self.opts.base_url, enc, format);
        if let Some(q) = quality {
            url.push_str(&format!("&quality={}", q));
        }
        // Screenshot returns raw bytes or JSON with base64
        let req = self.http.get(&url).headers(self.auth_header());
        let resp = req.send().await.map_err(SolariError::from)?;
        let status = resp.status();
        if !status.is_success() {
            let bytes = resp.bytes().await.map_err(|e| SolariError::Transport(e.to_string()))?;
            return Err(self.parse_error(status, bytes).await);
        }
        let content_type = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
        let bytes = resp.bytes().await.map_err(|e| SolariError::Transport(e.to_string()))?;
        if content_type.contains("application/json") {
            let val: serde_json::Value = serde_json::from_slice(&bytes).map_err(SolariError::Json)?;
            if let Some(b64) = val.get("data").and_then(|v| v.as_str()).or_else(|| val.get("image").and_then(|v| v.as_str())).or_else(|| val.get("png").and_then(|v| v.as_str())) {
                let decoded = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64).map_err(|e| SolariError::Other(e.to_string()))?;
                return Ok(decoded);
            }
            return Err(SolariError::Other(format!("unexpected screenshot json: {}", val)));
        }
        Ok(bytes.to_vec())
    }

    pub async fn stream_url(&self, id: &str) -> Result<StreamInfo, SolariError> {
        let enc = Self::encode_id(id);
        let url = format!("{}/desktops/{}/stream", self.opts.base_url, enc);
        let res: Result<StreamInfo, SolariError> = self.request_with_retry(reqwest::Method::POST, &url, Some(serde_json::json!({})), None).await;
        match res {
            Ok(s) => Ok(s),
            Err(SolariError::NotFound(_)) | Err(SolariError::Http { status: 404, .. }) => {
                // Fallback: GET session and use stream_url field
                let sess = self.get(id).await?;
                if let Some(u) = sess.stream_url {
                    Ok(StreamInfo { stream_url: u, token: None })
                } else {
                    Err(SolariError::Other("no streamUrl available".into()))
                }
            }
            Err(e) => Err(e),
        }
    }

    pub async fn mouse_move(&self, id: &str, x: u32, y: u32, humanize: bool) -> Result<(), SolariError> {
        let enc = Self::encode_id(id);
        let url = format!("{}/desktops/{}/mouse/move", self.opts.base_url, enc);
        let body = serde_json::json!({ "x": x, "y": y, "humanize": humanize });
        self.request_with_retry::<serde_json::Value>(reqwest::Method::POST, &url, Some(body), None).await.map(|_| ())
    }

    pub async fn mouse_click(&self, id: &str, x: u32, y: u32, button: &str, humanize: bool) -> Result<(), SolariError> {
        let enc = Self::encode_id(id);
        let url = format!("{}/desktops/{}/mouse/click", self.opts.base_url, enc);
        let body = serde_json::json!({ "x": x, "y": y, "button": button, "humanize": humanize });
        self.request_with_retry::<serde_json::Value>(reqwest::Method::POST, &url, Some(body), None).await.map(|_| ())
    }

    pub async fn keyboard_type(&self, id: &str, text: &str) -> Result<(), SolariError> {
        let enc = Self::encode_id(id);
        let url = format!("{}/desktops/{}/keyboard/type", self.opts.base_url, enc);
        let body = serde_json::json!({ "text": text });
        self.request_with_retry::<serde_json::Value>(reqwest::Method::POST, &url, Some(body), None).await.map(|_| ())
    }

    pub async fn keyboard_press(&self, id: &str, keys: Vec<String>) -> Result<(), SolariError> {
        let enc = Self::encode_id(id);
        let url = format!("{}/desktops/{}/keyboard/press", self.opts.base_url, enc);
        let body = serde_json::json!({ "keys": keys });
        self.request_with_retry::<serde_json::Value>(reqwest::Method::POST, &url, Some(body), None).await.map(|_| ())
    }

    pub async fn display_set(&self, id: &str, width: u32, height: u32) -> Result<(), SolariError> {
        let enc = Self::encode_id(id);
        let url = format!("{}/desktops/{}/display", self.opts.base_url, enc);
        let body = serde_json::json!({ "width": width, "height": height });
        self.request_with_retry::<serde_json::Value>(reqwest::Method::POST, &url, Some(body), None).await.map(|_| ())
    }

    pub async fn open_app(&self, id: &str, app: &str, args: Vec<String>) -> Result<String, SolariError> {
        let enc = Self::encode_id(id);
        let url = format!("{}/desktops/{}/open", self.opts.base_url, enc);
        let body = serde_json::json!({ "app": app, "args": args });
        let val: serde_json::Value = self.request_with_retry(reqwest::Method::POST, &url, Some(body), None).await?;
        Ok(val.get("pid").and_then(|v| v.as_str()).unwrap_or("").to_string())
    }

    pub async fn clipboard_set(&self, id: &str, text: &str) -> Result<(), SolariError> {
        let enc = Self::encode_id(id);
        let url = format!("{}/desktops/{}/clipboard", self.opts.base_url, enc);
        let body = serde_json::json!({ "text": text });
        self.request_with_retry::<serde_json::Value>(reqwest::Method::POST, &url, Some(body), None).await.map(|_| ())
    }

    pub async fn clipboard_get(&self, id: &str) -> Result<String, SolariError> {
        let enc = Self::encode_id(id);
        let url = format!("{}/desktops/{}/clipboard", self.opts.base_url, enc);
        let val: serde_json::Value = self.request_with_retry(reqwest::Method::GET, &url, None, None).await?;
        Ok(val.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string())
    }

    pub async fn process_list(&self, id: &str) -> Result<serde_json::Value, SolariError> {
        let enc = Self::encode_id(id);
        let url = format!("{}/desktops/{}/process/list", self.opts.base_url, enc);
        self.request_with_retry(reqwest::Method::GET, &url, None, None).await
    }

    pub async fn process_kill(&self, id: &str, pid: &str) -> Result<(), SolariError> {
        let enc = Self::encode_id(id);
        let url = format!("{}/desktops/{}/process/kill", self.opts.base_url, enc);
        let body = serde_json::json!({ "pid": pid });
        self.request_with_retry::<serde_json::Value>(reqwest::Method::POST, &url, Some(body), None).await.map(|_| ())
    }

    pub async fn snapshot(&self, id: &str, name: Option<String>) -> Result<String, SolariError> {
        let enc = Self::encode_id(id);
        let url = format!("{}/desktops/{}/snapshot", self.opts.base_url, enc);
        let body = serde_json::json!({ "name": name });
        let val: serde_json::Value = self.request_with_retry(reqwest::Method::POST, &url, Some(body), None).await?;
        Ok(val.get("snapshotId").and_then(|v| v.as_str()).or_else(|| val.get("id").and_then(|v| v.as_str())).unwrap_or("").to_string())
    }

    pub async fn revert(&self, id: &str, snapshot_id: &str) -> Result<(), SolariError> {
        let enc = Self::encode_id(id);
        let url = format!("{}/desktops/{}/revert", self.opts.base_url, enc);
        let body = serde_json::json!({ "snapshotId": snapshot_id });
        self.request_with_retry::<serde_json::Value>(reqwest::Method::POST, &url, Some(body), None).await.map(|_| ())
    }
}

fn rand_jitter() -> u64 {
    // Simple jitter using system time nanos
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().subsec_nanos() as u64 % 100
}

// Helper for urlencoding without extra dep — use percent-encoding
mod urlencoding {
    pub fn encode(input: &str) -> String {
        let mut out = String::new();
        for b in input.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
                _ => out.push_str(&format!("%{:02X}", b)),
            }
        }
        out
    }
}
