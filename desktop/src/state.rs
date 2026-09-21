use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppScreen {
    Splash,
    Auth,
    Toggle,
    Mesh,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserProfile {
    pub id: String,
    pub email: Option<String>,
    pub providers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedSession {
    pub access_token: String,
    pub user: UserProfile,
    pub expires_at: Option<i64>, // Unix timestamp in milliseconds
}

impl PersistedSession {
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            now >= expires_at
        } else {
            false
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthMethodInfo {
    pub wallet_address: String,
    pub kind: String, // "phantom", "solflare", "native ed25519"
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MeshConnectionState {
    Disconnected,
    Connecting,
    Connected { node_count: u32 },
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeshNode {
    pub id: u32,
    pub price: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeshTask {
    pub id: u64,
    pub cmd: String,
    pub node: String,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub screen: AppScreen,
    pub is_dark: bool,
    pub session: Option<PersistedSession>,
    pub auth_method: Option<AuthMethodInfo>,
    pub mesh_status: MeshConnectionState,
    pub is_toggled: bool,
    pub nodes: Vec<MeshNode>,
    pub tasks: Vec<MeshTask>,
    pub auth_busy: Option<String>,
    pub auth_error: Option<String>,
    pub auth_status_text: Option<String>,
}

fn theme_config_path() -> Option<std::path::PathBuf> {
    if let Ok(config_home) = std::env::var("XDG_CONFIG_HOME") {
        Some(std::path::PathBuf::from(config_home).join("sherd").join("theme.json"))
    } else if let Ok(home) = std::env::var("HOME") {
        Some(std::path::PathBuf::from(home).join(".config").join("sherd").join("theme.json"))
    } else {
        None
    }
}

pub fn load_theme_preference() -> Option<bool> {
    if let Ok(val) = std::env::var("SHERD_DARK_MODE") {
        return match val.to_lowercase().as_str() {
            "1" | "true" | "yes" | "dark" => Some(true),
            "0" | "false" | "no" | "light" => Some(false),
            _ => None,
        };
    }
    let path = theme_config_path()?;
    let content = std::fs::read_to_string(path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
    parsed.get("is_dark").and_then(|v| v.as_bool())
}

pub fn save_theme_preference(is_dark: bool) {
    if let Some(path) = theme_config_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let val = serde_json::json!({ "is_dark": is_dark });
        let _ = std::fs::write(path, val.to_string());
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            screen: AppScreen::Splash,
            is_dark: load_theme_preference().unwrap_or(true),
            session: None,
            auth_method: None,
            mesh_status: MeshConnectionState::Disconnected,
            is_toggled: false,
            nodes: vec![
                MeshNode { id: 1, price: "0.010".to_string() },
                MeshNode { id: 2, price: "0.014".to_string() },
                MeshNode { id: 3, price: "0.008".to_string() },
                MeshNode { id: 4, price: "0.019".to_string() },
                MeshNode { id: 5, price: "0.011".to_string() },
            ],
            tasks: vec![
                MeshTask {
                    id: 1,
                    cmd: "npm run build".to_string(),
                    node: "Node 4".to_string(),
                    status: "done".to_string(),
                },
                MeshTask {
                    id: 2,
                    cmd: "python train.py --epochs 10".to_string(),
                    node: "Node 2".to_string(),
                    status: "running".to_string(),
                },
            ],
            auth_busy: None,
            auth_error: None,
            auth_status_text: None,
        }
    }
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_authenticated(&self) -> bool {
        self.session.is_some()
    }

    pub fn set_session(&mut self, session: PersistedSession, auth_method: Option<AuthMethodInfo>) {
        self.session = Some(session);
        self.auth_method = auth_method;
        self.auth_busy = None;
        self.auth_error = None;
        self.auth_status_text = None;
        self.screen = AppScreen::Toggle;
    }

    pub fn logout(&mut self) {
        self.session = None;
        self.auth_method = None;
        self.is_toggled = false;
        self.screen = AppScreen::Auth;
    }

    pub fn toggle_client(&mut self) {
        self.is_toggled = !self.is_toggled;
    }

    pub fn toggle_theme(&mut self) {
        self.is_dark = !self.is_dark;
        save_theme_preference(self.is_dark);
    }
}
