use std::{net::SocketAddr, sync::Arc};

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use serde::Deserialize;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
};
use tracing::{info, warn, Level};

use crate::{
    config::OsKind,
    providers::CreateVmOpts,
    service::VmService,
    stream::InputEvent,
};

#[derive(Clone)]
struct AppState {
    service: Arc<VmService>,
    auth_url: String,
}

pub async fn serve_http(service: Arc<VmService>, addr: SocketAddr, auth_url: String) -> anyhow::Result<()> {
    let state = AppState { service, auth_url };
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/health", get(health))
        .route("/vm/create", post(create_vm))
        .route("/vm/list", get(list_vms))
        .route("/vm/:id/status", get(get_status))
        .route("/vm/:id", delete(destroy_vm))
        .route("/vm/:id/pause", post(pause_vm))
        .route("/vm/:id/resume", post(resume_vm))
        .route("/vm/:id/stream", get(get_stream))
        .route("/vm/:id/screenshot", get(screenshot))
        .route("/vm/:id/exec", post(exec_cmd))
        .route("/vm/:id/upload", post(upload_file))
        .route("/vm/:id/input", post(send_input))
        .route("/vm/:id/health", get(vm_health))
        .route("/vm/cleanup", post(cleanup_vms))
        .route("/ipc", post(ipc_bridge))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO).latency_unit(tower_http::LatencyUnit::Millis)),
        )
        .layer(cors)
        .with_state(state);

    info!(%addr, "sherd-vm HTTP listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok", "service": "sherd-vm" }))
}

#[derive(Debug, Deserialize)]
struct CreateBody {
    os: Option<String>,
    template: Option<String>,
    resolution: Option<String>,
    cpu: Option<u8>,
    #[serde(alias = "mem_mb", alias = "memMb")]
    mem_mb: Option<u32>,
    #[serde(alias = "timeout_ms", alias = "timeoutMs")]
    timeout_ms: Option<u64>,
    lifecycle: Option<String>,
    #[serde(alias = "from_snapshot", alias = "fromSnapshot")]
    from_snapshot: Option<String>,
}

async fn create_vm(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateBody>,
) -> impl IntoResponse {
    if let Err(e) = check_auth(&headers, &state.auth_url).await {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": e }))).into_response();
    }
    let os: OsKind = body.os.as_deref().unwrap_or("linux").parse().unwrap_or(OsKind::Linux);
    let opts = CreateVmOpts {
        os,
        template: body.template,
        resolution: body.resolution,
        cpu: body.cpu,
        mem_mb: body.mem_mb,
        timeout_ms: body.timeout_ms,
        lifecycle: body.lifecycle,
        from_snapshot: body.from_snapshot,
        volumes: None,
    };
    match state.service.create(opts).await {
        Ok(sess) => (StatusCode::CREATED, Json(serde_json::to_value(&sess).unwrap())).into_response(),
        Err(e) => {
            let msg = e.to_string();
            tracing::error!(error = %msg, "vm create failed");
            if msg.contains("ConcurrencyLimit") || msg.contains("concurrency") || msg.contains("429") {
                (StatusCode::TOO_MANY_REQUESTS, Json(serde_json::json!({ "error": msg }))).into_response()
            } else if msg.contains("NotImplemented") || msg.contains("not implemented") {
                (StatusCode::NOT_IMPLEMENTED, Json(serde_json::json!({ "error": msg }))).into_response()
            } else {
                (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": msg }))).into_response()
            }
        }
    }
}

async fn get_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = check_auth(&headers, &state.auth_url).await {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": e }))).into_response();
    }
    match state.service.status(&id).await {
        Ok(st) => (StatusCode::OK, Json(serde_json::to_value(&st).unwrap())).into_response(),
        Err(e) => (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

async fn destroy_vm(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = check_auth(&headers, &state.auth_url).await {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": e }))).into_response();
    }
    match state.service.destroy(&id).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

async fn pause_vm(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = check_auth(&headers, &state.auth_url).await {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": e }))).into_response();
    }
    match state.service.pause(&id).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

async fn resume_vm(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = check_auth(&headers, &state.auth_url).await {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": e }))).into_response();
    }
    match state.service.resume(&id).await {
        Ok(sess) => (StatusCode::OK, Json(serde_json::to_value(&sess).unwrap())).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

async fn get_stream(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = check_auth(&headers, &state.auth_url).await {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": e }))).into_response();
    }
    match state.service.stream_url(&id).await {
        Ok(url) => (StatusCode::OK, Json(serde_json::json!({ "url": url, "stream_url": url }))).into_response(),
        Err(e) => (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

async fn screenshot(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if let Err(e) = check_auth(&headers, &state.auth_url).await {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": e }))).into_response();
    }
    let format = params.get("format").map(|s| s.as_str()).unwrap_or("png");
    let quality = params.get("quality").and_then(|s| s.parse::<u8>().ok());
    match state.service.screenshot(&id, format, quality).await {
        Ok(bytes) => {
            let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);
            (StatusCode::OK, Json(serde_json::json!({ "data_base64": b64, "format": format }))).into_response()
        }
        Err(e) => (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct ExecBody {
    cmd: String,
    args: Option<Vec<String>>,
}

async fn exec_cmd(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ExecBody>,
) -> impl IntoResponse {
    if let Err(e) = check_auth(&headers, &state.auth_url).await {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": e }))).into_response();
    }
    match state.service.exec(&id, &body.cmd, body.args.unwrap_or_default()).await {
        Ok(out) => (StatusCode::OK, Json(serde_json::to_value(&out).unwrap())).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct UploadBody {
    path: String,
    content_base64: String,
}

async fn upload_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<UploadBody>,
) -> impl IntoResponse {
    if let Err(e) = check_auth(&headers, &state.auth_url).await {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": e }))).into_response();
    }
    let bytes = match base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &body.content_base64) {
        Ok(b) => b,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": format!("invalid base64: {}", e) }))).into_response(),
    };
    match state.service.upload(&id, &body.path, &bytes).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct InputBody {
    event: InputEvent,
}

async fn send_input(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<InputBody>,
) -> impl IntoResponse {
    if let Err(e) = check_auth(&headers, &state.auth_url).await {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": e }))).into_response();
    }
    match state.service.send_input(&id, body.event).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

async fn vm_health(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = check_auth(&headers, &state.auth_url).await {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": e }))).into_response();
    }
    match state.service.health(&id).await {
        Ok(h) => (StatusCode::OK, Json(serde_json::to_value(&h).unwrap())).into_response(),
        Err(e) => (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

async fn list_vms(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = check_auth(&headers, &state.auth_url).await {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": e }))).into_response();
    }
    let sessions = state.service.list().await;
    (StatusCode::OK, Json(serde_json::to_value(&sessions).unwrap())).into_response()
}

async fn ipc_bridge(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<crate::ipc::VmRequest>,
) -> impl IntoResponse {
    // Allow unauthenticated for health, but check for others if token present
    // For MVP, if Authorization header present, validate; else allow (desktop mock)
    if headers.contains_key("authorization") {
        if let Err(e) = check_auth(&headers, &state.auth_url).await {
            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": e }))).into_response();
        }
    }
    // Dispatch via service — reuse ipc:: dispatch logic by calling service directly
    // For simplicity, handle a few common cases
    let resp = match req {
        crate::ipc::VmRequest::Create(opts) => match state.service.create(opts).await {
            Ok(s) => crate::ipc::VmResponse::Created(s),
            Err(e) => crate::ipc::VmResponse::Error { message: e.to_string() },
        },
        crate::ipc::VmRequest::Status { session_id } => match state.service.status(&session_id).await {
            Ok(st) => crate::ipc::VmResponse::Status(st),
            Err(e) => crate::ipc::VmResponse::Error { message: e.to_string() },
        },
        crate::ipc::VmRequest::Get { session_id } => match state.service.get(&session_id).await {
            Ok(s) => crate::ipc::VmResponse::Session(s),
            Err(e) => crate::ipc::VmResponse::Error { message: e.to_string() },
        },
        crate::ipc::VmRequest::Destroy { session_id } => match state.service.destroy(&session_id).await {
            Ok(()) => crate::ipc::VmResponse::Ok,
            Err(e) => crate::ipc::VmResponse::Error { message: e.to_string() },
        },
        crate::ipc::VmRequest::List => {
            let sessions = state.service.list().await;
            crate::ipc::VmResponse::List(sessions)
        }
        crate::ipc::VmRequest::StreamUrl { session_id } => match state.service.stream_url(&session_id).await {
            Ok(url) => crate::ipc::VmResponse::StreamUrl { url },
            Err(e) => crate::ipc::VmResponse::Error { message: e.to_string() },
        },
        other => crate::ipc::VmResponse::Error { message: format!("ipc bridge: unhandled request {:?}", other) },
    };
    (StatusCode::OK, Json(serde_json::to_value(&resp).unwrap())).into_response()
}

fn is_auth_required() -> bool {
    std::env::var("SHERD_VM_REQUIRE_AUTH")
        .map(|v| v == "1" || v.to_ascii_lowercase() == "true")
        .unwrap_or(false)
}

async fn cleanup_vms(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = check_auth(&headers, &state.auth_url).await {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": e }))).into_response();
    }
    // List remote VMs via Solari and destroy them to free cap 0
    // Use the provider's list if available, else try direct Solari API
    let sessions = state.service.list().await;
    let mut destroyed = 0;
    let mut errors = vec![];
    // Also try to list remote via Solari client directly
    // For now, destroy tracked sessions; remote stale VMs need manual console cleanup
    for sess in &sessions {
        match state.service.destroy(&sess.session_id).await {
            Ok(()) => destroyed += 1,
            Err(e) => errors.push(format!("{}: {}", sess.session_id, e)),
        }
    }
    // Try to destroy the known stale sandbox from Solari listing
    // Attempt direct Solari API cleanup via service's provider
    // Fallback: try to destroy via raw Solari API if we can get the sandboxId
    let solari_key = std::env::var("SOLARI_API_KEY").unwrap_or_default();
    if !solari_key.is_empty() {
        let client = reqwest::Client::new();
        if let Ok(resp) = client
            .get("https://api.getsolari.com/sandboxes")
            .header("Authorization", format!("Bearer {}", solari_key))
            .send()
            .await
        {
            if let Ok(val) = resp.json::<serde_json::Value>().await {
                if let Some(arr) = val.get("sandboxes").and_then(|v| v.as_array()) {
                    for item in arr {
                        if let Some(id) = item.get("sandboxId").and_then(|v| v.as_str()) {
                            // Skip if already destroyed via tracked sessions
                            if sessions.iter().any(|s| s.session_id == id) {
                                continue;
                            }
                            let url = format!("https://api.getsolari.com/sandboxes/{}", id);
                            match client
                                .delete(&url)
                                .header("Authorization", format!("Bearer {}", solari_key))
                                .send()
                                .await
                            {
                                Ok(r) if r.status().is_success() => destroyed += 1,
                                Ok(r) => errors.push(format!("{}: {}", id, r.status())),
                                Err(e) => errors.push(format!("{}: {}", id, e)),
                            }
                        }
                    }
                }
            }
        }
    }
    (
        StatusCode::OK,
        Json(serde_json::json!({ "destroyed": destroyed, "errors": errors, "tracked": sessions.len() })),
    )
        .into_response()
}

async fn check_auth(headers: &HeaderMap, auth_url: &str) -> Result<(), String> {
    // Local demo: auth is optional unless SHERD_VM_REQUIRE_AUTH=1
    if !is_auth_required() {
        return Ok(());
    }
    let Some(auth) = headers.get("authorization").and_then(|v| v.to_str().ok()) else {
        return Err("missing Authorization: set SHERD_VM_REQUIRE_AUTH=0 for local demo or pass Bearer token".into());
    };
    if !auth.starts_with("Bearer ") {
        return Err("invalid Authorization header".into());
    }
    let token = auth.trim_start_matches("Bearer ").trim();
    if token.is_empty() {
        return Err("empty Bearer token".into());
    }
    // Validate via SHERD_AUTH_URL /auth/me — if auth service unreachable, allow (demo)
    let url = format!("{}/auth/me", auth_url.trim_end_matches('/'));
    let client = reqwest::Client::new();
    match client.get(&url).header("Authorization", format!("Bearer {}", token)).send().await {
        Ok(resp) if resp.status().is_success() => Ok(()),
        Ok(resp) if resp.status() == StatusCode::UNAUTHORIZED => Err("invalid or expired token".into()),
        Ok(_) => Ok(()), // other errors — allow for demo
        Err(e) => {
            warn!("auth check failed (allowing for demo): {}", e);
            Ok(())
        }
    }
}
