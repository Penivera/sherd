use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::state::AppState;
use crate::theme::{Theme, ORANGE};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    Login,
    Register,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveField {
    Email,
    Password,
    None,
}

pub fn render_auth(
    state: &AppState,
    theme: &Theme,
    mode: AuthMode,
    email: &str,
    password: &str,
    active_field: ActiveField,
    on_toggle_mode: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    on_focus_email: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    on_focus_password: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    on_submit_email: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    on_login_google: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    on_login_github: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    on_login_solana: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let is_busy = state.auth_busy.is_some();
    let busy_action = state.auth_busy.as_deref().unwrap_or("");

    let heading = match mode {
        AuthMode::Login => "Sign in to Sherd",
        AuthMode::Register => "Create your Sherd account",
    };

    let submit_label = match mode {
        AuthMode::Login => {
            if busy_action == "email" {
                "Signing in..."
            } else {
                "Sign in with email"
            }
        }
        AuthMode::Register => {
            if busy_action == "email" {
                "Registering..."
            } else {
                "Register"
            }
        }
    };

    let mode_switch_label = match mode {
        AuthMode::Login => "No account? Register",
        AuthMode::Register => "Have an account? Sign in",
    };

    let masked_password: String = "•".repeat(password.len());

    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .size_full()
        .bg(theme.background)
        .px(px(24.0))
        .child(
            div()
                .flex()
                .flex_col()
                .w_full()
                .max_w(px(320.0))
                .child(
                    div()
                        .text_size(px(20.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text_primary)
                        .text_center()
                        .mb(px(4.0))
                        .child(heading),
                )
                .child(
                    div()
                        .text_size(px(14.0))
                        .text_color(theme.text_muted)
                        .text_center()
                        .mb(px(24.0))
                        .child("Authenticate to continue as a client."),
                )
                // Email input
                .child(
                    div()
                        .w_full()
                        .rounded(px(12.0))
                        .border_2()
                        .border_color(if active_field == ActiveField::Email {
                            ORANGE
                        } else {
                            theme.input_border
                        })
                        .bg(theme.input_bg)
                        .px(px(16.0))
                        .py(px(10.0))
                        .mb(px(8.0))
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, on_focus_email)
                        .child(
                            div()
                                .text_size(px(14.0))
                                .text_color(if email.is_empty() {
                                    theme.text_muted
                                } else {
                                    theme.text_primary
                                })
                                .child(if email.is_empty() {
                                    if active_field == ActiveField::Email {
                                        "|".to_string()
                                    } else {
                                        "Email".to_string()
                                    }
                                } else {
                                    email.to_string()
                                }),
                        ),
                )
                // Password input
                .child(
                    div()
                        .w_full()
                        .rounded(px(12.0))
                        .border_2()
                        .border_color(if active_field == ActiveField::Password {
                            ORANGE
                        } else {
                            theme.input_border
                        })
                        .bg(theme.input_bg)
                        .px(px(16.0))
                        .py(px(10.0))
                        .mb(px(12.0))
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, on_focus_password)
                        .child(
                            div()
                                .text_size(px(14.0))
                                .text_color(if password.is_empty() {
                                    theme.text_muted
                                } else {
                                    theme.text_primary
                                })
                                .child(if password.is_empty() {
                                    if active_field == ActiveField::Password {
                                        "|".to_string()
                                    } else {
                                        "Password (min 8 chars)".to_string()
                                    }
                                } else {
                                    masked_password
                                }),
                        ),
                )
                // Email submit button
                .child(
                    div()
                        .w_full()
                        .py(px(10.0))
                        .rounded(px(12.0))
                        .bg(ORANGE)
                        .text_color(rgb(0xffffff))
                        .text_size(px(14.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_center()
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap(px(8.0))
                        .cursor_pointer()
                        .when(!is_busy, |s| {
                            s.on_mouse_down(MouseButton::Left, on_submit_email)
                        })
                        .child("✉")
                        .child(submit_label),
                )
                // Mode toggle (Login / Register)
                .child(
                    div()
                        .w_full()
                        .mt(px(8.0))
                        .mb(px(16.0))
                        .text_size(px(12.0))
                        .text_color(theme.text_muted)
                        .text_center()
                        .cursor_pointer()
                        .hover(|s| s.text_color(theme.text_primary))
                        .on_mouse_down(MouseButton::Left, on_toggle_mode)
                        .child(mode_switch_label),
                )
                // Divider "or"
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.0))
                        .mb(px(16.0))
                        .child(div().h(px(1.0)).flex_1().bg(theme.divider))
                        .child(
                            div()
                                .text_size(px(12.0))
                                .text_color(theme.text_muted)
                                .child("or"),
                        )
                        .child(div().h(px(1.0)).flex_1().bg(theme.divider)),
                )
                // Third-party buttons
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        // Google
                        .child(
                            div()
                                .w_full()
                                .py(px(10.0))
                                .rounded(px(12.0))
                                .border_2()
                                .border_color(theme.input_border)
                                .bg(theme.card_bg)
                                .text_color(theme.text_primary)
                                .text_size(px(14.0))
                                .font_weight(FontWeight::MEDIUM)
                                .flex()
                                .items_center()
                                .justify_center()
                                .gap(px(8.0))
                                .cursor_pointer()
                                .when(!is_busy, |s| {
                                    s.on_mouse_down(MouseButton::Left, on_login_google)
                                })
                                .child("G")
                                .child(if busy_action == "google" {
                                    "Connecting to Google..."
                                } else {
                                    "Continue with Google"
                                }),
                        )
                        // GitHub
                        .child(
                            div()
                                .w_full()
                                .py(px(10.0))
                                .rounded(px(12.0))
                                .border_2()
                                .border_color(theme.input_border)
                                .bg(theme.card_bg)
                                .text_color(theme.text_primary)
                                .text_size(px(14.0))
                                .font_weight(FontWeight::MEDIUM)
                                .flex()
                                .items_center()
                                .justify_center()
                                .gap(px(8.0))
                                .cursor_pointer()
                                .when(!is_busy, |s| {
                                    s.on_mouse_down(MouseButton::Left, on_login_github)
                                })
                                .child("🐙")
                                .child(if busy_action == "github" {
                                    "Connecting to GitHub..."
                                } else {
                                    "Continue with GitHub"
                                }),
                        )
                        // Solana
                        .child(
                            div()
                                .w_full()
                                .py(px(10.0))
                                .rounded(px(12.0))
                                .border_2()
                                .border_color(theme.input_border)
                                .bg(theme.card_bg)
                                .text_color(theme.text_primary)
                                .text_size(px(14.0))
                                .font_weight(FontWeight::MEDIUM)
                                .flex()
                                .items_center()
                                .justify_center()
                                .gap(px(8.0))
                                .cursor_pointer()
                                .when(!is_busy, |s| {
                                    s.on_mouse_down(MouseButton::Left, on_login_solana)
                                })
                                .child("💳")
                                .child(if busy_action == "solana" {
                                    "Signing with Solana wallet..."
                                } else {
                                    "Continue with Solana wallet"
                                }),
                        ),
                )
                // Wallet / auth status text
                .when_some(state.auth_status_text.as_ref(), |parent, text| {
                    parent.child(
                        div()
                            .text_size(px(12.0))
                            .text_color(theme.text_muted)
                            .text_center()
                            .mt(px(16.0))
                            .child(text.clone()),
                    )
                })
                // Error message
                .when_some(state.auth_error.as_ref(), |parent, err| {
                    parent.child(
                        div()
                            .text_size(px(12.0))
                            .text_color(rgb(0xef4444))
                            .text_center()
                            .mt(px(16.0))
                            .child(err.clone()),
                    )
                }),
        )
}
