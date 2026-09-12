//! The sherd background service. Owns the platform backend, the domain
//! service, and a local IPC socket that the CLI (and later a GUI, or any
//! other app) talks to. See `sherd-protocol` for the wire format and
//! `network/README.md` / the plan for the overall architecture.

mod connection;

use std::sync::Arc;

use interprocess::local_socket::{tokio::prelude::*, GenericNamespaced, ListenerOptions};
use sherd_core::{SherdConfig, SherdService};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let backend = platform_backend();
    let config = SherdConfig::default();
    let device_ssid = config.device_ssid.clone();
    let service = Arc::new(SherdService::new(backend, config));

    tracing::info!(%device_ssid, "starting sherd daemon");

    // Best-effort: try to get connected (join or host) right away, without
    // blocking the IPC server from coming up.
    {
        let service = Arc::clone(&service);
        tokio::spawn(async move {
            let outcome = service.auto_connect().await;
            tracing::info!(?outcome, "startup auto-connect finished");
        });
    }

    let name = sherd_protocol::SOCKET_NAME.to_ns_name::<GenericNamespaced>()?;
    let listener = match ListenerOptions::new().name(name).create_tokio() {
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            anyhow::bail!(
                "socket \"{}\" is already in use -- is another sherd-daemon already running?",
                sherd_protocol::SOCKET_NAME
            );
        }
        result => result?,
    };

    tracing::info!(socket = sherd_protocol::SOCKET_NAME, "listening for clients");

    loop {
        let conn = match listener.accept().await {
            Ok(conn) => conn,
            Err(e) => {
                tracing::warn!("incoming connection error: {e}");
                continue;
            }
        };
        let service = Arc::clone(&service);
        tokio::spawn(connection::handle(service, conn));
    }
}

#[cfg(windows)]
fn platform_backend() -> sherd_platform::PlatformBackend {
    sherd_platform_windows::backend()
}

#[cfg(not(windows))]
fn platform_backend() -> sherd_platform::PlatformBackend {
    sherd_platform_linux::backend()
}
