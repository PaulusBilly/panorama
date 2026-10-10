//! Embedded assets keep startup independent of disk access.
use gpui::{App, AssetSource, Font, FontFallbacks, SharedString};
use std::borrow::Cow;

/// Bundled SVG and icon source.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        let data: Option<&'static [u8]> = match path {
            "logo.svg" => Some(include_bytes!("../assets/logo.svg")),
            "panorama.svg" => Some(include_bytes!("../assets/panorama.svg")),
            "search.svg" => Some(include_bytes!("../assets/icons/search.svg")),
            "menu-2.svg" => Some(include_bytes!("../assets/icons/menu-2.svg")),
            "player-play-filled.svg" => {
                Some(include_bytes!("../assets/icons/player-play-filled.svg"))
            }
            "back.svg" => Some(include_bytes!("../assets/back.svg")),
            "minimize.svg" => Some(include_bytes!("../assets/minimize.svg")),
            "maximize.svg" => Some(include_bytes!("../assets/maximize.svg")),
            "restore.svg" => Some(include_bytes!("../assets/restore.svg")),
            "close.svg" => Some(include_bytes!("../assets/close.svg")),
            _ => None,
        };
        Ok(data.map(Cow::Borrowed))
    }
    fn list(&self, _: &str) -> gpui::Result<Vec<SharedString>> {
        Ok([
            "logo.svg",
            "back.svg",
            "minimize.svg",
            "maximize.svg",
            "restore.svg",
            "close.svg",
        ]
        .map(Into::into)
        .to_vec())
    }
}

/// Register the proven static 400/500/700 DM Sans faces before opening a window.
pub fn register_fonts(cx: &App) -> Result<(), String> {
    cx.text_system()
        .add_fonts(vec![
            Cow::Borrowed(include_bytes!("../assets/fonts/DMSans-Regular.ttf")),
            Cow::Borrowed(include_bytes!("../assets/fonts/DMSans-Medium.ttf")),
            Cow::Borrowed(include_bytes!("../assets/fonts/DMSans-Bold.ttf")),
        ])
        .map_err(|error| format!("Cannot register DM Sans: {error}"))
}

/// The application font with ordered platform fallbacks.
pub fn font() -> Font {
    Font {
        family: if cfg!(target_os = "windows") {
            "DM Sans 14pt"
        } else {
            "DM Sans"
        }
        .into(),
        fallbacks: Some(FontFallbacks::from_fonts(vec![
            "Segoe UI".into(),
            ".SystemUIFont".into(),
        ])),
        ..Default::default()
    }
}
