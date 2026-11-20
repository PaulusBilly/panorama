use std::error::Error;
use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use iroh::endpoint::{Connection, PortmapperConfig, RecvStream, SendStream, presets};
use iroh::{Endpoint, EndpointAddr, SecretKey};
use iroh_tickets::endpoint::EndpointTicket;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::time::timeout;

use super::MAX_LINE;

pub const ALPN: &[u8] = b"panorama/party/1";

const INVITE_PREFIX: &str = "panorama://party/";
const ONLINE_TIMEOUT: Duration = Duration::from_secs(15);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Network {
    /// Hole punching, with the n0 relays as fallback and for address lookup.
    Internet,
    /// Relays only: forces the relayed path so it can be tested.
    RelayOnly,
    /// 127.0.0.1 only, no relays, no lookups: for tests that must not touch the network.
    Loopback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportError {
    Bind,
    BadKey,
    Random,
    Offline,
    Connect,
    Timeout,
    Stream,
    InvalidLine,
    LineTooLong,
    Truncated,
    InvalidUtf8,
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Bind => "Could not open the party network endpoint",
            Self::BadKey => "Stored party key is invalid",
            Self::Random => "No randomness available",
            Self::Offline => "Could not reach a relay server",
            Self::Connect => "Could not connect to the party",
            Self::Timeout => "Party connection timed out",
            Self::Stream => "Party connection lost",
            Self::InvalidLine => "Party message contains a newline or is too long",
            Self::LineTooLong => "Party message is too long",
            Self::Truncated => "Party connection closed mid-message",
            Self::InvalidUtf8 => "Party message is not valid text",
        })
    }
}

impl Error for TransportError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InviteError {
    Scheme,
    Shape,
    Ticket,
    Secret,
}

impl fmt::Display for InviteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Scheme => "Not a Panorama party link",
            Self::Shape => "Party link is malformed",
            Self::Ticket => "Party link has an invalid address",
            Self::Secret => "Party link has an invalid secret",
        })
    }
}

impl Error for InviteError {}

#[derive(Clone)]
pub struct PartySecret([u8; 16]);

impl PartySecret {
    pub fn generate() -> Result<Self, TransportError> {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|_| TransportError::Random)?;
        Ok(Self(bytes))
    }

    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Accepts exactly 32 lowercase hex characters.
    pub fn from_hex(hex: &str) -> Option<Self> {
        let digits: &[u8; 32] = hex.as_bytes().try_into().ok()?;
        let mut bytes = [0u8; 16];
        for (byte, [high, low]) in bytes.iter_mut().zip(digits.as_chunks::<2>().0) {
            *byte = (nibble(*high)? << 4) | nibble(*low)?;
        }
        Some(Self(bytes))
    }
}

fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    }
}

impl PartialEq for PartySecret {
    fn eq(&self, other: &Self) -> bool {
        let diff = self
            .0
            .iter()
            .zip(other.0.iter())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b));
        std::hint::black_box(diff) == 0
    }
}

impl Eq for PartySecret {}

impl fmt::Debug for PartySecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PartySecret(..)")
    }
}

#[derive(Clone, Debug)]
pub struct Invite {
    pub addr: EndpointAddr,
    pub secret: PartySecret,
}

pub fn parse_invite(invite: &str) -> Result<Invite, InviteError> {
    let rest = invite
        .trim()
        .strip_prefix(INVITE_PREFIX)
        .ok_or(InviteError::Scheme)?;
    let (ticket, query) = rest.split_once('?').ok_or(InviteError::Shape)?;
    let secret = query.strip_prefix("s=").ok_or(InviteError::Shape)?;
    let ticket = EndpointTicket::from_str(ticket).map_err(|_| InviteError::Ticket)?;
    let secret = PartySecret::from_hex(secret).ok_or(InviteError::Secret)?;
    Ok(Invite {
        addr: ticket.endpoint_addr().clone(),
        secret,
    })
}

/// The relay alone is enough to dial: iroh trades direct addresses over it for hole punching, so
/// the link stays short. Loopback has no relay, so it keeps its addresses.
fn invite_for(addr: EndpointAddr, network: Network, secret: &PartySecret) -> String {
    let addr = match network {
        Network::Loopback => addr,
        Network::Internet | Network::RelayOnly => {
            let relays = addr.relay_urls().cloned().collect::<Vec<_>>();
            relays
                .into_iter()
                .fold(EndpointAddr::new(addr.id), EndpointAddr::with_relay_url)
        }
    };
    format!(
        "{INVITE_PREFIX}{}?s={}",
        EndpointTicket::new(addr),
        secret.to_hex()
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathKind {
    Direct,
    Relayed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathStatus {
    pub kind: PathKind,
    pub rtt: Duration,
}

#[derive(Clone, Debug)]
pub struct PartyEndpoint {
    endpoint: Endpoint,
    network: Network,
}

impl PartyEndpoint {
    /// Binds on the internet. Returns the secret key to persist so the identity survives restarts.
    pub async fn bind(key: Option<[u8; 32]>) -> Result<(Self, [u8; 32]), TransportError> {
        Self::bind_on(key, Network::Internet).await
    }

    pub async fn bind_on(
        key: Option<[u8; 32]>,
        network: Network,
    ) -> Result<(Self, [u8; 32]), TransportError> {
        let key = match key {
            Some(bytes) => SecretKey::from_bytes(&bytes),
            None => SecretKey::generate(),
        };
        let bytes = key.to_bytes();
        let builder = match network {
            Network::Internet => Endpoint::builder(presets::N0),
            Network::RelayOnly => Endpoint::builder(presets::N0).clear_ip_transports(),
            Network::Loopback => Endpoint::builder(presets::Minimal)
                .clear_ip_transports()
                .bind_addr("127.0.0.1:0")
                .map_err(|_| TransportError::Bind)?
                .portmapper_config(PortmapperConfig::Disabled),
        };
        let endpoint = builder
            .secret_key(key)
            .alpns(vec![ALPN.to_vec()])
            .bind()
            .await
            .map_err(|_| TransportError::Bind)?;
        Ok((Self { endpoint, network }, bytes))
    }

    pub fn id(&self) -> String {
        self.endpoint.id().to_string()
    }

    /// Waits for a relay first so the ticket carries an address that works behind NAT.
    pub async fn invite(&self, secret: &PartySecret) -> Result<String, TransportError> {
        if self.network != Network::Loopback {
            timeout(ONLINE_TIMEOUT, self.endpoint.online())
                .await
                .map_err(|_| TransportError::Offline)?;
        }
        Ok(invite_for(self.endpoint.addr(), self.network, secret))
    }

    /// `None` once the endpoint is closed.
    pub async fn accept(&self) -> Option<Result<PartyConn, TransportError>> {
        let incoming = self.endpoint.accept().await?;
        let handshake = async {
            let conn = incoming.await.map_err(|_| TransportError::Connect)?;
            // A QUIC stream only reaches the acceptor once the opener writes to it, so this waits
            // for the member's first line.
            let (send, recv) = conn.accept_bi().await.map_err(|_| TransportError::Stream)?;
            Ok(PartyConn::new(conn, send, recv))
        };
        Some(
            timeout(HANDSHAKE_TIMEOUT, handshake)
                .await
                .unwrap_or(Err(TransportError::Timeout)),
        )
    }

    pub async fn connect(&self, invite: &Invite) -> Result<PartyConn, TransportError> {
        let dial = async {
            let conn = self
                .endpoint
                .connect(invite.addr.clone(), ALPN)
                .await
                .map_err(|_| TransportError::Connect)?;
            let (send, recv) = conn.open_bi().await.map_err(|_| TransportError::Stream)?;
            Ok(PartyConn::new(conn, send, recv))
        };
        timeout(CONNECT_TIMEOUT, dial)
            .await
            .unwrap_or(Err(TransportError::Timeout))
    }

    pub async fn close(&self) {
        self.endpoint.close().await;
    }
}

pub struct PartyConn {
    send: PartySender,
    recv: PartyReceiver,
}

/// The writing half of a [`PartyConn`], which also reports the connection's path.
pub struct PartySender {
    conn: Connection,
    send: SendStream,
}

/// The reading half of a [`PartyConn`].
pub struct PartyReceiver {
    recv: BufReader<RecvStream>,
}

impl PartyConn {
    fn new(conn: Connection, send: SendStream, recv: RecvStream) -> Self {
        Self {
            send: PartySender { conn, send },
            recv: PartyReceiver {
                recv: BufReader::new(recv),
            },
        }
    }

    pub fn remote_id(&self) -> String {
        self.send.conn.remote_id().to_string()
    }

    pub fn path(&self) -> Option<PathStatus> {
        self.send.path()
    }

    pub async fn send(&mut self, line: &str) -> Result<(), TransportError> {
        self.send.send(line).await
    }

    pub async fn recv(&mut self) -> Result<Option<String>, TransportError> {
        self.recv.recv().await
    }

    /// Halves that read and write at the same time.
    pub fn split(self) -> (PartySender, PartyReceiver) {
        (self.send, self.recv)
    }
}

/// Reads a connection's path while its sender is busy elsewhere.
#[derive(Clone)]
pub struct PartyPath(Connection);

impl PartyPath {
    /// The path iroh currently sends on, or `None` while none is selected.
    pub fn get(&self) -> Option<PathStatus> {
        let paths = self.0.paths();
        let path = paths.iter().find(|path| path.is_selected())?;
        let kind = if path.is_relay() {
            PathKind::Relayed
        } else if path.is_ip() {
            PathKind::Direct
        } else {
            return None;
        };
        Some(PathStatus {
            kind,
            rtt: path.rtt(),
        })
    }
}

impl PartySender {
    pub fn path(&self) -> Option<PathStatus> {
        self.path_handle().get()
    }

    pub fn path_handle(&self) -> PartyPath {
        PartyPath(self.conn.clone())
    }

    pub async fn send(&mut self, line: &str) -> Result<(), TransportError> {
        if line.len() > MAX_LINE || line.contains('\n') {
            return Err(TransportError::InvalidLine);
        }
        let mut framed = Vec::with_capacity(line.len() + 1);
        framed.extend_from_slice(line.as_bytes());
        framed.push(b'\n');
        self.send
            .write_all(&framed)
            .await
            .map_err(|_| TransportError::Stream)
    }

    /// Ends the stream and waits, at most `grace`, for the peer to close, so the last lines
    /// arrive before the connection goes.
    pub async fn finish(self, grace: Duration) {
        let Self { conn, mut send } = self;
        let _ = send.finish();
        let _ = timeout(grace, conn.closed()).await;
        conn.close(0u32.into(), b"bye");
    }
}

impl PartyReceiver {
    /// One line without its newline, `None` at a clean end of stream. A line longer than
    /// [`MAX_LINE`] errors before more than `MAX_LINE` bytes are buffered.
    pub async fn recv(&mut self) -> Result<Option<String>, TransportError> {
        let mut line = Vec::new();
        loop {
            let available = self
                .recv
                .fill_buf()
                .await
                .map_err(|_| TransportError::Stream)?;
            if available.is_empty() {
                return if line.is_empty() {
                    Ok(None)
                } else {
                    Err(TransportError::Truncated)
                };
            }
            let newline = available.iter().position(|&b| b == b'\n');
            let take = newline.unwrap_or(available.len());
            if line.len() + take > MAX_LINE {
                return Err(TransportError::LineTooLong);
            }
            line.extend_from_slice(&available[..take]);
            self.recv.consume(take + usize::from(newline.is_some()));
            if newline.is_some() {
                return String::from_utf8(line)
                    .map(Some)
                    .map_err(|_| TransportError::InvalidUtf8);
            }
        }
    }
}

#[cfg(test)]
#[path = "transport_tests.rs"]
mod tests;
