//! This device's own persistent identity: an Ed25519 keypair generated once
//! and saved to disk, so `device_id` (the public key, hex-encoded) is stable
//! across daemon restarts -- unlike the hotspot SSID's random suffix (see
//! `SherdConfig::device_ssid`), which is exactly the gap this closes.
//! `Contact::device_id` in `models.rs` anticipated this: "will likely become
//! a persisted per-install identity" -- this module is that.
//!
//! Deliberately its own thing rather than reusing `mesh::identity::Keypair`
//! (same idea, same primitive -- an Ed25519 keypair with the public key as
//! the ID -- but a separate implementation) since `network` and `mesh` are
//! meant to stay parallel, independent stacks for now.

use std::io;
use std::path::{Path, PathBuf};

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;

/// This device's identity: a keypair plus the human-readable name it
/// introduces itself with over the wire.
pub struct DeviceIdentity {
    signing_key: SigningKey,
    device_id: String,
    display_name: String,
}

impl DeviceIdentity {
    /// Load the keypair at `path`, or generate and save a fresh one if none
    /// exists yet (first run). A corrupt/short key file is treated the same
    /// as missing -- logged and replaced -- rather than failing the daemon
    /// outright, since losing this identity just means peers see this
    /// device as "new" again, not data loss.
    pub fn load_or_create(path: &Path, display_name: String) -> io::Result<Self> {
        let signing_key = match std::fs::read(path) {
            Ok(bytes) if bytes.len() == 32 => {
                let seed: [u8; 32] = bytes.try_into().expect("length checked above");
                SigningKey::from_bytes(&seed)
            }
            Ok(_) => {
                tracing::warn!(
                    "This device's identity file ({}) was damaged, so a new identity was created. \
                     Other devices will see this one as a new contact.",
                    path.display()
                );
                Self::generate_and_save(path)?
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                tracing::info!("First run: created a permanent identity for this device ({}).", path.display());
                Self::generate_and_save(path)?
            }
            Err(e) => {
                tracing::warn!(
                    "Couldn't read this device's identity file ({}: {e}), so a new identity was created. \
                     Other devices will see this one as a new contact.",
                    path.display()
                );
                Self::generate_and_save(path)?
            }
        };

        let device_id = hex_encode(signing_key.verifying_key().as_bytes());
        Ok(Self { signing_key, device_id, display_name })
    }

    fn generate_and_save(path: &Path) -> io::Result<SigningKey> {
        let signing_key = SigningKey::generate(&mut OsRng);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, signing_key.to_bytes())?;
        Ok(signing_key)
    }

    /// This device's stable ID: its Ed25519 public key, hex-encoded.
    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Sign arbitrary bytes with this device's private key (used to prove
    /// possession of the claimed `device_id` during the mailbox handshake).
    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        self.signing_key.sign(message).to_bytes()
    }

    /// Verify a signature was produced by the private key matching
    /// `device_id_hex`. Used to check a peer's `Hello` isn't just someone
    /// claiming another device's ID.
    pub fn verify(device_id_hex: &str, message: &[u8], signature: &[u8; 64]) -> bool {
        let Some(pubkey_bytes) = hex_decode(device_id_hex) else { return false };
        let Ok(pubkey_bytes): Result<[u8; 32], _> = pubkey_bytes.try_into() else { return false };
        let Ok(verifying_key) = VerifyingKey::from_bytes(&pubkey_bytes) else { return false };
        let signature = Signature::from_bytes(signature);
        verifying_key.verify(message, &signature).is_ok()
    }
}

/// The first 8 characters of a `device_id` -- enough to tell devices apart
/// at a glance, and accepted anywhere a full ID is (see
/// `mailbox::PeerRegistry::resolve`). Full IDs are 64 characters.
pub fn short_id(device_id: &str) -> &str {
    &device_id[..device_id.len().min(8)]
}

/// Where this device's identity lives by default: a `sherd` folder under the
/// OS's standard per-user config location (`%APPDATA%` on Windows,
/// `$XDG_CONFIG_HOME` or `~/.config` elsewhere) -- the same convention
/// `desktop`'s `state.rs` already uses for its own settings.
pub fn default_identity_path() -> PathBuf {
    default_data_dir().join("identity.key")
}

pub fn default_data_dir() -> PathBuf {
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("sherd")
    }
    #[cfg(not(windows))]
    {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            PathBuf::from(xdg).join("sherd")
        } else if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(".config").join("sherd")
        } else {
            std::env::temp_dir().join("sherd")
        }
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!("sherd-identity-test-{}", std::process::id()));
        let path = dir.join("identity.key");
        let _ = std::fs::remove_dir_all(&dir);

        let first = DeviceIdentity::load_or_create(&path, "test".to_string()).expect("create");
        let second = DeviceIdentity::load_or_create(&path, "test".to_string()).expect("reload");
        assert_eq!(first.device_id(), second.device_id());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn signature_verifies_against_claimed_id() {
        let dir = std::env::temp_dir().join(format!("sherd-identity-test-sig-{}", std::process::id()));
        let path = dir.join("identity.key");
        let _ = std::fs::remove_dir_all(&dir);

        let identity = DeviceIdentity::load_or_create(&path, "test".to_string()).expect("create");
        let sig = identity.sign(b"hello");
        assert!(DeviceIdentity::verify(identity.device_id(), b"hello", &sig));
        assert!(!DeviceIdentity::verify(identity.device_id(), b"tampered", &sig));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
