//! Persisted account sessions backed by the Stremio core.
//!
//! Install [`env::PanoramaEnv`] once before starting a session. Only one live
//! session uses the process-wide Env; starting another waits for its release.
//! Dropping a session cancels concurrent effects and drains queued persistence
//! in the background. A subsequent start waits for this cleanup.
//! Tests swapping Env state must hold `env::TEST_MUTEX` for their entire lifetime,
//! including all sessions and runtime cleanup.

pub mod env;
mod error;
mod events;
mod fetch;
mod manage;
mod migration;
mod model;
mod preferences;
mod session;
mod types;

pub use error::{CoreError, CoreErrorKind};
pub use session::CoreSession;
pub use types::{CoreChange, Descriptor, Profile};

#[cfg(test)]
mod tests;
