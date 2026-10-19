use super::Theme;
use gpui::{App, px};
use gpui_component::{Theme as ComponentTheme, ThemeMode};

pub(super) fn install(palette: Theme, dark: bool, cx: &mut App) {
    ComponentTheme::change(
        if dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        },
        None,
        cx,
    );
    ComponentTheme::update(cx, |theme| {
        theme.font_family = crate::assets::font().family;
        theme.font_size = px(16.0);
        theme.radius = px(0.0);
        theme.radius_lg = px(0.0);
        let colors = &mut theme.colors;
        colors.background = palette.canvas.into();
        colors.foreground = palette.ink.into();
        colors.border = palette.rule.into();
        colors.accent = palette.hover.into();
        colors.accent_foreground = palette.ink.into();
        colors.muted = palette.surface.into();
        colors.muted_foreground = palette.ink_muted.into();
        colors.popover = palette.canvas.into();
        colors.popover_foreground = palette.ink.into();
        colors.ring = palette.focus_outer.into();
        colors.selection = palette.selected.into();
        colors.scrollbar = palette.canvas.into();
        colors.scrollbar_thumb = palette.rule.into();
        colors.scrollbar_thumb_hover = palette.ink_muted.into();
        colors.button = palette.surface.into();
        colors.button_foreground = palette.ink.into();
        colors.button_hover = palette.hover.into();
        colors.button_active = palette.selected.into();
        colors.primary = palette.ink.into();
        colors.primary_foreground = palette.inverse.into();
        colors.secondary = palette.surface.into();
        colors.secondary_foreground = palette.ink.into();
        colors.secondary_hover = palette.hover.into();
        colors.secondary_active = palette.selected.into();
        colors.danger = palette.danger.into();
        colors.danger_foreground = palette.inverse.into();
        colors.input = palette.rule.into();
        colors.caret = palette.ink.into();
        colors.skeleton = palette.skeleton_fade.into();
    });
}
