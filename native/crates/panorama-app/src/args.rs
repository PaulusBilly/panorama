//! Validated startup and screenshot arguments.
use crate::router::Route;
use std::{ffi::OsString, path::PathBuf};

/// One invocation's startup options.
#[derive(Clone, Debug)]
pub struct Args {
    /// Initial destination.
    pub route: Route,
    /// Optional debug capture path.
    pub screenshot: Option<PathBuf>,
    /// Initial logical window size.
    pub size: (f32, f32),
    /// Dark is available only in screenshot mode.
    pub dark: bool,
    /// Disable route motion.
    pub reduced_motion: bool,
    /// Use the in-process Electron preview data.
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
    /// Open Home's search band before capture.
    pub open_search: bool,
    /// Present the fixture account as signed out.
    pub signed_out: bool,
    /// Return no fixture playback sources.
    pub no_sources: bool,
    /// Hold fixture metadata in its initial loading state.
    pub film_loading: bool,
}

impl Args {
    /// Parse command line arguments without disk access.
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, String> {
        let mut args = args.into_iter();
        let mut result = Self {
            route: Route::Home,
            screenshot: None,
            size: (1280.0, 800.0),
            dark: false,
            reduced_motion: false,
            fixtures: false,
            scroll: vec![],
            hover_first_card: false,
            focus_first_card: false,
            open_account_menu: false,
            open_sign_in: false,
            bench_scroll: false,
            open_search: false,
            signed_out: false,
            no_sources: false,
            film_loading: false,
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
                    if !(960..=16384).contains(&w) || !(600..=16384).contains(&h) {
                        return Err("Window size must be between 960x600 and 16384x16384".into());
                    }
                    result.size = (w as f32, h as f32);
                }
                "--theme" => {
                    screenshot_options = true;
                    let theme = args.next().ok_or("--theme needs light or dark")?;
                    result.dark = match theme.to_str() {
                        Some("light") => false,
                        Some("dark") => true,
                        _ => return Err("--theme needs light or dark".into()),
                    };
                }
                "--reduced-motion" => result.reduced_motion = true,
                "--fixtures" => result.fixtures = true,
                "--bench-scroll" => {
                    result.bench_scroll = true;
                    result.fixtures = true;
                }
                "--hover-first-card" => result.hover_first_card = true,
                "--focus-first-card" => result.focus_first_card = true,
                "--open-account-menu" => result.open_account_menu = true,
                "--open-sign-in" => result.open_sign_in = true,
                "--open-search" => result.open_search = true,
                "--signed-out" => result.signed_out = true,
                "--no-sources" => result.no_sources = true,
                "--film-loading" => result.film_loading = true,
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
                _ => return Err(format!("Unknown argument: {}", arg.to_string_lossy())),
            }
        }
        if screenshot_options && result.screenshot.is_none() {
            return Err("--theme and --size require --screenshot".into());
        }
        result.fixtures |= result.screenshot.is_some();
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(args: &[&str]) -> Result<Args, String> {
        Args::parse(args.iter().map(OsString::from))
    }
    #[test]
    fn valid_arguments() {
        assert!(!parse(&[]).unwrap().dark);
        let args = parse(&[
            "--screenshot",
            "film:a:b",
            "out.png",
            "--size",
            "1280x800",
            "--theme",
            "dark",
            "--reduced-motion",
        ])
        .unwrap();
        assert_eq!(args.route, Route::Film { id: "a:b".into() });
        assert_eq!(args.size, (1280.0, 800.0));
        assert!(args.dark && args.reduced_motion);
        assert!(parse(&["--reduced-motion"]).unwrap().reduced_motion);
    }
    #[test]
    fn malformed_arguments() {
        for args in [
            vec!["--screenshot"],
            vec!["--screenshot", "home"],
            vec!["--screenshot", "home", "--theme"],
            vec!["--theme", "dark"],
            vec!["--size", "1280x800"],
            vec!["--unknown"],
            vec!["--screenshot", "film:", "out.png"],
            vec!["--screenshot", "home", "out.png", "--theme", "blue"],
            vec!["--screenshot", "home", "out.png", "--size", "NaNx800"],
            vec!["--screenshot", "home", "out.png", "--size", "0x0"],
            vec!["--screenshot", "home", "out.png", "--size", "1280x800x1"],
        ] {
            assert!(parse(&args).is_err(), "{args:?}");
        }
    }
}
