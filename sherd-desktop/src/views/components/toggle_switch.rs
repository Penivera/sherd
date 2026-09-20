use gpui::*;

use crate::theme::{Theme, ORANGE};

pub fn render_toggle_switch(on: bool, theme: &Theme) -> impl IntoElement {
    let bg_color = if on {
        ORANGE
    } else if theme.is_dark {
        rgb(0x404040)
    } else {
        rgb(0xe5e5e5)
    };

    div()
        .w(px(64.0))
        .h(px(36.0))
        .rounded(px(18.0))
        .bg(bg_color)
        .flex()
        .items_center()
        .px(px(4.0))
        .cursor_pointer()
        .child(
            div()
                .flex()
                .size_full()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .w(px(28.0))
                        .h(px(28.0))
                        .rounded(px(14.0))
                        .bg(rgb(0xffffff))
                        .shadow_sm()
                )
        )
}
