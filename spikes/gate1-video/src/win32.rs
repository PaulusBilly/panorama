use std::{fs::File, io::BufWriter, path::Path};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{Input::KeyboardAndMouse::EnableWindow, WindowsAndMessaging::*},
    },
    core::w,
};

unsafe extern "system" fn video_proc(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if message == WM_ERASEBKGND {
        LRESULT(1)
    } else {
        unsafe { DefWindowProcW(hwnd, message, w, l) }
    }
}

pub struct Video {
    pub parent: HWND,
    pub child: HWND,
    last_size: (i32, i32),
}

impl Video {
    pub fn new(parent: HWND) -> Result<Self, String> {
        unsafe {
            println!(
                "approach=A parent-hwnd={:?} ex-style={:#x}",
                parent,
                GetWindowLongPtrW(parent, GWL_EXSTYLE)
            );
            let instance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
            let class = WNDCLASSW {
                lpfnWndProc: Some(video_proc),
                hInstance: instance.into(),
                lpszClassName: w!("Gate1MpvVideo"),
                ..Default::default()
            };
            if RegisterClassW(&class) == 0 {
                return Err(format!(
                    "RegisterClassW: {}",
                    windows::core::Error::from_thread()
                ));
            }
            let child = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class.lpszClassName,
                w!(""),
                WS_CHILD | WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
                0,
                0,
                1,
                1,
                Some(parent),
                None,
                Some(instance.into()),
                None,
            )
            .map_err(|e| e.to_string())?;
            let _ = EnableWindow(child, false);
            let mut video = Self {
                parent,
                child,
                last_size: (0, 0),
            };
            video.resize()?;
            Ok(video)
        }
    }

    pub fn size(&self) -> Result<(i32, i32), String> {
        let mut rect = RECT::default();
        unsafe { GetClientRect(self.parent, &mut rect) }.map_err(|e| e.to_string())?;
        Ok((rect.right - rect.left, rect.bottom - rect.top))
    }

    pub fn resize(&mut self) -> Result<(), String> {
        let size = self.size()?;
        if size != self.last_size {
            unsafe {
                SetWindowPos(
                    self.child,
                    Some(HWND_BOTTOM),
                    0,
                    0,
                    size.0,
                    size.1,
                    SWP_NOACTIVATE | SWP_SHOWWINDOW,
                )
            }
            .map_err(|e| e.to_string())?;
            self.last_size = size;
        }
        Ok(())
    }
}

impl Drop for Video {
    fn drop(&mut self) {
        unsafe {
            if IsWindow(Some(self.child)).as_bool() {
                let _ = DestroyWindow(self.child);
            }
        }
    }
}

pub fn screenshot(hwnd: HWND, path: &Path) -> Result<(), String> {
    unsafe {
        let mut rect = RECT::default();
        GetWindowRect(hwnd, &mut rect).map_err(|e| e.to_string())?;
        let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
        if width <= 0 || height <= 0 || IsIconic(hwnd).as_bool() {
            return Err("Cannot capture a minimized or empty window".into());
        }
        let desktop = GetDC(None);
        if desktop.is_invalid() {
            return Err("GetDC(desktop) failed; an interactive desktop is required".into());
        }
        let memory = CreateCompatibleDC(Some(desktop));
        if memory.is_invalid() {
            ReleaseDC(None, desktop);
            return Err("CreateCompatibleDC failed".into());
        }
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let bitmap = CreateCompatibleBitmap(desktop, width, height);
        if bitmap.is_invalid() {
            let _ = DeleteDC(memory);
            ReleaseDC(None, desktop);
            return Err("CreateCompatibleBitmap failed".into());
        }
        let old = SelectObject(memory, bitmap.into());
        let result = (|| {
            BitBlt(
                memory,
                0,
                0,
                width,
                height,
                Some(desktop),
                rect.left,
                rect.top,
                SRCCOPY | CAPTUREBLT,
            )
            .map_err(|e| format!("Desktop BitBlt failed: {e}"))?;
            SelectObject(memory, old);
            let mut bytes = vec![0u8; width as usize * height as usize * 4];
            if GetDIBits(
                memory,
                bitmap,
                0,
                height as u32,
                Some(bytes.as_mut_ptr().cast()),
                &mut info,
                DIB_RGB_COLORS,
            ) != height
            {
                return Err("GetDIBits failed".into());
            }
            for pixel in bytes.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
                pixel[3] = 255;
            }
            let file = File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
            let mut encoder = png::Encoder::new(BufWriter::new(file), width as u32, height as u32);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
            writer.write_image_data(&bytes).map_err(|e| e.to_string())?;
            writer.finish().map_err(|e| e.to_string())?;
            println!(
                "screenshot={} region={}x{} at {},{}",
                path.display(),
                width,
                height,
                rect.left,
                rect.top
            );
            Ok(())
        })();
        SelectObject(memory, old);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(memory);
        ReleaseDC(None, desktop);
        result
    }
}
