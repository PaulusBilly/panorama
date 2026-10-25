//! Embedded assets keep startup independent of disk access.
use gpui::{App, AssetSource, Font, FontFallbacks, FontWeight, SharedString, Styled};
use std::{borrow::Cow, path::Path};

/// Bundled SVG and icon source.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        let data: Option<&'static [u8]> = match path {
            "icon.svg" => Some(include_bytes!("../assets/icon.svg")),
            "player-x.svg" => Some(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6L6 18M6 6l12 12"/></svg>"#),
            "player-loader-2.svg" => Some(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3a9 9 0 1 0 9 9"/></svg>"#),
            "logo.svg" => Some(include_bytes!("../assets/logo.svg")),
            "search.svg" => Some(include_bytes!("../assets/icons/search.svg")),
            "menu-2.svg" => Some(include_bytes!("../assets/icons/menu-2.svg")),
            "x.svg" => Some(include_bytes!("../assets/icons/x.svg")),
            "arrow-right.svg" => Some(include_bytes!("../assets/icons/arrow-right.svg")),
            "plus.svg" => Some(include_bytes!("../assets/icons/plus.svg")),
            "arrow-up.svg" => Some(include_bytes!("../assets/icons/arrow-up.svg")),
            "arrow-down.svg" => Some(include_bytes!("../assets/icons/arrow-down.svg")),
            "chevron-down-thin.svg" => Some(include_bytes!("../assets/icons/chevron-down-thin.svg")),
            "check.svg" => Some(include_bytes!("../assets/icons/check.svg")),
            "color-wheel.svg" => Some(include_bytes!("../assets/icons/color-wheel.svg")),
            "loader-2.svg" => Some(include_bytes!("../assets/icons/loader-2.svg")),
            "star-filled.svg" => Some(include_bytes!("../assets/icons/star-filled.svg")),
            "clock-filled.svg" => Some(include_bytes!("../assets/icons/clock-filled.svg")),
            // Tabler Icons "award" (MIT), the mark for an award with no bundled logo.
            "award.svg" => Some(include_bytes!("../assets/icons/award.svg")),
            "player-play-filled.svg" => {
                Some(include_bytes!("../assets/icons/player-play-filled.svg"))
            }
            "player-pause-filled.svg" => {
                Some(include_bytes!("../assets/icons/player-pause-filled.svg"))
            }
            "player-play-sharp.svg" => {
                Some(include_bytes!("../assets/icons/player-play-sharp.svg"))
            }
            "player-pause-sharp.svg" => {
                Some(include_bytes!("../assets/icons/player-pause-sharp.svg"))
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
            "arrow-left.svg" => Some(include_bytes!("../assets/icons/arrow-left.svg")),
            "home.svg" => Some(include_bytes!("../assets/icons/home.svg")),
            "addons.svg" => Some(include_bytes!("../assets/icons/addons.svg")),
            "person.svg" => Some(include_bytes!("../assets/icons/person.svg")),
            "refresh.svg" => Some(include_bytes!("../assets/icons/refresh.svg")),
            "back.svg" => Some(include_bytes!("../assets/back.svg")),
            "minimize.svg" => Some(include_bytes!("../assets/minimize.svg")),
            "maximize.svg" => Some(include_bytes!("../assets/maximize.svg")),
            "restore.svg" => Some(include_bytes!("../assets/restore.svg")),
            "close.svg" => Some(include_bytes!("../assets/close.svg")),
            path if path.starts_with("awards/") => crate::award_marks::asset(path),
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

/// The bundled static Aspekta faces, by file name.
const FONTS: [(&str, &[u8]); 3] = [
    (
        "Aspekta-400.ttf",
        include_bytes!("../assets/fonts/Aspekta-400.ttf"),
    ),
    (
        "Aspekta-500.ttf",
        include_bytes!("../assets/fonts/Aspekta-500.ttf"),
    ),
    (
        "Aspekta-700.ttf",
        include_bytes!("../assets/fonts/Aspekta-700.ttf"),
    ),
];

/// Register the static 400/500/700 Aspekta faces before opening a window.
pub fn register_fonts(cx: &App) -> Result<(), String> {
    cx.text_system()
        .add_fonts(
            FONTS
                .iter()
                .map(|(_, bytes)| Cow::Borrowed(*bytes))
                .collect(),
        )
        .map_err(|error| format!("Cannot register Aspekta: {error}"))
}

/// Copy the Aspekta faces into `dir` for libass, which only loads fonts from disk. Files that
/// already match are left alone; others are replaced through a temporary file. Blocking.
pub fn write_fonts(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for (name, bytes) in FONTS {
        let path = dir.join(name);
        if std::fs::read(&path).is_ok_and(|existing| existing == bytes) {
            continue;
        }
        let partial = dir.join(format!("{name}.partial"));
        std::fs::write(&partial, bytes)?;
        std::fs::rename(&partial, &path)?;
    }
    Ok(())
}

/// The family that holds `weight`. DirectWrite files each static Aspekta face under its own
/// family ("Aspekta 400", "Aspekta 500", "Aspekta 700") because the faces name their weights with
/// numbers, so on Windows a weight change must also change the family.
pub fn family(weight: FontWeight) -> &'static str {
    if !cfg!(target_os = "windows") {
        "Aspekta"
    } else if weight.0 < 450.0 {
        "Aspekta 400"
    } else if weight.0 < 600.0 {
        "Aspekta 500"
    } else {
        "Aspekta 700"
    }
}

/// Set a font's weight together with the family that contains it.
pub fn weighted(font: &mut Font, weight: FontWeight) {
    font.family = family(weight).into();
    font.weight = weight;
}

/// Weight setter for elements; use it instead of `font_weight` so the family follows the weight.
pub trait Weighted: Styled + Sized {
    /// Apply `weight` and its Aspekta family.
    fn weight(self, weight: FontWeight) -> Self {
        self.font_family(family(weight)).font_weight(weight)
    }
}

impl<T: Styled> Weighted for T {}

/// The application font with ordered platform fallbacks.
pub fn font() -> Font {
    Font {
        family: family(FontWeight::NORMAL).into(),
        fallbacks: Some(FontFallbacks::from_fonts(vec![
            "Segoe UI".into(),
            ".SystemUIFont".into(),
        ])),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn write_fonts_installs_and_repairs_the_faces() {
        let dir = std::env::temp_dir().join(format!("panorama-fonts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        super::write_fonts(&dir).unwrap();
        std::fs::write(dir.join("Aspekta-500.ttf"), b"truncated").unwrap();
        super::write_fonts(&dir).unwrap();
        for (name, bytes) in super::FONTS {
            assert_eq!(std::fs::read(dir.join(name)).unwrap(), bytes, "{name}");
        }
        assert!(!dir.join("Aspekta-500.ttf.partial").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
