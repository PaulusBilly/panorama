//! Validated startup and screenshot arguments.
use crate::router::Route;
use std::{ffi::OsString, path::PathBuf};

/// What the system passes when someone opens an invite link.
const INVITE_LINK: &str = "panorama://party/";

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
    /// Press Play on Home's first fixture film with a resume point, as Continue does.
    pub play_hero: bool,
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
    /// Capture against the real services and store, after the network has had time to answer.
    pub live: bool,
    /// Open the first award tile past its first lines.
    pub awards_expand_first: bool,
    /// Ordered scroll deltas; an infinite delta scrolls to the end.
    pub scroll: Vec<f32>,
    /// Expand the first visible card's image.
    pub hover_first_card: bool,
    /// With `--hover-first-card`, plays the pinned preview's trailer for a capture.
    pub hover_trailer: bool,
    /// With `--hover-trailer`, opens the card once its trailer shows, as a click would.
    pub open_trailer_card: bool,
    /// Show the first card's keyboard focus ring.
    pub focus_first_card: bool,
    /// Open the account popup before capture.
    pub open_account_menu: bool,
    /// Open the sidebar's Join watch party field before capture.
    pub open_join: bool,
    /// Open the sign-in dialog before capture.
    pub open_sign_in: bool,
    /// Open the watch party lobby over a made-up party: `true` as its host, `false` as a friend.
    pub open_party_lobby: Option<bool>,
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
    /// Keep the store, its lock and the caches under this directory instead of the platform default.
    pub data_dir: Option<PathBuf>,
    /// Debug: host a party; whatever the player opens is what it watches.
    pub party_host: bool,
    /// Join this party link at startup, from `--party-join` or an opened invite link (never logged).
    pub party_join: Option<String>,
    /// Debug: the name to show in the party, for this run only.
    pub party_name: Option<String>,
    /// Debug: write the host's invite link here once it is ready.
    pub party_invite_file: Option<PathBuf>,
    /// Debug: open this film (`film:<id>`, `series:<id>` or `anime:<id>`) and play it.
    pub party_play: Option<(panorama_core::addons::Kind, String)>,
    /// Show a made-up party in the player for a capture: `host`, `member` or `ended`.
    pub party_preview: Option<String>,
    /// Show this playback problem as if the startup check had found it.
    pub runtime_problem: Option<crate::playback::runtime::RuntimeProblem>,
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
            play_hero: false,
            route: Route::Home,
            screenshot: None,
            size: (1280.0, 800.0),
            reduced_motion: false,
            fixtures: false,
            live: false,
            awards_expand_first: false,
            scroll: vec![],
            hover_first_card: false,
            hover_trailer: false,
            open_trailer_card: false,
            focus_first_card: false,
            open_account_menu: false,
            open_join: false,
            open_sign_in: false,
            open_party_lobby: None,
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
            data_dir: None,
            party_host: false,
            party_join: None,
            party_name: None,
            party_invite_file: None,
            party_play: None,
            party_preview: None,
            runtime_problem: None,
        };
        let mut screenshot_options = false;
        let mut opened_link = false;
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
                "--live" => result.live = true,
                "--awards-expand-first" => result.awards_expand_first = true,
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
                "--data-dir" => {
                    result.data_dir = Some(
                        args.next()
                            .filter(|value| !value.is_empty())
                            .ok_or("--data-dir needs a path")?
                            .into(),
                    );
                }
                "--party-host" => result.party_host = true,
                "--party-join" => {
                    result.party_join = Some(
                        args.next()
                            .and_then(|value| value.into_string().ok())
                            .filter(|value| !value.is_empty())
                            .ok_or("--party-join needs a party link")?,
                    );
                }
                "--party-name" => {
                    result.party_name = Some(
                        args.next()
                            .and_then(|value| value.into_string().ok())
                            .and_then(|value| crate::party::display_name(&value))
                            .ok_or("--party-name needs a name")?,
                    );
                }
                "--party-invite-file" => {
                    result.party_invite_file = Some(
                        args.next()
                            .filter(|value| !value.is_empty())
                            .ok_or("--party-invite-file needs a path")?
                            .into(),
                    );
                }
                "--party-play" => {
                    let value = args.next().ok_or("--party-play needs film:<id>")?;
                    match Route::parse(value.to_str().ok_or("Invalid film")?)? {
                        Route::Film { id, kind } => result.party_play = Some((kind, id)),
                        _ => return Err("--party-play needs film:<id>".into()),
                    }
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
                "--play-hero" => result.play_hero = true,
                "--bench-scroll" => {
                    result.bench_scroll = true;
                    result.fixtures = true;
                }
                "--hover-first-card" => result.hover_first_card = true,
                "--open-trailer-card" => {
                    result.hover_first_card = true;
                    result.hover_trailer = true;
                    result.open_trailer_card = true;
                }
                "--hover-trailer" => {
                    result.hover_first_card = true;
                    result.hover_trailer = true;
                }
                "--focus-first-card" => result.focus_first_card = true,
                "--open-account-menu" => result.open_account_menu = true,
                "--open-join" => result.open_join = true,
                "--open-sign-in" => result.open_sign_in = true,
                "--open-party-lobby" => {
                    result.open_party_lobby = Some(
                        match args.next().as_deref().and_then(|value| value.to_str()) {
                            Some("host") => true,
                            Some("member") => false,
                            _ => return Err("--open-party-lobby needs host or member".into()),
                        },
                    );
                }
                "--signed-out" => result.signed_out = true,
                "--no-sources" => result.no_sources = true,
                "--film-loading" => result.film_loading = true,
                "--hover-seek" => result.hover_seek = true,
                "--fullscreen" => result.fullscreen = true,
                "--open-control" => {
                    let value = args.next().ok_or("--open-control needs a control")?;
                    match value.to_str() {
                        Some(value @ ("edit" | "color" | "select" | "scroll" | "appearance")) => {
                            result.open_control = Some(value.to_owned());
                        }
                        _ => {
                            return Err(
                                "--open-control needs edit, color, select, scroll or appearance"
                                    .into(),
                            );
                        }
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
                "--party-preview" => {
                    let kind = args
                        .next()
                        .and_then(|value| value.to_str().map(str::to_owned))
                        .filter(|value| matches!(value.as_str(), "host" | "member" | "ended"))
                        .ok_or("--party-preview needs host, member or ended")?;
                    result.party_preview = Some(kind);
                }
                "--runtime-problem" => {
                    result.runtime_problem = Some(
                        args.next()
                            .as_deref()
                            .and_then(|value| value.to_str())
                            .and_then(crate::playback::runtime::RuntimeProblem::parse)
                            .ok_or("--runtime-problem needs missing, zip, blocked or version")?,
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
                            Some("party") => PanelId::Party,
                            _ => {
                                return Err(
                                    "--open-panel needs audio, sources, subtitles, settings or party"
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
                link if link.starts_with(INVITE_LINK) && !opened_link => {
                    opened_link = true;
                    if panorama_core::party::transport::parse_invite(link).is_ok() {
                        result.party_join = Some(link.to_owned());
                    }
                }
                _ => return Err("Unknown argument".into()),
            }
        }
        if screenshot_options && result.screenshot.is_none() {
            return Err("--size requires --screenshot".into());
        }
        if result.party_host && result.party_join.is_some() {
            return Err("Choose --party-host or --party-join".into());
        }
        if result.party_invite_file.is_some() && !result.party_host {
            return Err("--party-invite-file requires --party-host".into());
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
        if result.live && (result.screenshot.is_none() || result.fixtures) {
            return Err("--live needs --screenshot and no --fixtures".into());
        }
        result.fixtures |= result.screenshot.is_some() && !result.live || real_playback;
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
    fn party_flags_parse_and_never_echo_the_link() {
        let link = "panorama://party/endpointabc?s=0123456789abcdef0123456789abcdef";
        let args = parse(&["--party-join", link, "--party-name", "  Sam  "]).unwrap();
        assert_eq!(args.party_join.as_deref(), Some(link));
        assert_eq!(args.party_name.as_deref(), Some("Sam"));
        assert!(!format!("{args:?}").contains("endpointabc"));
        let args = parse(&[
            "--party-host",
            "--party-invite-file",
            "/tmp/invite",
            "--party-play",
            "film:tt0111161",
        ])
        .unwrap();
        assert!(args.party_host);
        assert_eq!(
            args.party_play,
            Some((panorama_core::addons::Kind::Movie, "tt0111161".into()))
        );
        assert!(parse(&["--party-host", "--party-join", link]).is_err());
        assert!(parse(&["--party-invite-file", "/tmp/invite"]).is_err());
        assert!(parse(&["--party-play", "home"]).is_err());
        assert!(parse(&["--party-name", " "]).is_err());
    }
    #[test]
    fn an_opened_invite_link_joins_and_a_broken_one_still_opens() {
        let link = crate::party::handoff::SAMPLE_INVITE;
        let args = parse(&[link]).unwrap();
        assert_eq!(args.party_join.as_deref(), Some(link));
        assert!(!format!("{args:?}").contains("endpoint"));
        let args = parse(&["panorama://party/broken?s=00"]).unwrap();
        assert_eq!(args.party_join, None);
        assert_eq!(args.route, Route::Home);
        assert!(parse(&[link, link]).is_err());
        assert!(parse(&["panorama://room/abc"]).is_err());
        assert!(parse(&["notes.txt"]).is_err());
    }
    #[test]
    fn every_runtime_problem_can_be_forced() {
        use crate::playback::runtime::RuntimeProblem;
        for (value, problem) in [
            ("missing", RuntimeProblem::Missing),
            ("zip", RuntimeProblem::RunningFromZip),
            ("blocked", RuntimeProblem::Blocked),
            ("version", RuntimeProblem::WrongVersion),
        ] {
            let args = parse(&["--runtime-problem", value]).unwrap();
            assert_eq!(args.runtime_problem, Some(problem));
        }
        assert!(parse(&["--runtime-problem", "gone"]).is_err());
        assert!(parse(&["--runtime-problem"]).is_err());
    }
    #[test]
    fn open_party_lobby_takes_a_side() {
        assert_eq!(
            parse(&["--open-party-lobby", "host"])
                .unwrap()
                .open_party_lobby,
            Some(true)
        );
        assert_eq!(
            parse(&["--open-party-lobby", "member"])
                .unwrap()
                .open_party_lobby,
            Some(false)
        );
        assert_eq!(parse(&[]).unwrap().open_party_lobby, None);
        assert!(parse(&["--open-party-lobby"]).is_err());
        assert!(parse(&["--open-party-lobby", "guest"]).is_err());
    }
    #[test]
    fn data_dir_takes_a_path() {
        let args = parse(&["--data-dir", "/tmp/pa"]).unwrap();
        assert_eq!(args.data_dir, Some(PathBuf::from("/tmp/pa")));
        assert_eq!(parse(&[]).unwrap().data_dir, None);
        assert!(parse(&["--data-dir"]).is_err());
        assert!(parse(&["--data-dir", ""]).is_err());
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
        assert_eq!(
            args.route,
            Route::Film {
                id: "a:b".into(),
                kind: panorama_core::addons::Kind::Movie
            }
        );
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
