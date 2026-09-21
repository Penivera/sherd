use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use thiserror::Error;
use tracing::{info, warn};

#[derive(Debug, Error)]
pub enum LoopbackError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Timed out waiting for OAuth callback")]
    Timeout,
    #[error("OAuth callback did not include a valid code")]
    MissingCode,
    #[error("Invalid request to callback server")]
    InvalidRequest,
}

pub struct LoopbackListener {
    listener: TcpListener,
    port: u16,
}

impl LoopbackListener {
    pub async fn bind() -> Result<Self, LoopbackError> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        info!("OAuth loopback server listening on 127.0.0.1:{}", port);
        Ok(Self { listener, port })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn redirect_uri(&self) -> String {
        format!("http://127.0.0.1:{}/callback", self.port)
    }

    pub async fn wait_for_code(self, timeout: Duration) -> Result<String, LoopbackError> {
        let (code, _) = self.wait_for_code_and_state(timeout).await?;
        Ok(code)
    }

    pub async fn wait_for_code_and_state(
        self,
        timeout: Duration,
    ) -> Result<(String, Option<String>), LoopbackError> {
        match tokio::time::timeout(timeout, self.accept_and_extract_params()).await {
            Ok(result) => result,
            Err(_) => {
                warn!("Timed out waiting for OAuth callback");
                Err(LoopbackError::Timeout)
            }
        }
    }

    async fn accept_and_extract_params(self) -> Result<(String, Option<String>), LoopbackError> {
        let (mut stream, _) = self.listener.accept().await?;
        let mut buffer = [0u8; 4096];
        let bytes_read = stream.read(&mut buffer).await?;
        let request = String::from_utf8_lossy(&buffer[..bytes_read]);

        let first_line = request.lines().next().unwrap_or("");
        let parts: Vec<&str> = first_line.split_whitespace().collect();
        if parts.len() < 2 || parts[0] != "GET" {
            let response = "HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\nInvalid request";
            let _ = stream.write_all(response.as_bytes()).await;
            return Err(LoopbackError::InvalidRequest);
        }

        let path = parts[1];
        let code = extract_query_param(path, "code");
        let state = extract_query_param(path, "state");

        let response_body = if code.is_some() {
            r#"<!DOCTYPE html>
<html>
<head><meta charset="utf-8"><title>Sherd - Authenticated</title></head>
<body style="font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background: #09090b; color: #f5f5f5; display: flex; flex-direction: column; align-items: center; justify-content: center; height: 90vh; margin: 0;">
  <div style="text-align: center; max-width: 360px; padding: 32px; background: #171717; border: 2px solid #262626; border-radius: 16px;">
    <h1 style="color: #F16852; font-size: 24px; margin-bottom: 8px;">Signed in to Sherd</h1>
    <p style="color: #a3a3a3; font-size: 14px; margin-bottom: 0;">You can close this tab and return to the Sherd desktop application.</p>
  </div>
</body>
</html>"#
        } else {
            r#"<!DOCTYPE html>
<html>
<head><meta charset="utf-8"><title>Sherd - Authentication Failed</title></head>
<body style="font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background: #09090b; color: #f5f5f5; display: flex; flex-direction: column; align-items: center; justify-content: center; height: 90vh; margin: 0;">
  <div style="text-align: center; max-width: 360px; padding: 32px; background: #171717; border: 2px solid #262626; border-radius: 16px;">
    <h1 style="color: #ef4444; font-size: 24px; margin-bottom: 8px;">Authentication Failed</h1>
    <p style="color: #a3a3a3; font-size: 14px; margin-bottom: 0;">No authorization code was provided. Please return to Sherd and try again.</p>
  </div>
</body>
</html>"#
        };

        let status = if code.is_some() { "200 OK" } else { "400 Bad Request" };
        let response = format!(
            "HTTP/1.1 {}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            status,
            response_body.len(),
            response_body
        );

        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.flush().await;

        let code = code.ok_or(LoopbackError::MissingCode)?;
        Ok((code, state))
    }
}

fn extract_query_param(path: &str, param_name: &str) -> Option<String> {
    let query_start = path.find('?')?;
    let query = &path[query_start + 1..];
    for pair in query.split('&') {
        let mut parts = pair.split('=');
        if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
            if k == param_name {
                return Some(v.to_string());
            }
        }
    }
    None
}
