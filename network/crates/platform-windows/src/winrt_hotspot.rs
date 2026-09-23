//! Hotspot control via the WinRT "Mobile Hotspot" API
//! (`NetworkOperatorTetheringManager`) — the mechanism actually behind
//! Settings > Mobile Hotspot on modern hardware.
//!
//! This exists because, verified live on a real Intel Wireless-AC 8260: the
//! legacy `netsh wlan hostednetwork` path in `hotspot.rs` fails with "The
//! group or resource is not in the correct state to perform the requested
//! operation" — Intel dropped that legacy SoftAP capability years ago — even
//! though Mobile Hotspot itself works fine from Windows Settings on the same
//! machine. The two features use unrelated driver capabilities; `netsh wlan
//! show drivers`'s "Hosted network supported" flag only reflects the former.
//! This module is the one actually used on most modern laptops; `hotspot.rs`
//! is kept as a fallback for the rarer adapters where the legacy path is
//! what works instead (see `composite.rs`).

use async_trait::async_trait;
use platform::{HotspotController, LinkState, LinkStatus, PlatformError, PlatformResult};
use windows::core::HSTRING;
use windows::Networking::Connectivity::NetworkInformation;
use windows::Networking::NetworkOperators::{
    NetworkOperatorTetheringManager, NetworkOperatorTetheringOperationResult,
    TetheringOperationStatus, TetheringOperationalState,
};
use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};

pub struct WinRtHotspotController;

#[async_trait]
impl HotspotController for WinRtHotspotController {
    async fn start(&self, ssid: &str, key: &str) -> PlatformResult<()> {
        let (ssid, key) = (ssid.to_string(), key.to_string());
        run_blocking(move || start_blocking(&ssid, &key)).await
    }

    async fn stop(&self) -> PlatformResult<()> {
        run_blocking(stop_blocking).await
    }

    async fn status(&self) -> PlatformResult<LinkStatus> {
        run_blocking(status_blocking).await
    }
}

async fn run_blocking<T, F>(f: F) -> PlatformResult<T>
where
    F: FnOnce() -> PlatformResult<T> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| PlatformError::CommandFailed(format!("WinRT task panicked: {e}")))?
}

/// RAII guard balancing `RoInitialize`/`RoUninitialize` on the current
/// thread. WinRT calls need the COM apartment initialized, and tokio's
/// blocking-pool threads don't do this on their own. Multithreaded (MTA) is
/// used rather than single-threaded so no message pump is required; calling
/// `RoInitialize` again on an already-initialized thread is explicitly
/// supported (it just bumps a ref count), so reusing pooled threads is fine.
struct RoApartment;

impl RoApartment {
    fn enter() -> PlatformResult<Self> {
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }
            .map_err(|e| PlatformError::CommandFailed(format!("RoInitialize failed: {e}")))?;
        Ok(Self)
    }
}

impl Drop for RoApartment {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}

pub(crate) fn current_manager() -> PlatformResult<NetworkOperatorTetheringManager> {
    let profile = NetworkInformation::GetInternetConnectionProfile().map_err(|e| {
        PlatformError::Unsupported(format!(
            "no active internet connection to tether from (are you connected to Wi-Fi/Ethernet?): {e}"
        ))
    })?;
    NetworkOperatorTetheringManager::CreateFromConnectionProfile(&profile).map_err(|e| {
        PlatformError::Unsupported(format!(
            "this connection's adapter does not support Mobile Hotspot: {e}"
        ))
    })
}

fn start_blocking(ssid: &str, key: &str) -> PlatformResult<()> {
    let _apartment = RoApartment::enter()?;
    let manager = current_manager()?;

    let config = manager.GetCurrentAccessPointConfiguration().map_err(|e| {
        PlatformError::CommandFailed(format!("could not read access point configuration: {e}"))
    })?;
    config
        .SetSsid(&HSTRING::from(ssid))
        .map_err(|e| PlatformError::CommandFailed(format!("could not set SSID: {e}")))?;
    config
        .SetPassphrase(&HSTRING::from(key))
        .map_err(|e| PlatformError::CommandFailed(format!("could not set passphrase: {e}")))?;
    manager
        .ConfigureAccessPointAsync(&config)
        .and_then(|op| op.join())
        .map_err(|e| PlatformError::CommandFailed(format!("could not apply configuration: {e}")))?;

    let result = manager
        .StartTetheringAsync()
        .and_then(|op| op.join())
        .map_err(|e| PlatformError::CommandFailed(format!("StartTetheringAsync failed: {e}")))?;
    check_result(result, "start")
}

fn stop_blocking() -> PlatformResult<()> {
    let _apartment = RoApartment::enter()?;
    let manager = current_manager()?;
    let result = manager
        .StopTetheringAsync()
        .and_then(|op| op.join())
        .map_err(|e| PlatformError::CommandFailed(format!("StopTetheringAsync failed: {e}")))?;
    check_result(result, "stop")
}

fn status_blocking() -> PlatformResult<LinkStatus> {
    let _apartment = RoApartment::enter()?;
    let manager = current_manager()?;

    let state = manager.TetheringOperationalState().map_err(|e| {
        PlatformError::CommandFailed(format!("could not read tethering state: {e}"))
    })?;
    let ssid = manager
        .GetCurrentAccessPointConfiguration()
        .and_then(|c| c.Ssid())
        .ok()
        .map(|s| s.to_string());

    let (state, detail) = if state == TetheringOperationalState::On {
        (LinkState::Up, "mobile hotspot on".to_string())
    } else if state == TetheringOperationalState::Off {
        (LinkState::Down, "mobile hotspot off".to_string())
    } else if state == TetheringOperationalState::InTransition {
        (LinkState::Starting, "mobile hotspot changing state".to_string())
    } else {
        (LinkState::Down, "mobile hotspot state unknown".to_string())
    };
    Ok(LinkStatus { state, ssid, detail })
}

fn check_result(result: NetworkOperatorTetheringOperationResult, verb: &str) -> PlatformResult<()> {
    let status = result
        .Status()
        .map_err(|e| PlatformError::CommandFailed(format!("could not read {verb} result: {e}")))?;
    if status == TetheringOperationStatus::Success {
        return Ok(());
    }
    let extra = result.AdditionalErrorMessage().map(|s| s.to_string()).unwrap_or_default();
    let message = format!("mobile hotspot {verb} failed: {} ({extra})", describe_status(status));

    let device_off = [
        TetheringOperationStatus::WiFiDeviceOff,
        TetheringOperationStatus::MobileBroadbandDeviceOff,
        TetheringOperationStatus::BluetoothDeviceOff,
    ];
    if device_off.contains(&status) {
        Err(PlatformError::Unsupported(message))
    } else {
        Err(PlatformError::CommandFailed(message))
    }
}

fn describe_status(status: TetheringOperationStatus) -> &'static str {
    match status {
        s if s == TetheringOperationStatus::Success => "success",
        s if s == TetheringOperationStatus::Unknown => "unknown error",
        s if s == TetheringOperationStatus::MobileBroadbandDeviceOff => {
            "mobile broadband device is off"
        }
        s if s == TetheringOperationStatus::WiFiDeviceOff => "Wi-Fi device is off",
        s if s == TetheringOperationStatus::BluetoothDeviceOff => "Bluetooth device is off",
        s if s == TetheringOperationStatus::EntitlementCheckTimeout => {
            "carrier entitlement check timed out"
        }
        s if s == TetheringOperationStatus::EntitlementCheckFailure => {
            "carrier entitlement check failed"
        }
        s if s == TetheringOperationStatus::OperationInProgress => "another operation is in progress",
        s if s == TetheringOperationStatus::NetworkLimitedConnectivity => {
            "network has limited connectivity"
        }
        s if s == TetheringOperationStatus::AlreadyOn => "already on",
        s if s == TetheringOperationStatus::RadioRestriction => "radio is restricted (e.g. airplane mode)",
        s if s == TetheringOperationStatus::BandInterference => "band interference",
        _ => "unrecognized status",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_known_statuses() {
        assert_eq!(describe_status(TetheringOperationStatus::Success), "success");
        assert_eq!(
            describe_status(TetheringOperationStatus::WiFiDeviceOff),
            "Wi-Fi device is off"
        );
    }
}
