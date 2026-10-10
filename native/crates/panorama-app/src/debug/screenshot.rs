//! Windows desktop capture isolated from the UI thread.
use gpui::{Context, Window};
use std::{path::PathBuf, sync::mpsc::Sender};

/// Schedule capture after a settled frame, then close the window after background I/O.
pub fn schedule<T: 'static>(
    window: &mut Window,
    cx: &mut Context<T>,
    path: PathBuf,
    outcome: Sender<Result<(), String>>,
) {
    let handle = window_handle(window);
    let executor = cx.background_executor().clone();
    window.on_next_frame(move |_, cx| {
        let task = executor.spawn(async move { handle.and_then(|handle| capture(handle, &path)) });
        cx.spawn(async move |cx| {
            let result = task.await;
            let _ = outcome.send(result);
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
}

#[cfg(target_os = "windows")]
fn window_handle(window: &Window) -> Result<usize, String> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window).map_err(|error| error.to_string())?;
    match handle.as_raw() {
        RawWindowHandle::Win32(handle) => Ok(handle.hwnd.get() as usize),
        _ => Err("Desktop capture requires a Windows HWND".into()),
    }
}

#[cfg(not(target_os = "windows"))]
fn window_handle(_: &Window) -> Result<usize, String> {
    Err("--screenshot desktop capture is implemented only on Windows".into())
}

#[cfg(not(target_os = "windows"))]
fn capture(_: usize, _: &std::path::Path) -> Result<(), String> {
    Err("--screenshot desktop capture is implemented only on Windows".into())
}

#[cfg(target_os = "windows")]
fn capture(handle: usize, path: &std::path::Path) -> Result<(), String> {
    use std::{fs::File, io::BufWriter};
    use windows::Win32::{
        Foundation::{HWND, POINT, RECT},
        Graphics::Gdi::*,
        UI::WindowsAndMessaging::{GetClientRect, IsIconic},
    };
    unsafe {
        let hwnd = HWND(handle as *mut _);
        let mut rect = RECT::default();
        GetClientRect(hwnd, &mut rect).map_err(|error| error.to_string())?;
        let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
        if width <= 0 || height <= 0 || IsIconic(hwnd).as_bool() {
            return Err("Cannot capture a minimized or empty window".into());
        }
        let mut origin = POINT {
            x: rect.left,
            y: rect.top,
        };
        if !ClientToScreen(hwnd, &mut origin).as_bool() {
            return Err("ClientToScreen failed".into());
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
        let bitmap = CreateCompatibleBitmap(desktop, width, height);
        if bitmap.is_invalid() {
            let _ = DeleteDC(memory);
            ReleaseDC(None, desktop);
            return Err("CreateCompatibleBitmap failed".into());
        }
        let old = SelectObject(memory, bitmap.into());
        if old.is_invalid() {
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(memory);
            ReleaseDC(None, desktop);
            return Err("SelectObject failed".into());
        }
        let result = (|| {
            BitBlt(
                memory,
                0,
                0,
                width,
                height,
                Some(desktop),
                origin.x,
                origin.y,
                SRCCOPY | CAPTUREBLT,
            )
            .map_err(|error| format!("Desktop BitBlt failed: {error}"))?;
            SelectObject(memory, old);
            let length = (width as usize)
                .checked_mul(height as usize)
                .and_then(|size| size.checked_mul(4))
                .ok_or("Capture dimensions overflow")?;
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(length)
                .map_err(|error| error.to_string())?;
            bytes.resize(length, 0u8);
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
            let file =
                File::create(path).map_err(|error| format!("{}: {error}", path.display()))?;
            let mut encoder = png::Encoder::new(BufWriter::new(file), width as u32, height as u32);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().map_err(|error| error.to_string())?;
            writer
                .write_image_data(&bytes)
                .map_err(|error| error.to_string())?;
            writer.finish().map_err(|error| error.to_string())?;
            Ok(())
        })();
        SelectObject(memory, old);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(memory);
        ReleaseDC(None, desktop);
        result
    }
}
