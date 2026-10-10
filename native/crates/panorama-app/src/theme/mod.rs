//! Panorama design tokens; light is the shipping default.
pub mod color;
mod component;
mod focus;
pub mod layout;
pub mod motion;
#[cfg(test)]
mod token_tests;
pub mod typography;

pub use crate::motion::ease_editorial;
pub use color::{hex, oklch};
pub use focus::focus_ring;
use gpui::Rgba;
pub use layout::{content_width, dialog_inset, page_gutter};
pub use motion::{DURATION_DELIBERATE, DURATION_FAST, DURATION_STANDARD, EASE_EDITORIAL, SHIMMER};
use std::sync::OnceLock;
pub use typography::Typography;

/// Color roles shared with app/globals.css.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    /// The CSS theme-canvas role.
    pub canvas: Rgba,
    /// The CSS theme-ink role.
    pub ink: Rgba,
    /// The CSS theme-ink-muted role.
    pub ink_muted: Rgba,
    /// The CSS theme-surface role.
    pub surface: Rgba,
    /// The CSS theme-surface-strong role.
    pub surface_strong: Rgba,
    /// The CSS theme-rule role.
    pub rule: Rgba,
    /// The CSS theme-danger role.
    pub danger: Rgba,
    /// The CSS theme-inverse role.
    pub inverse: Rgba,
    /// The CSS theme-scrim role.
    pub scrim: Rgba,
    /// The CSS theme-image-outline role.
    pub image_outline: Rgba,
    /// The CSS theme-focus-inner role.
    pub focus_inner: Rgba,
    /// The CSS theme-focus-outer role.
    pub focus_outer: Rgba,
    /// The CSS theme-hover role.
    pub hover: Rgba,
    /// The CSS theme-selected role.
    pub selected: Rgba,
    /// The CSS theme-disabled role.
    pub disabled: Rgba,
    /// The CSS theme-shadow role.
    pub shadow: Rgba,
    /// The CSS theme-hero-top role.
    pub hero_top: Rgba,
    /// The CSS theme-hero-mid role.
    pub hero_mid: Rgba,
    /// The CSS theme-hero-bottom role.
    pub hero_bottom: Rgba,
    /// The CSS theme-hero-copy-fade role.
    pub hero_copy_fade: Rgba,
    /// The CSS theme-artwork-fade role.
    pub artwork_fade: Rgba,
    /// The CSS theme-artwork-empty role.
    pub artwork_empty: Rgba,
    /// The CSS theme-skeleton-fade role.
    pub skeleton_fade: Rgba,
    /// The CSS theme-player-canvas role.
    pub player_canvas: Rgba,
    /// The CSS theme-player-ink role.
    pub player_ink: Rgba,
}

impl Default for Theme {
    fn default() -> Self {
        Self::light()
    }
}

impl Theme {
    /// The shipping light palette, expressed in its original CSS coordinates.
    pub fn light() -> Self {
        static LIGHT: OnceLock<Theme> = OnceLock::new();
        *LIGHT.get_or_init(|| Self {
            canvas: oklch(1.0, 0.0, 0.0, 1.0),
            ink: oklch(0.2, 0.0, 0.0, 1.0),
            ink_muted: oklch(0.42, 0.01, 84.588, 1.0),
            surface: hex(0xebebeb),
            surface_strong: oklch(0.853, 0.013, 82.401, 1.0),
            rule: oklch(0.2, 0.0, 0.0, 0.18),
            danger: oklch(0.464, 0.145, 29.093, 1.0),
            inverse: oklch(0.985, 0.005, 87.471, 1.0),
            scrim: oklch(0.1, 0.0, 0.0, 0.72),
            image_outline: oklch(0.0, 0.0, 0.0, 0.1),
            focus_inner: oklch(0.985, 0.005, 87.471, 1.0),
            focus_outer: oklch(0.2, 0.0, 0.0, 1.0),
            hover: oklch(0.2, 0.0, 0.0, 0.07),
            selected: oklch(0.2, 0.0, 0.0, 0.12),
            disabled: oklch(0.525, 0.01, 84.588, 0.55),
            shadow: oklch(0.1, 0.0, 0.0, 0.28),
            hero_top: oklch(0.08, 0.0, 0.0, 0.62),
            hero_mid: oklch(0.08, 0.0, 0.0, 0.03),
            hero_bottom: oklch(0.05, 0.0, 0.0, 0.82),
            hero_copy_fade: oklch(0.03, 0.0, 0.0, 0.4),
            artwork_fade: oklch(0.12, 0.0, 0.0, 0.58),
            artwork_empty: hex(0xfecd00),
            skeleton_fade: oklch(0.2, 0.0, 0.0, 0.28),
            player_canvas: oklch(0.08, 0.0, 0.0, 1.0),
            player_ink: oklch(0.985, 0.0, 0.0, 1.0),
        })
    }

    /// The dark palette inherits roles omitted from the CSS override.
    pub fn dark() -> Self {
        static DARK: OnceLock<Theme> = OnceLock::new();
        *DARK.get_or_init(|| Self {
            canvas: oklch(0.18, 0.01, 87.471, 1.0),
            ink: oklch(0.94, 0.005, 87.471, 1.0),
            ink_muted: oklch(0.7, 0.01, 84.588, 1.0),
            surface: oklch(0.24, 0.012, 84.58, 1.0),
            surface_strong: oklch(0.3, 0.013, 82.401, 1.0),
            rule: oklch(0.94, 0.005, 87.471, 0.2),
            danger: oklch(0.68, 0.145, 29.093, 1.0),
            inverse: oklch(0.18, 0.01, 87.471, 1.0),
            image_outline: oklch(1.0, 0.0, 0.0, 0.1),
            focus_inner: oklch(0.18, 0.01, 87.471, 1.0),
            focus_outer: oklch(0.94, 0.005, 87.471, 1.0),
            hover: oklch(0.94, 0.005, 87.471, 0.08),
            selected: oklch(0.94, 0.005, 87.471, 0.14),
            disabled: oklch(0.7, 0.01, 84.588, 0.5),
            shadow: oklch(0.05, 0.0, 0.0, 0.6),
            ..Self::light()
        })
    }

    /// Named color values for drift detection.
    pub fn colors(self) -> [(&'static str, Rgba); 25] {
        [
            ("canvas", self.canvas),
            ("ink", self.ink),
            ("ink-muted", self.ink_muted),
            ("surface", self.surface),
            ("surface-strong", self.surface_strong),
            ("rule", self.rule),
            ("danger", self.danger),
            ("inverse", self.inverse),
            ("scrim", self.scrim),
            ("image-outline", self.image_outline),
            ("focus-inner", self.focus_inner),
            ("focus-outer", self.focus_outer),
            ("hover", self.hover),
            ("selected", self.selected),
            ("disabled", self.disabled),
            ("shadow", self.shadow),
            ("hero-top", self.hero_top),
            ("hero-mid", self.hero_mid),
            ("hero-bottom", self.hero_bottom),
            ("hero-copy-fade", self.hero_copy_fade),
            ("artwork-fade", self.artwork_fade),
            ("artwork-empty", self.artwork_empty),
            ("skeleton-fade", self.skeleton_fade),
            ("player-canvas", self.player_canvas),
            ("player-ink", self.player_ink),
        ]
    }

    /// Synchronize the component palette and its Base projection.
    pub fn install(self, dark: bool, cx: &mut gpui::App) {
        component::install(self, dark, cx);
    }
}
