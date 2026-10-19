use std::fmt;

/// Sanitized failure kinds. No input, path, URL, or libmpv log text is retained.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MpvError {
    /// This platform has no libmpv host yet.
    UnsupportedPlatform,
    /// The explicit or executable runtime directory is unavailable.
    LibraryUnavailable,
    /// The runtime is missing a required C symbol.
    MissingSymbol,
    /// Only client API major version 2 is supported.
    IncompatibleApi,
    /// libmpv could not allocate a handle.
    CreateFailed,
    /// An argument contains NUL or is outside the accepted range.
    InvalidArgument,
    /// The option is not supported by the runtime.
    Option,
    /// The requested property is unavailable or not writable.
    Property,
    /// The command could not be executed.
    Command,
    /// The media could not be opened.
    Loading,
    /// The media container could not be read.
    Demuxer,
    /// No usable audio or video was found.
    NothingToPlay,
    /// An audio or video output failed.
    Output,
    /// Initialization or another runtime operation failed.
    Runtime,
    /// The player is closing or its command receiver has gone away.
    Closed,
    /// The single event receiver was already taken.
    EventsTaken,
    /// A worker could not be started or panicked.
    Thread,
    /// Teardown exceeded the bounded wait; resources remain with its worker.
    ShutdownTimeout,
    /// Win32 rejected a surface operation.
    Window,
}

impl MpvError {
    pub(crate) fn from_code(code: i32) -> Self {
        match code {
            -4 => Self::InvalidArgument,
            -7..=-5 => Self::Option,
            -11..=-8 => Self::Property,
            -12 => Self::Command,
            -13 => Self::Loading,
            -15..=-14 => Self::Output,
            -16 => Self::NothingToPlay,
            -17 => Self::Demuxer,
            _ => Self::Runtime,
        }
    }
}

impl fmt::Display for MpvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::UnsupportedPlatform => "Playback is not supported on this platform.",
            Self::LibraryUnavailable => "The packaged playback runtime is unavailable.",
            Self::MissingSymbol => "The playback runtime is incomplete.",
            Self::IncompatibleApi => "The playback runtime API is incompatible.",
            Self::CreateFailed => "Unable to create the player.",
            Self::InvalidArgument => "Invalid player argument.",
            Self::Option => "Unable to configure playback.",
            Self::Property => "Playback property is unavailable.",
            Self::Command => "Playback command failed.",
            Self::Loading => "Unable to open media.",
            Self::Demuxer => "Unable to read media.",
            Self::NothingToPlay => "No playable media was found.",
            Self::Output => "Playback output failed.",
            Self::Runtime => "Playback runtime failed.",
            Self::Closed => "The player is closed.",
            Self::EventsTaken => "The player event receiver was already taken.",
            Self::Thread => "Playback worker failed.",
            Self::ShutdownTimeout => "Playback teardown exceeded its deadline.",
            Self::Window => "Unable to update the video surface.",
        })
    }
}

impl std::error::Error for MpvError {}

pub(crate) fn check(code: i32) -> Result<(), MpvError> {
    if code < 0 {
        Err(MpvError::from_code(code))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_are_static_and_redacted() {
        for code in -100..=0 {
            let error = MpvError::from_code(code);
            let text = format!("{error}: {error:?}");
            assert!(!text.contains("http"));
            assert!(!text.contains("token"));
            assert!(!text.contains("127.0.0.1"));
        }
        assert_eq!(MpvError::from_code(-13), MpvError::Loading);
        assert_eq!(MpvError::from_code(-15), MpvError::Output);
        assert_eq!(MpvError::from_code(-17), MpvError::Demuxer);
        assert!(check(0).is_ok());
    }
}
