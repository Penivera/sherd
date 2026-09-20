use gpui::*;
use std::sync::Arc;
use std::time::Duration;

use sherd_desktop::auth::AuthManager;
use sherd_desktop::state::{AppScreen, AppState, AuthMethodInfo, MeshTask};
use sherd_desktop::theme::Theme;
use sherd_desktop::views::{render_auth, render_mesh, render_splash, render_toggle, ActiveField, AuthMode};

struct SherdApp {
    state: AppState,
    auth_manager: Arc<AuthManager>,
    focus_handle: FocusHandle,
    auth_mode: AuthMode,
    email_input: String,
    password_input: String,
    active_field: ActiveField,
}

impl SherdApp {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let auth_manager = Arc::new(AuthManager::default());
        let cached_session = auth_manager.load_cached_session();

        let focus_handle = cx.focus_handle();

        // Spawn startup 2-second splash timer
        let session_clone = cached_session.clone();

        cx.spawn(async move |this, app| {
            app.background_executor().timer(Duration::from_secs(2)).await;
            let _ = this.update(app, |app, cx| {
                if let Some(session) = session_clone {
                    app.state.set_session(session, None);
                    app.state.screen = AppScreen::Toggle;
                } else {
                    app.state.screen = AppScreen::Auth;
                }
                cx.notify();
            });
        })
        .detach();

        Self {
            state: AppState::default(),
            auth_manager,
            focus_handle,
            auth_mode: AuthMode::Login,
            email_input: String::new(),
            password_input: String::new(),
            active_field: ActiveField::None,
        }
    }

    fn handle_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if self.state.screen != AppScreen::Auth {
            return;
        }

        let key = &event.keystroke.key;
        match key.as_str() {
            "backspace" => {
                match self.active_field {
                    ActiveField::Email => {
                        self.email_input.pop();
                    }
                    ActiveField::Password => {
                        self.password_input.pop();
                    }
                    ActiveField::None => {}
                }
                cx.notify();
            }
            "tab" => {
                self.active_field = match self.active_field {
                    ActiveField::Email => ActiveField::Password,
                    ActiveField::Password => ActiveField::Email,
                    ActiveField::None => ActiveField::Email,
                };
                cx.notify();
            }
            "enter" => {
                self.submit_email(cx);
            }
            c if c.len() == 1 => {
                match self.active_field {
                    ActiveField::Email => {
                        self.email_input.push_str(c);
                    }
                    ActiveField::Password => {
                        self.password_input.push_str(c);
                    }
                    ActiveField::None => {}
                }
                cx.notify();
            }
            _ => {}
        }
    }

    fn submit_email(&mut self, cx: &mut Context<Self>) {
        if self.email_input.is_empty() || self.password_input.len() < 8 {
            self.state.auth_error = Some("Password must be at least 8 characters".to_string());
            cx.notify();
            return;
        }

        self.state.auth_busy = Some("email".to_string());
        self.state.auth_error = None;
        cx.notify();

        let email = self.email_input.clone();
        let password = self.password_input.clone();
        let mode = self.auth_mode;
        let auth_mgr = Arc::clone(&self.auth_manager);

        cx.spawn(async move |this, app| {
            let result = match mode {
                AuthMode::Login => auth_mgr.login_email(&email, &password).await,
                AuthMode::Register => auth_mgr.register_email(&email, &password).await,
            };

            let _ = this.update(app, |app, cx| {
                match result {
                    Ok(session) => {
                        app.state.set_session(session, None);
                        app.email_input.clear();
                        app.password_input.clear();
                    }
                    Err(err) => {
                        app.state.auth_busy = None;
                        app.state.auth_error = Some(err.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn login_oauth(&mut self, provider: &'static str, cx: &mut Context<Self>) {
        self.state.auth_busy = Some(provider.to_string());
        self.state.auth_error = None;
        cx.notify();

        let auth_mgr = Arc::clone(&self.auth_manager);
        cx.spawn(async move |this, app| {
            let result = auth_mgr.login_oauth(provider).await;
            let _ = this.update(app, |app, cx| {
                match result {
                    Ok(session) => {
                        app.state.set_session(session, None);
                    }
                    Err(err) => {
                        app.state.auth_busy = None;
                        app.state.auth_error = Some(err.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn login_solana(&mut self, cx: &mut Context<Self>) {
        self.state.auth_busy = Some("solana".to_string());
        self.state.auth_status_text = Some("Requesting challenge from server...".to_string());
        self.state.auth_error = None;
        cx.notify();

        let auth_mgr = Arc::clone(&self.auth_manager);
        cx.spawn(async move |this, app| {
            let result = auth_mgr.login_solana().await;
            let _ = this.update(app, |app, cx| {
                match result {
                    Ok((session, wallet_address)) => {
                        app.state.auth_status_text = Some("Verified ✓".to_string());
                        app.state.set_session(
                            session,
                            Some(AuthMethodInfo {
                                wallet_address,
                                kind: "native ed25519".to_string(),
                            }),
                        );
                    }
                    Err(err) => {
                        app.state.auth_busy = None;
                        app.state.auth_status_text = None;
                        app.state.auth_error = Some(err.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn simulate_task(&mut self, cx: &mut Context<Self>) {
        let commands = [
            "cargo build --release",
            "pytest -q",
            "ffmpeg -i in.mp4 out.mp4",
            "make all",
        ];
        let rand_idx = self.state.tasks.len() % commands.len();
        let node_id = (self.state.tasks.len() % 5) + 1;
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let task = MeshTask {
            id,
            cmd: commands[rand_idx].to_string(),
            node: format!("Node {}", node_id),
            status: "running".to_string(),
        };

        self.state.tasks.insert(0, task);
        cx.notify();

        // Simulate 1.4s completion
        cx.spawn(async move |this, app| {
            tokio::time::sleep(Duration::from_millis(1400)).await;
            let _ = this.update(app, |app, cx| {
                if let Some(t) = app.state.tasks.iter_mut().find(|t| t.id == id) {
                    t.status = "done".to_string();
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for SherdApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::new(self.state.is_dark);

        let screen_content = match self.state.screen {
            AppScreen::Splash => render_splash(&theme).into_any_element(),
            AppScreen::Auth => {
                let mode = self.auth_mode;
                let email = self.email_input.clone();
                let password = self.password_input.clone();
                let active_field = self.active_field;

                render_auth(
                    &self.state,
                    &theme,
                    mode,
                    &email,
                    &password,
                    active_field,
                    cx.listener(|this, _, _, cx| {
                        this.auth_mode = match this.auth_mode {
                            AuthMode::Login => AuthMode::Register,
                            AuthMode::Register => AuthMode::Login,
                        };
                        this.state.auth_error = None;
                        cx.notify();
                    }),
                    cx.listener(|this, _, _, cx| {
                        this.active_field = ActiveField::Email;
                        cx.notify();
                    }),
                    cx.listener(|this, _, _, cx| {
                        this.active_field = ActiveField::Password;
                        cx.notify();
                    }),
                    cx.listener(|this, _, _, cx| {
                        this.submit_email(cx);
                    }),
                    cx.listener(|this, _, _, cx| {
                        this.login_oauth("google", cx);
                    }),
                    cx.listener(|this, _, _, cx| {
                        this.login_oauth("github", cx);
                    }),
                    cx.listener(|this, _, _, cx| {
                        this.login_solana(cx);
                    }),
                )
                .into_any_element()
            }
            AppScreen::Toggle => render_toggle(
                self.state.is_toggled,
                &theme,
                cx.listener(|this, _, _, cx| {
                    this.state.toggle_client();
                    cx.notify();
                }),
                cx.listener(|this, _, _, cx| {
                    if this.state.is_toggled {
                        this.state.screen = AppScreen::Mesh;
                        cx.notify();
                    }
                }),
            )
            .into_any_element(),
            AppScreen::Mesh => render_mesh(
                &self.state,
                &theme,
                cx.listener(|this, _, _, cx| {
                    let _ = this.auth_manager.clear_cached_session();
                    this.state.logout();
                    cx.notify();
                }),
                cx.listener(|this, _, _, cx| {
                    this.simulate_task(cx);
                }),
            )
            .into_any_element(),
        };

        div()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                this.handle_key_down(event, cx);
            }))
            .relative()
            .size_full()
            .bg(theme.background)
            .child(
                // Theme toggle button (fixed top right)
                div()
                    .absolute()
                    .top(px(16.0))
                    .right(px(16.0))
                    .w(px(40.0))
                    .h(px(40.0))
                    .rounded(px(20.0))
                    .bg(if theme.is_dark {
                        rgb(0x262626)
                    } else {
                        rgb(0xf5f5f5)
                    })
                    .text_color(if theme.is_dark {
                        rgb(0xf5f5f5)
                    } else {
                        rgb(0x525252)
                    })
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.state.toggle_theme();
                            cx.notify();
                        }),
                    )
                    .child(if theme.is_dark { "☀️" } else { "🌙" }),
            )
            .child(screen_content)
    }
}

fn main() {
    tracing_subscriber::fmt::init();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to initialize Tokio runtime");
    let _guard = rt.enter();

    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(480.0), px(800.0)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("Sherd".into()),
                ..Default::default()
            }),
            window_min_size: Some(size(px(380.0), px(600.0))),
            ..Default::default()
        };

        cx.open_window(options, |_, cx| cx.new(|cx| SherdApp::new(cx)))
            .unwrap();
    });
}
