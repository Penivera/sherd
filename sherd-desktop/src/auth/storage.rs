use std::sync::{Arc, Mutex};
use keyring::Entry;
use thiserror::Error;

use crate::state::PersistedSession;

const SERVICE_NAME: &str = "com.sherd.desktop";
const ACCOUNT_NAME: &str = "current_session";

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("Keyring error: {0}")]
    Keyring(#[from] keyring::Error),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

#[derive(Clone)]
enum StorageBackend {
    Keyring {
        service: String,
        account: String,
        fallback: Arc<Mutex<Option<String>>>,
    },
    Memory(Arc<Mutex<Option<String>>>),
}

#[derive(Clone)]
pub struct SecureSessionStore {
    backend: StorageBackend,
}

impl Default for SecureSessionStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SecureSessionStore {
    pub fn new() -> Self {
        Self {
            backend: StorageBackend::Keyring {
                service: SERVICE_NAME.to_string(),
                account: ACCOUNT_NAME.to_string(),
                fallback: Arc::new(Mutex::new(None)),
            },
        }
    }

    pub fn with_account(service: &str, account: &str) -> Self {
        Self {
            backend: StorageBackend::Keyring {
                service: service.to_string(),
                account: account.to_string(),
                fallback: Arc::new(Mutex::new(None)),
            },
        }
    }

    pub fn in_memory() -> Self {
        Self {
            backend: StorageBackend::Memory(Arc::new(Mutex::new(None))),
        }
    }

    pub fn service_name(&self) -> &str {
        match &self.backend {
            StorageBackend::Keyring { service, .. } => service,
            StorageBackend::Memory(_) => "memory",
        }
    }

    pub fn account_name(&self) -> &str {
        match &self.backend {
            StorageBackend::Keyring { account, .. } => account,
            StorageBackend::Memory(_) => "memory",
        }
    }

    fn entry(service: &str, account: &str) -> Result<Entry, keyring::Error> {
        Entry::new(service, account)
    }

    pub fn save_session(&self, session: &PersistedSession) -> Result<(), StorageError> {
        let json = serde_json::to_string(session)?;
        match &self.backend {
            StorageBackend::Memory(mem) => {
                let mut guard = mem.lock().unwrap();
                *guard = Some(json);
            }
            StorageBackend::Keyring { service, account, fallback } => {
                let mut guard = fallback.lock().unwrap();
                *guard = Some(json.clone());
                if let Ok(entry) = Self::entry(service, account) {
                    let _ = entry.set_password(&json);
                }
            }
        }
        Ok(())
    }

    pub fn load_session(&self) -> Result<Option<PersistedSession>, StorageError> {
        let raw_json = match &self.backend {
            StorageBackend::Memory(mem) => {
                let guard = mem.lock().unwrap();
                guard.clone()
            }
            StorageBackend::Keyring { service, account, fallback } => {
                let from_keyring = Self::entry(service, account)
                    .ok()
                    .and_then(|entry| entry.get_password().ok());
                if let Some(pw) = from_keyring {
                    Some(pw)
                } else {
                    let guard = fallback.lock().unwrap();
                    guard.clone()
                }
            }
        };

        let json_str = match raw_json {
            Some(s) => s,
            None => return Ok(None),
        };

        let session: PersistedSession = match serde_json::from_str(&json_str) {
            Ok(s) => s,
            Err(_) => {
                self.clear_session()?;
                return Ok(None);
            }
        };

        if session.is_expired() {
            self.clear_session()?;
            return Ok(None);
        }

        Ok(Some(session))
    }

    pub fn clear_session(&self) -> Result<(), StorageError> {
        match &self.backend {
            StorageBackend::Memory(mem) => {
                let mut guard = mem.lock().unwrap();
                *guard = None;
            }
            StorageBackend::Keyring { service, account, fallback } => {
                let mut guard = fallback.lock().unwrap();
                *guard = None;
                if let Ok(entry) = Self::entry(service, account) {
                    let _ = entry.delete_credential();
                }
            }
        }
        Ok(())
    }
}
