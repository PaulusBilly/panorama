//! The macOS traffic lights, faded with the player's chrome.
use gpui::Window;

/// Sets the close, minimise and zoom buttons' opacity; at 0 they also stop taking clicks.
#[cfg(target_os = "macos")]
pub fn set_opacity(window: &Window, opacity: f32) {
    use objc2_app_kit::{NSView, NSWindowButton};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    // SAFETY: AppKit handles carry GPUI's live NSView; GPUI renders on the main thread.
    let view = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
    let Some(window) = view.window() else {
        return;
    };
    for kind in [
        NSWindowButton::CloseButton,
        NSWindowButton::MiniaturizeButton,
        NSWindowButton::ZoomButton,
    ] {
        if let Some(button) = window.standardWindowButton(kind) {
            button.setAlphaValue(f64::from(opacity.clamp(0.0, 1.0)));
            button.setHidden(opacity <= 0.0);
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn set_opacity(_: &Window, _: f32) {}
