//! The sherd background service. Owns the platform backend, the domain
//! service, and a local IPC socket that the CLI (and later a GUI, or any
//! other app) talks to. See `sherd-protocol` for the wire format and
//! `network/README.md` / the plan for the overall architecture.

mod connection;

use std::sync::Arc;

use interprocess::local_socket::{tokio::prelude::*, GenericNamespaced, ListenerOptions};
#[cfg(windows)]
use interprocess::os::windows::local_socket::ListenerOptionsExt;
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
    let mut listener_options = ListenerOptions::new().name(name);
    #[cfg(windows)]
    {
        listener_options = listener_options.security_descriptor(pipe_security_descriptor()?);
    }
    let listener = match listener_options.create_tokio() {
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

/// A security descriptor granting any local process full access to the IPC
/// pipe, with its mandatory integrity label dropped to Low.
///
/// `sherd-daemon` often runs elevated (High integrity — needed for `netsh
/// wlan set/start hostednetwork`), but its clients (the CLI, a future GUI)
/// normally run as the plain logged-in user (Medium integrity). Windows'
/// default "no write-up" mandatory policy would then let a client open the
/// pipe for reading but silently deny it write access (seen live: `sherd
/// status` failed with "Access is denied" against an elevated daemon) — so
/// both the DACL (`D:(A;;GA;;;WD)`, Generic-All for Everyone) and the
/// mandatory label (`S:(ML;;NW;;;LW)`, Low with No-Write-Up) need setting
/// explicitly. This is the standard SDDL incantation for a pipe an elevated
/// service needs unprivileged local clients to reach.
#[cfg(windows)]
fn pipe_security_descriptor(
) -> anyhow::Result<interprocess::os::windows::security_descriptor::SecurityDescriptor> {
    use interprocess::os::windows::security_descriptor::SecurityDescriptor;
    use widestring::U16CString;

    let sddl = U16CString::from_str("D:(A;;GA;;;WD)S:(ML;;NW;;;LW)")
        .expect("static SDDL string contains no interior NUL");
    SecurityDescriptor::deserialize(sddl.as_ucstr())
        .map_err(|e| anyhow::anyhow!("failed to build pipe security descriptor: {e}"))
}

#[cfg(not(windows))]
fn platform_backend() -> sherd_platform::PlatformBackend {
    sherd_platform_linux::backend()
}
