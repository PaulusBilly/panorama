pub mod color;
mod component;
mod focus;
pub mod layout;
pub mod motion;
pub mod scrim;
pub mod typography;

pub use color::{hex, oklch};
pub use focus::focus_ring;
use gpui::Rgba;
pub use layout::{Column, SIDEBAR_WIDTH};
pub use motion::{DURATION_DELIBERATE, DURATION_FAST, DURATION_STANDARD, EASE_EDITORIAL};
use std::sync::OnceLock;
pub use typography::{Typography, caps};

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    /// Page background.
    pub canvas: Rgba,
    /// Primary text, also the text colour over scrimmed artwork.
    pub ink: Rgba,
    /// Secondary text.
    pub ink_muted: Rgba,
    /// Addons panels, menus and dialogs.
    pub surface: Rgba,
    /// Skeleton bars and menu hover.
    pub surface_strong: Rgba,
    /// Hairlines.
    pub rule: Rgba,
    /// Errors.
    pub danger: Rgba,
    /// Primary button fill.
    pub accent: Rgba,
    /// Primary button hover, one step lighter.
    pub accent_hover: Rgba,
    /// Primary button press, one step darker.
    pub accent_pressed: Rgba,
    /// Secondary pill, round button and sidebar hover.
    pub glass: Rgba,
    /// Glass hover.
    pub glass_hover: Rgba,
    /// Unplayed part of a progress bar.
    pub progress_track: Rgba,
    /// Played part of a progress bar.
    pub progress_fill: Rgba,
    /// Opaque start of backdrop and card scrims; dialogs dim with it at partial alpha.
    pub scrim: Rgba,
    /// Hairline around images.
    pub image_outline: Rgba,
    /// Inner focus ring.
    pub focus_inner: Rgba,
    /// Outer focus ring.
    pub focus_outer: Rgba,
    /// Row hover wash.
    pub hover: Rgba,
    /// Selected row wash.
    pub selected: Rgba,
    /// Disabled text and icons.
    pub disabled: Rgba,
    /// Drop shadows.
    pub shadow: Rgba,
    /// Block shown where artwork is missing.
    pub artwork_empty: Rgba,
    /// Player background.
    pub player_canvas: Rgba,
}

impl Default for Theme {
    fn default() -> Self {
        static THEME: OnceLock<Theme> = OnceLock::new();
        *THEME.get_or_init(|| {
            let canvas = oklch(0.13, 0.0, 0.0, 1.0);
            let white = |alpha| oklch(1.0, 0.0, 0.0, alpha);
            Self {
                canvas,
                ink: white(1.0),
                ink_muted: white(0.62),
                surface: oklch(0.2, 0.0, 0.0, 1.0),
                surface_strong: oklch(0.27, 0.0, 0.0, 1.0),
                rule: white(0.14),
                danger: oklch(0.68, 0.145, 29.093, 1.0),
                accent: hex(0x3048ff),
                accent_hover: hex(0x4a5fff),
                accent_pressed: hex(0x2338e0),
                glass: white(0.18),
                glass_hover: white(0.26),
                progress_track: white(0.28),
                progress_fill: white(1.0),
                scrim: canvas,
                image_outline: white(0.1),
                focus_inner: canvas,
                focus_outer: white(1.0),
                hover: white(0.08),
                selected: white(0.14),
                disabled: white(0.38),
                shadow: oklch(0.05, 0.0, 0.0, 0.6),
                artwork_empty: oklch(0.16, 0.0, 0.0, 1.0),
                player_canvas: oklch(0.08, 0.0, 0.0, 1.0),
            }
        })
    }
}

impl Theme {
    /// Map roles onto gpui-component (primary = accent, radius 0) and set the window background.
    pub fn install(self, cx: &mut gpui::App) {
        component::install(self, cx);
    }
}
