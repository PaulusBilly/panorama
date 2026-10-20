//! Non-blocking libmpv host with owned Windows and macOS video surfaces.
#![deny(unsafe_op_in_unsafe_fn)]

mod error;
mod events;
#[allow(unsafe_code)]
mod ffi;
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod macos;
mod options;
mod player;
mod subtitle;
mod types;
/// Windows surfaces.
#[cfg(windows)]
#[allow(unsafe_code)]
pub mod win32;

pub use error::MpvError;
#[cfg(target_os = "macos")]
pub use macos::VideoSurface;
pub use player::{EXTRA_OPTIONS, Mpv, MpvLibrary, PROPERTIES, Player};
pub use subtitle::SubtitleStyle;
pub use types::*;
#[cfg(windows)]
pub use win32::VideoSurface;
