use std::sync::Arc;
use thiserror::Error;
use tracing::info;

use crate::auth::db::DbError;
use crate::auth::email::EmailAuthError;
use crate::auth::engine::{AuthEngine, AuthEngineError};
use crate::auth::oauth::{OAuthError, OAuthProvider};
use crate::auth::solana::SolanaAuthError;
use crate::auth::storage::{SecureSessionStore, StorageError};
use crate::auth::token::TokenError;
use crate::auth::wallet::{SolanaSigner, WalletError};
use crate::state::PersistedSession;

#[derive(Debug, Error)]
pub enum AuthManagerError {
    #[error("Authentication engine error: {0}")]
    Engine(#[from] AuthEngineError),
    #[error("Database error: {0}")]
    Db(#[from] DbError),
    #[error("Email auth error: {0}")]
    Email(#[from] EmailAuthError),
    #[error("Solana auth error: {0}")]
    Solana(#[from] SolanaAuthError),
    #[error("OAuth error: {0}")]
    OAuth(#[from] OAuthError),
    #[error("Token error: {0}")]
    Token(#[from] TokenError),
    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),
    #[error("Wallet error: {0}")]
    Wallet(#[from] WalletError),
    #[error("Unsupported OAuth provider: {0}")]
    UnsupportedProvider(String),
}

pub struct AuthManager {
    engine: Arc<AuthEngine>,
    storage: SecureSessionStore,
}


impl AuthManager {
    pub async fn new() -> Result<Self, AuthManagerError> {
        let engine = Arc::new(AuthEngine::new_default().await?);
        let storage = SecureSessionStore::new();
        Ok(Self { engine, storage })
    }

    pub fn with_engine(engine: Arc<AuthEngine>) -> Self {
        Self {
            engine,
            storage: SecureSessionStore::new(),
        }
    }

    pub async fn in_memory() -> Result<Self, AuthManagerError> {
        let engine = Arc::new(AuthEngine::new_in_memory().await?);
        let storage = SecureSessionStore::in_memory();
        Ok(Self { engine, storage })
    }

    pub async fn in_memory_with_account(_account: &str) -> Result<Self, AuthManagerError> {
        let engine = Arc::new(AuthEngine::new_in_memory().await?);
        let storage = SecureSessionStore::in_memory();
        Ok(Self { engine, storage })
    }

    pub fn with_storage(engine: Arc<AuthEngine>, storage: SecureSessionStore) -> Self {
        Self { engine, storage }
    }

    pub fn engine(&self) -> &AuthEngine {
        &self.engine
    }

    pub fn storage(&self) -> &SecureSessionStore {
        &self.storage
    }

    pub fn load_cached_session(&self) -> Option<PersistedSession> {
        self.storage.load_session().ok().flatten()
    }

    pub fn clear_cached_session(&self) -> Result<(), StorageError> {
        self.storage.clear_session()
    }

    pub async fn login_email(
        &self,
        email: &str,
        password: &str,
    ) -> Result<PersistedSession, AuthManagerError> {
        info!("Executing native email login");
        let session = self.engine.login_email(email, password).await?;
        self.storage.save_session(&session)?;
        info!("Email login successful; session persisted");
        Ok(session)
    }

    pub async fn register_email(
        &self,
        email: &str,
        password: &str,
    ) -> Result<PersistedSession, AuthManagerError> {
        info!("Executing native email registration");
        let session = self.engine.register_email(email, password).await?;
        self.storage.save_session(&session)?;
        info!("Email registration successful; session persisted");
        Ok(session)
    }

    pub async fn login_solana(&self) -> Result<(PersistedSession, String), AuthManagerError> {
        let signer = SolanaSigner::new_ephemeral();
        let wallet_address = signer.public_key_b58();
        let session = self.login_solana_with_signer(&signer).await?;
        Ok((session, wallet_address))
    }

    pub async fn login_solana_with_signer(
        &self,
        signer: &SolanaSigner,
    ) -> Result<PersistedSession, AuthManagerError> {
        info!("Executing native Solana authentication");
        let wallet_address = signer.public_key_b58();

        let challenge = self.engine.create_solana_challenge(&wallet_address).await?;
        let signature = signer.sign_message(&challenge.message)?;

        let session = self.engine.verify_solana_signature(
            &wallet_address,
            &challenge.nonce,
            &signature,
        ).await?;

        self.storage.save_session(&session)?;
        info!("Solana authentication successful; session persisted");
        Ok(session)
    }

    pub async fn login_oauth(
        &self,
        provider_name: &str,
    ) -> Result<PersistedSession, AuthManagerError> {
        info!("Executing native OAuth login for: {}", provider_name);
        let provider = match provider_name.to_lowercase().as_str() {
            "google" => OAuthProvider::Google,
            "github" => OAuthProvider::GitHub,
            other => return Err(AuthManagerError::UnsupportedProvider(other.to_string())),
        };

        let session = self.engine.authenticate_oauth(provider).await?;
        self.storage.save_session(&session)?;
        info!("OAuth login successful; session persisted");
        Ok(session)
    }

    pub async fn logout(&self) -> Result<(), AuthManagerError> {
        self.clear_cached_session()?;
        Ok(())
    }
}
