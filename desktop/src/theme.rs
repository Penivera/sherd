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

pub const GREEN: Rgba = Rgba {
    r: 34.0 / 255.0,
    g: 197.0 / 255.0,
    b: 94.0 / 255.0,
    a: 1.0,
};

// Spacing Design Tokens
pub const SPACE_XXS: Pixels = px(2.0);
pub const SPACE_XS: Pixels = px(4.0);
pub const SPACE_SM: Pixels = px(8.0);
pub const SPACE_MD: Pixels = px(12.0);
pub const SPACE_LG: Pixels = px(16.0);
pub const SPACE_XL: Pixels = px(24.0);
pub const SPACE_2XL: Pixels = px(32.0);
pub const SPACE_3XL: Pixels = px(48.0);

// Desktop Typography Tokens
pub const FONT_2XS: Pixels = px(11.0);
pub const FONT_XS: Pixels = px(12.0);
pub const FONT_SM: Pixels = px(13.0);
pub const FONT_BASE: Pixels = px(14.0);
pub const FONT_MD: Pixels = px(16.0);
pub const FONT_LG: Pixels = px(18.0);
pub const FONT_XL: Pixels = px(22.0);
pub const FONT_2XL: Pixels = px(28.0);
pub const FONT_3XL: Pixels = px(36.0);

// Corner Radius Tokens
pub const RADIUS_SM: Pixels = px(6.0);
pub const RADIUS_MD: Pixels = px(8.0);
pub const RADIUS_LG: Pixels = px(12.0);
pub const RADIUS_FULL: Pixels = px(9999.0);

// Control Proportions
pub const INPUT_HEIGHT: Pixels = px(38.0);
pub const BUTTON_HEIGHT: Pixels = px(38.0);
pub const BUTTON_HEIGHT_SM: Pixels = px(30.0);

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub is_dark: bool,
    pub background: Rgba,
    pub sidebar_bg: Rgba,
    pub card_bg: Rgba,
    pub card_border: Rgba,
    pub text_primary: Rgba,
    pub text_muted: Rgba,
    pub text_sub_muted: Rgba,
    pub input_bg: Rgba,
    pub input_border: Rgba,
    pub divider: Rgba,
    pub badge_bg: Rgba,
    pub badge_text: Rgba,
    pub accent: Rgba,
    pub accent_light: Rgba,
    pub success: Rgba,
}

impl Theme {
    pub fn new(is_dark: bool) -> Self {
        if is_dark {
            Self {
                is_dark,
                background: rgb(0x0c0c0e),
                sidebar_bg: rgb(0x131316),
                card_bg: rgb(0x18181b),
                card_border: rgb(0x27272a),
                text_primary: rgb(0xf4f4f5),
                text_muted: rgb(0x9ca3af),
                text_sub_muted: rgb(0xd4d4d8),
                input_bg: rgb(0x141417),
                input_border: rgb(0x27272a),
                divider: rgb(0x27272a),
                badge_bg: rgb(0x27272a),
                badge_text: rgb(0xd4d4d8),
                accent: ORANGE,
                accent_light: rgb(0x38221c),
                success: GREEN,
            }
        } else {
            Self {
                is_dark,
                background: rgb(0xf8fafc),
                sidebar_bg: rgb(0xf1f5f9),
                card_bg: rgb(0xffffff),
                card_border: rgb(0xe2e8f0),
                text_primary: rgb(0x0f172a),
                text_muted: rgb(0x64748b),
                text_sub_muted: rgb(0x475569),
                input_bg: rgb(0xffffff),
                input_border: rgb(0xcbd5e1),
                divider: rgb(0xe2e8f0),
                badge_bg: rgb(0xf1f5f9),
                badge_text: rgb(0x334155),
                accent: ORANGE,
                accent_light: LIGHT_ORANGE,
                success: GREEN,
            }
        }
    }
}
