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

#[cfg(not(windows))]
fn main() {
    eprintln!("The playback example requires Windows.");
}
