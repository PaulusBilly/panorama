//! Keeps the display awake while a film plays, so the screensaver and display sleep stay away.

/// Holds a macOS "user initiated, display stays on" activity, or the Windows display-required
/// execution state, while switched on.
#[derive(Default)]
pub struct Awake {
    #[cfg(target_os = "macos")]
    activity: Option<
        objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2::runtime::NSObjectProtocol>>,
    >,
    #[cfg(target_os = "windows")]
    on: bool,
}

impl Awake {
    #[cfg(target_os = "macos")]
    pub fn set(&mut self, on: bool) {
        use objc2_foundation::{NSActivityOptions, NSProcessInfo, ns_string};
        if on == self.activity.is_some() {
            return;
        }
        let info = NSProcessInfo::processInfo();
        match self.activity.take() {
            // SAFETY: the token came from this process's `beginActivityWithOptions_reason`.
            Some(activity) => unsafe { info.endActivity(&activity) },
            None => {
                self.activity = Some(info.beginActivityWithOptions_reason(
                    NSActivityOptions::UserInitiated | NSActivityOptions::IdleDisplaySleepDisabled,
                    ns_string!("Playing a film"),
                ));
            }
        }
    }

    /// The execution state belongs to the calling thread, so `set` must stay on the UI thread.
    #[cfg(target_os = "windows")]
    pub fn set(&mut self, on: bool) {
        use windows::Win32::System::Power::{
            ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED, SetThreadExecutionState,
        };
        if on == self.on {
            return;
        }
        let flags = if on {
            ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED
        } else {
            ES_CONTINUOUS
        };
        // SAFETY: takes flags by value and only changes this thread's power request.
        unsafe { SetThreadExecutionState(flags) };
        self.on = on;
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    pub fn set(&mut self, _on: bool) {}
}

impl Drop for Awake {
    fn drop(&mut self) {
        self.set(false);
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::Awake;
    use windows::Win32::System::Power::{
        ES_CONTINUOUS, ES_DISPLAY_REQUIRED, EXECUTION_STATE, SetThreadExecutionState,
    };

    /// Reads this thread's execution state by setting it to itself.
    fn current() -> EXECUTION_STATE {
        // SAFETY: as in `Awake::set`; the second call restores what the first returned.
        unsafe {
            let state = SetThreadExecutionState(ES_CONTINUOUS);
            SetThreadExecutionState(state);
            state
        }
    }

    #[test]
    fn playback_holds_the_display_on_and_releases_it() {
        let mut awake = Awake::default();
        awake.set(true);
        assert!(
            current().contains(ES_DISPLAY_REQUIRED),
            "display held while playing"
        );
        awake.set(false);
        assert!(
            !current().contains(ES_DISPLAY_REQUIRED),
            "released when paused"
        );
        awake.set(true);
        drop(awake);
        assert!(!current().contains(ES_DISPLAY_REQUIRED), "released on drop");
    }
}
