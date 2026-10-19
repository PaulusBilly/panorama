//! Embedded assets keep startup independent of disk access.
use gpui::{App, AssetSource, Font, FontFallbacks, SharedString};
use std::borrow::Cow;

/// Bundled SVG and icon source.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        let data: Option<&'static [u8]> = match path {
            "icon.svg" => Some(include_bytes!("../../../../public/icon.svg")),
            "player-x.svg" => Some(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6L6 18M6 6l12 12"/></svg>"#),
            "player-loader-2.svg" => Some(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3a9 9 0 1 0 9 9"/></svg>"#),
            "logo.svg" => Some(include_bytes!("../assets/logo.svg")),
            "panorama.svg" => Some(include_bytes!("../assets/panorama.svg")),
            "search.svg" => Some(include_bytes!("../assets/icons/search.svg")),
            "menu-2.svg" => Some(include_bytes!("../assets/icons/menu-2.svg")),
            "x.svg" => Some(include_bytes!("../assets/icons/x.svg")),
            "arrow-right.svg" => Some(include_bytes!("../assets/icons/arrow-right.svg")),
            "plus.svg" => Some(include_bytes!("../assets/icons/plus.svg")),
            "arrow-up.svg" => Some(include_bytes!("../assets/icons/arrow-up.svg")),
            "arrow-down.svg" => Some(include_bytes!("../assets/icons/arrow-down.svg")),
            "chevron-down-thin.svg" => Some(include_bytes!("../assets/icons/chevron-down-thin.svg")),
            "check.svg" => Some(include_bytes!("../assets/icons/check.svg")),
            "loader-2.svg" => Some(include_bytes!("../assets/icons/loader-2.svg")),
            "star-filled.svg" => Some(include_bytes!("../assets/icons/star-filled.svg")),
            "player-play-filled.svg" => {
                Some(include_bytes!("../assets/icons/player-play-filled.svg"))
            }
            "player-pause-filled.svg" => {
                Some(include_bytes!("../assets/icons/player-pause-filled.svg"))
            }
            "volume-2.svg" => Some(include_bytes!("../assets/icons/volume-2.svg")),
            "volume-off.svg" => Some(include_bytes!("../assets/icons/volume-off.svg")),
            "maximize-player.svg" => Some(include_bytes!("../assets/icons/maximize.svg")),
            "minimize-player.svg" => Some(include_bytes!("../assets/icons/minimize.svg")),
            "playlist.svg" => Some(include_bytes!("../assets/icons/playlist.svg")),
            "playlist-filled.svg" => Some(include_bytes!("../assets/icons/playlist-filled.svg")),
            "list-details.svg" => Some(include_bytes!("../assets/icons/list-details.svg")),
            "list-details-filled.svg" => {
                Some(include_bytes!("../assets/icons/list-details-filled.svg"))
            }
            "badge-cc.svg" => Some(include_bytes!("../assets/icons/badge-cc.svg")),
            "badge-cc-filled.svg" => Some(include_bytes!("../assets/icons/badge-cc-filled.svg")),
            "minus.svg" => Some(include_bytes!("../assets/icons/minus.svg")),
            "chevron-down.svg" => Some(include_bytes!("../assets/icons/chevron-down.svg")),
            "settings.svg" => Some(include_bytes!("../assets/icons/settings.svg")),
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
