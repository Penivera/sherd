use interprocess::local_socket::{tokio::prelude::*, tokio::Stream, GenericNamespaced};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, warn};

use crate::daemon::protocol::{
    decode_line, encode_line, AutoOutcome, Request, Response, ServerMessage, StatusReport,
    SOCKET_NAME,
};

#[derive(Debug, Error)]
pub enum DaemonError {
    #[error("Could not reach sherd-daemon: {0} (is the daemon running?)")]
    NotReachable(String),
    #[error("I/O error during daemon communication: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Daemon reported an error: {0}")]
    Daemon(String),
    #[error("Unexpected daemon response: {0}")]
    UnexpectedResponse(String),
    #[error("Connection closed before response was received")]
    ConnectionClosed,
}

pub struct DaemonClient;

impl DaemonClient {
    pub async fn is_running() -> bool {
        Self::connect().await.is_ok()
    }

    async fn connect() -> Result<Stream, DaemonError> {
        let name = SOCKET_NAME
            .to_ns_name::<GenericNamespaced>()
            .map_err(|e| DaemonError::NotReachable(e.to_string()))?;

        Stream::connect(name)
            .await
            .map_err(|e| DaemonError::NotReachable(e.to_string()))
    }

    pub async fn send_request(request: Request) -> Result<Response, DaemonError> {
        let conn = Self::connect().await?;
        let mut recver = BufReader::new(&conn);
        let mut sender = &conn;

        let encoded = encode_line(&request)?;
        sender.write_all(encoded.as_bytes()).await?;

        let mut line = String::new();
        loop {
            line.clear();
            let bytes_read = recver.read_line(&mut line).await?;
            if bytes_read == 0 {
                return Err(DaemonError::ConnectionClosed);
            }

            match serde_json::from_str::<ServerMessage>(line.trim_end()) {
                Ok(ServerMessage::Response(response)) => return Ok(response),
                Ok(ServerMessage::Event(event)) => {
                    debug!(?event, "Received daemon event while waiting for response");
                }
                Err(err) => {
                    // Fall back to direct Response deserialization
                    if let Ok(resp) = decode_line::<Response>(&line) {
                        return Ok(resp);
                    }
                    warn!("Failed to parse daemon message: {}", err);
                }
            }
        }
    }

    pub async fn get_status() -> Result<StatusReport, DaemonError> {
        match Self::send_request(Request::Status).await? {
            Response::Status(report) => Ok(report),
            Response::Error { message } => Err(DaemonError::Daemon(message)),
            other => Err(DaemonError::UnexpectedResponse(format!("{:?}", other))),
        }
    }

    pub async fn auto_connect() -> Result<AutoOutcome, DaemonError> {
        match Self::send_request(Request::Auto).await? {
            Response::Auto(outcome) => Ok(outcome),
            Response::Error { message } => Err(DaemonError::Daemon(message)),
            other => Err(DaemonError::UnexpectedResponse(format!("{:?}", other))),
        }
    }
}
