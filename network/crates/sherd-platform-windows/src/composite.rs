//! Tries the WinRT Mobile Hotspot backend first (works on most modern
//! hardware — see `winrt_hotspot.rs`), falling back to the legacy `netsh
//! wlan hostednetwork` backend only when WinRT itself reports the mechanism
//! is unavailable ([`PlatformError::Unsupported`]), not when it's available
//! but a specific call failed ([`PlatformError::CommandFailed`]) — a real
//! failure from the primary backend should surface as-is, not be masked by
//! a confusing second attempt through an unrelated mechanism.

use async_trait::async_trait;
use sherd_platform::{
    CapabilityReport, HotspotController, LinkStatus, PlatformError, PlatformResult,
    WifiCapabilityChecker,
};

use crate::capability::NetshCapabilityChecker;
use crate::hotspot::NetshHotspotController;
use crate::winrt_capability::WinRtCapabilityChecker;
use crate::winrt_hotspot::WinRtHotspotController;

pub struct CompositeCapabilityChecker {
    winrt: WinRtCapabilityChecker,
    netsh: NetshCapabilityChecker,
}

impl CompositeCapabilityChecker {
    pub fn new() -> Self {
        Self { winrt: WinRtCapabilityChecker, netsh: NetshCapabilityChecker }
    }
}

#[async_trait]
impl WifiCapabilityChecker for CompositeCapabilityChecker {
    async fn check(&self) -> PlatformResult<CapabilityReport> {
        match self.winrt.check().await {
            Ok(report) => Ok(report),
            Err(e) => {
                tracing::debug!("WinRT capability check unavailable ({e}); falling back to netsh");
                self.netsh.check().await
            }
        }
    }
}

pub struct CompositeHotspotController {
    winrt: WinRtHotspotController,
    netsh: NetshHotspotController,
}

impl CompositeHotspotController {
    pub fn new() -> Self {
        Self { winrt: WinRtHotspotController, netsh: NetshHotspotController }
    }
}

#[async_trait]
impl HotspotController for CompositeHotspotController {
    async fn start(&self, ssid: &str, key: &str) -> PlatformResult<()> {
        match self.winrt.start(ssid, key).await {
            Err(PlatformError::Unsupported(reason)) => {
                tracing::debug!(
                    "Mobile Hotspot unavailable ({reason}); trying legacy hostednetwork"
                );
                self.netsh.start(ssid, key).await
            }
            other => other,
        }
    }

    async fn stop(&self) -> PlatformResult<()> {
        match self.winrt.stop().await {
            Err(PlatformError::Unsupported(_)) => self.netsh.stop().await,
            other => other,
        }
    }

    async fn status(&self) -> PlatformResult<LinkStatus> {
        match self.winrt.status().await {
            Err(PlatformError::Unsupported(_)) => self.netsh.status().await,
            other => other,
        }
    }
}
