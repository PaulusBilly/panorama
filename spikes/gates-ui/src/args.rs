use std::{path::PathBuf, time::Duration};

pub struct Args {
    pub screen: usize,
    pub screenshot: Option<PathBuf>,
    pub frames: Option<PathBuf>,
    pub after: Duration,
    pub cache_bytes: usize,
    pub bench_scroll: bool,
    pub bench_motion: bool,
    pub no_images: bool,
    pub local_posters: Option<PathBuf>,
}

impl Args {
    pub fn parse() -> Result<Self, String> {
        let mut args = std::env::args().skip(1);
        let mut result = Self {
            screen: 1,
            screenshot: None,
            frames: None,
            after: Duration::from_secs(3),
            cache_bytes: 96 * 1024 * 1024,
            bench_scroll: false,
            bench_motion: false,
            no_images: false,
            local_posters: None,
        };
        let mut explicit_screen = false;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--screen" => {
                    result.screen = args
                        .next()
                        .ok_or("--screen needs 1, 2, or 3")?
                        .parse()
                        .map_err(|_| "Invalid screen")?;
                    if !(1..=3).contains(&result.screen) {
                        return Err("--screen needs 1, 2, or 3".into());
                    }
                    explicit_screen = true;
                }
                "--screenshot" => {
                    result.screenshot = Some(absolute(
                        args.next().ok_or("--screenshot needs a PNG path")?,
                    )?)
                }
                "--frames" => {
                    result.frames =
                        Some(absolute(args.next().ok_or("--frames needs a directory")?)?)
                }
                "--after" => {
                    let seconds: f64 = args
                        .next()
                        .ok_or("--after needs seconds")?
                        .parse()
                        .map_err(|_| "Invalid seconds")?;
                    if !seconds.is_finite() || !(0.0..=86400.0).contains(&seconds) {
                        return Err("--after must be finite seconds from 0 to 86400".into());
                    }
                    result.after = Duration::from_secs_f64(seconds);
                }
                "--image-cache-mib" => {
                    let mib: usize = args
                        .next()
                        .ok_or("--image-cache-mib needs an integer")?
                        .parse()
                        .map_err(|_| "Invalid cache cap")?;
                    if !(1..=4096).contains(&mib) {
                        return Err("Cache cap must be 1..4096 MiB".into());
                    }
                    result.cache_bytes = mib * 1024 * 1024;
                }
                "--bench-scroll" => result.bench_scroll = true,
                "--bench-motion" => result.bench_motion = true,
                "--no-images" => result.no_images = true,
                "--local-posters" => {
                    result.local_posters = Some(absolute(
                        args.next().ok_or("--local-posters needs a directory")?,
                    )?)
                }
                _ => return Err(format!("Unknown argument {arg}")),
            }
        }
        if result.bench_scroll && (result.bench_motion || result.frames.is_some()) {
            return Err("Scroll and motion modes cannot be combined".into());
        }
        if result.screenshot.is_some()
            && (result.bench_scroll || result.bench_motion || result.frames.is_some())
        {
            return Err("Screenshot and benchmark/recording modes cannot be combined".into());
        }
        if result.frames.is_some() {
            result.bench_motion = true;
        }
        let mode_screen = if result.bench_scroll {
            1
        } else if result.bench_motion {
            3
        } else {
            result.screen
        };
        if explicit_screen && mode_screen != result.screen {
            return Err("Screen conflicts with benchmark mode".into());
        }
        result.screen = mode_screen;
        if let Some(path) = &result.frames {
            std::fs::create_dir_all(path).map_err(|e| e.to_string())?;
        }
        Ok(result)
    }
}

fn absolute(path: String) -> Result<PathBuf, String> {
    let path = PathBuf::from(path);
    Ok(if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(path)
    })
}
