#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use gpui::{Bounds, QuitMode, TitlebarOptions, WindowBounds, WindowOptions, prelude::*, px, size};
use panorama_app::{
    app::AppShell,
    app_state::AppState,
    args::Args,
    assets::{self, Assets},
    debug,
    services::{self, ServicesHost, StartupError},
    theme::Theme,
};
use std::{process::ExitCode, sync::mpsc, time::Duration};

fn main() -> ExitCode {
    let args = match Args::parse(std::env::args_os().skip(1)) {
        Ok(args) => args,
        Err(error) => {
            debug::report(&error);
            return ExitCode::FAILURE;
        }
    };
    let _logger = debug::logger();
    let runtime = match services::runtime() {
        Ok(runtime) => runtime,
        Err(StartupError::Failed(message)) => {
            return panorama_app::startup_error::show(message);
        }
        Err(StartupError::Locked) => return ExitCode::FAILURE,
    };
    let host = ServicesHost::new(runtime.handle().clone(), args.fixtures);
    let initial = runtime.block_on(host.start());
    let locked = matches!(initial, Err(StartupError::Locked));
    let screenshot = args.screenshot.clone();
    let capture_requested = screenshot.is_some();
    let (outcome, results) = mpsc::channel();
    let startup_outcome = outcome.clone();
    gpui_platform::application()
        .with_assets(Assets)
        .with_quit_mode(QuitMode::LastWindowClosed)
        .run(move |cx| {
            gpui_component::init(cx);
            cx.bind_keys([
                gpui::KeyBinding::new(
                    "tab",
                    panorama_app::routes::home::NextCard,
                    Some("PanoramaCard"),
                ),
                gpui::KeyBinding::new(
                    "shift-tab",
                    panorama_app::routes::home::PreviousCard,
                    Some("PanoramaCard"),
                ),
                gpui::KeyBinding::new("tab", panorama_app::login::NextField, Some("PanoramaLogin")),
                gpui::KeyBinding::new(
                    "shift-tab",
                    panorama_app::login::PreviousField,
                    Some("PanoramaLogin"),
                ),
                gpui::KeyBinding::new(
                    "escape",
                    panorama_app::login::CloseDialog,
                    Some("PanoramaLogin"),
                ),
                gpui::KeyBinding::new("enter", panorama_app::login::Submit, Some("PanoramaLogin")),
            ]);
            if let Err(error) = assets::register_fonts(cx) {
                let _ = startup_outcome.send(Err(error));
                cx.quit();
                return;
            }
            let theme = if args.dark {
                Theme::dark()
            } else {
                Theme::light()
            };
            theme.install(args.dark, cx);
            let bounds = Bounds::centered(None, size(px(args.size.0), px(args.size.1)), cx);
            let result = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(960.0), px(600.0))),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Panorama".into()),
                        appears_transparent: true,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |window, cx| {
                    if capture_requested {
                        window.activate_window();
                    }
                    if locked {
                        let view = cx.new(|_| AlreadyRunning);
                        return cx.new(|cx| gpui_component::Root::new(view, window, cx));
                    }
                    let state = cx.new(|cx| AppState::new(host.clone(), initial, cx));
                    let view = cx.new(|cx| AppShell::new(args, state, outcome, window, cx));
                    cx.new(|cx| gpui_component::Root::new(view, window, cx))
                },
            );
            match result {
                Ok(_) => {
                    cx.activate(true);
                    if capture_requested {
                        cx.spawn(async move |cx| {
                            cx.background_executor()
                                .timer(Duration::from_secs(15))
                                .await;
                            let _ = startup_outcome.send(Err(
                                "Screenshot timed out waiting for a settled frame".into(),
                            ));
                            cx.update(|cx| cx.quit());
                        })
                        .detach();
                    }
                }
                Err(error) => {
                    let _ =
                        startup_outcome.send(Err(format!("Cannot open Panorama window: {error}")));
                    cx.quit();
                }
            }
        });
    match results.try_recv() {
        Ok(Err(error)) => {
            debug::report(&error);
            ExitCode::FAILURE
        }
        Ok(Ok(())) => ExitCode::SUCCESS,
        Err(_) if screenshot.is_some() => {
            debug::report("Window closed before screenshot completed");
            ExitCode::FAILURE
        }
        Err(_) => ExitCode::SUCCESS,
    }
}

struct AlreadyRunning;
impl gpui::Render for AlreadyRunning {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        gpui::div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::white())
            .text_color(gpui::black())
            .child("Panorama is already running.")
    }
}
