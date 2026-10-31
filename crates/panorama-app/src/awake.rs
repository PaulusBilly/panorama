//! Keeps the display awake while a film plays, so the screensaver and display sleep stay away.

/// Holds a macOS "user initiated, display stays on" activity while switched on.
#[derive(Default)]
pub struct Awake {
    #[cfg(target_os = "macos")]
    activity: Option<
        objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2::runtime::NSObjectProtocol>>,
    >,
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

    // ponytail: macOS only; Windows needs SetThreadExecutionState when someone watches there.
    #[cfg(not(target_os = "macos"))]
    pub fn set(&mut self, _on: bool) {}
}

impl Drop for Awake {
    fn drop(&mut self) {
        self.set(false);
    }
}
