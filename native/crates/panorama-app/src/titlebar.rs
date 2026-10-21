//! Window chrome: an overlay drag strip, plus caption buttons off macOS.
use crate::theme::Theme;
#[cfg(not(target_os = "macos"))]
use crate::theme::focus_ring;
#[cfg(not(target_os = "macos"))]
use gpui::{App, FocusHandle, MouseButton, WindowControlArea, svg};
use gpui::{Context, Div, Window, div, prelude::*, px};

/// Top band that must stay free of interactive content (drag region).
#[cfg(target_os = "macos")]
pub const SAFE_TOP: f32 = 38.0;
/// Top band that must stay free of interactive content (drag region).
#[cfg(not(target_os = "macos"))]
pub const SAFE_TOP: f32 = 32.0;

/// Retained keyboard focus for native caption controls.
pub struct Titlebar {
    #[cfg(not(target_os = "macos"))]
    state: gpui::Entity<crate::app_state::AppState>,
    #[cfg(not(target_os = "macos"))]
    controls: [FocusHandle; 3],
}

impl Titlebar {
    /// Allocate caption focus independently of route views.
    pub fn new<T: 'static>(
        _cx: &mut Context<T>,
        _state: gpui::Entity<crate::app_state::AppState>,
    ) -> Self {
        Self {
            #[cfg(not(target_os = "macos"))]
            state: _state,
            #[cfg(not(target_os = "macos"))]
            controls: std::array::from_fn(|_| _cx.focus_handle()),
        }
    }

    /// Both platforms: an absolute overlay, full width x SAFE_TOP, drawn last, no layout space.
    /// Windows and Linux add minimise, maximise and close at the top right.
    #[cfg(not(target_os = "macos"))]
    pub fn render(&self, theme: Theme, keyboard_navigation: bool, window: &Window) -> Div {
        let maximized = window.is_maximized();
        strip()
            .flex()
            .when(!cfg!(target_os = "windows"), |bar| {
                bar.on_mouse_down(MouseButton::Left, |event, window, _| {
                    if event.click_count == 2 {
                        window.zoom_window();
                    } else {
                        window.start_window_move();
                    }
                })
            })
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .window_control_area(WindowControlArea::Drag),
            )
            .children([
                self.control(
                    0,
                    "Minimize window",
                    "minimize.svg",
                    WindowControlArea::Min,
                    (theme, keyboard_navigation),
                    window,
                ),
                self.control(
                    1,
                    if maximized {
                        "Restore window"
                    } else {
                        "Maximize window"
                    },
                    if maximized {
                        "restore.svg"
                    } else {
                        "maximize.svg"
                    },
                    WindowControlArea::Max,
                    (theme, keyboard_navigation),
                    window,
                ),
                self.control(
                    2,
                    "Close window",
                    "close.svg",
                    WindowControlArea::Close,
                    (theme, keyboard_navigation),
                    window,
                ),
            ])
    }

    #[cfg(not(target_os = "macos"))]
    fn control(
        &self,
        index: usize,
        label: &'static str,
        icon: &'static str,
        area: WindowControlArea,
        theme: (Theme, bool),
        window: &Window,
    ) -> gpui::Stateful<Div> {
        let (theme, keyboard_navigation) = theme;
        let state = self.state.clone();
        focus_ring(
            div()
                .id(("caption", index))
                .w(px(46.0))
                .h(px(SAFE_TOP))
                .flex()
                .items_center()
                .justify_center()
                .aria_label(label)
                .role(gpui::Role::Button)
                .hover(move |style| {
                    style.bg(if index == 2 {
                        theme.danger
                    } else {
                        theme.glass
                    })
                })
                .when(cfg!(target_os = "windows"), |button| {
                    button.window_control_area(area)
                })
                .on_click(move |event, window, cx| {
                    if !cfg!(target_os = "windows")
                        || matches!(event, gpui::ClickEvent::Keyboard(_))
                    {
                        if area == WindowControlArea::Close {
                            crate::playback::window::close(state.clone(), window, cx);
                        } else {
                            control_window(area, window, cx);
                        }
                    }
                })
                .child(svg().path(icon).size(px(10.0)).text_color(theme.ink)),
            &self.controls[index],
            theme,
            true,
            keyboard_navigation,
            0.0,
            window,
        )
    }

    /// macOS: drag strip only; the traffic lights sit over the sidebar's top.
    #[cfg(target_os = "macos")]
    pub fn render(&self, _: Theme, _: bool, _: &Window) -> Div {
        // The window owns titlebar drags, so this strip restores dragging and the system
        // double-click action.
        strip().on_mouse_down(gpui::MouseButton::Left, |event, window, _| {
            if event.click_count == 2 {
                window.titlebar_double_click();
            } else {
                window.start_window_move();
            }
        })
    }
}

/// The transparent band the chrome draws in.
fn strip() -> Div {
    div().absolute().top_0().left_0().w_full().h(px(SAFE_TOP))
}

#[cfg(not(target_os = "macos"))]
fn control_window(area: WindowControlArea, window: &mut Window, _: &mut App) {
    match area {
        WindowControlArea::Min => window.minimize_window(),
        WindowControlArea::Max => window.zoom_window(),
        WindowControlArea::Close => window.remove_window(),
        WindowControlArea::Drag => {}
    }
}
