use gpui::*;

use crate::theme::{Theme, FONT_BASE, FONT_SM, FONT_3XL, RADIUS_LG, RADIUS_FULL, SPACE_MD, SPACE_SM, SPACE_XL};

pub fn render_splash(theme: &Theme) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .size_full()
        .bg(theme.background)
        .gap(SPACE_XL)
        .child(
            // Desktop Brand Emblem
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(SPACE_MD)
                .child(
                    div()
                        .w(px(72.0))
                        .h(px(72.0))
                        .rounded(RADIUS_LG)
                        .bg(theme.accent)
                        .shadow_lg()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .text_size(px(36.0))
                                .text_color(rgb(0xffffff))
                                .child("⚡"),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(SPACE_SM)
                        .child(
                            div()
                                .text_size(FONT_3XL)
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme.text_primary)
                                .child("Sherd"),
                        )
                        .child(
                            div()
                                .text_size(FONT_BASE)
                                .text_color(theme.text_muted)
                                .child("Decentralized Edge Compute Network"),
                        ),
                ),
        )
        .child(
            // Loading status pill
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
                        .bg(theme.accent),
                )
                .child(
                    div()
                        .text_size(FONT_SM)
                        .text_color(theme.text_muted)
                        .child("Initializing client runtime..."),
                ),
        )
}

