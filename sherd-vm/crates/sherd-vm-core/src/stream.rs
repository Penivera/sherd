use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::{error::VmResult, vm::VmManager};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamInfo {
    pub session_id: String,
    pub stream_url: String,
    pub token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputEvent {
    #[serde(rename = "type")]
    pub kind: InputKind,
    pub x: Option<u32>,
    pub y: Option<u32>,
    pub button: Option<String>,
    pub keys: Option<Vec<String>>,
    pub text: Option<String>,
    pub humanize: Option<bool>,
    pub scroll_delta: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    MouseMove,
    MouseClick,
    MouseDown,
    MouseUp,
    MouseScroll,
    KeyType,
    KeyPress,
    KeyDown,
    KeyUp,
}

pub struct StreamBridge {
    manager: Arc<VmManager>,
}

impl StreamBridge {
    pub fn new(manager: Arc<VmManager>) -> Self {
        Self { manager }
    }

    /// Get the live VNC/stream URL for a session.
    /// This is a signed URL — treat as secret, do not log in full.
    pub async fn get_stream_url(&self, session_id: &str) -> VmResult<StreamInfo> {
        let url = self.manager.stream_url(session_id).await?;
        info!(session_id, "stream url fetched");
        Ok(StreamInfo { session_id: session_id.to_string(), stream_url: url, token: None })
    }

    /// Forward a single input event to the VM.
    /// For mouse/keyboard, delegates to provider's control plane.
    /// Supports `humanize: true` for curved mouse paths (helps on bot-detecting sites).
    pub async fn send_input(&self, session_id: &str, event: InputEvent) -> VmResult<()> {
        match event.kind {
            InputKind::MouseMove => {
                let x = event.x.unwrap_or(0);
                let y = event.y.unwrap_or(0);
                let humanize = event.humanize.unwrap_or(false);
                self.manager.mouse_move(session_id, x, y, humanize).await
            }
            InputKind::MouseClick => {
                let x = event.x.unwrap_or(0);
                let y = event.y.unwrap_or(0);
                let button = event.button.as_deref().unwrap_or("left");
                let humanize = event.humanize.unwrap_or(false);
                self.manager.mouse_click(session_id, x, y, button, humanize).await
            }
            InputKind::MouseDown | InputKind::MouseUp | InputKind::MouseScroll => {
                // For MVP, map to move/click; full drag/scroll via WebSocket control channel later
                let x = event.x.unwrap_or(0);
                let y = event.y.unwrap_or(0);
                let humanize = event.humanize.unwrap_or(false);
                self.manager.mouse_move(session_id, x, y, humanize).await
            }
            InputKind::KeyType => {
                let text = event.text.as_deref().unwrap_or("");
                self.manager.keyboard_type(session_id, text).await
            }
            InputKind::KeyPress | InputKind::KeyDown | InputKind::KeyUp => {
                let keys = event.keys.unwrap_or_default();
                if keys.is_empty() {
                    if let Some(t) = event.text {
                        return self.manager.keyboard_type(session_id, &t).await;
                    }
                    return Ok(());
                }
                self.manager.keyboard_press(session_id, keys).await
            }
        }
    }

    /// Capture a screenshot as base64 PNG/JPEG for polling fallback when WebSocket stream unavailable.
    pub async fn capture(&self, session_id: &str, format: &str, quality: Option<u8>) -> VmResult<Vec<u8>> {
        self.manager.screenshot(session_id, format, quality).await
    }

    pub async fn capture_base64(&self, session_id: &str, format: &str, quality: Option<u8>) -> VmResult<String> {
        let bytes = self.capture(session_id, format, quality).await?;
        Ok(base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes))
    }
}
