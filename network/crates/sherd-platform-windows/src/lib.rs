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
mod hotspot;
mod interfaces;
mod netsh;
mod station;

pub use capability::NetshCapabilityChecker;
pub use hotspot::NetshHotspotController;
pub use interfaces::WlanApiInterfaceEnumerator;
pub use station::NetshStationConnector;

use sherd_platform::PlatformBackend;

/// Build the Windows platform backend used by the daemon.
pub fn backend() -> PlatformBackend {
    PlatformBackend {
        capability: Box::new(NetshCapabilityChecker),
        hotspot: Box::new(NetshHotspotController),
        station: Box::new(NetshStationConnector),
        interfaces: Box::new(WlanApiInterfaceEnumerator),
    }
}
