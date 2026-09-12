use async_trait::async_trait;
use sherd_platform::{CapabilityLevel, CapabilityReport, PlatformResult, WifiCapabilityChecker};

use crate::netsh::{run_netsh, value_after_colon};

const CHECKED_VIA: &str = "netsh wlan show drivers";

pub struct NetshCapabilityChecker;

#[async_trait]
impl WifiCapabilityChecker for NetshCapabilityChecker {
    async fn check(&self) -> PlatformResult<CapabilityReport> {
        let output = run_netsh(&["wlan", "show", "drivers"]).await?;
        Ok(parse_capability(&output.stdout))
    }
}

fn parse_capability(stdout: &str) -> CapabilityReport {
    if stdout.trim().is_empty() {
        return CapabilityReport {
            level: CapabilityLevel::Unsupported,
            detail: "`netsh wlan show drivers` produced no output; no Wi-Fi driver found."
                .to_string(),
            checked_via: CHECKED_VIA.to_string(),
        };
    }

    let hosted_line = stdout
        .lines()
        .find(|line| line.to_lowercase().contains("hosted network supported"));

    let Some(line) = hosted_line else {
        return CapabilityReport {
            level: CapabilityLevel::StationOnly,
            detail: "Could not find a \"Hosted network supported\" line in `netsh wlan show \
                     drivers` output; assuming station-only until proven otherwise."
                .to_string(),
            checked_via: CHECKED_VIA.to_string(),
        };
    };

    match value_after_colon(line).map(|v| v.to_lowercase()).as_deref() {
        Some(v) if v.starts_with("yes") => CapabilityReport {
            level: CapabilityLevel::FullMeshCapable,
            detail: format!("{} (this adapter/driver can host a network)", line.trim()),
            checked_via: CHECKED_VIA.to_string(),
        },
        Some(v) if v.starts_with("no") => CapabilityReport {
            level: CapabilityLevel::StationOnly,
            detail: format!(
                "{} (this adapter/driver can only join networks, not host one)",
                line.trim()
            ),
            checked_via: CHECKED_VIA.to_string(),
        },
        _ => CapabilityReport {
            level: CapabilityLevel::StationOnly,
            detail: format!(
                "Could not parse \"{}\" as yes/no; assuming station-only.",
                line.trim()
            ),
            checked_via: CHECKED_VIA.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_supported() {
        let sample = "\
Interface name : Wi-Fi
    Driver                          : Intel(R) Wireless-AC 9560 160MHz
    Vendor                          : Intel Corporation
    Radio types supported          : 802.11a 802.11b 802.11g 802.11n 802.11ac
    Hosted network supported       : Yes
";
        let report = parse_capability(sample);
        assert_eq!(report.level, CapabilityLevel::FullMeshCapable);
    }

    #[test]
    fn recognizes_unsupported() {
        let sample = "\
Interface name : Wi-Fi
    Driver                          : Realtek RTL8188EE
    Hosted network supported       : No
";
        let report = parse_capability(sample);
        assert_eq!(report.level, CapabilityLevel::StationOnly);
    }

    #[test]
    fn missing_line_falls_back_to_station_only() {
        let sample = "Interface name : Wi-Fi\n    Driver : Some Driver\n";
        let report = parse_capability(sample);
        assert_eq!(report.level, CapabilityLevel::StationOnly);
    }

    #[test]
    fn empty_output_is_unsupported() {
        let report = parse_capability("   \n  ");
        assert_eq!(report.level, CapabilityLevel::Unsupported);
    }

    #[test]
    fn ambiguous_value_falls_back_to_station_only_not_panic() {
        let sample = "    Hosted network supported       : Maybe\n";
        let report = parse_capability(sample);
        assert_eq!(report.level, CapabilityLevel::StationOnly);
    }
}
