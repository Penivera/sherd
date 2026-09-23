//! Windows implementation of the `sherd-platform` traits.
//!
//! Strategy (see the plan): shell out to `netsh` for hotspot control and
//! station connect/disconnect/scan — the same thing Windows' own tooling
//! effectively does, and it works from a plain unpackaged daemon exe with no
//! app-package identity, unlike the WinRT tethering APIs. WlanAPI (native
//! FFI via the `windows` crate) is used only for the cheap, read-only
//! interface enumeration.
//!
//! Known limitation: `netsh` output is localized on non-English Windows
//! installs, and the keyword matching below (`hosted network supported`,
//! `SSID`, `Status`, ...) assumes an English locale. Ambiguous parses fail
//! soft (station-only / unknown) rather than panicking — see the unit tests
//! in `capability` for the exact fallback behavior.

#![cfg(windows)]

mod capability;
mod composite;
mod hotspot;
mod interfaces;
mod netsh;
mod station;
mod winrt_capability;
mod winrt_hotspot;

pub use capability::NetshCapabilityChecker;
pub use composite::{CompositeCapabilityChecker, CompositeHotspotController};
pub use hotspot::NetshHotspotController;
pub use interfaces::WlanApiInterfaceEnumerator;
pub use station::NetshStationConnector;
pub use winrt_capability::WinRtCapabilityChecker;
pub use winrt_hotspot::WinRtHotspotController;

use platform::PlatformBackend;

/// Build the Windows platform backend used by the daemon.
///
/// Capability and hotspot both go through [`CompositeCapabilityChecker`] /
/// [`CompositeHotspotController`], which prefer the WinRT Mobile Hotspot
/// mechanism (what actually works on most modern hardware — see
/// `winrt_hotspot.rs`) and fall back to the legacy `netsh wlan
/// hostednetwork` path only when WinRT itself is unavailable.
pub fn backend() -> PlatformBackend {
    PlatformBackend {
        capability: Box::new(CompositeCapabilityChecker::new()),
        hotspot: Box::new(CompositeHotspotController::new()),
        station: Box::new(NetshStationConnector),
        interfaces: Box::new(WlanApiInterfaceEnumerator),
    }
}

/// Whether this program has its console window to itself -- i.e. it was
/// started by double-clicking it, rather than typed into an existing
/// terminal. Such a window closes the instant the program exits, so
/// anything it printed last (an error, or a command's answer) vanishes
/// before anyone can read it; callers use this to pause first.
pub fn owns_console_window() -> bool {
    use windows::Win32::System::Console::GetConsoleProcessList;
    let mut pids = [0u32; 2];
    unsafe { GetConsoleProcessList(&mut pids) == 1 }
}

/// Whether this process is running elevated ("Run as administrator").
/// Hosting and changing Wi-Fi connections can fail without it, and the
/// resulting Windows errors rarely say "you need admin" -- so the daemon
/// checks up front and says so plainly instead.
pub fn is_elevated() -> bool {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut returned = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut TOKEN_ELEVATION as *mut core::ffi::c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}
