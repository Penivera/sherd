use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::theme::{
    Theme, BUTTON_HEIGHT, FONT_2XL, FONT_BASE, FONT_LG, FONT_SM, FONT_XS, FONT_2XS,
    RADIUS_FULL, RADIUS_LG, RADIUS_MD, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL,
    SPACE_2XL, SPACE_3XL,
};

pub fn render_toggle(
    is_toggled: bool,
    theme: &Theme,
    on_toggle: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    on_continue: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let button_bg = if is_toggled {
        theme.accent
    } else if theme.is_dark {
        rgb(0x27272a)
    } else {
        rgb(0xe2e8f0)
    };

    let button_text = if is_toggled {
        rgb(0xffffff)
    } else if theme.is_dark {
        rgb(0x71717a)
    } else {
        rgb(0x94a3b8)
    };

    let switch_bg = if is_toggled {
        theme.accent
    } else if theme.is_dark {
        rgb(0x3f3f46)
    } else {
        rgb(0xcbd5e1)
    };

    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .size_full()
        .bg(theme.background)
        .px(SPACE_3XL)
        .py(SPACE_2XL)
        .child(
            // Desktop Center Card
            div()
                .flex()
                .flex_col()
                .w_full()
                .max_w(px(600.0))
                .rounded(RADIUS_LG)
                .border_1()
                .border_color(theme.card_border)
                .bg(theme.card_bg)
                .p(SPACE_2XL)
                .shadow_lg()
                .child(
                    // Status badge & Icon row
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .mb(SPACE_LG)
                        .child(
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
                                                .child("🌐"),
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
                                                .child("Client Activation"),
                                        )
                                        .child(
                                            div()
                                                .text_size(FONT_XS)
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(theme.accent)
                                                .child("SHERD DECENTRALIZED MESH"),
                                        ),
                                ),
                        )
                        .child(
                            // Live state chip
                            div()
                                .flex()
                                .items_center()
                                .gap(SPACE_SM)
                                .px(SPACE_MD)
                                .py(SPACE_SM)
                                .rounded(RADIUS_FULL)
                                .bg(if is_toggled {
                                    theme.accent_light
                                } else {
                                    theme.badge_bg
                                })
                                .border_1()
                                .border_color(if is_toggled {
                                    theme.accent
                                } else {
                                    theme.card_border
                                })
                                .child(
                                    div()
                                        .w(px(8.0))
                                        .h(px(8.0))
                                        .rounded(RADIUS_FULL)
                                        .bg(if is_toggled {
                                            theme.success
                                        } else {
                                            theme.text_muted
                                        }),
                                )
                                .child(
                                    div()
                                        .text_size(FONT_XS)
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(if is_toggled {
                                            theme.text_primary
                                        } else {
                                            theme.text_muted
                                        })
                                        .child(if is_toggled { "ONLINE & READY" } else { "STANDBY" }),
                                ),
                        ),
                )
                .child(
                    div()
                        .text_size(FONT_LG)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text_primary)
                        .mb(SPACE_SM)
                        .child("Power on as Mesh Client"),
                )
                .child(
                    div()
                        .text_size(FONT_SM)
                        .text_color(theme.text_muted)
                        .line_height(relative(1.5))
                        .mb(SPACE_XL)
                        .child(
                            "Join this workstation to the decentralized mesh. Workloads executed from your terminal or developer tools will automatically route to verified peer nodes with optimal capacity.",
                        ),
                )
                // Desktop Network Parameters Chips (3-column layout)
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap(SPACE_MD)
                        .mb(SPACE_XL)
                        .child(render_param_chip("Protocol", "SHERD v1 IPC", theme))
                        .child(render_param_chip("Target Mesh", "Mainnet Beta", theme))
                        .child(render_param_chip("Dispatch Mode", "Autonomous", theme)),
                )
                // Toggle Switch Panel
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .p(SPACE_LG)
                        .rounded(RADIUS_MD)
                        .bg(theme.input_bg)
                        .border_1()
                        .border_color(theme.card_border)
                        .mb(SPACE_XL)
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(2.0))
                                .child(
                                    div()
                                        .text_size(FONT_SM)
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.text_primary)
                                        .child("Mesh Client Daemon Connection"),
                                )
                                .child(
                                    div()
                                        .text_size(FONT_XS)
                                        .text_color(theme.text_muted)
                                        .child(if is_toggled {
                                            "Daemon active; accepting command execution requests."
                                        } else {
                                            "Daemon dormant; flip switch to connect."
                                        }),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(SPACE_MD)
                                .child(
                                    div()
                                        .text_size(FONT_XS)
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(if is_toggled {
                                            theme.accent
                                        } else {
                                            theme.text_muted
                                        })
                                        .child(if is_toggled { "ON" } else { "OFF" }),
                                )
                                .child(
                                    // Desktop-proportioned Toggle Switch
                                    div()
                                        .w(px(52.0))
                                        .h(px(28.0))
                                        .rounded(px(14.0))
                                        .bg(switch_bg)
                                        .flex()
                                        .items_center()
                                        .px(px(3.0))
                                        .cursor_pointer()
                                        .on_mouse_down(MouseButton::Left, on_toggle)
                                        .child(
                                            div()
                                                .w(px(22.0))
                                                .h(px(22.0))
                                                .rounded(px(11.0))
                                                .bg(rgb(0xffffff))
                                                .shadow_sm()
                                                .when(is_toggled, |s| s.ml(px(24.0))),
                                        ),
                                ),
                        ),
                )
                // Continue / Enter Dashboard Button
                .child(
                    div()
                        .w_full()
                        .h(BUTTON_HEIGHT)
                        .rounded(RADIUS_MD)
                        .bg(button_bg)
                        .text_color(button_text)
                        .font_weight(FontWeight::MEDIUM)
                        .text_size(FONT_BASE)
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap(SPACE_SM)
                        .when(is_toggled, |s| {
                            s.cursor_pointer().on_mouse_down(MouseButton::Left, on_continue)
                        })
                        .child("Enter Mesh Dashboard →"),
                ),
        )
}

fn render_param_chip(label: &'static str, value: &'static str, theme: &Theme) -> impl IntoElement {
    div()
        .flex_1()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .p(SPACE_MD)
        .rounded(RADIUS_MD)
        .bg(theme.input_bg)
        .border_1()
        .border_color(theme.card_border)
        .child(
            div()
                .text_size(FONT_2XS)
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text_muted)
                .child(label),
        )
        .child(
            div()
                .text_size(FONT_SM)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.text_primary)
                .child(value),
        )
}

