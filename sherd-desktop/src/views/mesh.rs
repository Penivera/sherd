use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::state::{AppState, MeshNode, MeshTask};
use crate::theme::{
    Theme, BUTTON_HEIGHT_SM, FONT_LG, FONT_MD, FONT_SM, FONT_XS, FONT_2XS,
    RADIUS_FULL, RADIUS_LG, RADIUS_MD, RADIUS_SM, SPACE_LG, SPACE_MD, SPACE_SM,
    SPACE_XL, SPACE_XS, SPACE_2XL,
};

pub fn render_mesh(
    state: &AppState,
    theme: &Theme,
    on_logout: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    on_simulate: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let user_label = if let Some(ref auth) = state.auth_method {
        let short_addr = if auth.wallet_address.len() > 8 {
            format!(
                "{}...{}",
                &auth.wallet_address[..4],
                &auth.wallet_address[auth.wallet_address.len() - 4..]
            )
        } else {
            auth.wallet_address.clone()
        };
        format!("Solana • {}", short_addr)
    } else if let Some(ref session) = state.session {
        session.user.email.clone().unwrap_or_else(|| {
            if session.user.id.len() > 12 {
                format!("User {}", &session.user.id[..8])
            } else {
                session.user.id.clone()
            }
        })
    } else {
        "Connected Client".to_string()
    };

    div()
        .flex()
        .flex_col()
        .size_full()
        .bg(theme.background)
        .child(
            // TOP DESKTOP HEADER
            div()
                .flex()
                .items_center()
                .justify_between()
                .pl(SPACE_2XL)
                .pr(px(68.0))
                .py(SPACE_MD)
                .border_b_1()
                .border_color(theme.card_border)
                .bg(theme.card_bg)
                .child(
                    // Logo + Breadcrumb
                    div()
                        .flex()
                        .items_center()
                        .gap(SPACE_MD)
                        .child(
                            div()
                                .w(px(32.0))
                                .h(px(32.0))
                                .rounded(RADIUS_MD)
                                .bg(theme.accent)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    div()
                                        .text_size(FONT_MD)
                                        .text_color(rgb(0xffffff))
                                        .child("⚡"),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(SPACE_SM)
                                .child(
                                    div()
                                        .text_size(FONT_MD)
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(theme.text_primary)
                                        .child("Sherd"),
                                )
                                .child(
                                    div()
                                        .text_size(FONT_SM)
                                        .text_color(theme.text_muted)
                                        .child("/"),
                                )
                                .child(
                                    div()
                                        .text_size(FONT_SM)
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.text_sub_muted)
                                        .child("Mesh Dashboard"),
                                ),
                        )
                        .child(
                            // Live network status badge
                            div()
                                .flex()
                                .items_center()
                                .gap(SPACE_SM)
                                .px(SPACE_MD)
                                .py(px(4.0))
                                .rounded(RADIUS_FULL)
                                .bg(theme.accent_light)
                                .border_1()
                                .border_color(theme.accent)
                                .child(
                                    div()
                                        .w(px(6.0))
                                        .h(px(6.0))
                                        .rounded(RADIUS_FULL)
                                        .bg(theme.success),
                                )
                                .child(
                                    div()
                                        .text_size(FONT_XS)
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.text_primary)
                                        .child("247 Nodes Online"),
                                ),
                        ),
                )
                .child(
                    // Header Actions: User chip + Simulate button + Logout
                    div()
                        .flex()
                        .items_center()
                        .gap(SPACE_MD)
                        .child(
                            div()
                                .h(BUTTON_HEIGHT_SM)
                                .px(SPACE_MD)
                                .rounded(RADIUS_MD)
                                .bg(theme.accent)
                                .text_color(rgb(0xffffff))
                                .text_size(FONT_SM)
                                .font_weight(FontWeight::MEDIUM)
                                .flex()
                                .items_center()
                                .gap(SPACE_SM)
                                .cursor_pointer()
                                .on_mouse_down(MouseButton::Left, on_simulate)
                                .child("+ Simulate Task"),
                        )
                        .child(
                            // User identity chip
                            div()
                                .flex()
                                .items_center()
                                .gap(SPACE_SM)
                                .px(SPACE_MD)
                                .h(BUTTON_HEIGHT_SM)
                                .rounded(RADIUS_FULL)
                                .bg(theme.input_bg)
                                .border_1()
                                .border_color(theme.card_border)
                                .child(
                                    div()
                                        .text_size(FONT_XS)
                                        .child("👤"),
                                )
                                .child(
                                    div()
                                        .text_size(FONT_XS)
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.text_primary)
                                        .child(user_label),
                                ),
                        )
                        .child(
                            // Logout button
                            div()
                                .flex()
                                .items_center()
                                .gap(SPACE_XS)
                                .px(SPACE_MD)
                                .h(BUTTON_HEIGHT_SM)
                                .rounded(RADIUS_MD)
                                .border_1()
                                .border_color(theme.card_border)
                                .bg(theme.input_bg)
                                .text_color(theme.text_muted)
                                .text_size(FONT_XS)
                                .font_weight(FontWeight::MEDIUM)
                                .cursor_pointer()
                                .hover(|s| s.text_color(theme.text_primary).border_color(theme.accent))
                                .on_mouse_down(MouseButton::Left, on_logout)
                                .child("⏻ Log out"),
                        ),
                ),
        )
        .child(
            // DESKTOP MULTI-COLUMN WORKSPACE
            div()
                .flex()
                .flex_row()
                .flex_1()
                .size_full()
                .p(SPACE_XL)
                .gap(SPACE_XL)
                // LEFT SIDEBAR: Telemetry & Terminal Guidance
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .w(px(320.0))
                        .flex_shrink_0()
                        .gap(SPACE_LG)
                        // Client Telemetry Card
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .p(SPACE_LG)
                                .rounded(RADIUS_LG)
                                .bg(theme.card_bg)
                                .border_1()
                                .border_color(theme.card_border)
                                .shadow_sm()
                                .gap(SPACE_MD)
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .text_size(FONT_SM)
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(theme.text_primary)
                                                .child("Client Telemetry"),
                                        )
                                        .child(
                                            div()
                                                .text_size(FONT_2XS)
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(theme.accent)
                                                .child("LIVE"),
                                        ),
                                )
                                .child(render_stat_row("Connection Status", "Connected to Mesh", theme))
                                .child(render_stat_row("IPC Transport", "sherd-ipc.sock", theme))
                                .child(render_stat_row("Available Nodes", "247 active", theme))
                                .child(render_stat_row("Median Node Rate", "$0.012 / hr", theme))
                                .child(render_stat_row("Median Roundtrip", "14 ms", theme)),
                        )
                        // Terminal Execution Explainer Card
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .p(SPACE_LG)
                                .rounded(RADIUS_LG)
                                .bg(theme.card_bg)
                                .border_1()
                                .border_color(theme.card_border)
                                .shadow_sm()
                                .gap(SPACE_SM)
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(SPACE_SM)
                                        .child(
                                            div()
                                                .text_size(FONT_MD)
                                                .text_color(theme.accent)
                                                .child(">_"),
                                        )
                                        .child(
                                            div()
                                                .text_size(FONT_SM)
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(theme.text_primary)
                                                .child("Background Execution"),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_size(FONT_XS)
                                        .line_height(relative(1.5))
                                        .text_color(theme.text_muted)
                                        .child(
                                            "Return to your terminal or editor. Commands dispatched through the client daemon are distributed across the mesh nodes, and execution results stream directly back.",
                                        ),
                                ),
                        ),
                )
                // RIGHT MAIN PANEL: Active Nodes Grid + Task Activity Stream
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .gap(SPACE_XL)
                        // SECTION 1: Active Nodes in Mesh (Desktop Grid)
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(SPACE_MD)
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap(SPACE_SM)
                                                .child(
                                                    div()
                                                        .text_size(FONT_LG)
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .text_color(theme.text_primary)
                                                        .child("Active Mesh Nodes"),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(FONT_2XS)
                                                        .font_weight(FontWeight::MEDIUM)
                                                        .px(SPACE_SM)
                                                        .py(px(2.0))
                                                        .rounded(RADIUS_FULL)
                                                        .bg(theme.badge_bg)
                                                        .text_color(theme.text_sub_muted)
                                                        .child(format!("{} nodes", state.nodes.len())),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .text_size(FONT_XS)
                                                .text_color(theme.text_muted)
                                                .child("Sorted by lowest price & latency"),
                                        ),
                                )
                                // Responsive Desktop Grid of Node Cards
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .flex_wrap()
                                        .gap(SPACE_MD)
                                        .children(state.nodes.iter().map(|n| render_desktop_node_card(n, theme))),
                                ),
                        )
                        // SECTION 2: Task Execution Activity Stream (Desktop Table)
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(SPACE_MD)
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap(SPACE_SM)
                                                .child(
                                                    div()
                                                        .text_size(FONT_LG)
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .text_color(theme.text_primary)
                                                        .child("Task Execution Activity"),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(FONT_2XS)
                                                        .font_weight(FontWeight::MEDIUM)
                                                        .px(SPACE_SM)
                                                        .py(px(2.0))
                                                        .rounded(RADIUS_FULL)
                                                        .bg(theme.badge_bg)
                                                        .text_color(theme.text_sub_muted)
                                                        .child(format!("{} tasks", state.tasks.len())),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .text_size(FONT_XS)
                                                .text_color(theme.text_muted)
                                                .child("Real-time distributed workload logs"),
                                        ),
                                )
                                // Desktop Task Table
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .rounded(RADIUS_LG)
                                        .border_1()
                                        .border_color(theme.card_border)
                                        .bg(theme.card_bg)
                                        .shadow_sm()
                                        // Table Header
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .px(SPACE_LG)
                                                .py(SPACE_SM)
                                                .border_b_1()
                                                .border_color(theme.card_border)
                                                .bg(theme.sidebar_bg)
                                                .child(
                                                    div()
                                                        .w(px(80.0))
                                                        .text_size(FONT_2XS)
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .text_color(theme.text_muted)
                                                        .child("TASK ID"),
                                                )
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .text_size(FONT_2XS)
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .text_color(theme.text_muted)
                                                        .child("COMMAND / INSTRUCTION"),
                                                )
                                                .child(
                                                    div()
                                                        .w(px(140.0))
                                                        .text_size(FONT_2XS)
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .text_color(theme.text_muted)
                                                        .child("ASSIGNED NODE"),
                                                )
                                                .child(
                                                    div()
                                                        .w(px(100.0))
                                                        .text_size(FONT_2XS)
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .text_color(theme.text_muted)
                                                        .child("STATUS"),
                                                ),
                                        )
                                        // Table Rows
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .children(state.tasks.iter().map(|t| render_desktop_task_row(t, theme))),
                                        ),
                                ),
                        ),
                ),
        )
}

fn render_stat_row(label: &'static str, val: &'static str, theme: &Theme) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_size(FONT_XS)
                .text_color(theme.text_muted)
                .child(label),
        )
        .child(
            div()
                .text_size(FONT_XS)
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text_primary)
                .child(val),
        )
}

fn render_desktop_node_card(node: &MeshNode, theme: &Theme) -> impl IntoElement {
    div()
        .w(px(180.0))
        .p(SPACE_MD)
        .rounded(RADIUS_MD)
        .border_1()
        .border_color(theme.card_border)
        .bg(theme.card_bg)
        .shadow_sm()
        .flex()
        .flex_col()
        .gap(SPACE_SM)
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .w(px(28.0))
                        .h(px(28.0))
                        .rounded(RADIUS_SM)
                        .bg(theme.accent)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(FONT_XS)
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(0xffffff))
                        .child(node.id.to_string()),
                )
                .child(
                    div()
                        .px(SPACE_SM)
                        .py(px(2.0))
                        .rounded(RADIUS_FULL)
                        .bg(theme.accent_light)
                        .text_size(FONT_2XS)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.accent)
                        .child(format!("${}/hr", node.price)),
                ),
        )
        .child(
            div()
                .text_size(FONT_SM)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.text_primary)
                .child(format!("Peer Node {}", node.id)),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(FONT_2XS)
                        .text_color(theme.text_muted)
                        .child("Latency: 12ms"),
                )
                .child(
                    div()
                        .text_size(FONT_2XS)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.success)
                        .child("Ready"),
                ),
        )
}

fn render_desktop_task_row(task: &MeshTask, theme: &Theme) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .px(SPACE_LG)
        .py(SPACE_MD)
        .border_b_1()
        .border_color(theme.card_border)
        .child(
            div()
                .w(px(80.0))
                .text_size(FONT_XS)
                .font_family("monospace")
                .text_color(theme.text_muted)
                .child(format!("#{}", task.id % 10000)),
        )
        .child(
            div()
                .flex_1()
                .text_size(FONT_SM)
                .font_family("monospace")
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text_primary)
                .child(task.cmd.clone()),
        )
        .child(
            div()
                .w(px(140.0))
                .flex()
                .items_center()
                .gap(SPACE_SM)
                .child(
                    div()
                        .w(px(8.0))
                        .h(px(8.0))
                        .rounded(RADIUS_FULL)
                        .bg(theme.accent),
                )
                .child(
                    div()
                        .text_size(FONT_XS)
                        .text_color(theme.text_sub_muted)
                        .child(task.node.clone()),
                ),
        )
        .child(
            div()
                .w(px(100.0))
                .child(
                    div()
                        .px(SPACE_SM)
                        .py(px(2.0))
                        .rounded(RADIUS_FULL)
                        .flex_none()
                        .when(task.status == "running", |s| {
                            s.bg(theme.accent_light)
                                .child(
                                    div()
                                        .text_size(FONT_2XS)
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.accent)
                                        .child("● Running..."),
                                )
                        })
                        .when(task.status == "done", |s| {
                            s.bg(theme.badge_bg)
                                .child(
                                    div()
                                        .text_size(FONT_2XS)
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.text_muted)
                                        .child("✓ Complete"),
                                )
                        }),
                ),
        )
}

