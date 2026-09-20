use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::theme::{Theme, ORANGE};

pub fn render_toggle(
    is_toggled: bool,
    theme: &Theme,
    on_toggle: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    on_continue: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let button_bg = if is_toggled {
        ORANGE
    } else if theme.is_dark {
        rgb(0x262626)
    } else {
        rgb(0xf0f0f0)
    };

    let button_text = if is_toggled {
        rgb(0xffffff)
    } else if theme.is_dark {
        rgb(0x737373)
    } else {
        rgb(0xa3a3a3)
    };

    let switch_bg = if is_toggled {
        ORANGE
    } else if theme.is_dark {
        rgb(0x404040)
    } else {
        rgb(0xe5e5e5)
    };

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
                .items_center()
                .max_w(px(320.0))
                .child(
                    // Icon
                    div()
                        .w(px(48.0))
                        .h(px(48.0))
                        .rounded(px(12.0))
                        .bg(ORANGE)
                        .flex()
                        .items_center()
                        .justify_center()
                        .mb(px(16.0))
                        .child(
                            div()
                                .text_size(px(24.0))
                                .text_color(rgb(0xffffff))
                                .child("🌐"),
                        ),
                )
                .child(
                    div()
                        .text_size(px(20.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text_primary)
                        .mb(px(6.0))
                        .child("Power on as client"),
                )
                .child(
                    div()
                        .text_size(px(14.0))
                        .text_color(theme.text_muted)
                        .text_center()
                        .mb(px(32.0))
                        .child(
                            "Turn this on to join the mesh. Your commands will route to whichever node picks up the task.",
                        ),
                )
                .child(
                    // Toggle switch
                    div()
                        .w(px(64.0))
                        .h(px(36.0))
                        .rounded(px(18.0))
                        .bg(switch_bg)
                        .flex()
                        .items_center()
                        .px(px(4.0))
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, on_toggle)
                        .child(
                            div()
                                .w(px(28.0))
                                .h(px(28.0))
                                .rounded(px(14.0))
                                .bg(rgb(0xffffff))
                                .shadow_sm()
                                .when(is_toggled, |s| s.ml(px(28.0)))
                        ),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(theme.text_muted)
                        .mt(px(12.0))
                        .child(if is_toggled { "On" } else { "Off" }),
                )
                .child(
                    // Continue button
                    div()
                        .w_full()
                        .mt(px(40.0))
                        .py(px(12.0))
                        .rounded(px(12.0))
                        .bg(button_bg)
                        .text_color(button_text)
                        .font_weight(FontWeight::MEDIUM)
                        .text_size(px(14.0))
                        .text_center()
                        .when(is_toggled, |s| {
                            s.cursor_pointer().on_mouse_down(MouseButton::Left, on_continue)
                        })
                        .child("Continue"),
                ),
        )
}
