//! Platform caption chrome with native Windows hit testing.
use crate::theme::Theme;
#[cfg(not(target_os = "macos"))]
use crate::theme::focus_ring;
#[cfg(not(target_os = "macos"))]
use gpui::{App, FocusHandle, MouseButton, WindowControlArea, svg};
use gpui::{Context, Div, Window, div, prelude::*, px, rgb};

/// Retained keyboard focus for native caption controls.
pub struct Titlebar {
    #[cfg(not(target_os = "macos"))]
    controls: [FocusHandle; 3],
}

impl Titlebar {
    /// Allocate caption focus independently of route views.
    pub fn new<T: 'static>(_cx: &mut Context<T>) -> Self {
        Self {
            #[cfg(not(target_os = "macos"))]
            controls: std::array::from_fn(|_| _cx.focus_handle()),
        }
    }

    /// Render the caption; the root hides it in fullscreen.
    #[cfg(not(target_os = "macos"))]
    pub fn render(&self, theme: Theme, keyboard_navigation: bool, window: &Window) -> Div {
        let maximized = window.is_maximized();
        let caption = div()
            .h(px(32.0))
            .w_full()
            .flex_shrink_0()
            .flex()
            .items_center()
            .bg(rgb(0xf4f4f4))
            .text_color(rgb(0x181818))
            .border_b_1()
            .border_color(gpui::Rgba {
                a: 0.14,
                ..rgb(0x000000)
            })
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_center()
                    .pl(px(12.0))
                    .text_size(px(12.0))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .line_height(px(12.0))
                    .window_control_area(WindowControlArea::Drag)
                    .when(!cfg!(target_os = "windows"), |bar| {
                        bar.on_mouse_down(MouseButton::Left, |event, window, _| {
                            if event.click_count == 2 {
                                window.zoom_window();
                            } else {
                                window.start_window_move();
                            }
                        })
                    })
                    .child("Panorama"),
            );
        caption.children([
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
        focus_ring(
            div()
                .id(("caption", index))
                .w(px(46.0))
                .h(px(32.0))
                .flex()
                .items_center()
                .justify_center()
                .when(index == 2, |button| button.group("caption-close"))
                .aria_label(label)
                .role(gpui::Role::Button)
                .hover(move |style| {
                    if index == 2 {
                        style.bg(rgb(0xc42b1c)).text_color(rgb(0xffffff))
                    } else {
                        style.bg(gpui::Rgba {
                            a: 0.1,
                            ..rgb(0x000000)
                        })
                    }
                })
                .when(cfg!(target_os = "windows"), |button| {
                    button.window_control_area(area)
                })
                .on_click(move |event, window, cx| {
                    if !cfg!(target_os = "windows")
                        || matches!(event, gpui::ClickEvent::Keyboard(_))
                    {
                        control_window(area, window, cx);
                    }
                })
                .child(
                    svg()
                        .path(icon)
                        .size(px(10.0))
                        .text_color(rgb(0x181818))
                        .when(index == 2, |icon| {
                            icon.group_hover("caption-close", |style| {
                                style.text_color(rgb(0xffffff))
                            })
                        }),
                ),
            &self.controls[index],
            theme,
            true,
            keyboard_navigation,
            window,
        )
    }

    /// macOS keeps system traffic lights and native dragging.
    #[cfg(target_os = "macos")]
    pub fn render(&self, _: Theme, _: bool, _: &Window) -> Div {
        div()
            .h(px(38.0))
            .w_full()
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::Rgba {
                a: 0.96,
                ..rgb(0x19191b)
            })
            .text_color(gpui::Rgba {
                a: 0.88,
                ..rgb(0xffffff)
            })
            .border_b_1()
            .border_color(gpui::Rgba {
                a: 0.1,
                ..rgb(0xffffff)
            })
            .text_size(px(13.0))
            .font_weight(gpui::FontWeight::MEDIUM)
            .line_height(px(13.0))
            .child("Panorama")
    }
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
