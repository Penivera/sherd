use desktop::state::{load_theme_preference, AppState};
use desktop::theme::Theme;

#[test]
fn test_theme_dark_tokens() {
    let dark = Theme::new(true);
    assert!(dark.is_dark);
    assert_eq!(dark.background.r, 0x0c as f32 / 255.0);
    assert_eq!(dark.card_bg.r, 0x18 as f32 / 255.0);
    assert_eq!(dark.text_primary.r, 0xf4 as f32 / 255.0);
}

#[test]
fn test_theme_light_tokens() {
    let light = Theme::new(false);
    assert!(!light.is_dark);
    assert_eq!(light.background.r, 0xf8 as f32 / 255.0);
    assert_eq!(light.card_bg.r, 0xff as f32 / 255.0);
    assert_eq!(light.text_primary.r, 0x0f as f32 / 255.0);
}

#[test]
fn test_app_state_theme_toggle() {
    let mut state = AppState::new();
    let initial = state.is_dark;
    state.toggle_theme();
    assert_eq!(state.is_dark, !initial);
    state.toggle_theme();
    assert_eq!(state.is_dark, initial);
}

#[test]
fn test_theme_env_override() {
    std::env::set_var("SHERD_DARK_MODE", "dark");
    assert_eq!(load_theme_preference(), Some(true));

    std::env::set_var("SHERD_DARK_MODE", "light");
    assert_eq!(load_theme_preference(), Some(false));

    std::env::remove_var("SHERD_DARK_MODE");
}
