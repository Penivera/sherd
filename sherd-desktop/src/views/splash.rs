use gpui::*;

use crate::theme::{Theme, ORANGE};

pub fn render_splash(theme: &Theme) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .size_full()
        .bg(theme.background)
        .gap(px(16.0))
        .child(
            // Logo container
            div()
                .w(px(80.0))
                .h(px(80.0))
                .rounded(px(20.0))
                .bg(rgb(0xffffff))
                .shadow_lg()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_size(px(40.0))
                        .text_color(ORANGE)
                        .child("⚡")
                ),
        )
        .child(
            div()
                .text_size(px(36.0))
                .font_weight(FontWeight::BOLD)
                .text_color(theme.text_primary)
                .child("Sherd"),
        )
}
