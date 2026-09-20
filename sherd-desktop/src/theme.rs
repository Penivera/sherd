use gpui::*;

pub const ORANGE: Rgba = Rgba {
    r: 241.0 / 255.0,
    g: 104.0 / 255.0,
    b: 82.0 / 255.0,
    a: 1.0,
};

pub const LIGHT_ORANGE: Rgba = Rgba {
    r: 254.0 / 255.0,
    g: 234.0 / 255.0,
    b: 223.0 / 255.0,
    a: 1.0,
};

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub is_dark: bool,
    pub background: Rgba,
    pub card_bg: Rgba,
    pub card_border: Rgba,
    pub text_primary: Rgba,
    pub text_muted: Rgba,
    pub text_sub_muted: Rgba,
    pub input_bg: Rgba,
    pub input_border: Rgba,
    pub divider: Rgba,
}

impl Theme {
    pub fn new(is_dark: bool) -> Self {
        if is_dark {
            Self {
                is_dark,
                background: rgb(0x09090b),
                card_bg: rgb(0x171717),
                card_border: rgb(0x262626),
                text_primary: rgb(0xffffff),
                text_muted: rgb(0x737373),
                text_sub_muted: rgb(0xa3a3a3),
                input_bg: rgb(0x171717),
                input_border: rgb(0x262626),
                divider: rgb(0x262626),
            }
        } else {
            Self {
                is_dark,
                background: rgb(0xffffff),
                card_bg: rgb(0xffffff),
                card_border: rgb(0xf5f5f5),
                text_primary: rgb(0x171717),
                text_muted: rgb(0xa3a3a3),
                text_sub_muted: rgb(0x737373),
                input_bg: rgb(0xffffff),
                input_border: rgb(0xe5e5e5),
                divider: rgb(0xe5e5e5),
            }
        }
    }
}
