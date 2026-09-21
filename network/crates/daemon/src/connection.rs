//! Per-client connection handling: decode one newline-delimited-JSON
//! `Request` per line, dispatch it onto `SherdService`, write back a
//! `Response`, and separately forward `SherdService` events for as long as
//! the connection is open.
//!
//! Reads and writes both go through `&Stream` (per `interprocess`'s tokio
//! API), so the connection is wrapped in an `Arc` to let the reader task and
//! the writer task each hold a `'static` handle to it. All outgoing bytes —
//! both request responses and pushed events — flow through one mpsc channel
//! into a single writer task, so two tasks never write to the socket at the
//! same time.

use std::sync::Arc;

use interprocess::local_socket::tokio::Stream;
use engine::{FeatureNotReady, SherdService};
use protocol::{decode_line, encode_line, Request, Response, ServerMessage};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

const OUTBOX_CAPACITY: usize = 32;

pub async fn handle(service: Arc<SherdService>, conn: Stream) {
    let conn = Arc::new(conn);
    let (outbox_tx, outbox_rx) = mpsc::channel::<String>(OUTBOX_CAPACITY);

    let writer = tokio::spawn(run_writer(Arc::clone(&conn), outbox_rx));
    let events = tokio::spawn(run_event_forwarder(service.subscribe(), outbox_tx.clone()));

    run_reader(&service, &conn, outbox_tx).await;

    // The reader loop exiting means the client is gone (or the connection
    // broke); tear down its two helper tasks rather than leaking them.
    events.abort();
    let _ = writer.await;
}

async fn run_writer(conn: Arc<Stream>, mut outbox_rx: mpsc::Receiver<String>) {
    let mut sender = &*conn;
    while let Some(line) = outbox_rx.recv().await {
        if sender.write_all(line.as_bytes()).await.is_err() {
            break;
        }
    }
}

async fn run_event_forwarder(
    mut events: tokio::sync::broadcast::Receiver<protocol::Event>,
    outbox_tx: mpsc::Sender<String>,
) {
    loop {
        match events.recv().await {
            Ok(event) => {
                let Ok(line) = encode_line(&ServerMessage::Event(event)) else { continue };
                if outbox_tx.send(line).await.is_err() {
                    break;
                }
            }
            // A slow client can miss events; skip past them rather than
            // stalling or erroring the whole connection.
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
}

async fn run_reader(service: &SherdService, conn: &Arc<Stream>, outbox_tx: mpsc::Sender<String>) {
    let mut lines = BufReader::new(&**conn).lines();
    loop {
        let line = match lines.next_line().await {
            Ok(Some(line)) => line,
            Ok(None) => break, // client closed the connection
            Err(e) => {
                tracing::warn!("client read error: {e}");
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }

        let response = match decode_line::<Request>(&line) {
            Ok(request) => dispatch(service, request).await,
            Err(e) => Response::Error { message: format!("could not parse request: {e}") },
        };

        let Ok(out) = encode_line(&ServerMessage::Response(response)) else { continue };
        if outbox_tx.send(out).await.is_err() {
            break;
        }
    }
}

async fn dispatch(service: &SherdService, request: Request) -> Response {
    match request {
        Request::Auto => Response::Auto(service.auto_connect().await),
        Request::Capability => ok_or_error(service.capability().await, Response::Capability),
        Request::Status => Response::Status(service.status().await),
        Request::HotspotStart { ssid, key } => {
            unit_ok_or_error(service.hotspot_start(&ssid, &key).await)
        }
        Request::HotspotStop => unit_ok_or_error(service.hotspot_stop().await),
        Request::StationConnect { ssid, key } => {
            unit_ok_or_error(service.station_connect(&ssid, &key).await)
        }
        Request::StationDisconnect => unit_ok_or_error(service.station_disconnect().await),
        Request::SendMessage { to, body } => {
            let FeatureNotReady(feature) = service.send_message(&to, &body);
            Response::NotYetImplemented { feature: feature.to_string() }
        }
        Request::SendFile { to, path } => {
            let FeatureNotReady(feature) = service.send_file(&to, &path);
            Response::NotYetImplemented { feature: feature.to_string() }
        }
    }
}

fn unit_ok_or_error(result: platform::PlatformResult<()>) -> Response {
    match result {
        Ok(()) => Response::Ok,
        Err(e) => Response::Error { message: e.to_string() },
    }
}

fn ok_or_error<T>(
    result: platform::PlatformResult<T>,
    to_response: impl FnOnce(T) -> Response,
) -> Response {
    match result {
        Ok(value) => to_response(value),
        Err(e) => Response::Error { message: e.to_string() },
    }
}
