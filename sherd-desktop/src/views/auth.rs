use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::state::AppState;
use crate::theme::{
    Theme, BUTTON_HEIGHT, FONT_2XL, FONT_BASE, FONT_LG, FONT_MD, FONT_SM, FONT_XS, FONT_2XS,
    INPUT_HEIGHT, RADIUS_FULL, RADIUS_LG, RADIUS_MD, SPACE_LG, SPACE_MD, SPACE_SM,
    SPACE_XL, SPACE_XS, SPACE_2XL,
};

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
        AuthMode::Register => "Create your account",
    };

    let submit_label = match mode {
        AuthMode::Login => {
            if busy_action == "email" {
                "Signing in..."
            } else {
                "Sign in with Email"
            }
        }
        AuthMode::Register => {
            if busy_action == "email" {
                "Registering..."
            } else {
                "Create Account"
            }
        }
    };

    let masked_password: String = "•".repeat(password.len());

    div()
        .id("auth-scroll-container")
        .flex()
        .flex_col()
        .size_full()
        .overflow_y_scroll()
        .bg(theme.background)
        .child(
            div()
                .flex()
                .flex_row()
                .min_h_full()
                .w_full()
                .items_center()
                .justify_center()
                .p(SPACE_XL)
                .child(
                    // Desktop 2-column container
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_center()
                        .w_full()
                        .max_w(px(920.0))
                        .gap(SPACE_2XL)
                        // LEFT COLUMN: Branding & Edge Network Capability Panel
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w_0()
                                .gap(SPACE_LG)
                        .child(
                            // Brand badge
                            div()
                                .flex()
                                .items_center()
                                .gap(SPACE_MD)
                                .child(
                                    div()
                                        .w(px(44.0))
                                        .h(px(44.0))
                                        .rounded(RADIUS_MD)
                                        .bg(theme.accent)
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            div()
                                                .text_size(px(22.0))
                                                .text_color(rgb(0xffffff))
                                                .child("⚡"),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .child(
                                            div()
                                                .text_size(FONT_2XL)
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(theme.text_primary)
                                                .child("SHERD"),
                                        )
                                        .child(
                                            div()
                                                .text_size(FONT_XS)
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(theme.accent)
                                                .child("DECENTRALIZED COMPUTE MESH"),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .text_size(FONT_LG)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.text_primary)
                                .line_height(relative(1.3))
                                .child("High-performance compute dispatched directly to the edge."),
                        )
                        .child(
                            div()
                                .text_size(FONT_SM)
                                .text_color(theme.text_muted)
                                .line_height(relative(1.5))
                                .child("Execute builds, scripts, and distributed workloads across verified peer nodes. Built with native Rust, Zero-Trust cryptographic identity, and Solana settlement."),
                        )
                        // Feature bullets
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(SPACE_SM)
                                .child(render_feature_item(
                                    "⚡",
                                    "Autonomous routing",
                                    "Workloads dispatch to optimal nodes based on latency and capacity.",
                                    theme,
                                ))
                                .child(render_feature_item(
                                    "💳",
                                    "Ed25519 wallet signing",
                                    "Native cryptographic authentication via Solana keypairs.",
                                    theme,
                                ))
                                .child(render_feature_item(
                                    "🛡️",
                                    "Encrypted OS keychain",
                                    "Zero cleartext credentials stored on disk; native OS protection.",
                                    theme,
                                )),
                        )
                        // Network status chip
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(SPACE_SM)
                                .px(SPACE_MD)
                                .py(SPACE_SM)
                                .rounded(RADIUS_FULL)
                                .bg(theme.card_bg)
                                .border_1()
                                .border_color(theme.card_border)
                                .child(
                                    div()
                                        .w(px(8.0))
                                        .h(px(8.0))
                                        .rounded(RADIUS_FULL)
                                        .bg(theme.success)
                                        .flex_none(),
                                )
                                .child(
                                    div()
                                        .text_size(FONT_XS)
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.text_sub_muted)
                                        .truncate()
                                        .child("247 nodes active • avg 14ms latency • Mainnet Beta"),
                                ),
                        ),
                )
                // RIGHT COLUMN: Desktop Authentication Card
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .w(px(380.0))
                        .flex_shrink_0()
                        .rounded(RADIUS_LG)
                        .border_1()
                        .border_color(theme.card_border)
                        .bg(theme.card_bg)
                        .p(SPACE_XL)
                        .shadow_md()
                        .child(
                            // Header + Toggle tabs
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .mb(SPACE_MD)
                                .child(
                                    div()
                                        .text_size(FONT_LG)
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme.text_primary)
                                        .child(heading),
                                )
                                .child(
                                    div()
                                        .text_size(FONT_XS)
                                        .text_color(theme.accent)
                                        .font_weight(FontWeight::MEDIUM)
                                        .cursor_pointer()
                                        .hover(|s| s.underline())
                                        .on_mouse_down(MouseButton::Left, on_toggle_mode)
                                        .child(match mode {
                                            AuthMode::Login => "Create account",
                                            AuthMode::Register => "Sign in",
                                        }),
                                ),
                        )
                        .child(
                            div()
                                .text_size(FONT_XS)
                                .text_color(theme.text_muted)
                                .mb(SPACE_LG)
                                .child("Enter your credentials to connect as a client."),
                        )
                        // Email field
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(SPACE_XS)
                                .mb(SPACE_MD)
                                .child(
                                    div()
                                        .text_size(FONT_XS)
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.text_sub_muted)
                                        .child("Email address"),
                                )
                                .child(
                                    div()
                                        .w_full()
                                        .h(INPUT_HEIGHT)
                                        .rounded(RADIUS_MD)
                                        .border_1()
                                        .border_color(if active_field == ActiveField::Email {
                                            theme.accent
                                        } else {
                                            theme.input_border
                                        })
                                        .bg(theme.input_bg)
                                        .px(SPACE_MD)
                                        .flex()
                                        .items_center()
                                        .cursor_pointer()
                                        .on_mouse_down(MouseButton::Left, on_focus_email)
                                        .child(
                                            div()
                                                .text_size(FONT_BASE)
                                                .text_color(if email.is_empty() {
                                                    theme.text_muted
                                                } else {
                                                    theme.text_primary
                                                })
                                                .child(if email.is_empty() {
                                                    if active_field == ActiveField::Email {
                                                        "|".to_string()
                                                    } else {
                                                        "user@example.com".to_string()
                                                    }
                                                } else {
                                                    email.to_string()
                                                }),
                                        ),
                                ),
                        )
                        // Password field
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(SPACE_XS)
                                .mb(SPACE_LG)
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .text_size(FONT_XS)
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(theme.text_sub_muted)
                                                .child("Password"),
                                        )
                                        .child(
                                            div()
                                                .text_size(FONT_2XS)
                                                .text_color(theme.text_muted)
                                                .child("min 8 chars"),
                                        ),
                                )
                                .child(
                                    div()
                                        .w_full()
                                        .h(INPUT_HEIGHT)
                                        .rounded(RADIUS_MD)
                                        .border_1()
                                        .border_color(if active_field == ActiveField::Password {
                                            theme.accent
                                        } else {
                                            theme.input_border
                                        })
                                        .bg(theme.input_bg)
                                        .px(SPACE_MD)
                                        .flex()
                                        .items_center()
                                        .cursor_pointer()
                                        .on_mouse_down(MouseButton::Left, on_focus_password)
                                        .child(
                                            div()
                                                .text_size(FONT_BASE)
                                                .text_color(if password.is_empty() {
                                                    theme.text_muted
                                                } else {
                                                    theme.text_primary
                                                })
                                                .child(if password.is_empty() {
                                                    if active_field == ActiveField::Password {
                                                        "|".to_string()
                                                    } else {
                                                        "••••••••".to_string()
                                                    }
                                                } else {
                                                    masked_password
                                                }),
                                        ),
                                ),
                        )
                        // Submit button
                        .child(
                            div()
                                .w_full()
                                .h(BUTTON_HEIGHT)
                                .rounded(RADIUS_MD)
                                .bg(theme.accent)
                                .text_color(rgb(0xffffff))
                                .text_size(FONT_BASE)
                                .font_weight(FontWeight::MEDIUM)
                                .flex()
                                .items_center()
                                .justify_center()
                                .gap(SPACE_SM)
                                .cursor_pointer()
                                .when(!is_busy, |s| {
                                    s.on_mouse_down(MouseButton::Left, on_submit_email)
                                })
                                .child("✉")
                                .child(submit_label),
                        )
                        // Divider
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(SPACE_MD)
                                .my(SPACE_MD)
                                .child(div().h(px(1.0)).flex_1().bg(theme.divider))
                                .child(
                                    div()
                                        .text_size(FONT_2XS)
                                        .text_color(theme.text_muted)
                                        .child("or continue with"),
                                )
                                .child(div().h(px(1.0)).flex_1().bg(theme.divider)),
                        )
                        // Third-party buttons: Google + GitHub side-by-side
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .gap(SPACE_SM)
                                .mb(SPACE_SM)
                                .child(
                                    div()
                                        .flex_1()
                                        .h(BUTTON_HEIGHT)
                                        .rounded(RADIUS_MD)
                                        .border_1()
                                        .border_color(theme.card_border)
                                        .bg(theme.input_bg)
                                        .text_color(theme.text_primary)
                                        .text_size(FONT_SM)
                                        .font_weight(FontWeight::MEDIUM)
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .gap(SPACE_SM)
                                        .cursor_pointer()
                                        .hover(|s| s.border_color(theme.accent))
                                        .when(!is_busy, |s| {
                                            s.on_mouse_down(MouseButton::Left, on_login_google)
                                        })
                                        .child("G")
                                        .child(if busy_action == "google" {
                                            "Connecting..."
                                        } else {
                                            "Google"
                                        }),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .h(BUTTON_HEIGHT)
                                        .rounded(RADIUS_MD)
                                        .border_1()
                                        .border_color(theme.card_border)
                                        .bg(theme.input_bg)
                                        .text_color(theme.text_primary)
                                        .text_size(FONT_SM)
                                        .font_weight(FontWeight::MEDIUM)
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .gap(SPACE_SM)
                                        .cursor_pointer()
                                        .hover(|s| s.border_color(theme.accent))
                                        .when(!is_busy, |s| {
                                            s.on_mouse_down(MouseButton::Left, on_login_github)
                                        })
                                        .child("🐙")
                                        .child(if busy_action == "github" {
                                            "Connecting..."
                                        } else {
                                            "GitHub"
                                        }),
                                ),
                        )
                        // Solana Wallet Button (full width)
                        .child(
                            div()
                                .w_full()
                                .h(BUTTON_HEIGHT)
                                .rounded(RADIUS_MD)
                                .border_1()
                                .border_color(theme.card_border)
                                .bg(theme.input_bg)
                                .text_color(theme.text_primary)
                                .text_size(FONT_SM)
                                .font_weight(FontWeight::MEDIUM)
                                .flex()
                                .items_center()
                                .justify_center()
                                .gap(SPACE_SM)
                                .cursor_pointer()
                                .hover(|s| s.border_color(theme.accent))
                                .when(!is_busy, |s| {
                                    s.on_mouse_down(MouseButton::Left, on_login_solana)
                                })
                                .child("💳")
                                .child(if busy_action == "solana" {
                                    "Signing with Solana..."
                                } else {
                                    "Continue with Solana Wallet"
                                }),
                        )
                        // Auth status text
                        .when_some(state.auth_status_text.as_ref(), |parent, text| {
                            parent.child(
                                div()
                                    .text_size(FONT_XS)
                                    .text_color(theme.text_muted)
                                    .text_center()
                                    .mt(SPACE_MD)
                                    .child(text.clone()),
                            )
                        })
                        // Error message
                        .when_some(state.auth_error.as_ref(), |parent, err| {
                            parent.child(
                                div()
                                    .text_size(FONT_XS)
                                    .text_color(rgb(0xef4444))
                                    .text_center()
                                    .mt(SPACE_MD)
                                    .child(err.clone()),
                            )
                        }),
                ),
        )
        )
}

fn render_feature_item(icon: &'static str, title: &'static str, desc: &'static str, theme: &Theme) -> impl IntoElement {
    div()
        .flex()
        .items_start()
        .gap(SPACE_MD)
        .child(
            div()
                .text_size(FONT_MD)
                .flex_none()
                .child(icon),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(px(1.0))
                .child(
                    div()
                        .text_size(FONT_SM)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.text_primary)
                        .child(title),
                )
                .child(
                    div()
                        .text_size(FONT_XS)
                        .text_color(theme.text_muted)
                        .child(desc),
                ),
        )
}

