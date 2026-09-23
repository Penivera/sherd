use std::time::Duration;

/// Runtime configuration for the "join if possible, else host" flow and for
/// naming this device's own network.
///
/// **Known limitation / TODO**: `shared_key` is a single passphrase baked
/// into every sherd install so that any two sherd devices can find and join
/// each other with zero setup, which is the whole point of the "just
/// works" auto-connect flow the user asked for. That also means, as shipped,
/// any device running this software can join any other's hotspot — there is
/// no pairing or per-mesh secret yet. Replacing this with a
/// configured/paired passphrase (env var, config file, or a QR-code-style
/// pairing flow) is necessary before this leaves a trusted/testing setting.
#[derive(Debug, Clone)]
pub struct SherdConfig {
    /// SSID prefix that marks a network as a sherd network worth joining,
    /// and that this device's own hotspot SSID will also start with.
    pub network_prefix: String,
    /// Shared Wi-Fi passphrase used both to host and to join. See the
    /// limitation above.
    pub shared_key: String,
    /// This device's own hotspot SSID. The default here is a random
    /// placeholder; `SherdService::new` replaces it with one derived from
    /// the device's permanent identity (e.g. `Sherd-1B13A2`), so the
    /// hotspot keeps the same name across restarts and other devices'
    /// saved Wi-Fi profiles for it keep working.
    pub device_ssid: String,
    /// How often the daemon's background watchdog checks that a link
    /// (station or hotspot) is still up, retrying `auto_connect` if neither
    /// is -- what keeps this device "always on": connected to some sherd
    /// network if possible, hosting its own otherwise. See
    /// `sherd_daemon::supervise`.
    pub watchdog_interval: Duration,
    /// Human-readable name this device introduces itself with to other
    /// sherd devices (see `identity::DeviceIdentity`) -- what shows up as
    /// the sender in a received message, not a login/account name.
    pub display_name: String,
}

impl Default for SherdConfig {
    fn default() -> Self {
        let network_prefix = "Sherd".to_string();
        let device_ssid = format!("{network_prefix}-{}", random_suffix());
        Self {
            network_prefix,
            shared_key: "sherd-mesh-default".to_string(),
            device_ssid,
            watchdog_interval: Duration::from_secs(15),
            display_name: default_display_name(),
        }
    }
}

/// Falls back through the environment variables Windows/Linux/macOS
/// actually set for "this machine's name" before giving up on a generic
/// placeholder -- there's no crate-free portable way to ask the OS
/// directly, and pulling one in just for a friendly default isn't worth it.
fn default_display_name() -> String {
    for var in ["COMPUTERNAME", "HOSTNAME"] {
        if let Ok(name) = std::env::var(var) {
            if !name.trim().is_empty() {
                return name;
            }
        }
    }
    "sherd-device".to_string()
}

/// A short, human-friendly, per-process-random suffix for this device's
/// default SSID (e.g. "A1F4"), so two sherd devices hosting at once don't
/// collide. Not persisted across restarts yet — see the module TODO list in
/// the plan about giving each device a stable identity.
///
/// Uses `RandomState`'s OS-seeded keys rather than pulling in a `rand`
/// dependency for four hex digits.
fn random_suffix() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    let value = RandomState::new().build_hasher().finish();
    format!("{:04X}", (value & 0xFFFF) as u16)
}
