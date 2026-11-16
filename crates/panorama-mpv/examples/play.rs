#![cfg_attr(target_os = "macos", allow(unsafe_code))]
#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use panorama_mpv::{Mpv, PlayerEvent, PlayerOptions, win32::PreviewWindow};
    use std::{path::PathBuf, thread, time::Duration};

    let source = std::env::args().nth(1).ok_or("Usage: play <file-or-url>")?;
    let source = if std::path::Path::new(&source).is_file() {
        std::path::Path::new(&source)
            .canonicalize()?
            .to_string_lossy()
            .into_owned()
    } else {
        source
    };
    let runtime = std::env::var_os("PANORAMA_LIBMPV_DIR").map(PathBuf::from);
    let library = Mpv::load_library(runtime.as_deref())?;
    let mut window = PreviewWindow::create()?;
    let surface = window.video_surface()?;
    surface.set_bounds(window.bounds()?)?;
    surface.show();
    let mut player = library.create(PlayerOptions {
        wid: Some(surface.raw_window()),
        extra: Vec::new(),
    })?;
    let events = player.events()?;
    player.load(&source)?;
    let mut closing = false;
    let mut completion = None;
    let mut player = Some(player);
    loop {
        if !window.pump() && !closing {
            closing = true;
            if let Some(player) = player.take() {
                completion = Some(player.close());
            }
        }
        if !closing {
            surface.set_bounds(window.bounds()?)?;
        }
        for event in events.try_iter() {
            match event {
                PlayerEvent::HwdecCurrent(Some(decoder)) => println!("hwdec-current={decoder}"),
                PlayerEvent::EndFile { reason, .. } => println!("end-file={reason:?}"),
                PlayerEvent::Error(error) => eprintln!("{error}"),
                PlayerEvent::Shutdown => return Ok(()),
                _ => {}
            }
        }
        if let Some(completion) = &completion
            && let Ok(result) = completion.try_recv()
            && let Err(error) = result
        {
            eprintln!("{error}; waiting for actual Shutdown with the parent retained");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSEventMask, NSView,
        NSWindow, NSWindowStyleMask,
    };
    use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSPoint, NSRect, NSSize};
    use panorama_mpv::{Mpv, PhysicalRect, PlayerEvent, PlayerOptions, VideoSurface};
    use raw_window_handle::{
        AppKitWindowHandle, HandleError, HasWindowHandle, RawWindowHandle, WindowHandle,
    };
    use std::{path::PathBuf, ptr::NonNull};

    struct Sibling(NonNull<std::ffi::c_void>);
    impl HasWindowHandle for Sibling {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            let raw = RawWindowHandle::AppKit(AppKitWindowHandle::new(self.0));
            // SAFETY: the view outlives every borrow in this example.
            Ok(unsafe { WindowHandle::borrow_raw(raw) })
        }
    }

    let source = std::env::args().nth(1).ok_or("Usage: play <file-or-url>")?;
    let mtm = MainThreadMarker::new().ok_or("main thread")?;
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    let frame = NSRect::new(NSPoint::new(100.0, 100.0), NSSize::new(960.0, 540.0));
    // SAFETY: standard AppKit window construction on the main thread.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            frame,
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: the window is retained by this function until exit.
    unsafe { window.setReleasedWhenClosed(false) };
    let content = window.contentView().ok_or("content view")?;
    let sibling = NSView::initWithFrame(NSView::alloc(mtm), content.bounds());
    content.addSubview(&sibling);
    window.makeKeyAndOrderFront(None);
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    println!("window-number={}", window.windowNumber());

    let surface = VideoSurface::from_window(&Sibling(NonNull::from(&*sibling).cast()))?;
    let scale = window.backingScaleFactor();
    let bounds = PhysicalRect {
        x: 0,
        y: 0,
        width: (960.0 * scale) as i32,
        height: (540.0 * scale) as i32,
    };
    surface.set_bounds(bounds)?;
    surface.show();
    let runtime = std::env::var_os("PANORAMA_LIBMPV_DIR").map(PathBuf::from);
    let library = Mpv::load_library(runtime.as_deref())?;
    let mut player = library.create(PlayerOptions {
        wid: Some(surface.raw_window()),
        extra: Vec::new(),
    })?;
    let events = player.events()?;
    player.load(&source)?;
    loop {
        // SAFETY: event pumping on the main thread.
        while let Some(event) = unsafe {
            app.nextEventMatchingMask_untilDate_inMode_dequeue(
                NSEventMask::Any,
                Some(&NSDate::dateWithTimeIntervalSinceNow(0.01)),
                NSDefaultRunLoopMode,
                true,
            )
        } {
            app.sendEvent(&event);
        }
        if !window.isVisible() {
            return Ok(());
        }
        surface.set_bounds(bounds)?;
        for event in events.try_iter() {
            match event {
                PlayerEvent::HwdecCurrent(Some(decoder)) => println!("hwdec-current={decoder}"),
                PlayerEvent::EndFile { reason, .. } => println!("end-file={reason:?}"),
                PlayerEvent::Error(error) => eprintln!("{error}"),
                PlayerEvent::Shutdown => return Ok(()),
                _ => {}
            }
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn main() {
    eprintln!("The playback example requires Windows or macOS.");
}
