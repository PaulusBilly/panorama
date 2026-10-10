//! Loopback media transport ported from `desktop/main/media-proxy.ts` and its helpers.

pub mod cache;
pub mod fetch;
pub mod policy;
pub mod proxy;
pub mod range;
pub mod retry;

use std::fmt;

/// Sanitized failures; transport diagnostics never retain source URLs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaError {
    /// Destination is not a credential-free HTTP URL.
    InvalidDestination,
    /// More than ten redirects were returned.
    RedirectLimit,
    /// Network or filesystem operation failed.
    Transport,
    /// No upstream bytes arrived before the watchdog deadline.
    Timeout,
    /// Operation was cancelled.
    Closed,
    /// Cache budgets cannot admit the requested buffer.
    Admission,
    MemoryBudget,
    ChunkTooLarge,
    /// Upstream returned an unexpected status.
    Status(u16),
    /// Representation integrity failed.
    Representation(&'static str),
    /// Caller could not refresh the source.
    ResolverFailed,
    /// Refresh budget was exhausted.
    ResolveLimit,
}

impl fmt::Display for MediaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDestination => f.write_str("Invalid media destination"),
            Self::RedirectLimit => f.write_str("Media redirect limit exceeded"),
            Self::Transport => f.write_str("Media transfer failed"),
            Self::Timeout => f.write_str("Media request timed out"),
            Self::Closed => f.write_str("Media request aborted"),
            Self::Admission => f.write_str("Media cache admission failed"),
            Self::MemoryBudget => {
                f.write_str("Media cache memory budget must be at least 2097152 bytes")
            }
            Self::ChunkTooLarge => f.write_str("Media chunk exceeds cache memory budget"),
            Self::Status(status) => write!(f, "Unexpected media response {status}"),
            Self::Representation(reason) => f.write_str(reason),
            Self::ResolverFailed => f.write_str("Media resolver failed"),
            Self::ResolveLimit => f.write_str("Media resolution limit exceeded"),
        }
    }
}

impl std::error::Error for MediaError {}

pub(crate) fn lock<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(crate) fn token() -> Result<String, MediaError> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| MediaError::Transport)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}
