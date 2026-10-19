use super::*;

fn parent() -> HWND {
    // SAFETY: The built-in class and arguments are valid; this test owns the window.
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("STATIC"),
            w!(""),
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            1,
            1,
            None,
            None,
            None,
            None,
        )
        .unwrap()
    }
}

#[test]
fn parent_destruction_prevents_stale_lease_from_destroying_replacement() {
    let parent = parent();
    let mut surface = VideoSurface::create(parent).unwrap();
    // SAFETY: This test owns the parent on its creation thread.
    unsafe { DestroyWindow(parent) }.unwrap();
    assert!(surface.lease.state.destroyed.load(Ordering::Acquire));
    assert_eq!(Arc::strong_count(&surface.lease.state), 1);
    let replacement = self::parent();
    Arc::get_mut(&mut surface.lease).unwrap().id = replacement.0 as usize;
    drop(surface);
    // SAFETY: Win32 validates the opaque handle; this test owns the replacement.
    unsafe {
        let survived = IsWindow(Some(replacement)).as_bool();
        if survived {
            DestroyWindow(replacement).unwrap();
        }
        assert!(survived, "stale lease destroyed the replacement window");
    }
}

#[test]
fn queued_destruction_rejects_a_previous_lease_token() {
    let parent = parent();
    let first = VideoSurface::create(parent).unwrap();
    let token = first.lease.state.token;
    drop(first);
    let replacement = VideoSurface::create(parent).unwrap();
    // SAFETY: Both HWNDs belong to this test thread; the private message has no pointers.
    unsafe {
        SendMessageW(
            replacement.hwnd(),
            DESTROY_SURFACE,
            Some(WPARAM(token)),
            Some(LPARAM(0)),
        );
        assert!(IsWindow(Some(replacement.hwnd())).as_bool());
    }
    drop(replacement);
    // SAFETY: This test owns the parent on its creation thread.
    unsafe { DestroyWindow(parent) }.unwrap();
}

#[test]
fn off_thread_final_release_destroys_the_live_child_on_dispatch() {
    let parent = parent();
    let surface = VideoSurface::create(parent).unwrap();
    let hwnd = surface.hwnd();
    let state = surface.lease.state.clone();
    let retained = surface.raw_window();
    drop(surface);
    std::thread::spawn(move || drop(retained)).join().unwrap();
    // SAFETY: The child and its message queue belong to this test thread.
    unsafe {
        assert!(IsWindow(Some(hwnd)).as_bool());
        let mut message = MSG::default();
        assert!(
            PeekMessageW(
                &mut message,
                Some(hwnd),
                DESTROY_SURFACE,
                DESTROY_SURFACE,
                PM_REMOVE,
            )
            .as_bool()
        );
        DispatchMessageW(&message);
        assert!(!IsWindow(Some(hwnd)).as_bool());
        DestroyWindow(parent).unwrap();
    }
    assert!(state.destroyed.load(Ordering::Acquire));
    assert_eq!(Arc::strong_count(&state), 1);
}
