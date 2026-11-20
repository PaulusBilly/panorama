//! Watch party sync, without network or UI. The host is the only authority: members send
//! requests, the host decides the party's timeline and broadcasts it, and each member corrects
//! its own player against that timeline. Sessions are sans-IO reducers driven with `now_ms`.

mod clock;
mod drift;
mod host;
mod member;
mod protocol;
mod release;
pub mod transport;

pub use clock::ClockSync;
pub use drift::{Correction, correct};
pub use host::{ConnId, HostInput, HostOutput, HostSession, HostSettings};
pub use member::{MemberInput, MemberOutput, MemberSession};
pub use protocol::{
    Action, ByeReason, Down, MemberState, MemberView, Notice, NoticeKind, PartyMedia,
    PlaybackState, ProtocolError, Up, decode_down, decode_up, encode,
};
pub use release::{PartyRelease, TorrentRef, find};

pub const PROTOCOL_VERSION: u16 = 1;
/// Including the host.
pub const MAX_MEMBERS: usize = 8;
/// Bytes per line on the wire.
pub const MAX_LINE: usize = 65_536;

#[cfg(test)]
mod tests;
