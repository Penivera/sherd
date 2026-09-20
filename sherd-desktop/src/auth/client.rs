use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

use crate::state::UserProfile;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("Authentication failed ({status}): {detail}")]
    Api { status: u16, detail: String },
    #[error("Server returned an invalid response")]
    InvalidResponse,
    #[error("Not authenticated")]
    NotAuthenticated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub user: UserProfile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolanaChallengeResponse {
    pub nonce: String,
    pub message: String,
    pub expires_at: String,
}

#[derive(Debug, Serialize)]
struct RegisterBody<'a> {
    email: &'a str,
    password: &'a str,
}

#[derive(Debug, Serialize)]
struct LoginBody<'a> {
    email: &'a str,
    password: &'a str,
}

#[derive(Debug, Serialize)]
struct SolanaChallengeBody<'a> {
    wallet_address: &'a str,
}

#[derive(Debug, Serialize)]
struct SolanaVerifyBody<'a> {
    wallet_address: &'a str,
    nonce: &'a str,
    signature: &'a str,
}

#[derive(Debug, Serialize)]
struct ExchangeCodeBody<'a> {
    code: &'a str,
}

#[derive(Debug, Deserialize)]
struct ErrorDetail {
    detail: Option<String>,
}

#[derive(Clone)]
pub struct AuthClient {
    base_url: String,
    http: reqwest::Client,
}

impl AuthClient {
    pub fn new(base_url: String) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default();

        Self { base_url, http }
    }

    pub fn set_base_url(&mut self, base_url: String) {
        self.base_url = base_url;
    }

    pub async fn health_check(&self) -> Result<bool, AuthError> {
        let url = format!("{}/health", self.base_url);
        let resp = self.http.get(&url).send().await?;
        Ok(resp.status().is_success())
    }

    async fn handle_response<T: for<'de> Deserialize<'de>>(
        &self,
        resp: reqwest::Response,
    ) -> Result<T, AuthError> {
        let status = resp.status();
        if status.is_success() {
            resp.json::<T>().await.map_err(AuthError::Network)
        } else {
            let detail = resp
                .json::<ErrorDetail>()
                .await
                .ok()
                .and_then(|e| e.detail)
                .unwrap_or_else(|| format!("HTTP {}", status.as_u16()));
            Err(AuthError::Api {
                status: status.as_u16(),
                detail,
            })
        }
    }

    pub async fn register(&self, email: &str, password: &str) -> Result<TokenResponse, AuthError> {
        let url = format!("{}/auth/register", self.base_url);
        let resp = self
            .http
            .post(&url)
            .json(&RegisterBody { email, password })
            .send()
            .await?;
        self.handle_response(resp).await
    }

    pub async fn login(&self, email: &str, password: &str) -> Result<TokenResponse, AuthError> {
        let url = format!("{}/auth/login", self.base_url);
        let resp = self
            .http
            .post(&url)
            .json(&LoginBody { email, password })
            .send()
            .await?;
        self.handle_response(resp).await
    }

    pub async fn me(&self, token: &str) -> Result<UserProfile, AuthError> {
        let url = format!("{}/auth/me", self.base_url);
        let resp = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await?;
        self.handle_response(resp).await
    }

    pub async fn request_solana_challenge(
        &self,
        wallet_address: &str,
    ) -> Result<SolanaChallengeResponse, AuthError> {
        let url = format!("{}/auth/solana/challenge", self.base_url);
        let resp = self
            .http
            .post(&url)
            .json(&SolanaChallengeBody { wallet_address })
            .send()
            .await?;
        self.handle_response(resp).await
    }

    pub async fn verify_solana_signature(
        &self,
        wallet_address: &str,
        nonce: &str,
        signature: &str,
    ) -> Result<TokenResponse, AuthError> {
        let url = format!("{}/auth/solana/verify", self.base_url);
        let resp = self
            .http
            .post(&url)
            .json(&SolanaVerifyBody {
                wallet_address,
                nonce,
                signature,
            })
            .send()
            .await?;
        self.handle_response(resp).await
    }

    pub async fn exchange_code(&self, code: &str) -> Result<TokenResponse, AuthError> {
        let url = format!("{}/auth/token/exchange", self.base_url);
        let resp = self
            .http
            .post(&url)
            .json(&ExchangeCodeBody { code })
            .send()
            .await?;
        self.handle_response(resp).await
    }
}
