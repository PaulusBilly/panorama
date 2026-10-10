use std::{error::Error, fmt, io};

use rusqlite::ErrorCode;

/// Store failures, without stored values or SQLite diagnostic messages.
#[derive(Debug)]
pub enum StoreError {
    /// A key component failed validation; the component itself is not retained.
    InvalidKey {
        /// A static description of the validation failure.
        reason: &'static str,
    },
    /// A value exceeded the maximum accepted byte length.
    ValueTooLarge {
        /// The rejected byte length.
        size: usize,
        /// The maximum accepted byte length.
        limit: usize,
    },
    /// The database was written by a newer build and has been left untouched.
    NewerSchema {
        /// The version found in the database.
        found: i64,
        /// The highest version supported by this build.
        supported: i64,
    },
    /// A filesystem operation failed.
    Io(io::Error),
    /// SQLite failed; only numeric/category information is retained.
    Sqlite {
        /// SQLite's primary error category, when available.
        code: Option<ErrorCode>,
        /// SQLite's extended numeric error code, when available.
        extended_code: Option<i32>,
    },
    /// SQLite's integrity check reported corruption.
    Corrupt,
    /// Another connection prevented the sign-out WAL from being truncated.
    CheckpointBusy,
    /// Another `Store` (in this or another process) already has this database open.
    Locked,
}

impl StoreError {
    pub(super) fn is_corruption(&self) -> bool {
        matches!(
            self,
            Self::Corrupt
                | Self::Sqlite {
                    code: Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase),
                    ..
                }
        )
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidKey { reason } => write!(f, "invalid storage key: {reason}"),
            Self::ValueTooLarge { size, limit } => {
                write!(f, "storage value is {size} bytes; limit is {limit}")
            }
            Self::NewerSchema { found, supported } => {
                write!(
                    f,
                    "database schema {found} is newer than supported schema {supported}"
                )
            }
            Self::Io(error) => write!(f, "store filesystem error: {error}"),
            Self::Sqlite {
                code,
                extended_code,
            } => {
                write!(f, "store SQLite error: {code:?} ({extended_code:?})")
            }
            Self::Corrupt => f.write_str("database failed its integrity check"),
            Self::CheckpointBusy => {
                f.write_str("sign-out WAL truncation was blocked by another connection")
            }
            Self::Locked => f.write_str("the database is already open in another store"),
        }
    }
}

impl Error for StoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for StoreError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        match error {
            rusqlite::Error::SqliteFailure(error, _) => Self::Sqlite {
                code: Some(error.code),
                extended_code: Some(error.extended_code),
            },
            _ => Self::Sqlite {
                code: None,
                extended_code: None,
            },
        }
    }
}
