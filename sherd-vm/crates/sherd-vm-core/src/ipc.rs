use std::sync::Arc;

use interprocess::local_socket::{tokio::prelude::*, GenericNamespaced, ListenerOptions, ToNsName};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, info, warn};

use crate::{
    providers::CreateVmOpts,
    service::VmService,
    stream::InputEvent,
};

const VM_SOCKET_NAME: &str = "sherd-vm.sock";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum VmRequest {
    Create(CreateVmOpts),
    Get { session_id: String },
    Status { session_id: String },
    Destroy { session_id: String },
    Pause { session_id: String },
    Resume { session_id: String },
    Exec { session_id: String, cmd: String, args: Vec<String> },
    Upload { session_id: String, path: String, content_base64: String },
    Download { session_id: String, path: String },
    List,
    StreamUrl { session_id: String },
    Screenshot { session_id: String, format: Option<String>, quality: Option<u8> },
    Input { session_id: String, event: InputEvent },
    Health { session_id: String },
    // Auth-aware: client should send JWT for validation
    AuthCheck { token: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum VmResponse {
    Created(crate::providers::VmSession),
    Session(crate::providers::VmSession),
    Status(crate::providers::VmStatus),
    StreamUrl { url: String },
    Screenshot { data_base64: String },
    Exec(crate::providers::ExecOutput),
    Downloaded { content_base64: String },
    List(Vec<crate::providers::VmSession>),
    Health(crate::providers::VmHealth),
    Ok,
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum VmServerMessage {
    Response(VmResponse),
    Event(crate::service::VmEvent),
}

fn encode_line<T: Serialize>(value: &T) -> serde_json::Result<String> {
    let mut line = serde_json::to_string(value)?;
    line.push('\n');
    Ok(line)
}

fn decode_line<T: for<'de> Deserialize<'de>>(line: &str) -> serde_json::Result<T> {
    serde_json::from_str(line.trim_end())
}

pub async fn serve(service: Arc<VmService>, socket_name: Option<String>) -> anyhow::Result<()> {
    let name = socket_name.unwrap_or_else(|| VM_SOCKET_NAME.to_string());
    let ns_name = name.clone().to_ns_name::<GenericNamespaced>()?;
    let listener = match ListenerOptions::new().name(ns_name).create_tokio() {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            anyhow::bail!("VM IPC socket already in use ({}): {}", name, e);
        }
        Err(e) => return Err(e.into()),
    };
    info!(socket = %name, "sherd-vm IPC listening");
    loop {
        let conn = match listener.accept().await {
            Ok(c) => c,
            Err(e) => {
                warn!("accept failed: {}", e);
                continue;
            }
        };
        let svc = Arc::clone(&service);
        tokio::spawn(async move {
            if let Err(e) = handle_conn(svc, conn).await {
                debug!("connection ended: {}", e);
            }
        });
    }
}

async fn handle_conn(service: Arc<VmService>, conn: interprocess::local_socket::tokio::Stream) -> anyhow::Result<()> {
    let conn = Arc::new(conn);
    let (reader_half, writer_half) = (Arc::clone(&conn), Arc::clone(&conn));
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(32);

    // Writer task — single writer, like sherd-daemon/src/connection.rs
    let writer = tokio::spawn(async move {
        let mut writer = &*writer_half;
        while let Some(line) = rx.recv().await {
            if writer.write_all(line.as_bytes()).await.is_err() {
                break;
            }
        }
    });

    // Event forwarder
    let mut events = service.subscribe();
    let tx_events = tx.clone();
    let event_fwd = tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(ev) => {
                    let msg = VmServerMessage::Event(ev);
                    if let Ok(line) = encode_line(&msg) {
                        if tx_events.send(line).await.is_err() {
                            break;
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    warn!("event lagged, skipped {} events", n);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Reader loop
    let mut reader = BufReader::new(&*reader_half);
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            break;
        }
        let req: Result<VmRequest, _> = decode_line(&line);
        let resp = match req {
            Ok(r) => dispatch(&service, r).await,
            Err(e) => VmResponse::Error { message: format!("invalid request: {}", e) },
        };
        let msg = VmServerMessage::Response(resp);
        if let Ok(encoded) = encode_line(&msg) {
            if tx.send(encoded).await.is_err() {
                break;
            }
        }
    }

    event_fwd.abort();
    writer.abort();
    Ok(())
}

async fn dispatch(service: &VmService, req: VmRequest) -> VmResponse {
    match req {
        VmRequest::Create(opts) => match service.create(opts).await {
            Ok(s) => VmResponse::Created(s),
            Err(e) => VmResponse::Error { message: e.to_string() },
        },
        VmRequest::Get { session_id } => match service.get(&session_id).await {
            Ok(s) => VmResponse::Session(s),
            Err(e) => VmResponse::Error { message: e.to_string() },
        },
        VmRequest::Status { session_id } => match service.status(&session_id).await {
            Ok(st) => VmResponse::Status(st),
            Err(e) => VmResponse::Error { message: e.to_string() },
        },
        VmRequest::Destroy { session_id } => match service.destroy(&session_id).await {
            Ok(()) => VmResponse::Ok,
            Err(e) => VmResponse::Error { message: e.to_string() },
        },
        VmRequest::Pause { session_id } => match service.pause(&session_id).await {
            Ok(()) => VmResponse::Ok,
            Err(e) => VmResponse::Error { message: e.to_string() },
        },
        VmRequest::Resume { session_id } => match service.resume(&session_id).await {
            Ok(s) => VmResponse::Session(s),
            Err(e) => VmResponse::Error { message: e.to_string() },
        },
        VmRequest::Exec { session_id, cmd, args } => match service.exec(&session_id, &cmd, args).await {
            Ok(o) => VmResponse::Exec(o),
            Err(e) => VmResponse::Error { message: e.to_string() },
        },
        VmRequest::Upload { session_id, path, content_base64 } => {
            let bytes = match base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &content_base64) {
                Ok(b) => b,
                Err(e) => return VmResponse::Error { message: format!("invalid base64: {}", e) },
            };
            match service.upload(&session_id, &path, &bytes).await {
                Ok(()) => VmResponse::Ok,
                Err(e) => VmResponse::Error { message: e.to_string() },
            }
        }
        VmRequest::Download { session_id, path } => {
            // Use manager directly for fs_read
            match service.manager().fs_read(&session_id, &path).await {
                Ok(bytes) => {
                    let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);
                    VmResponse::Downloaded { content_base64: b64 }
                }
                Err(e) => VmResponse::Error { message: e.to_string() },
            }
        }
        VmRequest::List => {
            let sessions = service.list().await;
            VmResponse::List(sessions)
        }
        VmRequest::StreamUrl { session_id } => match service.stream_url(&session_id).await {
            Ok(url) => VmResponse::StreamUrl { url },
            Err(e) => VmResponse::Error { message: e.to_string() },
        },
        VmRequest::Screenshot { session_id, format, quality } => {
            let fmt = format.as_deref().unwrap_or("png");
            match service.screenshot(&session_id, fmt, quality).await {
                Ok(bytes) => {
                    let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);
                    VmResponse::Screenshot { data_base64: b64 }
                }
                Err(e) => VmResponse::Error { message: e.to_string() },
            }
        }
        VmRequest::Input { session_id, event } => match service.send_input(&session_id, event).await {
            Ok(()) => VmResponse::Ok,
            Err(e) => VmResponse::Error { message: e.to_string() },
        },
        VmRequest::Health { session_id } => match service.health(&session_id).await {
            Ok(h) => VmResponse::Health(h),
            Err(e) => VmResponse::Error { message: e.to_string() },
        },
        VmRequest::AuthCheck { token } => {
            // Validate JWT via SHERD_AUTH_URL /auth/me
            let auth_url = std::env::var("SHERD_AUTH_URL")
                .or_else(|_| std::env::var("VITE_API_BASE_URL"))
                .unwrap_or_else(|_| "http://localhost:8000".into());
            let url = format!("{}/auth/me", auth_url.trim_end_matches('/'));
            let client = reqwest::Client::new();
            match client.get(&url).header("Authorization", format!("Bearer {}", token)).send().await {
                Ok(resp) if resp.status().is_success() => VmResponse::Ok,
                Ok(resp) => VmResponse::Error { message: format!("auth failed: {}", resp.status()) },
                Err(e) => VmResponse::Error { message: format!("auth check error: {}", e) },
            }
        }
    }
}

// Client helper for CLI / desktop
pub async fn send_request(req: VmRequest, socket_name: Option<String>) -> anyhow::Result<VmResponse> {
    use interprocess::local_socket::tokio::Stream;
    let name = socket_name.unwrap_or_else(|| VM_SOCKET_NAME.to_string());
    let ns_name = name.to_ns_name::<GenericNamespaced>()?;
    let mut stream = Stream::connect(ns_name).await?;
    let line = encode_line(&req)?;
    stream.write_all(line.as_bytes()).await?;
    let mut reader = BufReader::new(&mut stream);
    let mut resp_line = String::new();
    loop {
        resp_line.clear();
        let n = reader.read_line(&mut resp_line).await?;
        if n == 0 {
            anyhow::bail!("connection closed before response");
        }
        let msg: VmServerMessage = decode_line(&resp_line)?;
        match msg {
            VmServerMessage::Response(r) => return Ok(r),
            VmServerMessage::Event(_) => continue, // skip events while awaiting response
        }
    }
}
