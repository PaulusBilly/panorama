use crate::{
    assets::{self, Assets},
    theme::{Theme, Typography, content_width, focus_ring},
};
use gpui::{
    Bounds, Context, FocusHandle, Render, Window, WindowBounds, WindowOptions, div, prelude::*, px,
    size,
};
use std::{
    ffi::OsString,
    path::PathBuf,
    process::{Command, ExitCode},
};

/// Show a retryable error even when the Tokio runtime could not be created.
pub fn show(message: String) -> ExitCode {
    let executable = std::env::current_exe().ok();
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    gpui_platform::application()
        .with_assets(Assets)
        .with_quit_mode(gpui::QuitMode::LastWindowClosed)
        .run(move |cx| {
            gpui_component::init(cx);
            let _ = assets::register_fonts(cx);
            Theme::light().install(false, cx);
            let bounds = Bounds::centered(None, size(px(1280.0), px(800.0)), cx);
            let result = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |window, cx| {
                    let view = cx.new(|cx| StartupErrorView {
                        message,
                        executable,
                        arguments,
                        focus: cx.focus_handle(),
                        pending: false,
                    });
                    cx.new(|cx| gpui_component::Root::new(view, window, cx))
                },
            );
            if result.is_err() {
                cx.quit();
            }
        });
    ExitCode::FAILURE
}

struct StartupErrorView {
    message: String,
    executable: Option<PathBuf>,
    arguments: Vec<OsString>,
    focus: FocusHandle,
    pending: bool,
}
impl Render for StartupErrorView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::light();
        let width = f32::from(window.viewport_size().width);
        let button = focus_ring(
            div()
                .id("reload-runtime")
                .min_h(px(40.0))
                .px(px(4.0))
                .text_size(px(13.0))
                .border_b_1()
                .border_color(theme.ink)
                .flex()
                .items_center()
                .cursor_pointer()
                .role(gpui::Role::Button)
                .aria_label("Reload Panorama")
                .on_click(cx.listener(|view, _, _, cx| {
                    if view.pending {
                        return;
                    }
                    let Some(executable) = view.executable.clone() else {
                        return;
                    };
                    view.pending = true;
                    let args = view.arguments.clone();
                    let work = cx
                        .background_executor()
                        .spawn(async move { Command::new(executable).args(args).spawn().is_ok() });
                    cx.spawn(async move |entity, cx| {
                        let started = work.await;
                        let _ = entity.update(cx, |view, cx| {
                            if started {
                                cx.quit();
                            } else {
                                view.pending = false;
                                cx.notify();
                            }
                        });
                    })
                    .detach();
                }))
                .child("Reload Panorama"),
            &self.focus,
            theme,
            !self.pending,
            true,
            window,
        );
        div()
            .size_full()
            .bg(theme.canvas)
            .text_color(theme.ink)
            .font(assets::font())
            .child(
                div()
                    .w(px(content_width(width)))
                    .mx_auto()
                    .mt(px(32.0))
                    .py(px(22.0))
                    .border_y_1()
                    .border_color(theme.rule)
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(32.0))
                    .child(div().body().child(self.message.clone()))
                    .child(button),
            )
    }
}
