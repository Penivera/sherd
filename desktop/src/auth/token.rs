use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

pub const DEFAULT_JWT_SECRET: &str =
    "08b88e2575367d7c8d2b388f74f36693355de3a6c296d5434f12d64d7f4420b5";
pub const DEFAULT_EXPIRE_MINUTES: u64 = 60;

#[derive(Debug, Error)]
pub enum TokenError {
    #[error("JWT error: {0}")]
    Jwt(#[from] jsonwebtoken::errors::Error),
    #[error("Invalid token claims")]
    InvalidClaims,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub exp: u64,
}

pub struct TokenManager {
    secret: String,
    expire_minutes: u64,
}

impl Default for TokenManager {
    fn default() -> Self {
        let secret = std::env::var("JWT_SECRET_KEY").unwrap_or_else(|_| DEFAULT_JWT_SECRET.to_string());
        let expire_minutes = std::env::var("JWT_EXPIRE_MINUTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_EXPIRE_MINUTES);

        Self {
            secret,
            expire_minutes,
        }
    }
}

impl TokenManager {
    pub fn new(secret: String, expire_minutes: u64) -> Self {
        Self {
            secret,
            expire_minutes,
        }
    }

    pub fn create_token(&self, user_id: &str) -> Result<(String, i64), TokenError> {
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let exp_secs = now_secs + (self.expire_minutes * 60);

        let claims = Claims {
            sub: user_id.to_string(),
            exp: exp_secs,
        };

        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.secret.as_bytes()),
        )?;

        let exp_ms = (exp_secs as i64) * 1000;
        Ok((token, exp_ms))
    }

    pub fn verify_token(&self, token: &str) -> Result<String, TokenError> {
        let mut validation = Validation::default();
        validation.validate_exp = true;

        let token_data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.secret.as_bytes()),
            &validation,
        )?;

        Ok(token_data.claims.sub)
    }
}
