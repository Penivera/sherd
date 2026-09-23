//! Reading and switching the Wi-Fi radio via WinRT's `Windows.Devices.Radios`
//! -- the same on/off switch as the Wi-Fi toggle in Windows' quick settings.
//! Used so the daemon can turn Wi-Fi back on if someone switches it off,
//! which would otherwise silently cut this device out of the mesh.

use platform::{PlatformError, PlatformResult};
use windows::Devices::Radios::{Radio, RadioAccessStatus, RadioKind, RadioState};

use crate::winrt_hotspot::{run_blocking, RoApartment};

pub(crate) async fn is_wifi_on() -> Option<bool> {
    run_blocking(|| {
        let _apartment = RoApartment::enter()?;
        let radio = wifi_radio()?;
        let state = radio.State().map_err(|e| PlatformError::CommandFailed(e.message()))?;
        Ok(state == RadioState::On)
    })
    .await
    .ok()
}

pub(crate) async fn turn_wifi_on() -> PlatformResult<()> {
    run_blocking(|| {
        let _apartment = RoApartment::enter()?;
        let access = Radio::RequestAccessAsync()
            .and_then(|op| op.join())
            .map_err(|e| PlatformError::CommandFailed(e.message()))?;
        if access != RadioAccessStatus::Allowed {
            return Err(PlatformError::Unsupported(
                "Windows didn't allow Sherd to switch Wi-Fi on".to_string(),
            ));
        }
        let status = wifi_radio()?
            .SetStateAsync(RadioState::On)
            .and_then(|op| op.join())
            .map_err(|e| PlatformError::CommandFailed(e.message()))?;
        match status {
            s if s == RadioAccessStatus::Allowed => Ok(()),
            s if s == RadioAccessStatus::DeniedBySystem => Err(PlatformError::Unsupported(
                "Windows is blocking it (is airplane mode on?)".to_string(),
            )),
            _ => Err(PlatformError::CommandFailed("Windows refused to switch Wi-Fi on".to_string())),
        }
    })
    .await
}

fn wifi_radio() -> PlatformResult<Radio> {
    let radios = Radio::GetRadiosAsync()
        .and_then(|op| op.join())
        .map_err(|e| PlatformError::CommandFailed(e.message()))?;
    for radio in radios {
        if radio.Kind().ok() == Some(RadioKind::WiFi) {
            return Ok(radio);
        }
    }
    Err(PlatformError::Unsupported("no Wi-Fi radio found".to_string()))
}
