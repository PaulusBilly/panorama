mod args;
mod bench;
mod fonts;
mod logic;
mod motion;
mod network;
mod win32;

use args::Args;
use bench::{Reports, Samples, mib};
use gpui::{
    Animation, AnimationExt, AnyElement, App, Bounds, Context, FocusHandle, FontWeight,
    KeyDownEvent, QuitMode, RenderImage, TitlebarOptions, UniformListScrollHandle, Window,
    WindowBounds, WindowOptions, div, img, point, prelude::*, px, rgb, size, uniform_list,
};
use logic::{ByteLru, Poster, Tween};
use motion::Motion;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    collections::{HashMap, HashSet},
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

struct CachedPoster {
    image: Arc<RenderImage>,
    loaded: Instant,
}
struct ScrollBench {
    pass: usize,
    samples: Samples,
    first_end: Option<win32::Memory>,
}
struct MotionBench {
    step: usize,
    samples: Option<Samples>,
    next: Instant,
}

struct Gate {
    args: Args,
    screen: usize,
    focus: FocusHandle,
    motion: Motion,
    font_result: String,
    client: reqwest::blocking::Client,
    posters: Vec<Poster>,
    catalog_ready: bool,
    cache: ByteLru<usize, CachedPoster>,
    loading: HashSet<usize>,
    errors: HashMap<usize, String>,
    hovered: HashMap<usize, Tween>,
    visible: HashSet<usize>,
    scroll: UniformListScrollHandle,
    columns: usize,
    scroll_bench: Option<ScrollBench>,
    motion_bench: Option<MotionBench>,
    refresh: u32,
    launched: Instant,
    hwnd: usize,
    captured: usize,
    capturing: bool,
    last_capture: Instant,
    finished_motion: bool,
    failed: Arc<AtomicBool>,
    reports: Reports,
}

impl Gate {
    fn new(
        args: Args,
        font_result: String,
        client: reqwest::blocking::Client,
        failed: Arc<AtomicBool>,
        reports: Reports,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Self, String> {
        let handle = HasWindowHandle::window_handle(window).map_err(|e| e.to_string())?;
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return Err("gates-ui requires Windows".into());
        };
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let now = Instant::now();
        let catalog_client = client.clone();
        let local = args.local_posters.clone();
        let task = cx.background_executor().spawn(async move {
            if let Some(dir) = local {
                let bytes = std::fs::read(dir.join("catalog.json")).map_err(|e| e.to_string())?;
                let (_, posters) = logic::parse_catalog(&bytes)?;
                if posters.len() != 500 {
                    return Err("Local catalog needs 500 posters".into());
                }
                Ok(posters)
            } else if args.no_images {
                Ok((0..500)
                    .map(|index| Poster {
                        id: index.to_string(),
                        name: format!("Poster {index}"),
                        poster: String::new(),
                    })
                    .collect())
            } else {
                network::catalog(&catalog_client)
            }
        });
        cx.spawn(async move |entity, cx| {
            let result = task.await;
            let _ = entity.update(cx, |this, cx| {
                match result {
                    Ok(posters) => {
                        this.posters = posters;
                        this.catalog_ready = true;
                    }
                    Err(error) => this.fail(format!("Catalog: {error}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
        Ok(Self {
            reports,
            screen: args.screen,
            cache: ByteLru::new(args.cache_bytes),
            motion_bench: args.bench_motion.then_some(MotionBench {
                step: 0,
                samples: None,
                next: now + Duration::from_millis(500),
            }),
            args,
            focus,
            motion: Motion::new(cx),
            font_result,
            client,
            posters: Vec::new(),
            catalog_ready: false,
            loading: HashSet::new(),
            errors: HashMap::new(),
            hovered: HashMap::new(),
            visible: HashSet::new(),
            scroll: UniformListScrollHandle::new(),
            columns: 6,
            scroll_bench: None,
            refresh: win32::refresh_rate()?,
            launched: now,
            hwnd: handle.hwnd.get() as usize,
            captured: 0,
            capturing: false,
            last_capture: now,
            finished_motion: false,
            failed,
        })
    }

    fn fail(&self, error: String, cx: &mut Context<Self>) {
        eprintln!("Gates UI failed: {error}");
        self.failed.store(true, Ordering::Relaxed);
        cx.quit();
    }

    fn start_images(&mut self, window: &Window, cx: &mut Context<Self>) {
        let scale = window.scale_factor();
        if self.screen != 1 || self.args.no_images {
            return;
        }
        let mut candidates: Vec<_> = self.visible.iter().copied().collect();
        candidates.sort_unstable();
        for index in candidates {
            if self.loading.len() >= 6 {
                break;
            }
            if self.cache.contains(&index)
                || self.loading.contains(&index)
                || self.errors.contains_key(&index)
            {
                continue;
            }
            let Some(poster) = self.posters.get(index) else {
                continue;
            };
            let url = poster.poster.clone();
            let client = self.client.clone();
            let local = self
                .args
                .local_posters
                .as_ref()
                .map(|dir| dir.join(format!("{index}.poster")));
            self.loading.insert(index);
            let task = cx
                .background_executor()
                .spawn(async move { network::poster(&client, &url, scale, local.as_deref()) });
            cx.spawn_in(window, async move |entity, cx| {
                let result = task.await;
                let _ = cx.update(|window, cx| {
                    entity.update(cx, |this, cx| {
                        this.loading.remove(&index);
                        match result {
                            Ok(image) => {
                                let bytes = image.as_bytes(0).map_or(0, |data| data.len());
                                let evicted = this.cache.insert(
                                    index,
                                    CachedPoster {
                                        image,
                                        loaded: Instant::now(),
                                    },
                                    bytes,
                                    &this.visible,
                                );
                                for entry in evicted {
                                    cx.drop_image(entry.image, Some(window));
                                }
                                if !this.cache.contains(&index) && this.visible.contains(&index) {
                                    this.errors.insert(
                                        index,
                                        "Cache cap cannot hold the visible set".into(),
                                    );
                                }
                            }
                            Err(error) => {
                                eprintln!("poster index={index}: {error}");
                                this.errors.insert(index, error);
                            }
                        }
                        cx.notify();
                    })
                });
            })
            .detach();
        }
    }

    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = Instant::now();
        self.motion.finish_close(window, cx, now);
        self.start_images(window, cx);
        if self.args.bench_scroll {
            if self.scroll_bench.is_none() {
                if now.duration_since(self.launched) > Duration::from_secs(120) {
                    self.fail("Timed out waiting for visible posters".into(), cx);
                    return;
                }
                if self
                    .visible
                    .iter()
                    .any(|index| self.errors.contains_key(index))
                {
                    self.fail("Cannot start bench: a visible image failed".into(), cx);
                    return;
                }
                if self.catalog_ready
                    && !self.visible.is_empty()
                    && (self.args.no_images
                        || self.visible.iter().all(|index| {
                            self.cache.get(index).is_some_and(|entry| {
                                now.duration_since(entry.loaded) >= Duration::from_millis(220)
                            })
                        }))
                {
                    match Samples::deferred(now, self.reports.clone()) {
                        Ok(samples) => {
                            self.scroll_bench = Some(ScrollBench {
                                pass: 1,
                                samples,
                                first_end: None,
                            })
                        }
                        Err(error) => {
                            self.fail(error, cx);
                            return;
                        }
                    }
                }
            } else if let Err(error) = self.scroll_tick(now) {
                self.fail(error, cx);
                return;
            }
            if self
                .scroll_bench
                .as_ref()
                .is_some_and(|bench| bench.pass == 3)
            {
                cx.quit();
                return;
            }
        }
        if self.args.bench_motion
            && !self.finished_motion
            && let Err(error) = self.motion_tick(window, cx, now)
        {
            self.fail(error, cx);
            return;
        }
        if let Some(path) = self.args.screenshot.clone() {
            if now.duration_since(self.launched) >= self.args.after && !self.capturing {
                self.capture(path, true, cx);
            }
        } else if let Some(dir) = self.args.frames.clone() {
            if !self.finished_motion
                && !self.capturing
                && now.duration_since(self.last_capture) >= Duration::from_millis(33)
            {
                self.capture(dir.join(format!("{:05}.png", self.captured)), false, cx);
            }
            if self.finished_motion && !self.capturing {
                println!(
                    "recorded_frames={} directory={}",
                    self.captured,
                    dir.display()
                );
                cx.quit();
            }
        } else if self.finished_motion {
            cx.quit();
        }
    }

    fn capture(&mut self, path: std::path::PathBuf, quit: bool, cx: &mut Context<Self>) {
        self.capturing = true;
        self.last_capture = Instant::now();
        let hwnd = self.hwnd;
        let task = cx.background_executor().spawn(async move {
            win32::screenshot(windows::Win32::Foundation::HWND(hwnd as *mut _), &path)
        });
        cx.spawn(async move |entity, cx| {
            let result = task.await;
            let _ = entity.update(cx, |this, cx| {
                this.capturing = false;
                match result {
                    Ok(()) => {
                        this.captured += 1;
                        if quit {
                            cx.quit();
                        }
                    }
                    Err(error) => this.fail(error, cx),
                }
            });
        })
        .detach();
    }

    fn scroll_tick(&mut self, now: Instant) -> Result<(), String> {
        let bench = self.scroll_bench.as_mut().unwrap();
        bench.samples.frame(now);
        let ready = self
            .visible
            .iter()
            .filter(|index| self.cache.contains(index))
            .count();
        bench.samples.log(
            now,
            bench.pass,
            self.cache.bytes,
            self.cache.len(),
            ready,
            self.visible.len(),
        )?;
        bench.samples.observe_memory()?;
        let elapsed = now.duration_since(bench.samples.start).as_secs_f32();
        let active = self.visible.iter().copied().min();
        if let Some(index) = active {
            self.hovered
                .entry(index)
                .or_insert_with(|| Tween::fixed(0.0));
        }
        for (index, tween) in &mut self.hovered {
            let target = if Some(*index) == active { 1.0 } else { 0.0 };
            if tween.to != target {
                tween.retarget(target, Duration::from_millis(160), true, now);
            }
        }
        let handle = &self.scroll.0.borrow().base_handle;
        let max = f32::from(handle.max_offset().y);
        let fraction = if elapsed <= 10.0 {
            elapsed / 10.0
        } else {
            (20.0 - elapsed) / 10.0
        }
        .clamp(0.0, 1.0);
        handle.set_offset(point(px(0.0), px(-max * fraction)));
        if elapsed >= 20.0 {
            let end =
                bench
                    .samples
                    .finish(&format!("scroll-pass-{}", bench.pass), now, self.refresh)?;
            if let Some(first) = bench.first_end {
                let working_delta = end.working as i64 - first.working as i64;
                let private_delta = end.private as i64 - first.private as i64;
                self.reports.lock().unwrap().push(format!(
                    "MEMORY pass2-minus-pass1 working_delta_mib={:.3} private_delta_mib={:.3} threshold_lt_16_mib={}",
                    working_delta as f64 / 1048576.0,
                    private_delta as f64 / 1048576.0,
                    if working_delta < 16 * 1048576 && private_delta < 16 * 1048576 {
                        "PASS"
                    } else {
                        "FAIL"
                    }
                ));
                bench.pass = 3;
            } else {
                bench.first_end = Some(end);
                bench.pass = 2;
                bench.samples = Samples::deferred(now, self.reports.clone())?;
            }
        }
        Ok(())
    }

    fn motion_tick(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        now: Instant,
    ) -> Result<(), String> {
        let mut bench = self.motion_bench.take().unwrap();
        if let Some(samples) = &mut bench.samples {
            samples.frame(now);
            let duration = if bench.step < 40 {
                320
            } else if bench.step.is_multiple_of(2) {
                220
            } else {
                160
            };
            if now.duration_since(samples.start) >= Duration::from_millis(duration) {
                let label = if bench.step < 40 {
                    if bench.step.is_multiple_of(2) {
                        "page-A-to-B"
                    } else {
                        "page-B-to-A"
                    }
                } else if bench.step.is_multiple_of(2) {
                    "dialog-open"
                } else {
                    "dialog-close"
                };
                samples.motion(
                    label,
                    if bench.step < 40 {
                        bench.step / 2 + 1
                    } else {
                        (bench.step - 40) / 2 + 1
                    },
                    now,
                );
                bench.step += 1;
                bench.samples = None;
                bench.next = now + Duration::from_millis(100);
            }
        } else if now >= bench.next {
            if bench.step == 80 {
                self.finished_motion = true;
            } else {
                if bench.step < 40 {
                    self.motion.navigate(bench.step.is_multiple_of(2), now);
                } else {
                    self.motion
                        .dialog(bench.step.is_multiple_of(2), window, cx, now);
                }
                bench.samples = Some(Samples::new(now)?);
            }
        }
        self.motion_bench = Some(bench);
        Ok(())
    }

    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        if self.args.bench_scroll || self.args.bench_motion {
            return;
        }
        println!(
            "input key={key} screen={} modal={} close_focus={} stay_focus={}",
            self.screen,
            self.motion.dialog_present,
            self.motion.close.is_focused(window),
            self.motion.stay.is_focused(window)
        );
        if self.motion.dialog_present {
            match key {
                "escape" => self.motion.dialog(false, window, cx, Instant::now()),
                "d" => self
                    .motion
                    .dialog(!self.motion.dialog_open, window, cx, Instant::now()),
                "tab" => self.motion.tab(window, cx),
                "enter" | "space" if self.motion.close.is_focused(window) => {
                    self.motion.dialog(false, window, cx, Instant::now())
                }
                _ => {}
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        match key {
            "1" | "2" | "3" => {
                self.screen = key.parse().unwrap();
                window.focus(&self.focus, cx);
            }
            "enter" if self.screen == 3 => {
                self.motion.navigate(!self.motion.page_b, Instant::now())
            }
            "backspace" | "escape" if self.screen == 3 => {
                self.motion.navigate(false, Instant::now())
            }
            "d" if self.screen == 3 => self.motion.dialog(true, window, cx, Instant::now()),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn tile(&mut self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let title = self.posters[index].name.clone();
        let now = Instant::now();
        let hover = self
            .hovered
            .get(&index)
            .copied()
            .unwrap_or_else(|| Tween::fixed(0.0));
        let cached = self
            .cache
            .get(&index)
            .map(|entry| (entry.image.clone(), entry.loaded));
        let mut poster = div()
            .absolute()
            .top_0()
            .left_0()
            .w(px(180.0))
            .h(px(270.0))
            .rounded(px(6.0))
            .bg(rgb(0x2b2a27));
        if let Some((image, loaded)) = cached {
            let fade = Tween {
                from: 0.0,
                to: 1.0,
                start: loaded,
                duration: Duration::from_millis(220),
                eased: false,
            };
            poster = poster.child(img(image).size_full().rounded(px(6.0)).with_animation(
                ("image-fade", index),
                Animation::new(Duration::from_millis(220)),
                move |image, _| image.opacity(fade.value(Instant::now())),
            ));
        }
        poster = poster.child(
            div()
                .absolute()
                .inset_0()
                .rounded(px(6.0))
                .border_1()
                .border_color(rgb(0xffffff).opacity(0.1 + 0.35 * hover.value(now))),
        );
        let poster = poster.with_animation(
            ("hover", index),
            Animation::new(Duration::from_millis(160)),
            move |poster, _| {
                let amount = hover.value(Instant::now());
                poster
                    .w(px(180.0 * (1.0 + 0.03 * amount)))
                    .h(px(270.0 * (1.0 + 0.03 * amount)))
                    .left(px(-2.7 * amount))
                    .top(px(-4.05 * amount))
            },
        );
        div()
            .w(px(180.0))
            .h(px(304.0))
            .flex_shrink_0()
            .child(
                div()
                    .id(("poster", index))
                    .relative()
                    .w(px(180.0))
                    .h(px(270.0))
                    .cursor_pointer()
                    .on_hover(cx.listener(move |this, over, _, cx| {
                        this.hovered
                            .entry(index)
                            .or_insert_with(|| Tween::fixed(0.0))
                            .retarget(
                                if *over { 1.0 } else { 0.0 },
                                Duration::from_millis(160),
                                true,
                                Instant::now(),
                            );
                        cx.notify();
                    }))
                    .child(poster),
            )
            .child(
                div()
                    .mt(px(8.0))
                    .text_size(px(13.0))
                    .line_height(px(18.2))
                    .font_weight(FontWeight::MEDIUM)
                    .truncate()
                    .child(title),
            )
            .into_any_element()
    }

    fn grid(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        self.columns = (((f32::from(window.viewport_size().width) - 128.0 + 24.0) / 204.0).floor()
            as usize)
            .max(1);
        if !self.catalog_ready {
            return div()
                .p(px(64.0))
                .child("Fetching 500 Cinemeta posters…")
                .into_any_element();
        }
        let columns = self.columns;
        let rows = self.posters.len().div_ceil(columns);
        let entity = cx.entity();
        uniform_list("poster-grid", rows, move |range, _, cx: &mut App| {
            entity.update(cx, |this, cx| {
                let mut visible = HashSet::new();
                let result = range
                    .map(|row| {
                        let indices = row * columns..((row + 1) * columns).min(this.posters.len());
                        visible.extend(indices.clone());
                        div()
                            .h(px(328.0))
                            .px(px(64.0))
                            .pt(px(8.0))
                            .flex()
                            .gap(px(24.0))
                            .children(indices.map(|index| this.tile(index, cx)))
                    })
                    .collect::<Vec<_>>();
                this.visible = visible;
                this.hovered.retain(|index, _| this.visible.contains(index));
                result
            })
        })
        .track_scroll(&self.scroll)
        .size_full()
        .into_any_element()
    }

    fn specimen(&self) -> AnyElement {
        let samples = [
            (11.0, 1.4),
            (13.0, 1.4),
            (16.0, 1.55),
            (18.0, 1.3),
            (24.0, 1.15),
            (36.0, 1.1),
            (52.0, 1.05),
        ];
        let column = |family: &'static str, label: &'static str| {
            div().flex_1().min_w_0().font_family(family).child(div().text_size(px(18.0)).font_weight(FontWeight::BOLD).child(label))
                .children(samples.into_iter().map(move |(size, lh)| {
                    div().mt(px(28.0)).child(div().text_size(px(11.0)).text_color(rgb(0xa6a49e)).child(format!("{size}px / line height {lh}")))
                        .children([400.0, 500.0, 700.0].into_iter().map(move |weight| div().mt(px(12.0)).text_size(px(size)).line_height(px(size * lh)).font_weight(FontWeight(weight))
                            .child(div().child(format!("{weight:.0}  Panorama — The Grand Budapest Hotel 2014 · 1h 39m · ★ 8.1/10")))
                            .child(div().child("Aa Bb Cc Dd 0123456789 !? ,.;: () [] {} @ # % & /"))
                            .child(div().child("Shin Gojira シン・ゴジラ · Amélie · Öğrenci"))))
                }))
        };
        div()
            .id("type-specimen")
            .size_full()
            .overflow_y_scroll()
            .p(px(64.0))
            .child(div().text_size(px(16.0)).child(self.font_result.clone()))
            .child(
                div()
                    .mt(px(16.0))
                    .font_family(fonts::STATIC_FAMILY)
                    .text_size(px(24.0))
                    .child("Bundled DM Sans: Panorama — The Grand Budapest Hotel 2014"),
            )
            .child(
                div()
                    .font_family("Segoe UI")
                    .text_size(px(24.0))
                    .child("Segoe UI: Panorama — The Grand Budapest Hotel 2014"),
            )
            .child(
                div()
                    .mt(px(32.0))
                    .flex()
                    .gap(px(40.0))
                    .child(column(
                        fonts::VARIABLE_FAMILY,
                        "Variable TTF (DM Vari alias) · requested 400 / 500 / 700",
                    ))
                    .child(column(
                        fonts::STATIC_FAMILY,
                        "Static TTFs (DM Sans) · 400 / 500 / 700",
                    )),
            )
            .into_any_element()
    }
}

impl Render for Gate {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.weak_entity();
        window.on_next_frame(move |window, cx| {
            let _ = entity.update(cx, |this, cx| this.tick(window, cx));
        });
        window.request_animation_frame();
        let body = match self.screen {
            1 => self.grid(window, cx),
            2 => self.specimen(),
            _ => self.motion.render(window, cx).into_any_element(),
        };
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x1a1917))
            .text_color(rgb(0xeeedea))
            .font_family(fonts::STATIC_FAMILY)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key))
            .child(
                div()
                    .h(px(56.0))
                    .flex_shrink_0()
                    .px(px(64.0))
                    .flex()
                    .items_center()
                    .gap(px(28.0))
                    .text_size(px(13.0))
                    .children(
                        [(1, "1  Posters"), (2, "2  Type"), (3, "3  Motion")]
                            .into_iter()
                            .map(|(screen, label)| {
                                div()
                                    .id(("tab", screen))
                                    .cursor_pointer()
                                    .text_color(rgb(0xeeedea).opacity(if self.screen == screen {
                                        1.0
                                    } else {
                                        0.5
                                    }))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        if !this.motion.dialog_present
                                            && !this.args.bench_scroll
                                            && !this.args.bench_motion
                                        {
                                            this.screen = screen;
                                            window.focus(&this.focus, cx);
                                            cx.notify();
                                        }
                                    }))
                                    .child(label)
                            }),
                    )
                    .child(div().ml_auto().text_color(rgb(0xa6a49e)).child(format!(
                        "{} Hz · decoded {:.1} MiB / {} images",
                        self.refresh,
                        mib(self.cache.bytes),
                        self.cache.len()
                    ))),
            )
            .child(div().flex_1().min_h_0().overflow_hidden().child(body))
            .when(self.screen == 3 && self.motion.dialog_present, |root| {
                root.child(self.motion.overlay(window, cx))
            })
    }
}

fn main() -> ExitCode {
    let (args, client) =
        match Args::parse().and_then(|args| network::client().map(|client| (args, client))) {
            Ok(value) => value,
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        };
    let failed = Arc::new(AtomicBool::new(false));
    let app_failed = failed.clone();
    let reports = Reports::default();
    let app_reports = reports.clone();
    gpui_platform::application()
        .with_quit_mode(QuitMode::LastWindowClosed)
        .run(move |cx| {
            let font_result = match fonts::register(cx) {
                Ok(result) => result,
                Err(error) => {
                    eprintln!("Font registration/probe failed: {error}");
                    app_failed.store(true, Ordering::Relaxed);
                    cx.quit();
                    return;
                }
            };
            let bounds = Bounds::centered(None, size(px(1440.0), px(900.0)), cx);
            let result = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Gates UI".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |window, cx| {
                    cx.new(|cx| {
                        Gate::new(
                            args,
                            font_result,
                            client,
                            app_failed.clone(),
                            app_reports.clone(),
                            window,
                            cx,
                        )
                        .unwrap_or_else(|error| {
                            eprintln!("Gates UI startup failed: {error}");
                            std::process::exit(1);
                        })
                    })
                },
            );
            match result {
                Ok(_) => cx.activate(true),
                Err(error) => {
                    eprintln!("Cannot open Gates UI: {error}");
                    app_failed.store(true, Ordering::Relaxed);
                    cx.quit();
                }
            }
        });
    for report in reports.lock().unwrap().drain(..) {
        println!("{report}");
    }
    if failed.load(Ordering::Relaxed) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
