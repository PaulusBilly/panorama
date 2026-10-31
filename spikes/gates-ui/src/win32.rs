// Win32 desktop capture, process counters, and DirectWrite probing require FFI.
#![allow(unsafe_code)]

use std::{fs::File, io::BufWriter, path::Path};
use windows::{
    Win32::{
        Foundation::{HWND, RECT},
        Graphics::{DirectWrite::*, Gdi::*},
        System::{ProcessStatus::*, Threading::GetCurrentProcess},
        UI::WindowsAndMessaging::*,
    },
    core::{HSTRING, Interface},
};

#[derive(Clone, Copy)]
pub struct Memory {
    pub working: usize,
    pub private: usize,
}

pub fn memory() -> Result<Memory, String> {
    let mut counters = PROCESS_MEMORY_COUNTERS_EX {
        cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX).cast(),
            counters.cb,
        )
    }
    .map_err(|e| e.to_string())?;
    Ok(Memory {
        working: counters.WorkingSetSize,
        private: counters.PrivateUsage,
    })
}

pub fn refresh_rate() -> Result<u32, String> {
    let mut mode = DEVMODEW {
        dmSize: size_of::<DEVMODEW>() as u16,
        ..Default::default()
    };
    if !unsafe { EnumDisplaySettingsW(None, ENUM_CURRENT_SETTINGS, &mut mode) }.as_bool()
        || mode.dmDisplayFrequency < 2
    {
        return Err("Cannot read display refresh rate".into());
    }
    Ok(mode.dmDisplayFrequency)
}

pub fn font_probe(statics: &[&[u8]], variable: &[u8]) -> Result<String, String> {
    let result = (|| -> windows::core::Result<String> {
        unsafe {
            let factory: IDWriteFactory5 = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let loader = factory.CreateInMemoryFontFileLoader()?;
            factory.RegisterFontFileLoader(&loader)?;
            let result = (|| -> windows::core::Result<String> {
                let builder = factory.CreateFontSetBuilder()?;
                for bytes in statics.iter().copied().chain([variable]) {
                    let file = loader.CreateInMemoryFontFileReference(
                        &factory,
                        bytes.as_ptr().cast(),
                        bytes.len() as u32,
                        None,
                    )?;
                    builder.AddFontFile(&file)?;
                }
                let set = builder.CreateFontSet()?;
                let collection = factory.CreateFontCollectionFromFontSet(&set)?;
                let set = collection.GetFontSet()?;
                let mut variable_weights = Vec::new();
                let mut static_weights = Vec::new();
                for family in [crate::fonts::STATIC_FAMILY, crate::fonts::VARIABLE_FAMILY] {
                    for weight in [400, 500, 700] {
                        let matches = set.GetMatchingFonts(
                            &HSTRING::from(family),
                            DWRITE_FONT_WEIGHT(weight),
                            DWRITE_FONT_STRETCH_NORMAL,
                            DWRITE_FONT_STYLE_NORMAL,
                        )?;
                        let face = matches.GetFontFaceReference(0)?.CreateFontFace()?;
                        let names = face.GetFamilyNames()?;
                        let len = names.GetStringLength(0)?;
                        let mut name = vec![0u16; len as usize + 1];
                        names.GetString(0, &mut name)?;
                        let name = String::from_utf16_lossy(&name[..len as usize]);
                        let actual = face.GetWeight().0;
                        let mut axes_log = String::new();
                        if let Ok(face5) = face.cast::<IDWriteFontFace5>() {
                            let mut axes = vec![
                                DWRITE_FONT_AXIS_VALUE::default();
                                face5.GetFontAxisValueCount() as usize
                            ];
                            face5.GetFontAxisValues(&mut axes)?;
                            for axis in axes {
                                axes_log.push_str(&format!(
                                    " {}={}",
                                    String::from_utf8_lossy(&axis.axisTag.0.to_le_bytes()),
                                    axis.value
                                ));
                            }
                        }
                        println!(
                            "DirectWrite requested={family} weight={weight} actual-family={name} actual-weight={actual} axes:{axes_log}"
                        );
                        if family == crate::fonts::VARIABLE_FAMILY {
                            variable_weights.push(actual);
                        } else {
                            static_weights.push(actual);
                        }
                    }
                }
                Ok(format!(
                    "Variable selected weights {variable_weights:?}; static {static_weights:?}"
                ))
            })();
            factory.UnregisterFontFileLoader(&loader)?;
            result
        }
    })();
    result.map_err(|e| e.to_string())
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
            encoder.set_compression(png::Compression::Fast);
            encoder.set_filter(png::Filter::Up);
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
