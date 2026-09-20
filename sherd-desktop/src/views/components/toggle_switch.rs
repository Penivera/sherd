use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::theme::Theme;

pub fn render_toggle_switch(on: bool, theme: &Theme) -> impl IntoElement {
    let bg_color = if on {
        theme.accent
    } else if theme.is_dark {
        rgb(0x3f3f46)
    } else {
        rgb(0xcbd5e1)
    };

    div()
        .w(px(48.0))
        .h(px(26.0))
        .rounded(px(13.0))
        .bg(bg_color)
        .flex()
        .items_center()
        .px(px(3.0))
        .cursor_pointer()
        .child(
            div()
                .w(px(20.0))
                .h(px(20.0))
                .rounded(px(10.0))
                .bg(rgb(0xffffff))
                .shadow_sm()
                .when(on, |s| s.ml(px(22.0))),
        )
}

