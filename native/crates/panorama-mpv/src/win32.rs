use crate::{MpvError, RawWindow};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    marker::PhantomData,
    rc::Rc,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
        UI::{Input::KeyboardAndMouse::EnableWindow, WindowsAndMessaging::*},
    },
    core::w,
};
mod preview;
pub use preview::PreviewWindow;

const DESTROY_SURFACE: u32 = WM_APP + 31;

// SAFETY: Win32 calls this registered callback with its ABI. GWLP_USERDATA owns
// one Arc<WindowState> from WM_NCCREATE until WM_NCDESTROY on the owning thread.
unsafe extern "system" fn video_proc(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if message == WM_NCCREATE {
        // SAFETY: WM_NCCREATE supplies CREATESTRUCTW; lpCreateParams points to the
        // Arc borrowed by create() for this synchronous call. Retain it for the HWND.
        unsafe {
            let creation = &*(l.0 as *const CREATESTRUCTW);
            let Some(state) = creation.lpCreateParams.cast::<Arc<WindowState>>().as_ref() else {
                return LRESULT(0);
            };
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Arc::into_raw(state.clone()) as isize);
        }
    }
    // SAFETY: Only this procedure writes GWLP_USERDATA; its Arc remains owned
    // by the window until WM_NCDESTROY, and callbacks run on the owning thread.
    let state = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const WindowState;
    if message == WM_NCDESTROY && !state.is_null() {
        // SAFETY: Remove and release the window's unique raw Arc exactly once.
        // The lease retains its own Arc and observes destruction across threads.
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            let state = Arc::from_raw(state);
            state.destroyed.store(true, Ordering::Release);
        }
    }
    if message == WM_ERASEBKGND {
        return LRESULT(1);
    }
    if message == DESTROY_SURFACE {
        // SAFETY: The window's state is live on this dispatch thread. The unique
        // token prevents a queued message from destroying a recycled HWND.
        unsafe {
            if let Some(state) = state.as_ref()
                && state.token == w.0
                && !state.destroyed.load(Ordering::Acquire)
            {
                let _ = DestroyWindow(hwnd);
            }
        }
        return LRESULT(0);
    }
    // SAFETY: Forwarding Win32-provided callback arguments unchanged.
    unsafe { DefWindowProcW(hwnd, message, w, l) }
}

struct WindowState {
    destroyed: AtomicBool,
    token: usize,
}

pub(crate) struct WindowLease {
    pub(crate) id: usize,
    thread: u32,
    state: Arc<WindowState>,
}

impl Drop for WindowLease {
    fn drop(&mut self) {
        if self.state.destroyed.load(Ordering::Acquire) {
            return;
        }
        let hwnd = HWND(self.id as *mut _);
        // SAFETY: WM_NCDESTROY invalidates ownership before a handle can be reused.
        // On the owner thread destruction cannot race this check. Off-thread posts
        // carry a unique token checked by the procedure before destroying anything.
        unsafe {
            if GetCurrentThreadId() == self.thread {
                let _ = DestroyWindow(hwnd);
            } else {
                let _ = PostMessageW(
                    Some(hwnd),
                    DESTROY_SURFACE,
                    WPARAM(self.state.token),
                    LPARAM(0),
                );
            }
        }
    }
}

/// Physical client-relative rectangle, in pixels.
#[derive(Clone, Copy, Debug, Default)]
pub struct PhysicalRect {
    /// Left offset.
    pub x: i32,
    /// Top offset.
    pub y: i32,
    /// Non-negative width.
    pub width: i32,
    /// Non-negative height.
    pub height: i32,
}

/// Disabled bottom child HWND from Gate 1 approach A. Use on the parent's thread.
/// Drop destroys it after every player lease has been released; off-thread final
/// release posts destruction to its window procedure. Keep the parent alive and
/// pumping messages until mpv `Shutdown` and the posted destruction are processed.
pub struct VideoSurface {
    lease: Arc<WindowLease>,
    _owner_thread: PhantomData<Rc<()>>,
}

impl VideoSurface {
    /// Creates the child on the parent's owning thread.
    pub fn create(parent: HWND) -> Result<Self, MpvError> {
        static NEXT_TOKEN: AtomicUsize = AtomicUsize::new(1);
        let state = Arc::new(WindowState {
            destroyed: AtomicBool::new(false),
            token: NEXT_TOKEN
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |token| {
                    token.checked_add(1)
                })
                .map_err(|_| MpvError::Window)?,
        });
        // SAFETY: Win32 validates the opaque parent; class/callback are process-wide
        // constants, and all creation arguments live through the call.
        unsafe {
            if !IsWindow(Some(parent)).as_bool()
                || GetWindowThreadProcessId(parent, None) != GetCurrentThreadId()
            {
                return Err(MpvError::Window);
            }
            register()?;
            let instance = GetModuleHandleW(None).map_err(|_| MpvError::Window)?;
            let child = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("PanoramaRustMpvSurface"),
                w!(""),
                WS_CHILD | WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
                0,
                0,
                1,
                1,
                Some(parent),
                None,
                Some(instance.into()),
                Some((&state as *const Arc<WindowState>).cast()),
            )
            .map_err(|_| MpvError::Window)?;
            let _ = EnableWindow(child, false);
            let surface = Self {
                lease: Arc::new(WindowLease {
                    id: child.0 as usize,
                    thread: GetCurrentThreadId(),
                    state,
                }),
                _owner_thread: PhantomData,
            };
            surface.set_bounds(PhysicalRect {
                width: 1,
                height: 1,
                ..Default::default()
            })?;
            Ok(surface)
        }
    }

    /// Extracts a Windows parent from GPUI's `HasWindowHandle` implementation.
    pub fn from_window(parent: &impl HasWindowHandle) -> Result<Self, MpvError> {
        let handle = parent.window_handle().map_err(|_| MpvError::Window)?;
        match handle.as_raw() {
            RawWindowHandle::Win32(handle) => Self::create(HWND(handle.hwnd.get() as *mut _)),
            _ => Err(MpvError::UnsupportedPlatform),
        }
    }

    /// Retains the child for `PlayerOptions::wid`; no raw pointer escapes.
    pub fn raw_window(&self) -> RawWindow {
        RawWindow {
            lease: self.lease.clone(),
        }
    }

    /// Positions at HWND_BOTTOM using physical pixels; never activates the child.
    pub fn set_bounds(&self, rect: PhysicalRect) -> Result<(), MpvError> {
        if rect.width < 0 || rect.height < 0 {
            return Err(MpvError::InvalidArgument);
        }
        // SAFETY: Owned child, confined to its creation thread by !Send/!Sync.
        unsafe {
            SetWindowPos(
                self.hwnd(),
                Some(HWND_BOTTOM),
                rect.x,
                rect.y,
                rect.width,
                rect.height,
                SWP_NOACTIVATE,
            )
        }
        .map_err(|_| MpvError::Window)
    }

    /// Shows the surface without activating it.
    pub fn show(&self) {
        // SAFETY: Owned child on its creation thread.
        let _ = unsafe { ShowWindow(self.hwnd(), SW_SHOWNA) };
    }

    /// Hides the surface.
    pub fn hide(&self) {
        // SAFETY: Owned child on its creation thread.
        let _ = unsafe { ShowWindow(self.hwnd(), SW_HIDE) };
    }

    fn hwnd(&self) -> HWND {
        HWND(self.lease.id as *mut _)
    }
}

fn register() -> Result<(), MpvError> {
    static REGISTERED: OnceLock<Result<(), MpvError>> = OnceLock::new();
    *REGISTERED.get_or_init(|| {
        // SAFETY: Register once, with static class name and correct callback ABI.
        unsafe {
            let instance = GetModuleHandleW(None).map_err(|_| MpvError::Window)?;
            let class = WNDCLASSW {
                lpfnWndProc: Some(video_proc),
                hInstance: instance.into(),
                lpszClassName: w!("PanoramaRustMpvSurface"),
                ..Default::default()
            };
            if RegisterClassW(&class) == 0 {
                Err(MpvError::Window)
            } else {
                Ok(())
            }
        }
    })
}

#[cfg(test)]
mod tests;
