use async_trait::async_trait;
use sherd_platform::{
    LinkState, LinkStatus, PlatformError, PlatformResult, ScanResult, StationConnector,
};

use crate::netsh::{run_netsh, value_after_colon};

pub struct NetshStationConnector;

#[async_trait]
impl StationConnector for NetshStationConnector {
    async fn scan(&self) -> PlatformResult<Vec<ScanResult>> {
        let out = run_netsh(&["wlan", "show", "networks"]).await?;
        Ok(parse_scan(&out.stdout))
    }

    async fn connect(&self, ssid: &str, key: &str) -> PlatformResult<()> {
        let profile_path = write_profile(ssid, key).await?;

        let add = run_netsh(&[
            "wlan",
            "add",
            "profile",
            &format!("filename={}", profile_path.display()),
            "user=all",
        ])
        .await?;
        let _ = tokio::fs::remove_file(&profile_path).await;
        if !add.success {
            return Err(classify_connect_failure(&add.stdout, &add.stderr));
        }

        let connect = run_netsh(&[
            "wlan",
            "connect",
            &format!("name={ssid}"),
            &format!("ssid={ssid}"),
        ])
        .await?;
        if !connect.success {
            return Err(classify_connect_failure(&connect.stdout, &connect.stderr));
        }
        // `netsh wlan connect` returns as soon as the request is accepted,
        // not once the link is actually up — callers that need to know the
        // real outcome should poll `status()` afterwards.
        Ok(())
    }

    async fn disconnect(&self) -> PlatformResult<()> {
        let out = run_netsh(&["wlan", "disconnect"]).await?;
        if out.success {
            Ok(())
        } else {
            Err(PlatformError::CommandFailed(out.stdout.trim().to_string()))
        }
    }

    async fn status(&self) -> PlatformResult<LinkStatus> {
        let out = run_netsh(&["wlan", "show", "interfaces"]).await?;
        Ok(parse_interface_status(&out.stdout))
    }
}

fn classify_connect_failure(stdout: &str, stderr: &str) -> PlatformError {
    let combined = format!("{stdout} {stderr}").to_lowercase();
    if combined.contains("not supported") || combined.contains("no wireless interface") {
        PlatformError::Unsupported(stdout.trim().to_string())
    } else {
        PlatformError::CommandFailed(stdout.trim().to_string())
    }
}

/// `netsh wlan show networks` prints entries like `SSID 1 : MyNetwork`.
/// Signal strength isn't requested (`mode=Bssid` would add it) to keep the
/// output — and this parser — locale-simpler; see the crate-level doc
/// comment about the same tradeoff for capability detection.
fn parse_scan(stdout: &str) -> Vec<ScanResult> {
    stdout
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            if !trimmed.to_lowercase().starts_with("ssid") {
                return None;
            }
            let ssid = trimmed.splitn(2, ':').nth(1)?.trim();
            if ssid.is_empty() {
                return None;
            }
            Some(ScanResult {
                ssid: ssid.to_string(),
                signal_percent: None,
            })
        })
        .collect()
}

fn parse_interface_status(stdout: &str) -> LinkStatus {
    let state = stdout
        .lines()
        .find(|l| l.trim_start().to_lowercase().starts_with("state"))
        .and_then(value_after_colon)
        .map(|v| v.to_lowercase());

    let ssid = stdout
        .lines()
        .find(|l| {
            let t = l.trim_start().to_lowercase();
            t.starts_with("ssid") && !t.starts_with("bssid")
        })
        .and_then(value_after_colon);

    match state.as_deref() {
        Some(v) if v.starts_with("connected") => LinkStatus {
            state: LinkState::Up,
            ssid,
            detail: "connected".to_string(),
        },
        Some(v) if v.starts_with("disconnected") => LinkStatus {
            state: LinkState::Down,
            ssid: None,
            detail: "disconnected".to_string(),
        },
        Some(v) if v.contains("connecting") || v.contains("authenticating") => LinkStatus {
            state: LinkState::Starting,
            ssid,
            detail: v.to_string(),
        },
        Some(other) => LinkStatus {
            state: LinkState::Down,
            ssid,
            detail: other.to_string(),
        },
        None => LinkStatus::down("no Wi-Fi interface reported by `netsh wlan show interfaces`"),
    }
}

/// Writes a minimal WPA2-Personal/AES WLAN profile so `netsh wlan add
/// profile` has something to connect with — `netsh wlan connect` alone
/// can't supply a passphrase for a network it hasn't seen before.
async fn write_profile(ssid: &str, key: &str) -> PlatformResult<std::path::PathBuf> {
    let xml = format!(
        r#"<?xml version="1.0"?>
<WLANProfile xmlns="http://www.microsoft.com/networking/WLAN/profile/v1">
    <name>{ssid}</name>
    <SSIDConfig>
        <SSID>
            <name>{ssid}</name>
        </SSID>
    </SSIDConfig>
    <connectionType>ESS</connectionType>
    <connectionMode>manual</connectionMode>
    <MSM>
        <security>
            <authEncryption>
                <authentication>WPA2PSK</authentication>
                <encryption>AES</encryption>
                <useOneX>false</useOneX>
            </authEncryption>
            <sharedKey>
                <keyType>passPhrase</keyType>
                <protected>false</protected>
                <keyMaterial>{key}</keyMaterial>
            </sharedKey>
        </security>
    </MSM>
</WLANProfile>"#,
        ssid = xml_escape(ssid),
        key = xml_escape(key),
    );

    let path = std::env::temp_dir().join(format!("sherd-profile-{}.xml", sanitize(ssid)));
    tokio::fs::write(&path, xml).await?;
    Ok(path)
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scan_results() {
        let sample = "\
Interface name : Wi-Fi
There are 2 networks currently visible.

SSID 1 : Sherd-a1b2
    Network type            : Infrastructure
    Authentication          : WPA2-Personal
    Encryption              : CCMP

SSID 2 : SomeoneElsesWifi
    Network type            : Infrastructure
    Authentication          : WPA2-Personal
    Encryption              : CCMP
";
        let results = parse_scan(sample);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].ssid, "Sherd-a1b2");
        assert_eq!(results[1].ssid, "SomeoneElsesWifi");
    }

    #[test]
    fn parses_connected_state() {
        let sample = "\
    Name                   : Wi-Fi
    State                  : connected
    SSID                   : Sherd-a1b2
    BSSID                  : 00:11:22:33:44:55
";
        let status = parse_interface_status(sample);
        assert_eq!(status.state, LinkState::Up);
        assert_eq!(status.ssid.as_deref(), Some("Sherd-a1b2"));
    }

    #[test]
    fn parses_disconnected_state() {
        let sample = "    State                  : disconnected\n";
        let status = parse_interface_status(sample);
        assert_eq!(status.state, LinkState::Down);
    }
}
