use async_trait::async_trait;
use sherd_platform::{HotspotController, LinkState, LinkStatus, PlatformError, PlatformResult};

use crate::netsh::{run_netsh, value_after_colon};

/// Hosts an access point via the legacy `netsh wlan hostednetwork` path.
///
/// Known limitation: `hostednetwork` only brings up the virtual AP
/// interface — it does not by itself route traffic between it and the
/// upstream (station) connection. Making Internet/LAN traffic actually flow
/// to hosted-network clients needs Internet Connection Sharing configured
/// separately; that automation is a follow-up, not done here.
pub struct NetshHotspotController;

#[async_trait]
impl HotspotController for NetshHotspotController {
    async fn start(&self, ssid: &str, key: &str) -> PlatformResult<()> {
        let set = run_netsh(&[
            "wlan",
            "set",
            "hostednetwork",
            "mode=allow",
            &format!("ssid={ssid}"),
            &format!("key={key}"),
        ])
        .await?;
        if !set.success {
            return Err(classify_failure(&set.stdout, &set.stderr));
        }

        let start = run_netsh(&["wlan", "start", "hostednetwork"]).await?;
        if !start.success || start.stdout.to_lowercase().contains("not started") {
            return Err(classify_failure(&start.stdout, &start.stderr));
        }
        Ok(())
    }

    async fn stop(&self) -> PlatformResult<()> {
        let out = run_netsh(&["wlan", "stop", "hostednetwork"]).await?;
        if out.success {
            Ok(())
        } else {
            Err(PlatformError::CommandFailed(out.stdout.trim().to_string()))
        }
    }

    async fn status(&self) -> PlatformResult<LinkStatus> {
        let out = run_netsh(&["wlan", "show", "hostednetwork"]).await?;
        Ok(parse_status(&out.stdout))
    }
}

/// `netsh` reports an unsupported adapter/driver with wording like "not
/// supported" or "not in the correct state" rather than a distinct error
/// code, so that's what we match on to turn it into
/// [`PlatformError::Unsupported`] instead of a generic failure.
fn classify_failure(stdout: &str, stderr: &str) -> PlatformError {
    let combined = format!("{stdout} {stderr}").to_lowercase();
    if combined.contains("not supported")
        || combined.contains("not in the correct state")
        || combined.contains("group or resource")
    {
        PlatformError::Unsupported(stdout.trim().to_string())
    } else {
        PlatformError::CommandFailed(stdout.trim().to_string())
    }
}

fn parse_status(stdout: &str) -> LinkStatus {
    let ssid = stdout
        .lines()
        .find(|l| l.trim_start().to_lowercase().starts_with("ssid name"))
        .and_then(value_after_colon)
        .map(|v| v.trim_matches('"').to_string());

    let status_value = stdout
        .lines()
        .find(|l| l.trim_start().to_lowercase().starts_with("status"))
        .and_then(value_after_colon)
        .map(|v| v.to_lowercase());

    match status_value.as_deref() {
        Some(v) if v.starts_with("started") => LinkStatus {
            state: LinkState::Up,
            ssid,
            detail: "hosted network started".to_string(),
        },
        Some(v) if v.starts_with("not started") => LinkStatus {
            state: LinkState::Down,
            ssid,
            detail: "hosted network not started".to_string(),
        },
        _ => LinkStatus::down("could not determine hosted network status"),
    }
}
