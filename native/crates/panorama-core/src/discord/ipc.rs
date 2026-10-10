use std::{fmt, io};

use futures::future::BoxFuture;
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncWrite};

/// The Discord application ID, identical to `applicationId` in the TS main module.
pub const APPLICATION_ID: &str = "1549772711264395274";

const MAX_BODY: usize = 65536;

/// A bidirectional byte stream to Discord (a named pipe, Unix socket or test fake).
pub trait Io: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> Io for T {}

/// Opens the stream for one candidate path; failures are expected and silent.
pub trait Connector: Send + Sync + 'static {
    /// Connects to `path` (one of [`discord_paths`]).
    fn connect(&self, path: &str) -> BoxFuture<'static, io::Result<Box<dyn Io>>>;
}

/// The real transport: `\\?\pipe\discord-ipc-N` on Windows, Unix sockets elsewhere.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemConnector;

impl Connector for SystemConnector {
    fn connect(&self, path: &str) -> BoxFuture<'static, io::Result<Box<dyn Io>>> {
        let path = path.to_owned();
        Box::pin(async move {
            #[cfg(windows)]
            {
                let pipe = tokio::net::windows::named_pipe::ClientOptions::new().open(&path)?;
                Ok(Box::new(pipe) as Box<dyn Io>)
            }
            #[cfg(unix)]
            {
                Ok(Box::new(tokio::net::UnixStream::connect(&path).await?) as Box<dyn Io>)
            }
            #[cfg(not(any(windows, unix)))]
            {
                let _ = path;
                Err(io::Error::from(io::ErrorKind::Unsupported))
            }
        })
    }
}

/// Which path convention to use.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Platform {
    /// Windows named pipes.
    Windows,
    /// Unix sockets in the runtime/temp directory.
    Unix,
}

impl Platform {
    /// The platform this build runs on.
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Unix
        }
    }
}

/// The ten candidate IPC paths in connection order. `env` looks up an environment
/// variable; empty values are skipped like JavaScript's `||`.
pub fn discord_paths(platform: Platform, env: impl Fn(&str) -> Option<String>) -> Vec<String> {
    let prefix = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"]
        .into_iter()
        .find_map(|name| env(name).filter(|value| !value.is_empty()))
        .unwrap_or_else(|| "/tmp".into());
    (0..10)
        .map(|index| match platform {
            Platform::Windows => format!(r"\\?\pipe\discord-ipc-{index}"),
            Platform::Unix => format!("{}/discord-ipc-{index}", prefix.trim_end_matches('/')),
        })
        .collect()
}

/// Encodes one frame: little-endian opcode and length, then the raw body.
pub(super) fn raw_frame(opcode: u32, body: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(8 + body.len());
    frame.extend_from_slice(&opcode.to_le_bytes());
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(body);
    frame
}

/// Encodes `value` as a JSON frame with `opcode`.
pub fn discord_frame(opcode: u32, value: &Value) -> Vec<u8> {
    raw_frame(opcode, value.to_string().as_bytes())
}

/// A frame header declared a body larger than 64 KiB.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameError;

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Oversized Discord frame")
    }
}

impl std::error::Error for FrameError {}

/// Reassembles frames from arbitrarily split reads.
#[derive(Debug, Default)]
pub struct FrameReader {
    buffer: Vec<u8>,
}

impl FrameReader {
    /// Appends received bytes.
    pub fn push(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
    }

    /// The next complete `(opcode, body)`, `None` while incomplete, or an error
    /// as soon as a header declares an oversized body.
    pub fn next_frame(&mut self) -> Result<Option<(u32, Vec<u8>)>, FrameError> {
        let Some(header) = self.buffer.first_chunk::<8>() else {
            return Ok(None);
        };
        let opcode = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
        let length = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
        if length > MAX_BODY {
            return Err(FrameError);
        }
        if self.buffer.len() < 8 + length {
            return Ok(None);
        }
        let body = self.buffer[8..8 + length].to_vec();
        self.buffer.drain(..8 + length);
        Ok(Some((opcode, body)))
    }
}
