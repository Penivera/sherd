use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use thiserror::Error;

use crate::auth::db::{AuthDb, DbError, UserRecord};
use crate::auth::token::{TokenError, TokenManager};

#[derive(Debug, Error)]
pub enum EmailAuthError {
    #[error("Database error: {0}")]
    Db(#[from] DbError),
    #[error("Token error: {0}")]
    Token(#[from] TokenError),
    #[error("Invalid email format")]
    InvalidEmail,
    #[error("Password must be at least 8 characters")]
    PasswordTooShort,
    #[error("Email already registered")]
    EmailAlreadyExists,
    #[error("Invalid email or password")]
    InvalidCredentials,
    #[error("Password hashing error: {0}")]
    Hashing(String),
}

pub struct EmailAuthService {
    db: AuthDb,
    token_manager: TokenManager,
}

impl EmailAuthService {
    pub fn new(db: AuthDb, token_manager: TokenManager) -> Self {
        Self { db, token_manager }
    }

    pub fn hash_password(password: &str) -> Result<String, EmailAuthError> {
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        argon2
            .hash_password(password.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|e| EmailAuthError::Hashing(e.to_string()))
    }

    pub fn verify_password(plain_password: &str, password_hash: &str) -> bool {
        let Ok(parsed_hash) = PasswordHash::new(password_hash) else {
            return false;
        };
        Argon2::default()
            .verify_password(plain_password.as_bytes(), &parsed_hash)
            .is_ok()
    }

    pub async fn register(
        &self,
        email: &str,
        password: &str,
    ) -> Result<(UserRecord, String, i64), EmailAuthError> {
        let email = email.trim().to_lowercase();
        if !is_valid_email(&email) {
            return Err(EmailAuthError::InvalidEmail);
        }
        if password.len() < 8 {
            return Err(EmailAuthError::PasswordTooShort);
        }

        let password_hash = Self::hash_password(password)?;
        let user = match self.db.create_user_with_email(&email, &password_hash).await {
            Ok(u) => u,
            Err(DbError::EmailAlreadyExists(_)) => return Err(EmailAuthError::EmailAlreadyExists),
            Err(e) => return Err(EmailAuthError::Db(e)),
        };

        let (token, exp_ms) = self.token_manager.create_token(&user.id)?;
        Ok((user, token, exp_ms))
    }

    pub async fn login(
        &self,
        email: &str,
        password: &str,
    ) -> Result<(UserRecord, String, i64), EmailAuthError> {
        let email = email.trim().to_lowercase();
        let user = self
            .db
            .get_user_by_email(&email)
            .await?
            .ok_or(EmailAuthError::InvalidCredentials)?;

        let hash = user
            .password_hash
            .as_deref()
            .ok_or(EmailAuthError::InvalidCredentials)?;

        if !Self::verify_password(password, hash) {
            return Err(EmailAuthError::InvalidCredentials);
        }

        let (token, exp_ms) = self.token_manager.create_token(&user.id)?;
        Ok((user, token, exp_ms))
    }
}

fn is_valid_email(email: &str) -> bool {
    if email.len() < 3 || email.len() > 320 {
        return false;
    }
    let parts: Vec<&str> = email.split('@').collect();
    if parts.len() != 2 {
        return false;
    }
    let (local, domain) = (parts[0], parts[1]);
    !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
}
