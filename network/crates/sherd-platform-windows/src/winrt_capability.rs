//! Capability check via the same WinRT API `winrt_hotspot.rs` uses to host
//! — `NetworkOperatorTetheringManager::GetTetheringCapabilityFromConnectionProfile`
//! answers "can Mobile Hotspot actually work here?" directly, instead of
//! inferring it from the unrelated legacy flag `netsh wlan show drivers`
//! reports (see `capability.rs`'s doc comment and `winrt_hotspot.rs` for why
//! that flag is misleading on modern hardware).

use async_trait::async_trait;
use sherd_platform::{CapabilityLevel, CapabilityReport, PlatformResult, WifiCapabilityChecker};
use windows::Networking::Connectivity::NetworkInformation;
use windows::Networking::NetworkOperators::{NetworkOperatorTetheringManager, TetheringCapability};

const CHECKED_VIA: &str = "NetworkOperatorTetheringManager (WinRT)";

pub struct WinRtCapabilityChecker;

#[async_trait]
impl WifiCapabilityChecker for WinRtCapabilityChecker {
    async fn check(&self) -> PlatformResult<CapabilityReport> {
        tokio::task::spawn_blocking(check_blocking).await.map_err(|e| {
            sherd_platform::PlatformError::CommandFailed(format!("WinRT task panicked: {e}"))
        })?
    }
}

fn check_blocking() -> PlatformResult<CapabilityReport> {
    let profile = NetworkInformation::GetInternetConnectionProfile().map_err(|e| {
        sherd_platform::PlatformError::Unsupported(format!(
            "no active internet connection to check (are you connected to Wi-Fi/Ethernet?): {e}"
        ))
    })?;
    let capability = NetworkOperatorTetheringManager::GetTetheringCapabilityFromConnectionProfile(
        &profile,
    )
    .map_err(|e| {
        sherd_platform::PlatformError::CommandFailed(format!(
            "GetTetheringCapabilityFromConnectionProfile failed: {e}"
        ))
    })?;

    let (level, reason) = classify(capability);
    Ok(CapabilityReport {
        level,
        detail: format!("Mobile Hotspot capability: {reason}"),
        checked_via: CHECKED_VIA.to_string(),
    })
}

fn classify(capability: TetheringCapability) -> (CapabilityLevel, &'static str) {
    match capability {
        c if c == TetheringCapability::Enabled => (CapabilityLevel::FullMeshCapable, "enabled"),
        c if c == TetheringCapability::DisabledByGroupPolicy => {
            (CapabilityLevel::StationOnly, "disabled by group policy")
        }
        c if c == TetheringCapability::DisabledByHardwareLimitation => {
            (CapabilityLevel::StationOnly, "disabled by hardware limitation")
        }
        c if c == TetheringCapability::DisabledByOperator => {
            (CapabilityLevel::StationOnly, "disabled by carrier/operator")
        }
        c if c == TetheringCapability::DisabledBySku => {
            (CapabilityLevel::StationOnly, "disabled for this Windows edition")
        }
        c if c == TetheringCapability::DisabledByRequiredAppNotInstalled => (
            CapabilityLevel::StationOnly,
            "disabled: a required app is not installed",
        ),
        c if c == TetheringCapability::DisabledBySystemCapability => {
            (CapabilityLevel::StationOnly, "disabled by system capability")
        }
        c if c == TetheringCapability::DisabledDueToUnknownCause => {
            (CapabilityLevel::StationOnly, "disabled for an unknown reason")
        }
        _ => (CapabilityLevel::StationOnly, "unrecognized capability value"),
    }
}
