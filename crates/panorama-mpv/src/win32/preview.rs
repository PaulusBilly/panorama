use super::*;
use windows::Win32::{
    Foundation::RECT,
    Graphics::Gdi::{BLACK_BRUSH, GetStockObject, HBRUSH},
};

// SAFETY: Registered Win32 callback; never accesses borrowed Rust data.
unsafe extern "system" fn preview_proc(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if message == WM_CLOSE {
        // SAFETY: This window belongs to the current message-dispatch thread.
        let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
        // SAFETY: WM_QUIT signals the preview loop without destroying mpv's parent.
        unsafe { PostQuitMessage(0) };
        return LRESULT(0);
    }
    // SAFETY: Forward unchanged Win32 callback arguments.
    unsafe { DefWindowProcW(hwnd, message, w, l) }
}

/// Small standalone owner window for the GPUI-free playback example.
/// Close requests hide it; Drop destroys it after the player has shut down.
pub struct PreviewWindow {
    hwnd: HWND,
    closing: bool,
    _owner_thread: PhantomData<Rc<()>>,
}

impl PreviewWindow {
    /// Creates a visible 960-by-540 client window with a black background.
    pub fn create() -> Result<Self, MpvError> {
        static REGISTERED: OnceLock<Result<(), MpvError>> = OnceLock::new();
        // SAFETY: All class/creation arguments are static or valid for the call;
        // the window is owned by the invoking thread.
        unsafe {
            let instance = GetModuleHandleW(None).map_err(|_| MpvError::Window)?;
            (*REGISTERED.get_or_init(|| {
                let class = WNDCLASSW {
                    lpfnWndProc: Some(preview_proc),
                    hInstance: instance.into(),
                    lpszClassName: w!("PanoramaMpvPreview"),
                    hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0),
                    ..Default::default()
                };
                if RegisterClassW(&class) == 0 {
                    Err(MpvError::Window)
                } else {
                    Ok(())
                }
            }))?;
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("PanoramaMpvPreview"),
                w!("Panorama mpv"),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE | WS_CLIPCHILDREN,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                976,
                579,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .map_err(|_| MpvError::Window)?;
            Ok(Self {
                hwnd,
                closing: false,
                _owner_thread: PhantomData,
            })
        }
    }

    /// Creates the retained video child underneath this window.
    pub fn video_surface(&self) -> Result<VideoSurface, MpvError> {
        VideoSurface::create(self.hwnd)
    }

    /// Current physical client bounds.
    pub fn bounds(&self) -> Result<PhysicalRect, MpvError> {
        let mut rect = RECT::default();
        // SAFETY: Live owner window and writable RECT.
        unsafe { GetClientRect(self.hwnd, &mut rect) }.map_err(|_| MpvError::Window)?;
        Ok(PhysicalRect {
            x: 0,
            y: 0,
            width: rect.right,
            height: rect.bottom,
        })
    }

    /// Drains available messages without blocking. Returns false after a close request.
    /// Continue calling it during asynchronous player shutdown for child destruction.
    pub fn pump(&mut self) -> bool {
        // SAFETY: Initialized MSG on the window's owning thread; Win32 fills it.
        unsafe {
            let mut message = MSG::default();
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                if message.message == WM_QUIT {
                    self.closing = true;
                } else {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
        }
        !self.closing
    }
}

impl Drop for PreviewWindow {
    fn drop(&mut self) {
        // SAFETY: Owning thread, after the example releases its player and surface.
        let _ = unsafe { DestroyWindow(self.hwnd) };
    }
}
