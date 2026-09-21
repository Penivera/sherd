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
