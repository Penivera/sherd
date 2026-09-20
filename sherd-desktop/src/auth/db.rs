use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("Database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Email already registered: {0}")]
    EmailAlreadyExists(String),
    #[error("User not found")]
    UserNotFound,
}

#[derive(Debug, Clone)]
pub struct UserRecord {
    pub id: String,
    pub email: Option<String>,
    pub password_hash: Option<String>,
    pub providers: Vec<String>,
}

#[derive(Clone)]
pub struct AuthDb {
    conn: Arc<Mutex<Connection>>,
}

impl AuthDb {
    pub fn open_in_memory() -> Result<Self, DbError> {
        let conn = Connection::open_in_memory()?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.init_schema()?;
        Ok(db)
    }

    pub fn open(path: &Path) -> Result<Self, DbError> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.init_schema()?;
        Ok(db)
    }

    pub fn default_path() -> PathBuf {
        if let Ok(data_dir) = std::env::var("SHERD_DATA_DIR") {
            return PathBuf::from(data_dir).join("auth.db");
        }

        if let Ok(home) = std::env::var("HOME") {
            let p = PathBuf::from(home).join(".local/share/sherd/auth.db");
            if let Some(parent) = p.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            return p;
        }

        PathBuf::from("sherd_auth.db")
    }

    pub fn open_default() -> Result<Self, DbError> {
        Self::open(&Self::default_path())
    }

    fn init_schema(&self) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS users (
                id TEXT PRIMARY KEY,
                email TEXT UNIQUE,
                password_hash TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS auth_identities (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                provider TEXT NOT NULL,
                provider_account_id TEXT NOT NULL,
                provider_metadata TEXT,
                created_at TEXT NOT NULL,
                UNIQUE (provider, provider_account_id)
            );

            CREATE TABLE IF NOT EXISTS solana_challenges (
                id TEXT PRIMARY KEY,
                wallet_address TEXT NOT NULL,
                nonce TEXT NOT NULL UNIQUE,
                message TEXT NOT NULL,
                created_at TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                consumed_at TEXT
            );

            CREATE TABLE IF NOT EXISTS oauth_exchange_codes (
                id TEXT PRIMARY KEY,
                code_hash TEXT NOT NULL UNIQUE,
                user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                created_at TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                consumed_at TEXT
            );
            ",
        )?;
        Ok(())
    }

    pub fn create_user_with_email(
        &self,
        email: &str,
        password_hash: &str,
    ) -> Result<UserRecord, DbError> {
        let conn = self.conn.lock().unwrap();
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM users WHERE email = ?1",
                params![email],
                |row| row.get(0),
            )
            .optional()?;

        if existing.is_some() {
            return Err(DbError::EmailAlreadyExists(email.to_string()));
        }

        let user_id = Uuid::new_v4().to_string();
        let identity_id = Uuid::new_v4().to_string();
        let now = chrono_now_iso8601();

        conn.execute(
            "INSERT INTO users (id, email, password_hash, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![user_id, email, password_hash, now, now],
        )?;

        conn.execute(
            "INSERT INTO auth_identities (id, user_id, provider, provider_account_id, created_at) VALUES (?1, ?2, 'email', ?3, ?4)",
            params![identity_id, user_id, email, now],
        )?;

        Ok(UserRecord {
            id: user_id,
            email: Some(email.to_string()),
            password_hash: Some(password_hash.to_string()),
            providers: vec!["email".to_string()],
        })
    }

    pub fn get_user_by_email(&self, email: &str) -> Result<Option<UserRecord>, DbError> {
        let conn = self.conn.lock().unwrap();
        let user = conn
            .query_row(
                "SELECT id, email, password_hash FROM users WHERE email = ?1",
                params![email],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .optional()?;

        let Some((id, email, password_hash)) = user else {
            return Ok(None);
        };

        let providers = self.load_providers_internal(&conn, &id)?;
        Ok(Some(UserRecord {
            id,
            email,
            password_hash,
            providers,
        }))
    }

    pub fn get_user_by_id(&self, user_id: &str) -> Result<Option<UserRecord>, DbError> {
        let conn = self.conn.lock().unwrap();
        let user = conn
            .query_row(
                "SELECT id, email, password_hash FROM users WHERE id = ?1",
                params![user_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .optional()?;

        let Some((id, email, password_hash)) = user else {
            return Ok(None);
        };

        let providers = self.load_providers_internal(&conn, &id)?;
        Ok(Some(UserRecord {
            id,
            email,
            password_hash,
            providers,
        }))
    }

    pub fn find_or_create_user_from_provider(
        &self,
        provider: &str,
        provider_account_id: &str,
        email: Option<&str>,
        metadata: Option<&str>,
    ) -> Result<UserRecord, DbError> {
        let conn = self.conn.lock().unwrap();

        // 1. Check if identity already exists
        let existing_user_id: Option<String> = conn
            .query_row(
                "SELECT user_id FROM auth_identities WHERE provider = ?1 AND provider_account_id = ?2",
                params![provider, provider_account_id],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(user_id) = existing_user_id {
            let providers = self.load_providers_internal(&conn, &user_id)?;
            let user_email: Option<String> = conn
                .query_row(
                    "SELECT email FROM users WHERE id = ?1",
                    params![user_id],
                    |row| row.get(0),
                )
                .optional()?;

            return Ok(UserRecord {
                id: user_id,
                email: user_email,
                password_hash: None,
                providers,
            });
        }

        // 2. Check if user with this email already exists
        let mut target_user_id = None;
        if let Some(em) = email {
            target_user_id = conn
                .query_row(
                    "SELECT id FROM users WHERE email = ?1",
                    params![em],
                    |row| row.get(0),
                )
                .optional()?;
        }

        let now = chrono_now_iso8601();
        let user_id = if let Some(uid) = target_user_id {
            uid
        } else {
            let new_uid = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO users (id, email, password_hash, created_at, updated_at) VALUES (?1, ?2, NULL, ?3, ?4)",
                params![new_uid, email, now, now],
            )?;
            new_uid
        };

        // Link new identity
        let identity_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO auth_identities (id, user_id, provider, provider_account_id, provider_metadata, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![identity_id, user_id, provider, provider_account_id, metadata, now],
        )?;

        let providers = self.load_providers_internal(&conn, &user_id)?;
        Ok(UserRecord {
            id: user_id,
            email: email.map(|s| s.to_string()),
            password_hash: None,
            providers,
        })
    }

    pub fn create_solana_challenge(
        &self,
        wallet_address: &str,
        nonce: &str,
        message: &str,
        expires_at: &str,
    ) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        let id = Uuid::new_v4().to_string();
        let now = chrono_now_iso8601();

        conn.execute(
            "INSERT INTO solana_challenges (id, wallet_address, nonce, message, created_at, expires_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, wallet_address, nonce, message, now, expires_at],
        )?;
        Ok(())
    }

    pub fn consume_solana_challenge(
        &self,
        nonce: &str,
        wallet_address: &str,
        now: &str,
    ) -> Result<Option<String>, DbError> {
        let conn = self.conn.lock().unwrap();

        // Atomically update consumed_at only if unconsumed, unexpired, and matching wallet
        let rows_affected = conn.execute(
            "UPDATE solana_challenges SET consumed_at = ?1
             WHERE nonce = ?2 AND wallet_address = ?3 AND consumed_at IS NULL AND expires_at > ?1",
            params![now, nonce, wallet_address],
        )?;

        if rows_affected != 1 {
            return Ok(None);
        }

        let message: String = conn.query_row(
            "SELECT message FROM solana_challenges WHERE nonce = ?1",
            params![nonce],
            |row| row.get(0),
        )?;

        Ok(Some(message))
    }

    pub fn create_oauth_exchange_code(
        &self,
        user_id: &str,
        code_hash: &str,
        expires_at: &str,
    ) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        let id = Uuid::new_v4().to_string();
        let now = chrono_now_iso8601();

        conn.execute(
            "INSERT INTO oauth_exchange_codes (id, code_hash, user_id, created_at, expires_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, code_hash, user_id, now, expires_at],
        )?;
        Ok(())
    }

    pub fn consume_oauth_exchange_code(
        &self,
        code_hash: &str,
        now: &str,
    ) -> Result<Option<UserRecord>, DbError> {
        let conn = self.conn.lock().unwrap();

        let rows_affected = conn.execute(
            "UPDATE oauth_exchange_codes SET consumed_at = ?1
             WHERE code_hash = ?2 AND consumed_at IS NULL AND expires_at > ?1",
            params![now, code_hash],
        )?;

        if rows_affected != 1 {
            return Ok(None);
        }

        let user_id: String = conn.query_row(
            "SELECT user_id FROM oauth_exchange_codes WHERE code_hash = ?1",
            params![code_hash],
            |row| row.get(0),
        )?;

        let (id, email): (String, Option<String>) = conn.query_row(
            "SELECT id, email FROM users WHERE id = ?1",
            params![user_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;

        let providers = self.load_providers_internal(&conn, &id)?;
        Ok(Some(UserRecord {
            id,
            email,
            password_hash: None,
            providers,
        }))
    }

    fn load_providers_internal(
        &self,
        conn: &Connection,
        user_id: &str,
    ) -> Result<Vec<String>, DbError> {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT provider FROM auth_identities WHERE user_id = ?1 ORDER BY provider",
        )?;
        let rows = stmt.query_map(params![user_id], |row| row.get(0))?;
        let mut providers = Vec::new();
        for p in rows {
            providers.push(p?);
        }
        Ok(providers)
    }
}

fn chrono_now_iso8601() -> String {
    let now = std::time::SystemTime::now();
    let duration = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();

    // Format ISO8601 UTC timestamp: YYYY-MM-DDTHH:MM:SSZ
    format_timestamp_iso8601(secs)
}

pub fn format_timestamp_iso8601(secs: u64) -> String {
    // Simple pure leap-year civil calendar calculation
    let days = secs / 86400;
    let rem_secs = secs % 86400;
    let hours = rem_secs / 3600;
    let minutes = (rem_secs % 3600) / 60;
    let seconds = rem_secs % 60;

    let (year, month, day) = days_to_date(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

fn days_to_date(days_since_epoch: u64) -> (i64, u32, u32) {
    let z = days_since_epoch as i64 + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}
