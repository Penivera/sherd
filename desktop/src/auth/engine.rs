use std::sync::Arc;
use thiserror::Error;

use crate::auth::db::{AuthDb, DbError};
use crate::auth::email::{EmailAuthError, EmailAuthService};
use crate::auth::oauth::{OAuthConfig, OAuthError, OAuthProvider, OAuthService};
use crate::auth::solana::{SolanaAuthError, SolanaAuthService, SolanaChallengeResult};
use crate::auth::token::{TokenError, TokenManager};
use crate::state::{PersistedSession, UserProfile};

#[derive(Debug, Error)]
pub enum AuthEngineError {
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
    #[error("User not found")]
    UserNotFound,
}

#[derive(Clone)]
pub struct AuthEngine {
    db: AuthDb,
    token_manager: Arc<TokenManager>,
    email_service: Arc<EmailAuthService>,
    solana_service: Arc<SolanaAuthService>,
    oauth_service: Arc<OAuthService>,
}

impl AuthEngine {
    pub fn new(db: AuthDb) -> Self {
        let token_manager = Arc::new(TokenManager::default());
        let email_service = Arc::new(EmailAuthService::new(db.clone(), TokenManager::default()));
        let solana_service =
            Arc::new(SolanaAuthService::new(db.clone(), TokenManager::default()));
        let oauth_service = Arc::new(OAuthService::new(
            db.clone(),
            TokenManager::default(),
            OAuthConfig::default(),
        ));

        Self {
            db,
            token_manager,
            email_service,
            solana_service,
            oauth_service,
        }
    }

    pub fn new_in_memory() -> Result<Self, AuthEngineError> {
        let db = AuthDb::open_in_memory()?;
        Ok(Self::new(db))
    }

    pub fn new_default() -> Result<Self, AuthEngineError> {
        let db = AuthDb::open_default()?;
        Ok(Self::new(db))
    }

    pub fn register_email(
        &self,
        email: &str,
        password: &str,
    ) -> Result<PersistedSession, AuthEngineError> {
        let (user, token, exp_ms) = self.email_service.register(email, password)?;
        Ok(PersistedSession {
            access_token: token,
            user: UserProfile {
                id: user.id,
                email: user.email,
                providers: user.providers,
            },
            expires_at: Some(exp_ms),
        })
    }

    pub fn login_email(
        &self,
        email: &str,
        password: &str,
    ) -> Result<PersistedSession, AuthEngineError> {
        let (user, token, exp_ms) = self.email_service.login(email, password)?;
        Ok(PersistedSession {
            access_token: token,
            user: UserProfile {
                id: user.id,
                email: user.email,
                providers: user.providers,
            },
            expires_at: Some(exp_ms),
        })
    }

    pub fn create_solana_challenge(
        &self,
        wallet_address: &str,
    ) -> Result<SolanaChallengeResult, AuthEngineError> {
        Ok(self.solana_service.create_challenge(wallet_address)?)
    }

    pub fn verify_solana_signature(
        &self,
        wallet_address: &str,
        nonce: &str,
        signature: &str,
    ) -> Result<PersistedSession, AuthEngineError> {
        let (user, token, exp_ms) =
            self.solana_service.verify(wallet_address, nonce, signature)?;
        Ok(PersistedSession {
            access_token: token,
            user: UserProfile {
                id: user.id,
                email: user.email,
                providers: user.providers,
            },
            expires_at: Some(exp_ms),
        })
    }

    pub async fn authenticate_oauth(
        &self,
        provider: OAuthProvider,
    ) -> Result<PersistedSession, AuthEngineError> {
        let (user, token, exp_ms) = self.oauth_service.authenticate(provider).await?;
        Ok(PersistedSession {
            access_token: token,
            user: UserProfile {
                id: user.id,
                email: user.email,
                providers: user.providers,
            },
            expires_at: Some(exp_ms),
        })
    }

    pub fn get_user_by_id(&self, user_id: &str) -> Result<Option<UserProfile>, AuthEngineError> {
        let user = self.db.get_user_by_id(user_id)?;
        Ok(user.map(|u| UserProfile {
            id: u.id,
            email: u.email,
            providers: u.providers,
        }))
    }

    pub fn verify_token(&self, token: &str) -> Result<String, AuthEngineError> {
        Ok(self.token_manager.verify_token(token)?)
    }
}
