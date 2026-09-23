//! The sherd background service. Owns the platform backend, the domain
//! service, and a local IPC socket that the CLI (and later a GUI, or any
//! other app) talks to. See `protocol` for the wire format and the repo
//! README for how it all fits together.

mod connection;

use std::sync::Arc;
use std::time::Duration;

use engine::{SherdConfig, SherdService};
use interprocess::local_socket::{tokio::prelude::*, GenericNamespaced, ListenerOptions};
#[cfg(windows)]
use interprocess::os::windows::local_socket::ListenerOptionsExt;
use platform::CapabilityLevel;
use protocol::AutoOutcome;

/// Longest the watchdog waits between attempts while something keeps
/// failing. Retries start at the normal check interval and double from
/// there, so a PC that can't host doesn't retry (and log) every 15 seconds
/// forever -- but still recovers within a few minutes once the problem is
/// fixed.
const MAX_RETRY_DELAY: Duration = Duration::from_secs(300);

/// How long shutdown gets to turn the hotspot off. Closing the console
/// window gives a program about 5 seconds before Windows kills it outright.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(4);

#[tokio::main]
async fn main() {
    init_logging();
    if let Err(e) = run().await {
        tracing::error!("{e:#}");
        pause_if_own_window();
        std::process::exit(1);
    }
}

async fn run() -> anyhow::Result<()> {
    let config = SherdConfig::default();
    let watchdog_interval = config.watchdog_interval;
    let service = Arc::new(SherdService::new(platform_backend(), config).await?);

    // Claim the IPC socket before touching the hotspot, so a second copy
    // of the daemon bows out instead of fighting the first one over it.
    let listener = create_ipc_listener()?;

    tracing::info!(
        "Sherd is running on this device as \"{}\" (ID {}).",
        service.display_name(),
        engine::identity::short_id(service.device_id())
    );
    tracing::info!(
        "Its hotspot is called \"{}\". Keep this window open (minimizing is fine) -- closing it turns \
         the hotspot off and stops Sherd.",
        service.config().device_ssid
    );
    warn_if_not_elevated();

    tokio::spawn(supervise(Arc::clone(&service), watchdog_interval));
    tokio::spawn(Arc::clone(&service).run_messaging());

    let reason = tokio::select! {
        _ = accept_clients(listener, Arc::clone(&service)) => "the local command listener stopped",
        reason = shutdown_signal() => reason,
    };

    tracing::info!("Stopping Sherd ({reason})...");
    if tokio::time::timeout(SHUTDOWN_GRACE, service.shutdown()).await.is_err() {
        tracing::warn!(
            "Timed out turning the hotspot off; it may still be on. Turn it off in Settings > \
             Network & internet > Mobile hotspot."
        );
    }
    tracing::info!("Sherd stopped.");
    Ok(())
}

fn create_ipc_listener() -> anyhow::Result<interprocess::local_socket::tokio::Listener> {
    let name = protocol::SOCKET_NAME.to_ns_name::<GenericNamespaced>()?;
    let mut options = ListenerOptions::new().name(name);
    #[cfg(windows)]
    {
        options = options.security_descriptor(pipe_security_descriptor()?);
    }
    match options.create_tokio() {
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => anyhow::bail!(
            "Sherd is already running on this PC (another daemon window is open). Use that one, or \
             close it before starting a new one."
        ),
        // What Windows actually reports when the other copy was started as
        // Administrator and this one wasn't (seen live): it won't let a
        // non-elevated process touch the elevated one's pipe at all.
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => anyhow::bail!(
            "Sherd is already running on this PC, in another window started as Administrator. Use \
             that one, or close it before starting a new one. (If you're updating Sherd, make sure the \
             old version's window is closed.)"
        ),
        result => Ok(result?),
    }
}

async fn accept_clients(listener: interprocess::local_socket::tokio::Listener, service: Arc<SherdService>) {
    loop {
        match listener.accept().await {
            Ok(conn) => {
                tokio::spawn(connection::handle(Arc::clone(&service), conn));
            }
            Err(e) => tracing::debug!("incoming local connection failed: {e}"),
        }
    }
}

/// Plain, readable log lines: local time, level, message.
/// `20:43:28  WARN The hotspot went off -- turning it back on.`
///
/// Two things made the previous output hard to read. First, raw terminal
/// color codes showed up as `←[2m...←[0m` garbage in consoles that don't
/// understand them (older Windows console windows, unless asked to), so
/// color support is now switched on explicitly and color is dropped if
/// that fails. Second, every line carried a module name and `key=value`
/// fields. Messages are now whole sentences instead.
///
/// `RUST_LOG` still overrides the level (e.g. `RUST_LOG=debug` for
/// troubleshooting detail). Database query logging is muted by default.
fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,sqlx=warn,sea_orm=warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_timer(LocalClock)
        .with_ansi(console_supports_color())
        .init();
}

struct LocalClock;

impl tracing_subscriber::fmt::time::FormatTime for LocalClock {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std::fmt::Result {
        write!(w, "{}", chrono::Local::now().format("%H:%M:%S"))
    }
}

fn console_supports_color() -> bool {
    use std::io::IsTerminal;
    if !std::io::stdout().is_terminal() {
        return false;
    }
    #[cfg(windows)]
    {
        nu_ansi_term::enable_ansi_support().is_ok()
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// If the daemon was started by double-clicking it, its window disappears
/// the moment it exits -- taking any error message with it. Hold the window
/// open so the reason can actually be read.
fn pause_if_own_window() {
    #[cfg(windows)]
    if platform_windows::owns_console_window() {
        eprintln!("\nPress Enter to close this window.");
        let _ = std::io::stdin().read_line(&mut String::new());
    }
}

fn warn_if_not_elevated() {
    #[cfg(windows)]
    if !platform_windows::is_elevated() {
        tracing::warn!(
            "Not running as Administrator, so turning the hotspot on and switching Wi-Fi networks may \
             fail. To fix: close this window, right-click daemon.exe, and choose \"Run as administrator\"."
        );
    }
}

/// Keeps this device's part of the mesh up for as long as the daemon runs.
///
/// Checks every `interval` whether things are as they should be (see
/// `SherdService::is_healthy`: hosting on a device that can host, connected
/// to a Sherd network on one that can't) and runs `auto_connect` when not.
/// This is also what turns the hotspot back on if someone switches it off
/// in Windows Settings: it's noticed within one interval and restarted.
///
/// While something keeps failing, retries back off (doubling, up to
/// [`MAX_RETRY_DELAY`]), and the same error is logged once, not on every
/// retry. A new error, or recovery, is always logged.
async fn supervise(service: Arc<SherdService>, interval: Duration) {
    let mut first_check = true;
    let mut failed_attempts: u32 = 0;
    let mut last_problem: Option<String> = None;

    loop {
        if service.is_shutting_down() {
            return;
        }

        let status = service.status().await;
        let can_host = matches!(status.capability.level, CapabilityLevel::FullMeshCapable);

        if service.is_healthy(&status) {
            if failed_attempts > 0 {
                tracing::info!("{}", if can_host { "The hotspot is back on." } else { "Reconnected to the mesh." });
            } else if first_check {
                tracing::info!("{}", if can_host { "The hotspot is already on." } else { "Already connected to the mesh." });
            }
            first_check = false;
            failed_attempts = 0;
            last_problem = None;
            tokio::time::sleep(interval).await;
            continue;
        }

        if first_check {
            tracing::info!("Connecting to the mesh...");
        } else if failed_attempts == 0 {
            tracing::warn!(
                "{}",
                if can_host {
                    "The hotspot went off -- turning it back on."
                } else {
                    "Lost the connection to the Sherd network -- looking for another one."
                }
            );
        }
        first_check = false;

        let outcome = service.auto_connect().await;
        if service.is_shutting_down() {
            return;
        }

        let succeeded = match &outcome {
            AutoOutcome::Hosting { .. } => true,
            AutoOutcome::Joined { .. } => !can_host,
            AutoOutcome::Unavailable { .. } => false,
        };
        let report = outcome.to_string();

        if succeeded {
            tracing::info!("{report}");
            failed_attempts = 0;
            last_problem = None;
            tokio::time::sleep(interval).await;
        } else {
            failed_attempts += 1;
            let delay = retry_delay(interval, failed_attempts);
            if last_problem.as_deref() != Some(report.as_str()) {
                tracing::warn!("{report} Will keep trying in the background (next try in {}).", describe_delay(delay));
            } else {
                tracing::debug!("still failing: {report}");
            }
            last_problem = Some(report);
            tokio::time::sleep(delay).await;
        }
    }
}

fn retry_delay(interval: Duration, failed_attempts: u32) -> Duration {
    let factor = 2u32.saturating_pow(failed_attempts.saturating_sub(1).min(8));
    (interval * factor).min(MAX_RETRY_DELAY)
}

fn describe_delay(delay: Duration) -> String {
    let secs = delay.as_secs();
    if secs < 60 {
        format!("{secs} seconds")
    } else if secs % 60 == 0 {
        format!("{} min", secs / 60)
    } else {
        format!("{} min {} s", secs / 60, secs % 60)
    }
}

/// Resolves when the daemon should stop, with a readable reason. On
/// Windows that includes the console window's close button: tokio holds
/// the process open while shutdown runs, within the ~5 seconds Windows
/// allows before killing it.
#[cfg(windows)]
async fn shutdown_signal() -> &'static str {
    use tokio::signal::windows::{ctrl_break, ctrl_c, ctrl_close, ctrl_logoff, ctrl_shutdown};

    macro_rules! wait_for {
        ($listener:expr) => {
            async {
                match $listener {
                    Ok(mut listener) => {
                        listener.recv().await;
                    }
                    Err(_) => std::future::pending::<()>().await,
                }
            }
        };
    }

    tokio::select! {
        _ = wait_for!(ctrl_c()) => "Ctrl+C was pressed",
        _ = wait_for!(ctrl_break()) => "Ctrl+Break was pressed",
        _ = wait_for!(ctrl_close()) => "the window was closed",
        _ = wait_for!(ctrl_logoff()) => "the user is signing out",
        _ = wait_for!(ctrl_shutdown()) => "Windows is shutting down",
    }
}

#[cfg(not(windows))]
async fn shutdown_signal() -> &'static str {
    use tokio::signal::unix::{signal, SignalKind};
    let mut term = signal(SignalKind::terminate()).ok();
    tokio::select! {
        _ = tokio::signal::ctrl_c() => "Ctrl+C was pressed",
        _ = async { match term.as_mut() { Some(t) => { t.recv().await; } None => std::future::pending::<()>().await } } => "asked to stop",
    }
}

#[cfg(windows)]
fn platform_backend() -> platform::PlatformBackend {
    platform_windows::backend()
}

#[cfg(not(windows))]
fn platform_backend() -> platform::PlatformBackend {
    platform_linux::backend()
}

/// A security descriptor granting any local process full access to the IPC
/// pipe, with its mandatory integrity label dropped to Low.
///
/// The daemon often runs elevated (High integrity -- needed for hosting),
/// but its clients (the CLI, a future GUI) normally run as the plain
/// logged-in user (Medium integrity). Windows' default "no write-up"
/// mandatory policy would then let a client open the pipe for reading but
/// silently deny it write access (seen live: `sherd status` failed with
/// "Access is denied" against an elevated daemon) -- so both the DACL
/// (`D:(A;;GA;;;WD)`, Generic-All for Everyone) and the mandatory label
/// (`S:(ML;;NW;;;LW)`, Low with No-Write-Up) need setting explicitly. This
/// is the standard SDDL incantation for a pipe an elevated service needs
/// unprivileged local clients to reach.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_delay_backs_off_and_caps() {
        let base = Duration::from_secs(15);
        assert_eq!(retry_delay(base, 1), Duration::from_secs(15));
        assert_eq!(retry_delay(base, 2), Duration::from_secs(30));
        assert_eq!(retry_delay(base, 3), Duration::from_secs(60));
        assert_eq!(retry_delay(base, 50), MAX_RETRY_DELAY);
    }
}
