//! Discord Rich Presence over the local Discord IPC socket.
//!
//! Port of `desktop/main/discord-presence.ts` and `desktop/shared/discord-presence.ts`.
//! [`DiscordPresence`] is an opt-in, UI-free handle: playback updates are coalesced
//! and throttled to one `SET_ACTIVITY` per 5 s, the connection walks
//! `discord-ipc-0..9` and retries every 15 s, and every failure is silent (Discord
//! simply not running just means no presence). Film titles are user-visible data,
//! never secrets; nothing else about the account is sent.

mod ipc;
mod payload;
mod presence;

#[cfg(test)]
mod tests;

pub use ipc::{
    APPLICATION_ID, Connector, FrameError, FrameReader, Io, Platform, SystemConnector,
    discord_frame, discord_paths,
};
pub use payload::{
    DiscordPlayback, InvalidPlayback, PlaybackState, discord_activity, discord_artwork,
    parse_discord_playback,
};
pub use presence::{DiscordPresence, DiscordSettings};
