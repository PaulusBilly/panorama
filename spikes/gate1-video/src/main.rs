// The spike needs Win32 HWND/GDI calls and runtime libmpv FFI to test composition.
#![allow(unsafe_code)]

mod mpv;
mod win32;

use gpui::{
    Bounds, Context, FocusHandle, KeyDownEvent, MouseButton, MouseDownEvent, QuitMode,
    Subscription, TitlebarOptions, Window, WindowBackgroundAppearance, WindowBounds, WindowOptions,
    div, prelude::*, px, relative, rgb, size,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    path::PathBuf,
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::Receiver,
    },
    time::{Duration, Instant},
};
use windows::Win32::Foundation::HWND;

struct Args {
    source: String,
    screenshot: Option<PathBuf>,
    after: Duration,
}

impl Args {
    fn parse() -> Result<Self, String> {
        let mut source = None;
        let mut screenshot = None;
        let mut after = 5.0;
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--screenshot" => {
                    screenshot = Some(PathBuf::from(
                        args.next().ok_or("--screenshot needs an output PNG path")?,
                    ))
                }
                "--after" => {
                    after = args
                        .next()
                        .ok_or("--after needs seconds")?
                        .parse::<f64>()
                        .map_err(|e| e.to_string())?;
                    if !after.is_finite() || !(0.0..=86400.0).contains(&after) {
                        return Err("--after must be finite seconds between 0 and 86400".into());
                    }
                }
                _ if !arg.starts_with('-') && source.is_none() => source = Some(arg),
                _ => return Err(format!("Unknown argument: {arg}")),
            }
        }
        let source = source.ok_or("Usage: gate1-video <file-path-or-https-url> [--screenshot <out.png>] [--after <seconds>]")?;
        let source = if source.starts_with("https://") {
            source
        } else {
            let path = PathBuf::from(&source)
                .canonicalize()
                .map_err(|error| format!("Cannot open media {source}: {error}"))?;
            if !path.is_file() {
                return Err(format!("Media is not a file: {}", path.display()));
            }
            path.to_string_lossy().into_owned()
        };
        let screenshot = screenshot.map(|path| {
            if path.is_absolute() {
                path
            } else {
                std::env::current_dir().unwrap_or_default().join(path)
            }
        });
        Ok(Self {
            source,
            screenshot,
            after: Duration::from_secs_f64(after),
        })
    }
}

struct Gate {
    mpv: mpv::Mpv,
    video: win32::Video,
    updates: Receiver<mpv::Update>,
    focus: FocusHandle,
    _bounds: Subscription,
    time: f64,
    duration: f64,
    paused: bool,
    hwdec: String,
    frames: u64,
    log_frames: u64,
    fps_frames: u64,
    fps: f64,
    last_log: Instant,
    last_overlay: Instant,
    launched: Instant,
    playing_since: Option<Instant>,
    args: Args,
    failed: Arc<AtomicBool>,
}

impl Gate {
    fn new(
        api: mpv::Api,
        args: Args,
        failed: Arc<AtomicBool>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Self, String> {
        let handle = HasWindowHandle::window_handle(window).map_err(|e| e.to_string())?;
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return Err("Gate 1 requires Windows".into());
        };
        let video = win32::Video::new(HWND(handle.hwnd.get() as *mut _))?;
        let (mpv, updates) = mpv::Mpv::start(api, video.child.0 as usize, args.source.clone())?;
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let bounds = cx.observe_window_bounds(window, |this, _, cx| {
            if let Err(error) = this.video.resize() {
                this.fail(error, cx);
            }
            cx.notify();
        });
        let entity = cx.weak_entity();
        window.on_window_should_close(cx, move |_, cx| {
            let _ = entity.update(cx, |this, _| this.mpv.stop());
            true
        });
        cx.spawn_in(window, async move |entity, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                if !matches!(
                    cx.update(|window, cx| entity.update(cx, |this, cx| this.tick(window, cx))),
                    Ok(Ok(()))
                ) {
                    break;
                }
            }
        })
        .detach();
        let now = Instant::now();
        Ok(Self {
            mpv,
            video,
            updates,
            focus,
            _bounds: bounds,
            time: 0.0,
            duration: 0.0,
            paused: false,
            hwdec: "unknown".into(),
            frames: 0,
            log_frames: 0,
            fps_frames: 0,
            fps: 0.0,
            last_log: now,
            last_overlay: now,
            launched: now,
            playing_since: None,
            args,
            failed,
        })
    }

    fn fail(&mut self, error: String, cx: &mut Context<Self>) {
        eprintln!("Gate 1 failed: {error}");
        self.failed.store(true, Ordering::Relaxed);
        self.mpv.stop();
        cx.quit();
    }

    fn tick(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let updates: Vec<_> = self.updates.try_iter().collect();
        for update in updates {
            match update {
                mpv::Update::Time(value) => {
                    self.time = value;
                    if value > 0.0 && !self.paused && self.playing_since.is_none() {
                        self.playing_since = Some(Instant::now());
                    }
                }
                mpv::Update::Duration(value) => self.duration = value,
                mpv::Update::Pause(value) => self.paused = value,
                mpv::Update::Hwdec(value) => self.hwdec = value,
                mpv::Update::End { reason, error } => {
                    println!("end-file reason={reason} error={error}");
                    if error < 0 {
                        self.fail(format!("mpv end-file error={error}"), cx);
                        return;
                    }
                }
                mpv::Update::Error(error) => {
                    self.fail(error, cx);
                    return;
                }
                mpv::Update::Log(message) => eprintln!("{message}"),
            }
            cx.notify();
        }
        let now = Instant::now();
        if now.duration_since(self.last_overlay) >= Duration::from_millis(500) {
            self.fps = (self.frames - self.fps_frames) as f64
                / now.duration_since(self.last_overlay).as_secs_f64();
            self.fps_frames = self.frames;
            self.last_overlay = now;
            cx.notify();
        }
        if now.duration_since(self.last_log) >= Duration::from_secs(1) {
            let (width, height) = self.video.size().unwrap_or_default();
            println!(
                "time-pos={:.3} duration={:.3} pause={} hwdec-current={} window={}x{} gpui-frames={} interval={:.3}s",
                self.time,
                self.duration,
                self.paused,
                self.hwdec,
                width,
                height,
                self.frames - self.log_frames,
                now.duration_since(self.last_log).as_secs_f64()
            );
            self.log_frames = self.frames;
            self.last_log = now;
        }
        if let Some(path) = &self.args.screenshot {
            if self
                .playing_since
                .is_some_and(|start| now.duration_since(start) >= self.args.after)
            {
                if let Err(error) = win32::screenshot(self.video.parent, path) {
                    self.fail(error, cx);
                    return;
                }
                self.mpv.stop();
                cx.quit();
            } else if now.duration_since(self.launched) > self.args.after + Duration::from_secs(90)
            {
                self.fail(
                    "Timed out waiting for playback before screenshot".into(),
                    cx,
                );
            }
        }
    }

    fn pause(&self) {
        println!("GPUI input: toggle pause");
        self.mpv.command(&["cycle", "pause"]);
    }

    fn seek(&self, seconds: f64, mode: &str) {
        println!("GPUI input: seek {seconds} {mode}");
        self.mpv.command(&["seek", &seconds.to_string(), mode]);
    }
}

fn clock(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

impl Render for Gate {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.weak_entity();
        window.on_next_frame(move |_, cx| {
            let _ = entity.update(cx, |this, _| this.frames += 1);
        });
        window.request_animation_frame();
        if let Err(error) = self.video.resize() {
            self.fail(error, cx);
        }
        let viewport_width = f32::from(window.viewport_size().width);
        let seek_width = (viewport_width - 440.0).max(1.0);
        let fraction = if self.duration > 0.0 {
            (self.time / self.duration).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        div()
            .size_full()
            .relative()
            .text_color(rgb(0xffffff))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "space" => this.pause(),
                    "f" => {
                        println!("GPUI input: fullscreen");
                        window.toggle_fullscreen();
                    }
                    "escape" if window.is_fullscreen() => window.toggle_fullscreen(),
                    "left" => this.seek(-10.0, "relative+exact"),
                    "right" => this.seek(10.0, "relative+exact"),
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .child(
                div()
                    .absolute()
                    .top(px(12.0))
                    .left(px(12.0))
                    .p(px(6.0))
                    .bg(rgb(0x0b0b0c).opacity(0.7))
                    .child(format!(
                        "hwdec-current: {} | GPUI: {:.1} fps",
                        self.hwdec, self.fps
                    )),
            )
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .w_full()
                    .h(px(64.0))
                    .flex()
                    .items_center()
                    .px(px(16.0))
                    .gap(px(12.0))
                    .bg(rgb(0x0b0b0c).opacity(0.7))
                    .child(
                        div()
                            .id("pause")
                            .w(px(80.0))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, window, cx| {
                                window.focus(&this.focus, cx);
                                this.pause();
                            }))
                            .child(if self.paused { "Play" } else { "Pause" }),
                    )
                    .child(
                        div()
                            .relative()
                            .w(px(seek_width))
                            .h(px(32.0))
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                    window.focus(&this.focus, cx);
                                    let fraction = ((f32::from(event.position.x) - 108.0)
                                        / seek_width)
                                        .clamp(0.0, 1.0);
                                    this.seek(
                                        f64::from(fraction) * this.duration,
                                        "absolute+exact",
                                    );
                                }),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .top(px(14.0))
                                    .w_full()
                                    .h(px(4.0))
                                    .bg(rgb(0x777777)),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .top(px(14.0))
                                    .w(relative(fraction))
                                    .h(px(4.0))
                                    .bg(rgb(0xffffff)),
                            ),
                    )
                    .child(div().w(px(180.0)).child(format!(
                        "{} / {}",
                        clock(self.time),
                        clock(self.duration)
                    )))
                    .child(
                        div()
                            .id("fullscreen")
                            .w(px(112.0))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, window, cx| {
                                window.focus(&this.focus, cx);
                                println!("GPUI input: fullscreen");
                                window.toggle_fullscreen();
                            }))
                            .child("Fullscreen"),
                    ),
            )
    }
}

fn main() -> ExitCode {
    let (args, api) = match Args::parse().and_then(|args| mpv::Api::load().map(|api| (args, api))) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let failed = Arc::new(AtomicBool::new(false));
    let app_failed = failed.clone();
    gpui_platform::application()
        .with_quit_mode(QuitMode::LastWindowClosed)
        .run(move |cx| {
            let bounds = Bounds::centered(None, size(px(1280.0), px(720.0)), cx);
            let result = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_background: WindowBackgroundAppearance::Transparent,
                    titlebar: Some(TitlebarOptions {
                        title: Some("Gate 1".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |window, cx| {
                    cx.new(|cx| {
                        Gate::new(api, args, app_failed.clone(), window, cx).unwrap_or_else(
                            |error| {
                                eprintln!("Gate 1 startup failed: {error}");
                                std::process::exit(1);
                            },
                        )
                    })
                },
            );
            match result {
                Ok(_) => cx.activate(true),
                Err(error) => {
                    eprintln!("Cannot open Gate 1 window: {error}");
                    app_failed.store(true, Ordering::Relaxed);
                    cx.quit();
                }
            }
        });
    println!("Gate 1 process exiting");
    if failed.load(Ordering::Relaxed) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
