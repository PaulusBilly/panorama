//! Non-blocking libmpv host with owned Windows video surfaces.
#![deny(unsafe_op_in_unsafe_fn)]

mod error;
mod events;
#[allow(unsafe_code)]
mod ffi;
mod options;
mod player;
mod subtitle;
mod types;
/// Windows surfaces; unavailable until Phase M on other platforms.
#[cfg(windows)]
#[allow(unsafe_code)]
pub mod win32;

pub use error::MpvError;
pub use player::{Mpv, MpvLibrary, Player};
pub use subtitle::SubtitleStyle;
pub use types::*;
