//! Placeholder Linux backend for the `sherd-platform` traits.
//!
//! Nothing here is implemented yet — every method returns
//! [`PlatformError::NotImplemented`]. This crate exists purely so the
//! `sherd-platform` trait seam already has a second implementor and the
//! daemon/core/protocol never need to change shape when Linux support is
//! filled in for real. Each stub documents the real mechanism to use then:
//!
//! - Concurrent STA+AP: `iw` supports multiple virtual interfaces on one
//!   radio (`iw phy phy0 interface add ap0 type __ap`), unlike Windows there
//!   is no separate "hosted network" concept to shell out to.
//! - Hotspot: `hostapd` configured against the AP-mode virtual interface.
//! - Station connect: `wpa_supplicant` (via `wpa_cli` or its D-Bus API) or
//!   `nmcli` if NetworkManager is present.
//! - True mesh (not just an IP overlay): 802.11s (`iw ... type mesh`) or
//!   `batman-adv` — a real driver-level capability Windows doesn't have, so
//!   this backend could eventually do more than mirror the Windows one.

use async_trait::async_trait;
use platform::{
    CapabilityReport, HotspotController, InterfaceEnumerator, InterfaceInfo, LinkStatus,
    PlatformBackend, PlatformError, PlatformResult, ScanResult, StationConnector,
    WifiCapabilityChecker,
};

pub struct NotImplementedCapabilityChecker;
pub struct NotImplementedHotspotController;
pub struct NotImplementedStationConnector;
pub struct NotImplementedInterfaceEnumerator;

#[async_trait]
impl WifiCapabilityChecker for NotImplementedCapabilityChecker {
    async fn check(&self) -> PlatformResult<CapabilityReport> {
        Err(PlatformError::NotImplemented)
    }
}

#[async_trait]
impl HotspotController for NotImplementedHotspotController {
    async fn start(&self, _ssid: &str, _key: &str) -> PlatformResult<()> {
        Err(PlatformError::NotImplemented)
    }
    async fn stop(&self) -> PlatformResult<()> {
        Err(PlatformError::NotImplemented)
    }
    async fn status(&self) -> PlatformResult<LinkStatus> {
        Err(PlatformError::NotImplemented)
    }
}

#[async_trait]
impl StationConnector for NotImplementedStationConnector {
    async fn scan(&self) -> PlatformResult<Vec<ScanResult>> {
        Err(PlatformError::NotImplemented)
    }
    async fn connect(&self, _ssid: &str, _key: &str) -> PlatformResult<()> {
        Err(PlatformError::NotImplemented)
    }
    async fn disconnect(&self) -> PlatformResult<()> {
        Err(PlatformError::NotImplemented)
    }
    async fn status(&self) -> PlatformResult<LinkStatus> {
        Err(PlatformError::NotImplemented)
    }
}

#[async_trait]
impl InterfaceEnumerator for NotImplementedInterfaceEnumerator {
    async fn list(&self) -> PlatformResult<Vec<InterfaceInfo>> {
        Err(PlatformError::NotImplemented)
    }
}

/// Build the (currently inert) Linux platform backend.
pub fn backend() -> PlatformBackend {
    PlatformBackend {
        capability: Box::new(NotImplementedCapabilityChecker),
        hotspot: Box::new(NotImplementedHotspotController),
        station: Box::new(NotImplementedStationConnector),
        interfaces: Box::new(NotImplementedInterfaceEnumerator),
    }
}
