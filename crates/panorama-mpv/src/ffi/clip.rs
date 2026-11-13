//! A muted, looping clip that libmpv draws into memory with its software renderer, for video
//! that lives inside the interface (a card's trailer) instead of on the native video surface.
//! Frames are composited by the app like any image, so they clip, scale and fade with it.
use super::{Api, Client, Handle, cstring};
use crate::{MpvError, error::check};
use std::{
    ffi::{c_int, c_void},
    ptr,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    thread,
    time::Duration,
};

type Context = *mut c_void;
type Update = unsafe extern "C" fn(*mut c_void);

/// `mpv_render_param`.
#[repr(C)]
struct Param {
    kind: c_int,
    data: *mut c_void,
}

const PARAM_END: c_int = 0;
const PARAM_API_TYPE: c_int = 1;
const PARAM_BLOCK_FOR_TARGET_TIME: c_int = 12;
const PARAM_SW_SIZE: c_int = 17;
const PARAM_SW_FORMAT: c_int = 18;
const PARAM_SW_STRIDE: c_int = 19;
const PARAM_SW_POINTER: c_int = 20;
/// `MPV_RENDER_UPDATE_FRAME`.
const UPDATE_FRAME: u64 = 1;
/// Longest wait between checks for mpv events while no frame is due.
const POLL: Duration = Duration::from_millis(40);
/// Largest frame drawn; previews are small, and memory and CPU scale with the area.
pub const MAX_CLIP_WIDTH: u32 = 960;
pub const MAX_CLIP_HEIGHT: u32 = 540;

/// mpv options for a clip. Fixed here: callers pass only a URL and a size.
const CLIP_OPTIONS: &[(&str, &str, bool)] = &[
    ("terminal", "no", false),
    ("msg-level", "all=no", false),
    ("vo", "libmpv", false),
    ("audio", "no", false),
    ("aid", "no", false),
    ("mute", "yes", false),
    ("sid", "no", false),
    ("loop-file", "inf", false),
    // Trailers open on studio logos over black; a preview starts past them.
    ("start", "8%", false),
    // Software decoding: a 480p trailer is cheap, and copying hardware frames back for the
    // software renderer costs about as much as decoding them.
    ("hwdec", "no", false),
    ("cache", "yes", false),
    ("demuxer-max-bytes", "16MiB", false),
    ("demuxer-max-back-bytes", "0", false),
    ("cache-pause", "no", false),
    ("network-timeout", "15", true),
    ("ytdl", "no", true),
    ("load-scripts", "no", true),
    ("osc", "no", true),
    ("input-default-bindings", "no", false),
    ("input-vo-keyboard", "no", true),
    ("idle", "yes", false),
];

/// Where a clip is in its life.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClipState {
    /// Opening the stream; no frame yet.
    Loading,
    /// At least one frame has been drawn.
    Playing,
    /// The stream could not be played; it stays failed.
    Failed,
}

/// One drawn frame: tightly packed BGRA rows with opaque alpha.
pub struct ClipFrame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

struct Shared {
    stop: AtomicBool,
    state: AtomicU8,
    latest: Mutex<Option<ClipFrame>>,
    /// Set by mpv's render update callback and by `stop`; the worker sleeps on it.
    due: Mutex<bool>,
    wake: Condvar,
}

impl Shared {
    fn signal(&self) {
        if let Ok(mut due) = self.due.lock() {
            *due = true;
        }
        self.wake.notify_one();
    }

    fn set_state(&self, state: ClipState) {
        // Failed is final; a late frame never revives a failed clip.
        let _ = self
            .state
            .try_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                (current != ClipState::Failed as u8).then_some(state as u8)
            });
    }
}

/// A playing clip. Dropping it stops playback; teardown finishes on its own thread, so the
/// caller never waits for mpv.
pub struct Clip {
    shared: Arc<Shared>,
}

impl Clip {
    /// The newest frame since the last call, if one was drawn.
    pub fn take_frame(&self) -> Option<ClipFrame> {
        self.shared.latest.lock().ok()?.take()
    }

    pub fn state(&self) -> ClipState {
        match self.shared.state.load(Ordering::Acquire) {
            0 => ClipState::Loading,
            1 => ClipState::Playing,
            _ => ClipState::Failed,
        }
    }
}

impl Drop for Clip {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        self.shared.signal();
    }
}

/// The render API's functions; resolved per clip, since older runtimes may lack them.
#[derive(Clone, Copy)]
struct Render {
    create: unsafe extern "C" fn(*mut Context, Handle, *mut Param) -> c_int,
    set_update: unsafe extern "C" fn(Context, Option<Update>, *mut c_void),
    update: unsafe extern "C" fn(Context) -> u64,
    render: unsafe extern "C" fn(Context, *mut Param) -> c_int,
    free: unsafe extern "C" fn(Context),
}

impl Render {
    fn load(api: &Api) -> Result<Self, MpvError> {
        // SAFETY: Each symbol uses render.h's C ABI; the library outlives the clip's worker,
        // which holds the `Api`.
        unsafe {
            let lib = &api._library;
            Ok(Self {
                create: *lib
                    .get(b"mpv_render_context_create\0")
                    .map_err(|_| MpvError::MissingSymbol)?,
                set_update: *lib
                    .get(b"mpv_render_context_set_update_callback\0")
                    .map_err(|_| MpvError::MissingSymbol)?,
                update: *lib
                    .get(b"mpv_render_context_update\0")
                    .map_err(|_| MpvError::MissingSymbol)?,
                render: *lib
                    .get(b"mpv_render_context_render\0")
                    .map_err(|_| MpvError::MissingSymbol)?,
                free: *lib
                    .get(b"mpv_render_context_free\0")
                    .map_err(|_| MpvError::MissingSymbol)?,
            })
        }
    }
}

/// Starts `url` muted and looping, drawn at `width`×`height` (clamped to the maximum).
/// Only `http(s)` URLs are accepted. Returns at once; frames arrive through [`Clip::take_frame`].
pub(crate) fn start(api: &Arc<Api>, url: &str, width: u32, height: u32) -> Result<Clip, MpvError> {
    if !(url.starts_with("https://") || url.starts_with("http://")) || url.contains('\0') {
        return Err(MpvError::InvalidArgument);
    }
    let (width, height) = clip_size(width, height).ok_or(MpvError::InvalidArgument)?;
    let render = Render::load(api)?;
    let shared = Arc::new(Shared {
        stop: AtomicBool::new(false),
        state: AtomicU8::new(ClipState::Loading as u8),
        latest: Mutex::new(None),
        due: Mutex::new(false),
        wake: Condvar::new(),
    });
    let worker = shared.clone();
    let api = api.clone();
    let url = url.to_owned();
    thread::Builder::new()
        .name("panorama-mpv-clip".into())
        .spawn(move || {
            if run(&api, render, &url, (width, height), &worker).is_err() {
                worker.set_state(ClipState::Failed);
            }
        })
        .map_err(|_| MpvError::Thread)?;
    Ok(Clip { shared })
}

/// Scales a requested size into the maximum, keeping its shape; `None` for an empty one.
pub(crate) fn clip_size(width: u32, height: u32) -> Option<(u32, u32)> {
    if width == 0 || height == 0 {
        return None;
    }
    let scale = (f64::from(MAX_CLIP_WIDTH) / f64::from(width))
        .min(f64::from(MAX_CLIP_HEIGHT) / f64::from(height))
        .min(1.0);
    let width = ((f64::from(width) * scale).round() as u32).max(1);
    let height = ((f64::from(height) * scale).round() as u32).max(1);
    Some((width, height))
}

unsafe extern "C" fn on_update(data: *mut c_void) {
    // SAFETY: `data` is the worker's `Shared`, kept alive until the context is freed, after
    // which mpv makes no more calls.
    let shared = unsafe { &*data.cast::<Shared>() };
    shared.signal();
}

fn run(
    api: &Arc<Api>,
    render: Render,
    url: &str,
    size: (u32, u32),
    shared: &Arc<Shared>,
) -> Result<(), MpvError> {
    let client = api.create(None)?;
    for &(key, value, best_effort) in CLIP_OPTIONS {
        let result = client.option(key, value);
        if !best_effort {
            result?;
        }
    }
    // SAFETY: Live handle, configured above and owned by this thread.
    check(unsafe { (client.api.initialize)(client.handle as Handle) })?;

    let mut context: Context = ptr::null_mut();
    let kind = cstring("sw")?;
    let mut params = [
        Param {
            kind: PARAM_API_TYPE,
            data: kind.as_ptr().cast_mut().cast(),
        },
        Param {
            kind: PARAM_END,
            data: ptr::null_mut(),
        },
    ];
    // SAFETY: Parameters are live through the call; mpv copies what it keeps.
    check(unsafe { (render.create)(&mut context, client.handle as Handle, params.as_mut_ptr()) })?;
    if context.is_null() {
        return Err(MpvError::Output);
    }
    let callback_data = Arc::as_ptr(shared).cast_mut().cast::<c_void>();
    // SAFETY: `shared` outlives the context: it is freed below, before this frame returns.
    unsafe { (render.set_update)(context, Some(on_update), callback_data) };

    let result = play(&client, render, context, url, size, shared);

    // SAFETY: The render context must be freed before the handle is destroyed; nothing calls
    // the update callback after this returns.
    unsafe { (render.free)(context) };
    drop(client);
    result
}

fn play(
    client: &Client,
    render: Render,
    context: Context,
    url: &str,
    (width, height): (u32, u32),
    shared: &Shared,
) -> Result<(), MpvError> {
    client.command(&["loadfile".into(), url.into(), "replace".into()])?;
    let stride = width as usize * 4;
    let mut size = [width as c_int, height as c_int];
    let format = cstring("bgr0")?;
    let mut stride_value = stride;
    let mut block: c_int = 0;
    while !shared.stop.load(Ordering::Acquire) {
        if let Ok(due) = shared.due.lock() {
            let (mut due, _) = shared
                .wake
                .wait_timeout_while(due, POLL, |due| !*due)
                .map_err(|_| MpvError::Thread)?;
            *due = false;
        }
        if shared.stop.load(Ordering::Acquire) {
            break;
        }
        if drain_events(client) {
            shared.set_state(ClipState::Failed);
            break;
        }
        // SAFETY: Called from the thread that renders, with the live context.
        let flags = unsafe { (render.update)(context) };
        if flags & UPDATE_FRAME == 0 {
            continue;
        }
        let mut bgra = vec![0u8; stride * height as usize];
        let mut params = [
            Param {
                kind: PARAM_SW_SIZE,
                data: size.as_mut_ptr().cast(),
            },
            Param {
                kind: PARAM_SW_FORMAT,
                data: format.as_ptr().cast_mut().cast(),
            },
            Param {
                kind: PARAM_SW_STRIDE,
                data: (&mut stride_value as *mut usize).cast(),
            },
            Param {
                kind: PARAM_SW_POINTER,
                data: bgra.as_mut_ptr().cast(),
            },
            Param {
                kind: PARAM_BLOCK_FOR_TARGET_TIME,
                data: (&mut block as *mut c_int).cast(),
            },
            Param {
                kind: PARAM_END,
                data: ptr::null_mut(),
            },
        ];
        // SAFETY: The buffer holds `stride * height` bytes and outlives the call.
        if unsafe { (render.render)(context, params.as_mut_ptr()) } < 0 {
            continue;
        }
        opaque(&mut bgra);
        if let Ok(mut latest) = shared.latest.lock() {
            *latest = Some(ClipFrame {
                width,
                height,
                bgra,
            });
        }
        shared.set_state(ClipState::Playing);
    }
    Ok(())
}

/// Reads every queued event; true when the file failed to load or play.
fn drain_events(client: &Client) -> bool {
    loop {
        // SAFETY: Only this thread waits on the handle; the event is read before the next wait.
        let Some(event) = (unsafe { (client.api.wait)(client.handle as Handle, 0.0).as_ref() })
        else {
            return false;
        };
        match event.id {
            0 => return false,
            1 => return true,
            7 => {
                // SAFETY: END_FILE carries `mpv_event_end_file`, valid until the next wait.
                let reason = unsafe { event.data.cast::<EndFileReason>().as_ref() };
                if reason.is_some_and(|end| end.reason == 4) {
                    return true;
                }
            }
            _ => {}
        }
    }
}

#[repr(C)]
struct EndFileReason {
    reason: c_int,
    error: c_int,
}

/// mpv leaves the padding byte of `bgr0` undefined; the app composites with alpha.
fn opaque(bgra: &mut [u8]) {
    for pixel in bgra.as_chunks_mut::<4>().0 {
        pixel[3] = 255;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_keep_their_shape_inside_the_maximum() {
        assert_eq!(clip_size(480, 270), Some((480, 270)));
        assert_eq!(clip_size(1920, 1080), Some((960, 540)));
        assert_eq!(clip_size(1080, 1920), Some((304, 540)));
        assert_eq!(clip_size(0, 270), None);
    }

    #[test]
    fn frames_become_opaque() {
        let mut pixels = vec![1, 2, 3, 0, 4, 5, 6, 9];
        opaque(&mut pixels);
        assert_eq!(pixels, vec![1, 2, 3, 255, 4, 5, 6, 255]);
    }

    /// Plays `PANORAMA_CLIP_URL` with the runtime in `PANORAMA_LIBMPV_DIR` and waits for frames.
    #[test]
    #[ignore = "needs the network and a libmpv runtime"]
    fn a_live_clip_draws_frames() {
        let url = std::env::var("PANORAMA_CLIP_URL").expect("PANORAMA_CLIP_URL");
        let dir = std::env::var_os("PANORAMA_LIBMPV_DIR").map(std::path::PathBuf::from);
        let library = crate::Mpv::load_library(dir.as_deref()).expect("libmpv");
        let started = std::time::Instant::now();
        let clip = library.clip(&url, 480, 270).expect("clip");
        let mut frames = 0;
        let mut first = None;
        while started.elapsed() < Duration::from_secs(12) && frames < 60 {
            if let Some(frame) = clip.take_frame() {
                assert_eq!((frame.width, frame.height), (480, 270));
                assert_eq!(frame.bgra.len(), 480 * 270 * 4);
                first.get_or_insert(started.elapsed());
                frames += 1;
            }
            assert_ne!(clip.state(), ClipState::Failed);
            thread::sleep(Duration::from_millis(10));
        }
        eprintln!("first frame after {first:?}, {frames} frames");
        assert!(frames >= 30, "only {frames} frames");
    }

    #[test]
    fn only_web_urls_start() {
        let Ok(library) = crate::Mpv::load_library(None) else {
            return;
        };
        for url in ["file:///etc/passwd", "/tmp/x.mp4", "https://a\0b"] {
            assert_eq!(
                library.clip(url, 480, 270).err(),
                Some(MpvError::InvalidArgument)
            );
        }
    }
}
