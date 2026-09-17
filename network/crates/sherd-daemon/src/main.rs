//! The sherd background service. Owns the platform backend, the domain
//! service, and a local IPC socket that the CLI (and later a GUI, or any
//! other app) talks to. See `sherd-protocol` for the wire format and
//! `network/README.md` / the plan for the overall architecture.

mod connection;

use std::sync::Arc;
use std::time::Duration;

use interprocess::local_socket::{tokio::prelude::*, GenericNamespaced, ListenerOptions};
#[cfg(windows)]
use interprocess::os::windows::local_socket::ListenerOptionsExt;
use sherd_core::{SherdConfig, SherdService};
use sherd_platform::{CapabilityLevel, LinkState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_logging();

    let backend = platform_backend();
    let config = SherdConfig::default();
    let device_ssid = config.device_ssid.clone();
    let watchdog_interval = config.watchdog_interval;
    let service = Arc::new(SherdService::new(backend, config));

    tracing::info!(%device_ssid, "starting sherd daemon");

    // Best-effort: get connected (join or host) right away, then keep
    // watching the link for as long as the daemon runs so a dropped
    // connection gets retried automatically instead of leaving the device
    // stranded until someone notices. See `supervise` below.
    {
        let service = Arc::clone(&service);
        tokio::spawn(supervise(service, watchdog_interval));
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

/// `tracing_subscriber::fmt::init()` with no `RUST_LOG` set only prints
/// `ERROR`-level events -- which this daemon almost never emits, since
/// failures (no capable adapter, no internet connection to tether from,
/// `netsh`/WinRT errors) are reported as `warn`/`info` or handed back as an
/// `AutoOutcome::Unavailable { reason }`. Launched by double-click (as
/// opposed to a terminal where someone thought to set `RUST_LOG=debug`),
/// that meant the console window came up and just sat there blank with no
/// way to tell *why* hosting or joining failed. Default to `info` instead so
/// startup, auto-connect outcomes, and every reconnect attempt are visible
/// out of the box; `RUST_LOG` still overrides this for more/less detail
/// (e.g. `RUST_LOG=debug` to also see the composite backend's WinRT-vs-netsh
/// fallback decisions).
fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

/// Keeps this device's contribution to the sherd mesh up for as long as the
/// daemon runs: an initial connect, then a standing health check that
/// re-runs `auto_connect` whenever what *must* be up isn't.
///
/// What counts as "must be up" depends on what this device is capable of,
/// because hosting is never optional on a capable device (see
/// `SherdService::auto_connect` -- every hosting node is what keeps the
/// mesh's range from shrinking to whichever one device happened to be
/// reachable): on a [`CapabilityLevel::FullMeshCapable`] adapter, only the
/// hotspot being `Up` counts as healthy, so if it drops for any reason
/// (someone turned Wi-Fi off, Windows tore it down, the uplink it was
/// tethering from went away) the very next tick restarts it -- the uplink
/// station link is a bonus and its being down doesn't by itself trigger
/// anything. On a station-only adapter there's no hotspot to keep up, so the
/// station link is what must stay `Up`; if the network this device joined
/// drops, the next tick re-scans and joins another sherd network if one is
/// visible. Either way a healthy device is left alone -- this never
/// interrupts a good connection or restarts a good hotspot just because
/// another sherd network came into range.
async fn supervise(service: Arc<SherdService>, interval: Duration) {
    let outcome = service.auto_connect().await;
    tracing::info!(?outcome, "startup auto-connect finished");

    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ticker.tick().await; // consume the immediate first tick; we just connected above

    loop {
        ticker.tick().await;

        let status = service.status().await;
        let is_up = |link: &Option<sherd_platform::LinkStatus>| {
            link.as_ref().is_some_and(|s| s.state == LinkState::Up)
        };
        let healthy = if matches!(status.capability.level, CapabilityLevel::FullMeshCapable) {
            is_up(&status.hotspot)
        } else {
            is_up(&status.station)
        };
        if healthy {
            continue;
        }

        tracing::warn!(?status.capability.level, "sherd link down; reconnecting");
        let outcome = service.auto_connect().await;
        tracing::info!(?outcome, "watchdog auto-connect finished");
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
