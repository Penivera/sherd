use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, Database, DatabaseConnection, EntityTrait,
    QueryFilter, QueryOrder, Set,
};
use std::path::{Path, PathBuf};
use thiserror::Error;
use uuid::Uuid;

use crate::auth::entity::{
    auth_identity, oauth_exchange_code, solana_challenge, user,
};

#[derive(Debug, Error)]
pub enum DbError {
    #[error("Database error: {0}")]
    Orm(#[from] sea_orm::DbErr),
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
    pub conn: DatabaseConnection,
}

impl AuthDb {
    pub async fn open_in_memory() -> Result<Self, DbError> {
        let conn = Database::connect("sqlite::memory:").await?;
        let db = Self { conn };
        db.init_schema().await?;
        Ok(db)
    }

    pub async fn open(path: &Path) -> Result<Self, DbError> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let conn = Database::connect(&url).await?;
        let db = Self { conn };
        db.init_schema().await?;
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

    pub async fn open_default() -> Result<Self, DbError> {
        Self::open(&Self::default_path()).await
    }

    async fn init_schema(&self) -> Result<(), DbError> {
        self.conn.execute_unprepared("PRAGMA foreign_keys = ON;").await?;
        self.conn
            .get_schema_registry("desktop::auth::entity")
            .sync(&self.conn)
            .await?;
        Ok(())
    }

    pub async fn create_user_with_email(
        &self,
        email: &str,
        password_hash: &str,
    ) -> Result<UserRecord, DbError> {
        let existing = user::Entity::find()
            .filter(user::Column::Email.eq(email))
            .one(&self.conn)
            .await?;

        if existing.is_some() {
            return Err(DbError::EmailAlreadyExists(email.to_string()));
        }

        let user_id = Uuid::new_v4().to_string();
        let identity_id = Uuid::new_v4().to_string();
        let now = chrono_now_iso8601();

        user::ActiveModel {
            id: Set(user_id.clone()),
            email: Set(Some(email.to_string())),
            password_hash: Set(Some(password_hash.to_string())),
            created_at: Set(now.clone()),
            updated_at: Set(now.clone()),
        }
        .insert(&self.conn)
        .await?;

        auth_identity::ActiveModel {
            id: Set(identity_id),
            user_id: Set(user_id.clone()),
            provider: Set("email".to_string()),
            provider_account_id: Set(email.to_string()),
            provider_metadata: Set(None),
            created_at: Set(now),
        }
        .insert(&self.conn)
        .await?;

        Ok(UserRecord {
            id: user_id,
            email: Some(email.to_string()),
            password_hash: Some(password_hash.to_string()),
            providers: vec!["email".to_string()],
        })
    }

    pub async fn get_user_by_email(&self, email: &str) -> Result<Option<UserRecord>, DbError> {
        let user = user::Entity::find()
            .filter(user::Column::Email.eq(email))
            .one(&self.conn)
            .await?;

        let Some(u) = user else {
            return Ok(None);
        };

        let providers = self.load_providers_internal(&u.id).await?;
        Ok(Some(UserRecord {
            id: u.id,
            email: u.email,
            password_hash: u.password_hash,
            providers,
        }))
    }

    pub async fn get_user_by_id(&self, user_id: &str) -> Result<Option<UserRecord>, DbError> {
        let user = user::Entity::find_by_id(user_id.to_string())
            .one(&self.conn)
            .await?;

        let Some(u) = user else {
            return Ok(None);
        };

        let providers = self.load_providers_internal(&u.id).await?;
        Ok(Some(UserRecord {
            id: u.id,
            email: u.email,
            password_hash: u.password_hash,
            providers,
        }))
    }

    pub async fn find_or_create_user_from_provider(
        &self,
        provider: &str,
        provider_account_id: &str,
        email: Option<&str>,
        metadata: Option<&str>,
    ) -> Result<UserRecord, DbError> {
        // 1. Check if identity already exists
        let existing_identity = auth_identity::Entity::find()
            .filter(auth_identity::Column::Provider.eq(provider))
            .filter(auth_identity::Column::ProviderAccountId.eq(provider_account_id))
            .one(&self.conn)
            .await?;

        if let Some(identity) = existing_identity {
            let user_id = identity.user_id;
            let providers = self.load_providers_internal(&user_id).await?;
            let user = user::Entity::find_by_id(user_id.clone()).one(&self.conn).await?;
            
            return Ok(UserRecord {
                id: user_id,
                email: user.and_then(|u| u.email),
                password_hash: None,
                providers,
            });
        }

        // 2. Check if user with this email already exists
        let mut target_user_id = None;
        if let Some(em) = email {
            let u = user::Entity::find()
                .filter(user::Column::Email.eq(em))
                .one(&self.conn)
                .await?;
            if let Some(u) = u {
                target_user_id = Some(u.id);
            }
        }

        let now = chrono_now_iso8601();
        let user_id = if let Some(uid) = target_user_id {
            uid
        } else {
            let new_uid = Uuid::new_v4().to_string();
            user::ActiveModel {
                id: Set(new_uid.clone()),
                email: Set(email.map(|s| s.to_string())),
                password_hash: Set(None),
                created_at: Set(now.clone()),
                updated_at: Set(now.clone()),
            }
            .insert(&self.conn)
            .await?;
            new_uid
        };

        // Link new identity
        let identity_id = Uuid::new_v4().to_string();
        auth_identity::ActiveModel {
            id: Set(identity_id),
            user_id: Set(user_id.clone()),
            provider: Set(provider.to_string()),
            provider_account_id: Set(provider_account_id.to_string()),
            provider_metadata: Set(metadata.map(|s| s.to_string())),
            created_at: Set(now),
        }
        .insert(&self.conn)
        .await?;

        let providers = self.load_providers_internal(&user_id).await?;
        Ok(UserRecord {
            id: user_id,
            email: email.map(|s| s.to_string()),
            password_hash: None,
            providers,
        })
    }

    pub async fn create_solana_challenge(
        &self,
        wallet_address: &str,
        nonce: &str,
        message: &str,
        expires_at: &str,
    ) -> Result<(), DbError> {
        let id = Uuid::new_v4().to_string();
        let now = chrono_now_iso8601();

        solana_challenge::ActiveModel {
            id: Set(id),
            wallet_address: Set(wallet_address.to_string()),
            nonce: Set(nonce.to_string()),
            message: Set(message.to_string()),
            created_at: Set(now),
            expires_at: Set(expires_at.to_string()),
            consumed_at: Set(None),
        }
        .insert(&self.conn)
        .await?;
        Ok(())
    }

    pub async fn consume_solana_challenge(
        &self,
        nonce: &str,
        wallet_address: &str,
        now: &str,
    ) -> Result<Option<String>, DbError> {
        let update_result = solana_challenge::Entity::update_many()
            .col_expr(solana_challenge::Column::ConsumedAt, sea_orm::sea_query::Expr::value(now.to_string()))
            .filter(solana_challenge::Column::Nonce.eq(nonce))
            .filter(solana_challenge::Column::WalletAddress.eq(wallet_address))
            .filter(solana_challenge::Column::ConsumedAt.is_null())
            .filter(solana_challenge::Column::ExpiresAt.gt(now))
            .exec(&self.conn)
            .await?;

        if update_result.rows_affected != 1 {
            return Ok(None);
        }

        let challenge = solana_challenge::Entity::find()
            .filter(solana_challenge::Column::Nonce.eq(nonce))
            .one(&self.conn)
            .await?;

        Ok(challenge.map(|c| c.message))
    }

    pub async fn create_oauth_exchange_code(
        &self,
        user_id: &str,
        code_hash: &str,
        expires_at: &str,
    ) -> Result<(), DbError> {
        let id = Uuid::new_v4().to_string();
        let now = chrono_now_iso8601();

        oauth_exchange_code::ActiveModel {
            id: Set(id),
            code_hash: Set(code_hash.to_string()),
            user_id: Set(user_id.to_string()),
            created_at: Set(now),
            expires_at: Set(expires_at.to_string()),
            consumed_at: Set(None),
        }
        .insert(&self.conn)
        .await?;
        Ok(())
    }

    pub async fn consume_oauth_exchange_code(
        &self,
        code_hash: &str,
        now: &str,
    ) -> Result<Option<UserRecord>, DbError> {
        let update_result = oauth_exchange_code::Entity::update_many()
            .col_expr(oauth_exchange_code::Column::ConsumedAt, sea_orm::sea_query::Expr::value(now.to_string()))
            .filter(oauth_exchange_code::Column::CodeHash.eq(code_hash))
            .filter(oauth_exchange_code::Column::ConsumedAt.is_null())
            .filter(oauth_exchange_code::Column::ExpiresAt.gt(now))
            .exec(&self.conn)
            .await?;

        if update_result.rows_affected != 1 {
            return Ok(None);
        }

        let code = oauth_exchange_code::Entity::find()
            .filter(oauth_exchange_code::Column::CodeHash.eq(code_hash))
            .one(&self.conn)
            .await?;
            
        let Some(code) = code else {
            return Ok(None);
        };

        let user = user::Entity::find_by_id(code.user_id.clone())
            .one(&self.conn)
            .await?;
            
        let Some(u) = user else {
            return Ok(None);
        };

        let providers = self.load_providers_internal(&u.id).await?;
        Ok(Some(UserRecord {
            id: u.id,
            email: u.email,
            password_hash: None,
            providers,
        }))
    }

    async fn load_providers_internal(
        &self,
        user_id: &str,
    ) -> Result<Vec<String>, DbError> {
        let mut providers = Vec::new();
        let rows = auth_identity::Entity::find()
            .filter(auth_identity::Column::UserId.eq(user_id))
            .order_by_asc(auth_identity::Column::Provider)
            .all(&self.conn)
            .await?;
            
        let mut last_provider = None;
        for row in rows {
            if last_provider != Some(row.provider.clone()) {
                last_provider = Some(row.provider.clone());
                providers.push(row.provider);
            }
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
