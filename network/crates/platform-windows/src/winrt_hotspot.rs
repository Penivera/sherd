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

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use platform::{HotspotController, LinkState, LinkStatus, PlatformError, PlatformResult};
use windows::core::HSTRING;
use windows::Networking::Connectivity::NetworkInformation;
use windows::Networking::NetworkOperators::{
    NetworkOperatorTetheringManager, NetworkOperatorTetheringOperationResult,
    TetheringOperationStatus, TetheringOperationalState,
};
use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};

/// The user's own Mobile Hotspot name/password from before sherd first
/// reconfigured it, so `stop` can put it back. Without this, running sherd
/// once would permanently leave Settings > Mobile hotspot showing
/// "Sherd-XXXX" with sherd's shared password.
#[derive(Clone)]
struct SavedSettings {
    ssid: String,
    passphrase: String,
}

#[derive(Default)]
pub struct WinRtHotspotController {
    original: Arc<Mutex<Option<SavedSettings>>>,
}

impl WinRtHotspotController {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl HotspotController for WinRtHotspotController {
    async fn start(&self, ssid: &str, key: &str) -> PlatformResult<()> {
        let (ssid, key) = (ssid.to_string(), key.to_string());
        let original = Arc::clone(&self.original);
        run_blocking(move || start_blocking(&ssid, &key, &original)).await
    }

    async fn stop(&self) -> PlatformResult<()> {
        let original = Arc::clone(&self.original);
        run_blocking(move || stop_blocking(&original)).await
    }

    async fn status(&self) -> PlatformResult<LinkStatus> {
        run_blocking(status_blocking).await
    }

    async fn configured(&self) -> Option<(String, String)> {
        run_blocking(|| {
            let _apartment = RoApartment::enter()?;
            let config = current_manager()?
                .GetCurrentAccessPointConfiguration()
                .map_err(|e| PlatformError::CommandFailed(e.message()))?;
            let ssid = config.Ssid().map_err(|e| PlatformError::CommandFailed(e.message()))?;
            let key = config.Passphrase().map_err(|e| PlatformError::CommandFailed(e.message()))?;
            Ok((ssid.to_string(), key.to_string()))
        })
        .await
        .ok()
    }

    async fn upstream_name(&self) -> Option<String> {
        run_blocking(|| {
            let _apartment = RoApartment::enter()?;
            let profile = NetworkInformation::GetInternetConnectionProfile()
                .map_err(|e| PlatformError::CommandFailed(e.to_string()))?;
            profile
                .ProfileName()
                .map(|n| n.to_string())
                .map_err(|e| PlatformError::CommandFailed(e.to_string()))
        })
        .await
        .ok()
        .filter(|name| !name.is_empty())
    }
}

pub(crate) async fn run_blocking<T, F>(f: F) -> PlatformResult<T>
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
pub(crate) struct RoApartment;

impl RoApartment {
    pub(crate) fn enter() -> PlatformResult<Self> {
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

/// Shown whenever Windows refuses the Mobile Hotspot calls outright (as
/// opposed to returning a specific `TetheringOperationStatus`) -- seen live
/// as `Unspecified error (0x80004005)` from
/// `GetCurrentAccessPointConfiguration` on a PC whose capability check
/// said hosting was enabled. Windows gives no further detail, so the best
/// we can do is point at the usual culprits.
const MOBILE_HOTSPOT_HINT: &str = "Check that Wi-Fi is turned on, try switching Mobile hotspot on by hand \
     in Settings > Network & internet > Mobile hotspot to see whether Windows allows it at all, and make \
     sure the \"Windows Mobile Hotspot Service\" isn't disabled in Services. If it won't turn on by hand \
     either, this PC can't host -- it will still join other Sherd networks.";

pub(crate) fn current_manager() -> PlatformResult<NetworkOperatorTetheringManager> {
    let profile = NetworkInformation::GetInternetConnectionProfile().map_err(|_| {
        PlatformError::Unsupported(
            "this PC has no internet connection to share. Windows' Mobile Hotspot can only share an \
             existing connection (Wi-Fi or Ethernet) -- connect to one first"
                .to_string(),
        )
    })?;
    NetworkOperatorTetheringManager::CreateFromConnectionProfile(&profile).map_err(|e| {
        PlatformError::Unsupported(format!(
            "Windows won't run Mobile Hotspot on this connection ({}). {MOBILE_HOTSPOT_HINT}",
            e.message()
        ))
    })
}

fn start_blocking(ssid: &str, key: &str, original: &Mutex<Option<SavedSettings>>) -> PlatformResult<()> {
    let _apartment = RoApartment::enter()?;
    let manager = current_manager()?;

    // Classified as `Unsupported` (not `CommandFailed`) so the composite
    // controller falls back to the legacy hosted-network path -- this is
    // Windows refusing the mechanism wholesale, not one call misbehaving.
    let config = manager.GetCurrentAccessPointConfiguration().map_err(|e| {
        PlatformError::Unsupported(format!(
            "Windows wouldn't let sherd read the Mobile Hotspot settings ({} {:#010X}). {MOBILE_HOTSPOT_HINT}",
            e.message(),
            e.code().0
        ))
    })?;
    let current_ssid = config.Ssid().map(|s| s.to_string()).unwrap_or_default();
    let current_key = config.Passphrase().map(|p| p.to_string()).unwrap_or_default();
    let settings_match = current_ssid == ssid && current_key == key;
    let is_on = manager.TetheringOperationalState().ok() == Some(TetheringOperationalState::On);

    // Already hosting exactly what we want: nothing to do. Avoids
    // bouncing a working hotspot (and kicking off every connected device)
    // just because `start` was called again.
    if is_on && settings_match {
        return Ok(());
    }

    // On, but someone renamed it or changed its password (in Windows
    // Settings, say). New settings only take effect when the hotspot
    // starts, so it has to go off and back on -- asking Windows to "start"
    // a running hotspot just answers "already on" and keeps the wrong name.
    if is_on {
        let result = manager
            .StopTetheringAsync()
            .and_then(|op| op.join())
            .map_err(|e| PlatformError::CommandFailed(format!("Windows couldn't restart the hotspot: {}", e.message())))?;
        check_result(result, "stop")?;
    }

    {
        // Never "save" a Sherd name as the user's original: that's left
        // over from an earlier run that didn't restore (older versions
        // didn't, and a force-killed daemon can't). Restoring it would
        // leave a hotspot that other Sherd devices mistake for a mesh
        // network whenever the user later turns it on for themselves.
        let mut saved = original.lock().expect("saved-settings lock poisoned");
        if saved.is_none() && current_ssid != ssid && !current_ssid.starts_with("Sherd-") {
            *saved = Some(SavedSettings { ssid: current_ssid, passphrase: current_key });
        }
    }

    config
        .SetSsid(&HSTRING::from(ssid))
        .map_err(|e| PlatformError::CommandFailed(format!("couldn't set the hotspot name: {}", e.message())))?;
    config
        .SetPassphrase(&HSTRING::from(key))
        .map_err(|e| PlatformError::CommandFailed(format!("couldn't set the hotspot password: {}", e.message())))?;
    manager
        .ConfigureAccessPointAsync(&config)
        .and_then(|op| op.join())
        .map_err(|e| PlatformError::CommandFailed(format!("Windows rejected the hotspot settings: {}", e.message())))?;

    let result = manager
        .StartTetheringAsync()
        .and_then(|op| op.join())
        .map_err(|e| PlatformError::CommandFailed(format!("Windows couldn't start the hotspot: {}", e.message())))?;
    check_result(result, "start")
}

fn stop_blocking(original: &Mutex<Option<SavedSettings>>) -> PlatformResult<()> {
    let _apartment = RoApartment::enter()?;
    let manager = current_manager()?;

    if manager.TetheringOperationalState().ok() != Some(TetheringOperationalState::Off) {
        let result = manager
            .StopTetheringAsync()
            .and_then(|op| op.join())
            .map_err(|e| PlatformError::CommandFailed(format!("Windows couldn't stop the hotspot: {}", e.message())))?;
        check_result(result, "stop")?;
    }

    // Put the user's own hotspot name/password back.
    let saved = original.lock().expect("saved-settings lock poisoned").take();
    if let Some(saved) = saved {
        let restore = || -> windows::core::Result<()> {
            let config = manager.GetCurrentAccessPointConfiguration()?;
            config.SetSsid(&HSTRING::from(saved.ssid.as_str()))?;
            config.SetPassphrase(&HSTRING::from(saved.passphrase.as_str()))?;
            manager.ConfigureAccessPointAsync(&config)?.join()
        };
        if let Err(e) = restore() {
            tracing::warn!(
                "Turned the hotspot off, but couldn't restore its original name ({}): {}",
                saved.ssid,
                e.message()
            );
        }
    }
    Ok(())
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
        (LinkState::Up, "on".to_string())
    } else if state == TetheringOperationalState::Off {
        (LinkState::Down, "off".to_string())
    } else if state == TetheringOperationalState::InTransition {
        (LinkState::Starting, "switching on/off".to_string())
    } else {
        (LinkState::Down, "state unknown".to_string())
    };
    Ok(LinkStatus { state, ssid, detail })
}

fn check_result(result: NetworkOperatorTetheringOperationResult, verb: &str) -> PlatformResult<()> {
    let status = result
        .Status()
        .map_err(|e| PlatformError::CommandFailed(format!("couldn't read the result of the {verb}: {e}")))?;
    // "Already on" when starting is the outcome we wanted, not a failure.
    if status == TetheringOperationStatus::Success
        || (verb == "start" && status == TetheringOperationStatus::AlreadyOn)
    {
        return Ok(());
    }
    let extra = result
        .AdditionalErrorMessage()
        .map(|s| s.to_string())
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(|s| format!(" ({s})"))
        .unwrap_or_default();
    let message = format!("Windows couldn't {verb} the hotspot: {}{extra}", describe_status(status));

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
        s if s == TetheringOperationStatus::Unknown => "Windows reported an unknown error",
        s if s == TetheringOperationStatus::MobileBroadbandDeviceOff => {
            "mobile broadband is turned off"
        }
        s if s == TetheringOperationStatus::WiFiDeviceOff => "Wi-Fi is turned off",
        s if s == TetheringOperationStatus::BluetoothDeviceOff => "Bluetooth is turned off",
        s if s == TetheringOperationStatus::EntitlementCheckTimeout => {
            "carrier entitlement check timed out"
        }
        s if s == TetheringOperationStatus::EntitlementCheckFailure => {
            "carrier entitlement check failed"
        }
        s if s == TetheringOperationStatus::OperationInProgress => "another operation is in progress",
        s if s == TetheringOperationStatus::NetworkLimitedConnectivity => {
            "the connection it would share has limited or no internet"
        }
        s if s == TetheringOperationStatus::AlreadyOn => "already on",
        s if s == TetheringOperationStatus::RadioRestriction => "wireless is blocked (is airplane mode on?)",
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
            "Wi-Fi is turned off"
        );
    }
}
