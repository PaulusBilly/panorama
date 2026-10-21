//! Validated startup and screenshot arguments.
use crate::router::Route;
use std::{ffi::OsString, path::PathBuf};

/// One invocation's startup options.
#[derive(Clone)]
pub struct Args {
    pub player_state: Option<crate::playback::fake::FixtureState>,
    pub play_file: Option<PathBuf>,
    pub play_url: Option<url::Url>,
    pub play_torrent: bool,
    /// Debug: play this info hash instead of the Big Buck Bunny one (never logged).
    pub torrent_hash: Option<String>,
    pub film_resume: bool,
    /// Initial destination.
    pub route: Route,
    /// Optional debug capture path.
    pub screenshot: Option<PathBuf>,
    /// Initial logical window size.
    pub size: (f32, f32),
    /// Disable route motion.
    pub reduced_motion: bool,
    /// Use the in-process preview data.
    pub fixtures: bool,
    /// Ordered scroll deltas; an infinite delta scrolls to the end.
    pub scroll: Vec<f32>,
    /// Expand the first visible card's image.
    pub hover_first_card: bool,
    /// Show the first card's keyboard focus ring.
    pub focus_first_card: bool,
    /// Open the account popup before capture.
    pub open_account_menu: bool,
    /// Open the sign-in dialog before capture.
    pub open_sign_in: bool,
    /// Measure five seconds of scrolling with 200 fixture cards.
    pub bench_scroll: bool,
    /// Present the fixture account as signed out.
    pub signed_out: bool,
    /// Return no fixture playback sources.
    pub no_sources: bool,
    /// Hold fixture metadata in its initial loading state.
    pub film_loading: bool,
    /// Open a player panel before capture.
    pub open_panel: Option<crate::routes::player::panel::PanelId>,
    /// Show the seek range in its hovered look.
    pub hover_seek: bool,
    /// Enter fullscreen before capture.
    pub fullscreen: bool,
    /// Playback settings for captures, as the stored JSON; never persisted.
    pub playback_settings: Option<crate::playback::output::PlaybackSettings>,
    /// Open a panel control before capture: `edit`, `color` or `select`.
    pub open_control: Option<String>,
    /// Subtitle preferences for captures, as the stored JSON; never persisted.
    pub subtitle_prefs: Option<crate::playback::subtitle_prefs::SubtitlePreferences>,
    /// Show the fixture addon preview on the Addons screen.
    pub addons_preview: bool,
    /// Show Torrentio's inline remove confirmation.
    pub addons_confirm_remove: bool,
    /// Show the invalid-link error on the Addons screen.
    pub addons_error: bool,
    /// Open Home's catalog menu before capture.
    pub open_catalog_menu: bool,
}

impl Args {
    /// Parse command line arguments without disk access.
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, String> {
        let mut args = args.into_iter();
        let mut result = Self {
            player_state: None,
            play_file: None,
            play_url: None,
            play_torrent: false,
            torrent_hash: None,
            film_resume: false,
            route: Route::Home,
            screenshot: None,
            size: (1280.0, 800.0),
            reduced_motion: false,
            fixtures: false,
            scroll: vec![],
            hover_first_card: false,
            focus_first_card: false,
            open_account_menu: false,
            open_sign_in: false,
            bench_scroll: false,
            signed_out: false,
            no_sources: false,
            film_loading: false,
            open_panel: None,
            hover_seek: false,
            fullscreen: false,
            subtitle_prefs: None,
            open_control: None,
            playback_settings: None,
            addons_preview: false,
            addons_confirm_remove: false,
            addons_error: false,
            open_catalog_menu: false,
        };
        let mut screenshot_options = false;
        while let Some(arg) = args.next() {
            match arg
                .to_str()
                .ok_or("Arguments must be valid Unicode except the PNG path")?
            {
                "--screenshot" if result.screenshot.is_none() => {
                    let route = args
                        .next()
                        .ok_or("--screenshot needs a route and output PNG path")?;
                    result.route =
                        Route::parse(route.to_str().ok_or("Route must be valid Unicode")?)?;
                    let path = args.next().ok_or("--screenshot needs an output PNG path")?;
                    if path.is_empty() || path.to_string_lossy().starts_with("--") {
                        return Err("Missing output PNG path".into());
                    }
                    result.screenshot = Some(path.into());
                }
                "--size" => {
                    screenshot_options = true;
                    let size = args.next().ok_or("--size needs WxH")?;
                    let (w, h) = size
                        .to_str()
                        .and_then(|s| s.split_once('x'))
                        .ok_or("--size needs WxH")?;
                    let w: u32 = w.parse().map_err(|_| "Invalid window width")?;
                    let h: u32 = h.parse().map_err(|_| "Invalid window height")?;
                    if !(480..=16384).contains(&w) || !(400..=16384).contains(&h) {
                        return Err("Window size must be between 480x400 and 16384x16384".into());
                    }
                    result.size = (w as f32, h as f32);
                }
                "--reduced-motion" => result.reduced_motion = true,
                "--fixtures" => result.fixtures = true,
                "--player-state" => {
                    let state = args.next().ok_or("--player-state needs a state")?;
                    result.player_state = Some(
                        crate::playback::fake::FixtureState::parse(
                            state.to_str().ok_or("Invalid player state")?,
                        )
                        .ok_or("Invalid player state")?,
                    );
                    result.fixtures = true;
                }
                "--play-file" => {
                    result.play_file = Some(
                        args.next()
                            .filter(|value| !value.is_empty())
                            .ok_or("--play-file needs a path")?
                            .into(),
                    );
                }
                "--play-url" => {
                    let value = args.next().ok_or("--play-url needs an HTTPS URL")?;
                    let url = value
                        .to_str()
                        .and_then(|value| url::Url::parse(value).ok())
                        .ok_or("Invalid playback URL")?;
                    if url.scheme() != "https"
                        || url.host_str().is_none()
                        || !url.username().is_empty()
                        || url.password().is_some()
                    {
                        return Err("Invalid playback URL".into());
                    }
                    result.play_url = Some(url);
                }
                "--play-torrent" => result.play_torrent = true,
                "--torrent-hash" => {
                    let hash = args
                        .next()
                        .and_then(|value| value.into_string().ok())
                        .filter(|value| {
                            value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
                        })
                        .ok_or("--torrent-hash needs 40 hex digits")?;
                    result.torrent_hash = Some(hash.to_ascii_lowercase());
                }
                "--film-resume" => result.film_resume = true,
                "--bench-scroll" => {
                    result.bench_scroll = true;
                    result.fixtures = true;
                }
                "--hover-first-card" => result.hover_first_card = true,
                "--focus-first-card" => result.focus_first_card = true,
                "--open-account-menu" => result.open_account_menu = true,
                "--open-sign-in" => result.open_sign_in = true,
                "--signed-out" => result.signed_out = true,
                "--no-sources" => result.no_sources = true,
                "--film-loading" => result.film_loading = true,
                "--hover-seek" => result.hover_seek = true,
                "--fullscreen" => result.fullscreen = true,
                "--open-control" => {
                    let value = args.next().ok_or("--open-control needs a control")?;
                    match value.to_str() {
                        Some(value @ ("edit" | "color" | "select" | "scroll")) => {
                            result.open_control = Some(value.to_owned());
                        }
                        _ => return Err("--open-control needs edit, color or select".into()),
                    }
                }
                "--playback-settings" => {
                    let value = args.next().ok_or("--playback-settings needs JSON")?;
                    let value = value.to_str().ok_or("Invalid playback settings")?;
                    result.playback_settings = Some(
                        crate::playback::output::PlaybackSettings::from_bytes(value.as_bytes()),
                    );
                }
                "--subtitle-prefs" => {
                    let value = args.next().ok_or("--subtitle-prefs needs JSON")?;
                    let value = value.to_str().ok_or("Invalid subtitle preferences")?;
                    let json =
                        serde_json::from_str(value).map_err(|_| "Invalid subtitle preferences")?;
                    result.subtitle_prefs = Some(
                        crate::playback::subtitle_prefs::SubtitlePreferences::normalise(&json),
                    );
                }
                "--open-panel" => {
                    use crate::routes::player::panel::PanelId;
                    result.open_panel = Some(
                        match args.next().as_deref().and_then(|panel| panel.to_str()) {
                            Some("audio") => PanelId::Audio,
                            Some("sources") => PanelId::Sources,
                            Some("subtitles") => PanelId::Subtitles,
                            Some("settings") => PanelId::Settings,
                            _ => {
                                return Err(
                                    "--open-panel needs audio, sources, subtitles or settings"
                                        .into(),
                                );
                            }
                        },
                    );
                }
                "--addons-preview" => result.addons_preview = true,
                "--addons-confirm-remove" => result.addons_confirm_remove = true,
                "--addons-error" => result.addons_error = true,
                "--open-catalog-menu" => result.open_catalog_menu = true,
                "--scroll" => {
                    let value = args.next().ok_or("--scroll needs a pixel delta or end")?;
                    let value = value.to_str().ok_or("Invalid scroll delta")?;
                    let delta = if value == "end" {
                        f32::INFINITY
                    } else {
                        value.parse::<f32>().map_err(|_| "Invalid scroll delta")?
                    };
                    if delta.is_nan() || delta == f32::NEG_INFINITY {
                        return Err("Invalid scroll delta".into());
                    }
                    result.scroll.push(delta);
                }
                _ => return Err("Unknown argument".into()),
            }
        }
        if screenshot_options && result.screenshot.is_none() {
            return Err("--size requires --screenshot".into());
        }
        if result.torrent_hash.is_some() && !result.play_torrent {
            return Err("--torrent-hash requires --play-torrent".into());
        }
        let real_playback =
            result.play_file.is_some() || result.play_url.is_some() || result.play_torrent;
        if usize::from(result.play_file.is_some())
            + usize::from(result.play_url.is_some())
            + usize::from(result.play_torrent)
            > 1
            || (real_playback && result.player_state.is_some())
        {
            return Err("Choose one playback input".into());
        }
        if result.play_file.is_some() && !cfg!(debug_assertions) && result.screenshot.is_none() {
            return Err("--play-file requires a debug build or --screenshot".into());
        }
        if real_playback && !matches!(result.route, Route::Player { .. }) {
            result.route = Route::Player {
                id: "tmdb:100".into(),
            };
        }
        if result.player_state.is_some() && result.screenshot.is_none() {
            result.route = Route::Player {
                id: "tmdb:100".into(),
            };
        }
        result.fixtures |= result.screenshot.is_some() || real_playback;
        Ok(result)
    }
}

impl std::fmt::Debug for Args {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Args")
            .field("fixtures", &self.fixtures)
            .field("player_state", &self.player_state)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(args: &[&str]) -> Result<Args, String> {
        Args::parse(args.iter().map(OsString::from))
    }
    #[test]
    fn a_torrent_hash_is_validated_and_never_echoed() {
        let hash = "ABCDEF0123456789abcdef0123456789abcdef01";
        let args = parse(&["--play-torrent", "--torrent-hash", hash]).unwrap();
        assert_eq!(
            args.torrent_hash.as_deref(),
            Some(hash.to_lowercase().as_str())
        );
        assert!(!format!("{args:?}").contains("abcdef0123"));
        for bad in ["abc", "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"] {
            let error = parse(&["--play-torrent", "--torrent-hash", bad]).unwrap_err();
            assert!(!error.contains(bad));
        }
        let error = parse(&["--torrent-hash", hash]).unwrap_err();
        assert!(!error.contains(hash));
    }
    #[test]
    fn valid_arguments() {
        let args = parse(&[
            "--screenshot",
            "film:a:b",
            "out.png",
            "--size",
            "1280x800",
            "--reduced-motion",
        ])
        .unwrap();
        assert_eq!(args.route, Route::Film { id: "a:b".into() });
        assert_eq!(args.size, (1280.0, 800.0));
        assert!(args.reduced_motion);
        assert!(parse(&["--reduced-motion"]).unwrap().reduced_motion);
    }
    #[test]
    fn malformed_arguments() {
        for args in [
            vec!["--screenshot"],
            vec!["--screenshot", "home"],
            vec!["--theme", "dark"],
            vec!["--size", "1280x800"],
            vec!["--unknown"],
            vec!["--screenshot", "film:", "out.png"],
            vec!["--screenshot", "home", "out.png", "--size", "NaNx800"],
            vec!["--screenshot", "home", "out.png", "--size", "0x0"],
            vec!["--screenshot", "home", "out.png", "--size", "1280x800x1"],
        ] {
            assert!(parse(&args).is_err(), "{args:?}");
        }
    }
}
