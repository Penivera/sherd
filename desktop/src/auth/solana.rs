use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use rand::RngCore;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

use crate::auth::db::{format_timestamp_iso8601, AuthDb, DbError, UserRecord};
use crate::auth::token::{TokenError, TokenManager};

pub const DEFAULT_CHALLENGE_TTL_SECS: u64 = 300;

#[derive(Debug, Error)]
pub enum SolanaAuthError {
    #[error("Database error: {0}")]
    Db(#[from] DbError),
    #[error("Token error: {0}")]
    Token(#[from] TokenError),
    #[error("Invalid Solana wallet address (must be valid 32-byte base58 string)")]
    InvalidWalletAddress,
    #[error("Invalid signature format (must be valid 64-byte base58 string)")]
    InvalidSignatureFormat,
    #[error("Invalid, expired, or already-used challenge")]
    InvalidOrExpiredChallenge,
    #[error("Invalid wallet signature")]
    InvalidSignature,
}

#[derive(Debug, Clone)]
pub struct SolanaChallengeResult {
    pub nonce: String,
    pub message: String,
    pub expires_at: String,
}

pub struct SolanaAuthService {
    db: AuthDb,
    token_manager: TokenManager,
    challenge_ttl_secs: u64,
}

impl SolanaAuthService {
    pub fn new(db: AuthDb, token_manager: TokenManager) -> Self {
        Self {
            db,
            token_manager,
            challenge_ttl_secs: DEFAULT_CHALLENGE_TTL_SECS,
        }
    }

    pub fn with_ttl(db: AuthDb, token_manager: TokenManager, ttl_secs: u64) -> Self {
        Self {
            db,
            token_manager,
            challenge_ttl_secs: ttl_secs,
        }
    }

    pub async fn create_challenge(
        &self,
        wallet_address: &str,
    ) -> Result<SolanaChallengeResult, SolanaAuthError> {
        if !is_valid_solana_address(wallet_address) {
            return Err(SolanaAuthError::InvalidWalletAddress);
        }

        let mut nonce_bytes = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = bs58::encode(nonce_bytes).into_string();

        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let exp_secs = now_secs + self.challenge_ttl_secs;

        let issued_at = format_timestamp_iso8601(now_secs);
        let expires_at = format_timestamp_iso8601(exp_secs);

        let message = format!(
            "Sign in to Sherd\n\nWallet: {}\nNonce: {}\nIssued At: {}\nExpires At: {}",
            wallet_address, nonce, issued_at, expires_at
        );

        self.db
            .create_solana_challenge(wallet_address, &nonce, &message, &expires_at).await?;

        Ok(SolanaChallengeResult {
            nonce,
            message,
            expires_at,
        })
    }

    pub async fn verify(
        &self,
        wallet_address: &str,
        nonce: &str,
        signature: &str,
    ) -> Result<(UserRecord, String, i64), SolanaAuthError> {
        if !is_valid_solana_address(wallet_address) {
            return Err(SolanaAuthError::InvalidWalletAddress);
        }

        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let now_iso = format_timestamp_iso8601(now_secs);

        // Atomically consume challenge from DB (checks nonce, wallet, expiration, unconsumed)
        let challenge_message = self
            .db
            .consume_solana_challenge(nonce, wallet_address, &now_iso).await?
            .ok_or(SolanaAuthError::InvalidOrExpiredChallenge)?;

        // Verify Ed25519 signature over the server-stored challenge message
        if !verify_ed25519_signature(wallet_address, &challenge_message, signature) {
            return Err(SolanaAuthError::InvalidSignature);
        }

        // Link or create user with Solana provider
        let user = self.db.find_or_create_user_from_provider(
            "solana",
            wallet_address,
            None,
            None,
        ).await?;

        let (token, exp_ms) = self.token_manager.create_token(&user.id)?;
        Ok((user, token, exp_ms))
    }
}

pub fn is_valid_solana_address(address: &str) -> bool {
    let Ok(bytes) = bs58::decode(address).into_vec() else {
        return false;
    };
    bytes.len() == 32
}

pub fn verify_ed25519_signature(wallet_address: &str, message: &str, signature_b58: &str) -> bool {
    let Ok(pubkey_bytes) = bs58::decode(wallet_address).into_vec() else {
        return false;
    };
    let Ok(sig_bytes) = bs58::decode(signature_b58).into_vec() else {
        return false;
    };

    if pubkey_bytes.len() != 32 || sig_bytes.len() != 64 {
        return false;
    }

    let Ok(pubkey_array): Result<[u8; 32], _> = pubkey_bytes.as_slice().try_into() else {
        return false;
    };
    let Ok(verifying_key) = VerifyingKey::from_bytes(&pubkey_array) else {
        return false;
    };

    let Ok(sig_array): Result<[u8; 64], _> = sig_bytes.as_slice().try_into() else {
        return false;
    };
    let signature = Signature::from_bytes(&sig_array);

    verifying_key.verify(message.as_bytes(), &signature).is_ok()
}
