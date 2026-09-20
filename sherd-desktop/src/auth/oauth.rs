use rand::RngCore;
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;
use thiserror::Error;
use tracing::info;

use crate::auth::db::{AuthDb, DbError, UserRecord};
use crate::auth::loopback::{LoopbackError, LoopbackListener};
use crate::auth::token::{TokenError, TokenManager};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthProvider {
    Google,
    GitHub,
}

#[derive(Debug, Error)]
pub enum OAuthError {
    #[error("Database error: {0}")]
    Db(#[from] DbError),
    #[error("Token error: {0}")]
    Token(#[from] TokenError),
    #[error("Loopback listener error: {0}")]
    Loopback(#[from] LoopbackError),
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("Provider '{0}' is not configured (missing CLIENT_ID or CLIENT_SECRET)")]
    NotConfigured(&'static str),
    #[error("Failed to launch system browser: {0}")]
    BrowserLaunch(String),
    #[error("OAuth state mismatch (possible CSRF attack)")]
    StateMismatch,
    #[error("Failed to exchange code: {0}")]
    ExchangeFailed(String),
    #[error("Failed to retrieve user profile from provider: {0}")]
    ProfileFailed(String),
}

#[derive(Debug, Clone)]
pub struct OAuthConfig {
    pub google_client_id: Option<String>,
    pub google_client_secret: Option<String>,
    pub github_client_id: Option<String>,
    pub github_client_secret: Option<String>,
}

impl Default for OAuthConfig {
    fn default() -> Self {
        Self {
            google_client_id: std::env::var("GOOGLE_CLIENT_ID").ok().filter(|s| !s.is_empty()),
            google_client_secret: std::env::var("GOOGLE_CLIENT_SECRET")
                .ok()
                .filter(|s| !s.is_empty()),
            github_client_id: std::env::var("GITHUB_CLIENT_ID").ok().filter(|s| !s.is_empty()),
            github_client_secret: std::env::var("GITHUB_CLIENT_SECRET")
                .ok()
                .filter(|s| !s.is_empty()),
        }
    }
}

pub struct OAuthService {
    db: AuthDb,
    token_manager: TokenManager,
    config: OAuthConfig,
    http: Client,
}

impl OAuthService {
    pub fn new(db: AuthDb, token_manager: TokenManager, config: OAuthConfig) -> Self {
        Self {
            db,
            token_manager,
            config,
            http: Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .unwrap_or_default(),
        }
    }

    pub async fn authenticate(
        &self,
        provider: OAuthProvider,
    ) -> Result<(UserRecord, String, i64), OAuthError> {
        let listener = LoopbackListener::bind().await?;
        let redirect_uri = listener.redirect_uri();

        let mut state_bytes = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut state_bytes);
        let state = bs58::encode(state_bytes).into_string();

        let auth_url = match provider {
            OAuthProvider::Google => {
                let client_id = self
                    .config
                    .google_client_id
                    .as_deref()
                    .ok_or(OAuthError::NotConfigured("Google"))?;
                format!(
                    "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope=openid%20email%20profile&state={}",
                    client_id,
                    urlencoding::encode(&redirect_uri),
                    state
                )
            }
            OAuthProvider::GitHub => {
                let client_id = self
                    .config
                    .github_client_id
                    .as_deref()
                    .ok_or(OAuthError::NotConfigured("GitHub"))?;
                format!(
                    "https://github.com/login/oauth/authorize?client_id={}&redirect_uri={}&scope=read:user%20user:email&state={}",
                    client_id,
                    urlencoding::encode(&redirect_uri),
                    state
                )
            }
        };

        info!("Opening browser for OAuth login...");
        if let Err(e) = open::that(&auth_url) {
            return Err(OAuthError::BrowserLaunch(e.to_string()));
        }

        let (code, returned_state) = listener
            .wait_for_code_and_state(Duration::from_secs(120))
            .await?;

        if let Some(st) = returned_state {
            if st != state {
                return Err(OAuthError::StateMismatch);
            }
        }

        let (provider_id, email, metadata) = match provider {
            OAuthProvider::Google => self.exchange_google(code, &redirect_uri).await?,
            OAuthProvider::GitHub => self.exchange_github(code, &redirect_uri).await?,
        };

        let provider_name = match provider {
            OAuthProvider::Google => "google",
            OAuthProvider::GitHub => "github",
        };

        let user = self.db.find_or_create_user_from_provider(
            provider_name,
            &provider_id,
            email.as_deref(),
            metadata.as_deref(),
        )?;

        let (token, exp_ms) = self.token_manager.create_token(&user.id)?;
        Ok((user, token, exp_ms))
    }

    async fn exchange_google(
        &self,
        code: String,
        redirect_uri: &str,
    ) -> Result<(String, Option<String>, Option<String>), OAuthError> {
        let client_id = self.config.google_client_id.as_deref().unwrap_or_default();
        let client_secret = self
            .config
            .google_client_secret
            .as_deref()
            .unwrap_or_default();

        #[derive(Deserialize)]
        struct GoogleTokenResp {
            access_token: String,
        }

        let resp = self
            .http
            .post("https://oauth2.googleapis.com/token")
            .form(&[
                ("code", code.as_str()),
                ("client_id", client_id),
                ("client_secret", client_secret),
                ("redirect_uri", redirect_uri),
                ("grant_type", "authorization_code"),
            ])
            .send()
            .await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(OAuthError::ExchangeFailed(format!("Google error: {}", body)));
        }

        let token_data: GoogleTokenResp = resp.json().await?;

        #[derive(Deserialize)]
        struct GoogleUserInfo {
            sub: String,
            email: Option<String>,
            name: Option<String>,
        }

        let user_resp = self
            .http
            .get("https://www.googleapis.com/oauth2/v3/userinfo")
            .bearer_auth(token_data.access_token)
            .send()
            .await?;

        if !user_resp.status().is_success() {
            return Err(OAuthError::ProfileFailed(
                "Failed to fetch Google user info".to_string(),
            ));
        }

        let user_data: GoogleUserInfo = user_resp.json().await?;
        Ok((user_data.sub, user_data.email, user_data.name))
    }

    async fn exchange_github(
        &self,
        code: String,
        redirect_uri: &str,
    ) -> Result<(String, Option<String>, Option<String>), OAuthError> {
        let client_id = self.config.github_client_id.as_deref().unwrap_or_default();
        let client_secret = self
            .config
            .github_client_secret
            .as_deref()
            .unwrap_or_default();

        #[derive(Deserialize)]
        struct GithubTokenResp {
            access_token: String,
        }

        let resp = self
            .http
            .post("https://github.com/login/oauth/access_token")
            .header("Accept", "application/json")
            .form(&[
                ("code", code.as_str()),
                ("client_id", client_id),
                ("client_secret", client_secret),
                ("redirect_uri", redirect_uri),
            ])
            .send()
            .await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(OAuthError::ExchangeFailed(format!("GitHub error: {}", body)));
        }

        let token_data: GithubTokenResp = resp.json().await?;

        #[derive(Deserialize)]
        struct GithubUser {
            id: i64,
            email: Option<String>,
            login: String,
        }

        let user_resp = self
            .http
            .get("https://api.github.com/user")
            .header("User-Agent", "Sherd-Desktop")
            .bearer_auth(&token_data.access_token)
            .send()
            .await?;

        if !user_resp.status().is_success() {
            return Err(OAuthError::ProfileFailed(
                "Failed to fetch GitHub user info".to_string(),
            ));
        }

        let user_data: GithubUser = user_resp.json().await?;
        let mut email = user_data.email;

        // If email is not public on /user, fetch /user/emails
        if email.is_none() {
            #[derive(Deserialize)]
            struct GithubEmail {
                email: String,
                primary: bool,
                verified: bool,
            }

            if let Ok(emails_resp) = self
                .http
                .get("https://api.github.com/user/emails")
                .header("User-Agent", "Sherd-Desktop")
                .bearer_auth(&token_data.access_token)
                .send()
                .await
            {
                if emails_resp.status().is_success() {
                    if let Ok(emails) = emails_resp.json::<Vec<GithubEmail>>().await {
                        for e in emails {
                            if e.primary && e.verified {
                                email = Some(e.email);
                                break;
                            }
                        }
                    }
                }
            }
        }

        Ok((
            user_data.id.to_string(),
            email,
            Some(user_data.login),
        ))
    }
}

mod urlencoding {
    pub fn encode(data: &str) -> String {
        let mut encoded = String::with_capacity(data.len() * 3);
        for byte in data.bytes() {
            match byte {
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    encoded.push(byte as char);
                }
                _ => {
                    encoded.push_str(&format!("%{:02X}", byte));
                }
            }
        }
        encoded
    }
}
