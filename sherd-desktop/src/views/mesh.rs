use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::state::{AppState, MeshNode, MeshTask};
use crate::theme::{Theme, LIGHT_ORANGE, ORANGE};

pub fn render_mesh(
    state: &AppState,
    theme: &Theme,
    on_logout: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    on_simulate: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let explainer_bg = if theme.is_dark {
        rgb(0x3a2620)
    } else {
        LIGHT_ORANGE
    };

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
        format!("Signed in with Solana wallet {} ({})", short_addr, auth.kind)
    } else if let Some(ref session) = state.session {
        format!(
            "Authenticated as {}",
            session.user.email.as_deref().unwrap_or(&session.user.id)
        )
    } else {
        "Connected".to_string()
    };

    div()
        .flex()
        .flex_col()
        .size_full()
        .bg(theme.background)
        .px(px(20.0))
        .py(px(24.0))
        .overflow_y_hidden()
        .child(
            // Top identity bar + logout
            div()
                .flex()
                .items_center()
                .justify_between()
                .mb(px(16.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(theme.text_muted)
                        .child(user_label),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(theme.text_muted)
                        .cursor_pointer()
                        .hover(|s| s.text_color(theme.text_primary))
                        .on_mouse_down(MouseButton::Left, on_logout)
                        .child("⏻ Log out"),
                ),
        )
        .child(
            // Status header
            div()
                .flex()
                .items_center()
                .justify_between()
                .mb(px(16.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            div()
                                .w(px(10.0))
                                .h(px(10.0))
                                .rounded(px(5.0))
                                .bg(ORANGE),
                        )
                        .child(
                            div()
                                .text_size(px(14.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.text_primary)
                                .child("Connected to mesh"),
                        ),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(theme.text_muted)
                        .child("247 nodes online"),
                ),
        )
        .child(
            // Explainer card
            div()
                .rounded(px(12.0))
                .p(px(16.0))
                .mb(px(20.0))
                .bg(explainer_bg)
                .flex()
                .gap(px(12.0))
                .child(
                    div()
                        .text_size(px(16.0))
                        .text_color(ORANGE)
                        .child(">_"),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .line_height(relative(1.4))
                        .text_color(if theme.is_dark {
                            rgb(0xd4d4d4)
                        } else {
                            rgb(0x404040)
                        })
                        .child(
                            "Go back to your terminal or editor as usual. Commands you run will be picked up and executed by an available node in the mesh, then results are sent back to you here.",
                        ),
                ),
        )
        .child(
            // Nodes header
            div()
                .text_size(px(14.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text_primary)
                .mb(px(8.0))
                .child("Nodes in the mesh"),
        )
        .child(
            // Nodes list
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .mb(px(20.0))
                .children(state.nodes.iter().map(|n| render_node_card(n, theme))),
        )
        .child(
            // Task activity header
            div()
                .text_size(px(14.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text_primary)
                .mb(px(8.0))
                .child("Task activity"),
        )
        .child(
            // Task activity list
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .mb(px(24.0))
                .children(state.tasks.iter().map(|t| render_task_card(t, theme))),
        )
        .child(
            // Simulate task button
            div()
                .w_full()
                .py(px(12.0))
                .rounded(px(12.0))
                .bg(ORANGE)
                .text_color(rgb(0xffffff))
                .text_size(px(14.0))
                .font_weight(FontWeight::MEDIUM)
                .text_center()
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, on_simulate)
                .child("Simulate a task"),
        )
}

fn render_node_card(node: &MeshNode, theme: &Theme) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .px(px(16.0))
        .py(px(10.0))
        .rounded(px(12.0))
        .border_2()
        .border_color(theme.card_border)
        .bg(theme.card_bg)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(12.0))
                .child(
                    div()
                        .w(px(32.0))
                        .h(px(32.0))
                        .rounded(px(8.0))
                        .bg(ORANGE)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(0xffffff))
                        .child(node.id.to_string()),
                )
                .child(
                    div()
                        .text_size(px(14.0))
                        .text_color(theme.text_primary)
                        .child(format!("Node {}", node.id)),
                ),
        )
        .child(
            div()
                .text_size(px(14.0))
                .text_color(theme.text_sub_muted)
                .child(format!("${}/hr", node.price)),
        )
}

fn render_task_card(task: &MeshTask, theme: &Theme) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .px(px(16.0))
        .py(px(12.0))
        .rounded(px(12.0))
        .border_2()
        .border_color(theme.card_border)
        .bg(theme.card_bg)
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_family("monospace")
                        .text_color(theme.text_primary)
                        .child(task.cmd.clone()),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(theme.text_muted)
                        .child(format!("picked up by {}", task.node)),
                ),
        )
        .child(
            div()
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .when(task.status == "running", |s| {
                    s.text_color(ORANGE).child("running…")
                })
                .when(task.status == "done", |s| {
                    s.text_color(theme.text_muted).child("✓ done")
                }),
        )
}
