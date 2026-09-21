use std::time::Duration;

use platform::{CapabilityLevel, CapabilityReport, LinkState, PlatformBackend};
use protocol::{AutoOutcome, Event, StatusReport};
use tokio::sync::broadcast;

use crate::config::SherdConfig;

/// How many times (500ms apart) to poll station status after a `connect()`
/// call before giving up on confirming the link actually came up. `netsh
/// wlan connect` returns as soon as the request is accepted, not once
/// negotiation finishes, so a real answer needs a short poll.
const CONNECT_POLL_ATTEMPTS: u32 = 10;
const CONNECT_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// A request reached the service but its feature isn't built yet.
#[derive(Debug, Clone, Copy)]
pub struct FeatureNotReady(pub &'static str);

/// Orchestrates the platform backend behind a stable, OS-agnostic API.
/// `sherd-daemon` is the only thing that constructs one of these (choosing
/// the right [`PlatformBackend`] for the current OS) and the only thing that
/// translates [`protocol::Request`]s into calls on it.
pub struct SherdService {
    backend: PlatformBackend,
    config: SherdConfig,
    events_tx: broadcast::Sender<Event>,
}

impl SherdService {
    pub fn new(backend: PlatformBackend, config: SherdConfig) -> Self {
        let (events_tx, _) = broadcast::channel(64);
        Self { backend, config, events_tx }
    }

    pub fn config(&self) -> &SherdConfig {
        &self.config
    }

    /// Subscribe to link/capability/auto-connect events. Every daemon
    /// client connection gets its own receiver.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events_tx.subscribe()
    }

    fn publish(&self, event: Event) {
        // No receivers (e.g. no clients connected yet) is not an error.
        let _ = self.events_tx.send(event);
    }

    pub async fn capability(&self) -> platform::PlatformResult<CapabilityReport> {
        let report = self.backend.capability.check().await?;
        self.publish(Event::CapabilityChanged(report.clone()));
        Ok(report)
    }

    pub async fn status(&self) -> StatusReport {
        let capability = match self.capability().await {
            Ok(report) => report,
            Err(e) => CapabilityReport {
                level: CapabilityLevel::Unsupported,
                detail: format!("capability check failed: {e}"),
                checked_via: "error".to_string(),
            },
        };
        let hotspot = self.backend.hotspot.status().await.ok();
        let station = self.backend.station.status().await.ok();
        StatusReport { capability, hotspot, station }
    }

    pub async fn hotspot_start(&self, ssid: &str, key: &str) -> platform::PlatformResult<()> {
        self.backend.hotspot.start(ssid, key).await?;
        if let Ok(status) = self.backend.hotspot.status().await {
            self.publish(Event::HotspotStatus(status));
        }
        Ok(())
    }

    pub async fn hotspot_stop(&self) -> platform::PlatformResult<()> {
        self.backend.hotspot.stop().await?;
        if let Ok(status) = self.backend.hotspot.status().await {
            self.publish(Event::HotspotStatus(status));
        }
        Ok(())
    }

    pub async fn station_connect(&self, ssid: &str, key: &str) -> platform::PlatformResult<()> {
        self.backend.station.connect(ssid, key).await?;
        if let Ok(status) = self.backend.station.status().await {
            self.publish(Event::StationStatus(status));
        }
        Ok(())
    }

    pub async fn station_disconnect(&self) -> platform::PlatformResult<()> {
        self.backend.station.disconnect().await?;
        if let Ok(status) = self.backend.station.status().await {
            self.publish(Event::StationStatus(status));
        }
        Ok(())
    }

    /// The one flow most users need. **Hosting is never optional on a
    /// capable device**: a sherd mesh's range comes from having a hotspot
    /// running at every node that can run one, not just at whichever single
    /// device happened to start first — so on a
    /// [`CapabilityLevel::FullMeshCapable`] adapter this always (re)starts
    /// this device's own hotspot, unconditionally, regardless of whether any
    /// other sherd network is visible. It *also* tries to join a visible
    /// sherd network as a station at the same time, best-effort, because a
    /// capable adapter can run both roles at once ("full mesh capable" =
    /// concurrent AP + station on one radio) — joining as well as hosting is
    /// what turns this device into a repeater that extends someone else's
    /// hotspot further, rather than just its own separate island. That join
    /// is attempted *before* starting the hotspot so that, when it
    /// succeeds, the hotspot has an actual internet/uplink connection to
    /// tether from (Windows' Mobile Hotspot shares an existing connection —
    /// it doesn't conjure one).
    ///
    /// A station-only device obviously can't host, so for it this is a
    /// plain join-if-visible-else-warn. Also run automatically by the
    /// daemon on startup, and by its watchdog (`sherd_daemon::supervise`)
    /// any time the hotspot (on a capable device) or the station link (on a
    /// station-only device) isn't up.
    pub async fn auto_connect(&self) -> AutoOutcome {
        let capability = match self.capability().await {
            Ok(report) => report,
            Err(e) => {
                return self.finish_auto(AutoOutcome::Unavailable {
                    reason: format!("capability check failed: {e}"),
                });
            }
        };

        if !matches!(capability.level, CapabilityLevel::FullMeshCapable) {
            return match self.join_uplink().await {
                Some(ssid) => self.finish_auto(AutoOutcome::Joined { ssid }),
                None => self.finish_auto(AutoOutcome::Unavailable {
                    reason: "no sherd network is visible nearby, and this device's Wi-Fi \
                             adapter can only join networks, not host one"
                        .to_string(),
                }),
            };
        }

        let uplink = self.join_uplink().await;
        let ssid = self.config.device_ssid.clone();
        match self.hotspot_start(&ssid, &self.config.shared_key).await {
            Ok(()) => self.finish_auto(AutoOutcome::Hosting { ssid, uplink }),
            Err(e) => self.finish_auto(AutoOutcome::Unavailable {
                reason: format!("could not host a network: {e}"),
            }),
        }
    }

    /// Best-effort: try every visible sherd network, strongest signal
    /// first, and join the first one that actually comes up -- a single
    /// candidate failing to join (gone by the time we connect, rejected us,
    /// etc.) shouldn't stop us from trying the next one. Returns the SSID
    /// joined, or `None` if none were visible or none worked.
    async fn join_uplink(&self) -> Option<String> {
        let scan = self.backend.station.scan().await.unwrap_or_else(|e| {
            tracing::warn!("scan for nearby sherd networks failed: {e}");
            Vec::new()
        });

        let mut candidates: Vec<_> = scan
            .iter()
            .filter(|result| result.ssid.starts_with(&self.config.network_prefix))
            // Never try to join the network we ourselves are about to host.
            .filter(|result| result.ssid != self.config.device_ssid)
            .collect();
        candidates.sort_by_key(|result| std::cmp::Reverse(result.signal_percent.unwrap_or(0)));

        for candidate in candidates {
            match self.station_connect(&candidate.ssid, &self.config.shared_key).await {
                Ok(()) if self.wait_for_station_up().await => {
                    return Some(candidate.ssid.clone());
                }
                Ok(()) => {
                    tracing::warn!(
                        "connect to {} was accepted but the link never came up; trying the next one",
                        candidate.ssid
                    );
                }
                Err(e) => tracing::warn!("failed to join {}: {e}; trying the next one", candidate.ssid),
            }
        }
        None
    }

    fn finish_auto(&self, outcome: AutoOutcome) -> AutoOutcome {
        self.publish(Event::AutoResult(outcome.clone()));
        outcome
    }

    async fn wait_for_station_up(&self) -> bool {
        for _ in 0..CONNECT_POLL_ATTEMPTS {
            if let Ok(status) = self.backend.station.status().await {
                if status.state == LinkState::Up {
                    return true;
                }
            }
            tokio::time::sleep(CONNECT_POLL_INTERVAL).await;
        }
        false
    }

    /// Reserved for the mesh-relay milestone (`libp2p` transport). Accepted
    /// today so the protocol/CLI/GUI surface doesn't change when it lands.
    pub fn send_message(&self, _to: &str, _body: &str) -> FeatureNotReady {
        FeatureNotReady("send_message (mesh relay not implemented yet)")
    }

    /// See [`SherdService::send_message`].
    pub fn send_file(&self, _to: &str, _path: &str) -> FeatureNotReady {
        FeatureNotReady("send_file (mesh relay not implemented yet)")
    }
}
