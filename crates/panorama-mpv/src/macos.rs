use crate::{MpvError, PhysicalRect, RawWindow};
use objc2::{MainThreadMarker, MainThreadOnly, rc::Retained};
use objc2_app_kit::{NSView, NSWindowOrderingMode};
use objc2_core_graphics::CGColor;
use objc2_foundation::{NSPoint, NSRect, NSSize};
use objc2_quartz_core::CAMetalLayer;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    ffi::c_void,
    marker::PhantomData,
    rc::Rc,
    sync::{Arc, Mutex},
};

/// `panorama_surface_state` of the `panorama-macvk` mpv patch (libmpv/patches).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct SurfaceState {
    width: i32,
    height: i32,
    visible: i32,
    fps: f64,
    scale: f64,
    headroom: f64,
    generation: u64,
}

/// `panorama_surface`, read by mpv's `panorama-macvk` context through `wid`.
#[repr(C)]
struct Descriptor {
    version: u64,
    layer: *mut c_void,
    opaque: *const c_void,
    snapshot: extern "C" fn(*const c_void) -> SurfaceState,
    swapped: extern "C" fn(*const c_void),
}

extern "C" fn snapshot(opaque: *const c_void) -> SurfaceState {
    // SAFETY: `opaque` is the lease's state, which outlives every player holding the lease.
    let state = unsafe { &*opaque.cast::<Mutex<SurfaceState>>() };
    state.lock().map(|state| *state).unwrap_or_default()
}

extern "C" fn swapped(_: *const c_void) {}

pub(crate) struct WindowLease {
    descriptor: Box<Descriptor>,
    state: Box<Mutex<SurfaceState>>,
    /// +1 retained `NSView`; released on the main thread.
    view: *mut NSView,
}

// SAFETY: The view pointer is only dereferenced on the main thread (VideoSurface is
// !Send, and Drop hops to the main queue); the state is a Mutex and the descriptor is
// immutable after construction.
unsafe impl Send for WindowLease {}
// SAFETY: As above; shared access never touches the view off the main thread.
unsafe impl Sync for WindowLease {}

impl WindowLease {
    pub(crate) fn id(&self) -> usize {
        &*self.descriptor as *const Descriptor as usize
    }
}

unsafe extern "C" {
    static _dispatch_main_q: c_void;
    fn dispatch_async_f(
        queue: *const c_void,
        context: *mut c_void,
        work: extern "C" fn(*mut c_void),
    );
}

extern "C" fn release_view(view: *mut c_void) {
    // SAFETY: Runs on the main thread with the lease's +1 reference, transferred here once.
    let view = unsafe { Retained::from_raw(view.cast::<NSView>()) };
    if let Some(view) = view {
        view.removeFromSuperview();
    }
}

impl Drop for WindowLease {
    fn drop(&mut self) {
        if MainThreadMarker::new().is_some() {
            release_view(self.view.cast());
        } else {
            // SAFETY: The main queue is a process-lifetime static; the work function
            // takes ownership of the +1 view reference.
            unsafe {
                dispatch_async_f(
                    (&raw const _dispatch_main_q).cast(),
                    self.view.cast(),
                    release_view,
                )
            };
        }
    }
}

/// Layer-hosting `NSView` with a `CAMetalLayer`, placed below GPUI's view in the
/// window's content view so a transparent GPUI frame shows the video underneath.
/// Use on the main thread. mpv renders through the patched `panorama-macvk` context.
pub struct VideoSurface {
    lease: Arc<WindowLease>,
    layer: Retained<CAMetalLayer>,
    _main_thread: PhantomData<Rc<()>>,
}

impl VideoSurface {
    /// Extracts GPUI's `NSView` from its `HasWindowHandle` implementation.
    pub fn from_window(parent: &impl HasWindowHandle) -> Result<Self, MpvError> {
        let mtm = MainThreadMarker::new().ok_or(MpvError::Window)?;
        let handle = parent.window_handle().map_err(|_| MpvError::Window)?;
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return Err(MpvError::UnsupportedPlatform);
        };
        // SAFETY: AppKit handles carry a live NSView pointer owned by the window.
        let sibling = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
        // SAFETY: Reading the view hierarchy on the main thread.
        let content = unsafe { sibling.superview() }.ok_or(MpvError::Window)?;
        let layer = CAMetalLayer::new();
        layer.setOpaque(true);
        // Until mpv presents its first frame the layer has no contents; without a colour the
        // window would show whatever sits behind it while a film starts.
        layer.setBackgroundColor(Some(&CGColor::new_srgb(0.0, 0.0, 0.0, 1.0)));
        layer.setWantsExtendedDynamicRangeContent(true);
        let view = NSView::initWithFrame(NSView::alloc(mtm), NSRect::ZERO);
        view.setLayer(Some(&layer));
        view.setWantsLayer(true);
        view.setHidden(true);
        content.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Below, Some(sibling));
        let state = Box::new(Mutex::new(SurfaceState::default()));
        let descriptor = Box::new(Descriptor {
            version: 1,
            layer: Retained::as_ptr(&layer).cast_mut().cast(),
            opaque: (&*state as *const Mutex<SurfaceState>).cast(),
            snapshot,
            swapped,
        });
        Ok(Self {
            lease: Arc::new(WindowLease {
                descriptor,
                state,
                view: Retained::into_raw(view),
            }),
            layer,
            _main_thread: PhantomData,
        })
    }

    /// Retains the surface for `PlayerOptions::wid`; no raw pointer escapes.
    pub fn raw_window(&self) -> RawWindow {
        RawWindow {
            lease: self.lease.clone(),
        }
    }

    /// Positions the view in physical pixels measured from the content view's top-left.
    pub fn set_bounds(&self, rect: PhysicalRect) -> Result<(), MpvError> {
        if rect.width < 0 || rect.height < 0 {
            return Err(MpvError::InvalidArgument);
        }
        let view = self.view();
        let window = view.window().ok_or(MpvError::Window)?;
        let scale = window.backingScaleFactor();
        // SAFETY: Reading the view hierarchy on the main thread.
        let content = unsafe { view.superview() }.ok_or(MpvError::Window)?;
        let height = content.bounds().size.height;
        let points = |pixels: i32| f64::from(pixels) / scale;
        view.setFrame(NSRect::new(
            NSPoint::new(
                points(rect.x),
                height - points(rect.y) - points(rect.height),
            ),
            NSSize::new(points(rect.width), points(rect.height)),
        ));
        self.layer.setContentsScale(scale);
        self.layer.setDrawableSize(NSSize::new(
            f64::from(rect.width.max(1)),
            f64::from(rect.height.max(1)),
        ));
        let screen = window.screen();
        self.update(|state| {
            state.width = rect.width;
            state.height = rect.height;
            state.scale = scale;
            state.fps = screen
                .as_ref()
                .map_or(60.0, |screen| screen.maximumFramesPerSecond() as f64);
            state.headroom = screen.as_ref().map_or(1.0, |screen| {
                screen
                    .maximumExtendedDynamicRangeColorComponentValue()
                    .max(1.0)
            });
        });
        Ok(())
    }

    /// Shows the surface.
    pub fn show(&self) {
        self.view().setHidden(false);
        self.update(|state| state.visible = 1);
    }

    /// Hides the surface; mpv skips rendering while hidden.
    pub fn hide(&self) {
        self.view().setHidden(true);
        self.update(|state| state.visible = 0);
    }

    fn view(&self) -> &NSView {
        // SAFETY: The lease keeps its +1 reference until it drops, after this surface.
        unsafe { &*self.lease.view }
    }

    /// mpv resizes its swapchain when the generation changes.
    fn update(&self, change: impl FnOnce(&mut SurfaceState)) {
        if let Ok(mut state) = self.lease.state.lock() {
            let mut next = *state;
            change(&mut next);
            if next != *state {
                next.generation = state.generation.wrapping_add(1);
                *state = next;
            }
        }
    }
}
