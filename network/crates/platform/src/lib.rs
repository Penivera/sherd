//! OS-agnostic contracts for everything sherd needs from the underlying
//! network stack: "can this device run an access point while staying
//! connected upstream?", "join an existing network", "host one".
//!
//! Nothing in this crate knows about Windows, Linux, `netsh`, or `iw`. Each
//! platform gets its own crate (e.g. `sherd-platform-windows`) that
//! implements these traits. `sherd-core` only ever talks to `Box<dyn Trait>`,
//! so adding a new platform backend never requires touching `sherd-core`,
//! the daemon, the wire protocol, or any client.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// How capable this device's Wi-Fi stack is of running sherd's full feature
/// set (simultaneous station + access point on one radio).
///
/// A device that can only connect to other people's hotspots is still a
/// first-class citizen — it just can't host one itself. Callers should warn,
/// not refuse, on [`CapabilityLevel::StationOnly`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CapabilityLevel {
    /// Driver/adapter supports running an access point concurrently with a
    /// station connection (or a dedicated AP-capable radio exists).
    FullMeshCapable,
    /// Only station (client) mode is available. Sending/receiving as a
    /// client of someone else's hotspot still works; this device cannot
    /// host its own.
    StationOnly,
    /// No usable Wi-Fi interface was found at all.
    Unsupported,
}

impl CapabilityLevel {
    pub fn is_warning(self) -> bool {
        !matches!(self, CapabilityLevel::FullMeshCapable)
    }
}

/// Result of probing this device's Wi-Fi capability, including *how* it was
/// determined so the UI/logs can explain a surprising verdict.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityReport {
    pub level: CapabilityLevel,
    /// Human-readable explanation, e.g. "Hosted network supported: No
    /// (netsh wlan show drivers)".
    pub detail: String,
    /// What mechanism produced this verdict, e.g. "netsh" or "WlanAPI".
    pub checked_via: String,
}

/// A single Wi-Fi-capable network interface on this machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceInfo {
    pub description: String,
    /// Stable identifier for the interface (GUID on Windows, ifname on
    /// Linux later). Opaque to callers outside the platform crate.
    pub id: String,
}

/// Coarse lifecycle state of a hotspot or station link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LinkState {
    Down,
    Starting,
    Up,
    Stopping,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkStatus {
    pub state: LinkState,
    pub ssid: Option<String>,
    pub detail: String,
}

impl LinkStatus {
    pub fn down(detail: impl Into<String>) -> Self {
        Self { state: LinkState::Down, ssid: None, detail: detail.into() }
    }
}

#[derive(Debug, Error)]
pub enum PlatformError {
    /// This platform backend hasn't implemented the operation yet (e.g. the
    /// Linux stub, today).
    #[error("not implemented on this platform yet")]
    NotImplemented,
    /// The operation is implemented but this device/driver can't do it
    /// (e.g. hosted network on an adapter without Virtual Wifi support).
    /// Carries enough detail for the caller to surface a warning.
    #[error("not supported by this device or driver: {0}")]
    Unsupported(String),
    /// An underlying OS command or API call ran but reported failure.
    #[error("operation failed: {0}")]
    CommandFailed(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type PlatformResult<T> = Result<T, PlatformError>;

/// Probes whether this device can run sherd's full STA+AP feature set.
#[async_trait]
pub trait WifiCapabilityChecker: Send + Sync {
    async fn check(&self) -> PlatformResult<CapabilityReport>;
}

/// Controls an access point ("hotspot") hosted by this device.
///
/// Implementations that can't host at all (see [`CapabilityLevel::StationOnly`])
/// should still exist and return [`PlatformError::Unsupported`] from `start`
/// rather than not existing — this keeps `sherd-core` from needing an
/// `Option<dyn HotspotController>` special case.
#[async_trait]
pub trait HotspotController: Send + Sync {
    async fn start(&self, ssid: &str, key: &str) -> PlatformResult<()>;
    /// Turn the hotspot off. Implementations that changed the OS's own
    /// hotspot settings in `start` (name/password) should put the user's
    /// original settings back here, so running sherd doesn't permanently
    /// rename someone's Mobile Hotspot. Calling this when the hotspot is
    /// already off is not an error.
    async fn stop(&self) -> PlatformResult<()>;
    async fn status(&self) -> PlatformResult<LinkStatus>;
    /// Name of the connection the hotspot is sharing onward (the Wi-Fi
    /// network or Ethernet connection this device gets its own internet
    /// from), when the backend can tell. Used to warn the user that their
    /// internet is being shared with the mesh.
    async fn upstream_name(&self) -> Option<String> {
        None
    }
    /// The name and password the hotspot is currently configured with,
    /// when the backend can read them -- so the daemon can notice someone
    /// renaming it (or changing its password) and put it back, since
    /// either change cuts every other device off from it.
    async fn configured(&self) -> Option<(String, String)> {
        None
    }
}

impl PlatformError {
    /// The underlying reason, without the category prefix `Display` adds
    /// ("operation failed: ...") -- for building human-facing sentences
    /// where that prefix just reads as noise.
    pub fn reason(&self) -> String {
        match self {
            PlatformError::NotImplemented => "not implemented on this platform yet".to_string(),
            PlatformError::Unsupported(r) | PlatformError::CommandFailed(r) => r.clone(),
            PlatformError::Io(e) => e.to_string(),
        }
    }
}

/// One network seen in a scan, before deciding whether to join it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub ssid: String,
    /// 0-100 signal quality when the backend can report it.
    pub signal_percent: Option<u8>,
}

/// Connects this device to someone else's network as a client.
#[async_trait]
pub trait StationConnector: Send + Sync {
    /// List currently visible networks. Used by the "join if one exists,
    /// else host my own" auto-connect flow to look for other sherd devices
    /// before deciding to host.
    async fn scan(&self) -> PlatformResult<Vec<ScanResult>>;
    async fn connect(&self, ssid: &str, key: &str) -> PlatformResult<()>;
    async fn disconnect(&self) -> PlatformResult<()>;
    async fn status(&self) -> PlatformResult<LinkStatus>;
    /// Whether the Wi-Fi radio is switched on, when the backend can tell.
    async fn is_radio_on(&self) -> Option<bool> {
        None
    }
    /// Switch the Wi-Fi radio on (e.g. after someone turned Wi-Fi off).
    async fn turn_radio_on(&self) -> PlatformResult<()> {
        Err(PlatformError::NotImplemented)
    }
}

/// Lists the Wi-Fi interfaces this machine has. Mostly diagnostic today;
/// will matter once a device might run multiple radios.
#[async_trait]
pub trait InterfaceEnumerator: Send + Sync {
    async fn list(&self) -> PlatformResult<Vec<InterfaceInfo>>;
    /// The broadcast address of every active IPv4 network this device is
    /// on (e.g. `192.168.137.255` for its own hotspot's subnet), so
    /// discovery can announce on all of them. A plain `255.255.255.255`
    /// broadcast only leaves through one interface, which on a device that
    /// both hosts a hotspot and is on another network means devices on one
    /// of the two never hear it.
    async fn ipv4_broadcast_addresses(&self) -> Vec<std::net::Ipv4Addr> {
        Vec::new()
    }
}

/// Bundles the four traits a platform backend must provide. `sherd-core`
/// takes one of these rather than four separate boxes.
pub struct PlatformBackend {
    pub capability: Box<dyn WifiCapabilityChecker>,
    pub hotspot: Box<dyn HotspotController>,
    pub station: Box<dyn StationConnector>,
    pub interfaces: Box<dyn InterfaceEnumerator>,
}
