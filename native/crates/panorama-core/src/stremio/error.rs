use std::{error::Error, fmt};

use stremio_core::{models::ctx::CtxError, runtime::EnvError};

/// Sanitized failure categories; no upstream diagnostics or payloads are retained.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoreErrorKind {
    /// Login credentials were rejected by the API.
    WrongCredentials,
    /// A transport request failed.
    Network,
    /// A request or sign-in exceeded its deadline.
    Timeout,
    /// Reading, writing, migration or secure sign-out failed.
    Storage,
    /// The process Env has already been installed.
    AlreadyInstalled,
    /// The Env configuration is invalid or has not been installed.
    Environment,
    /// Another core failure, including incomplete account collection fetches.
    Other,
}

/// An error containing only a sanitized category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CoreError {
    /// The category that callers may display or handle.
    pub kind: CoreErrorKind,
}

impl From<CoreErrorKind> for CoreError {
    fn from(kind: CoreErrorKind) -> Self {
        Self { kind }
    }
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self.kind {
            CoreErrorKind::WrongCredentials => "credentials rejected",
            CoreErrorKind::Network => "network request failed",
            CoreErrorKind::Timeout => "operation timed out",
            CoreErrorKind::Storage => "core storage failed",
            CoreErrorKind::AlreadyInstalled => "core environment already installed",
            CoreErrorKind::Environment => "core environment unavailable or invalid",
            CoreErrorKind::Other => "core operation failed",
        })
    }
}

impl Error for CoreError {}

pub(super) fn classify(error: &CtxError) -> CoreErrorKind {
    match error {
        CtxError::API(error) if matches!(error.code, 1 | 2) => CoreErrorKind::WrongCredentials,
        CtxError::Env(EnvError::Fetch(message)) if message == "request timed out" => {
            CoreErrorKind::Timeout
        }
        CtxError::Env(EnvError::Fetch(_)) => CoreErrorKind::Network,
        CtxError::Env(
            EnvError::StorageUnavailable
            | EnvError::StorageReadError(_)
            | EnvError::StorageWriteError(_)
            | EnvError::StorageSchemaVersionDowngrade(..)
            | EnvError::StorageSchemaVersionUpgrade(_),
        ) => CoreErrorKind::Storage,
        _ => CoreErrorKind::Other,
    }
}
