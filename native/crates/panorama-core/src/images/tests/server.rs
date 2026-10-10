use std::{
    convert::Infallible,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use axum::{
    Router,
    body::{Body, Bytes},
    extract::State,
    http::{HeaderMap, Response, StatusCode, Uri},
    routing::get,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{Notify, Semaphore},
    task::JoinHandle,
};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{
        self,
        pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer},
    },
    server::TlsStream,
};

use super::{ImageFormat, ImageLoader, ImageLoaderOptions, encoded};

pub(super) struct Counts {
    pub connections: AtomicUsize,
    pub requests: AtomicUsize,
    pub active: AtomicUsize,
    pub peak: AtomicUsize,
    pub entered: Notify,
    pub release: Semaphore,
}

impl Default for Counts {
    fn default() -> Self {
        Self {
            connections: AtomicUsize::new(0),
            requests: AtomicUsize::new(0),
            active: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            entered: Notify::new(),
            release: Semaphore::new(0),
        }
    }
}

pub(super) struct Server {
    address: SocketAddr,
    cert: reqwest::Certificate,
    pub counts: Arc<Counts>,
    task: JoinHandle<()>,
}

impl Server {
    pub async fn start() -> Self {
        let rcgen::CertifiedKey { cert, signing_key } =
            rcgen::generate_simple_self_signed(vec!["localhost".into(), "127.0.0.1".into()])
                .unwrap();
        let tls = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.der().clone()],
            PrivateKeyDer::from(PrivatePkcs8KeyDer::from(signing_key.serialize_der())),
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let counts = Arc::new(Counts::default());
        let router = Router::new()
            .fallback(get(handler))
            .with_state(Arc::clone(&counts));
        let listener = TlsListener {
            tcp: listener,
            acceptor: TlsAcceptor::from(Arc::new(tls)),
            counts: Arc::clone(&counts),
        };
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self {
            address,
            cert: reqwest::Certificate::from_der(cert.der()).unwrap(),
            counts,
            task,
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("https://localhost:{}{path}", self.address.port())
    }

    pub fn loader(&self, options: ImageLoaderOptions) -> ImageLoader {
        let client = super::super::loader::client_builder()
            .no_proxy()
            .resolve("localhost", self.address)
            .add_root_certificate(self.cert.clone())
            .build()
            .unwrap();
        ImageLoader::with_client(options, client).unwrap()
    }

    pub fn requests(&self) -> usize {
        self.counts.requests.load(Ordering::SeqCst)
    }

    pub fn connections(&self) -> usize {
        self.counts.connections.load(Ordering::SeqCst)
    }

    pub async fn wait_requests(&self, count: usize) {
        loop {
            let notified = self.counts.entered.notified();
            if self.requests() >= count {
                return;
            }
            notified.await;
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct TlsListener {
    tcp: TcpListener,
    acceptor: TlsAcceptor,
    counts: Arc<Counts>,
}

impl axum::serve::Listener for TlsListener {
    type Io = TlsStream<TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            let (stream, address) = self.tcp.accept().await.unwrap();
            self.counts.connections.fetch_add(1, Ordering::SeqCst);
            if let Ok(stream) = self.acceptor.accept(stream).await {
                return (stream, address);
            }
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        self.tcp.local_addr()
    }
}

struct Active(Arc<Counts>);

impl Drop for Active {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
    }
}

async fn handler(
    State(counts): State<Arc<Counts>>,
    headers: HeaderMap,
    uri: Uri,
) -> Response<Body> {
    counts.requests.fetch_add(1, Ordering::SeqCst);
    let active = counts.active.fetch_add(1, Ordering::SeqCst) + 1;
    counts.peak.fetch_max(active, Ordering::SeqCst);
    let _active = Active(Arc::clone(&counts));
    counts.entered.notify_waiters();
    let path = uri.path();
    if path == "/loopback" {
        return Response::builder()
            .status(StatusCode::FOUND)
            .header(
                "location",
                format!(
                    "https://{}/png",
                    headers["host"]
                        .to_str()
                        .unwrap()
                        .replace("localhost", "127.0.0.1")
                ),
            )
            .body(Body::empty())
            .unwrap();
    }
    if path.starts_with("/slow") {
        counts.release.acquire().await.unwrap().forget();
    }
    if path == "/http" {
        return Response::builder()
            .status(StatusCode::FOUND)
            .header("location", "http://127.0.0.1:1/private")
            .body(Body::empty())
            .unwrap();
    }
    if let Some(number) = path.strip_prefix("/redirect/") {
        let number: u32 = number.parse().unwrap();
        if number > 0 {
            return Response::builder()
                .status(StatusCode::FOUND)
                .header("location", format!("/redirect/{}", number - 1))
                .body(Body::empty())
                .unwrap();
        }
    }
    if path == "/chunked" {
        let chunks =
            futures::stream::iter((0..32).map(|_| Ok::<_, Infallible>(Bytes::from(vec![0; 512]))));
        return Response::builder()
            .header("content-type", "image/png")
            .body(Body::from_stream(chunks))
            .unwrap();
    }
    let (status, body) = match path {
        "/jpeg" => (StatusCode::OK, encoded(ImageFormat::Jpeg, 80, 40)),
        "/webp" => (StatusCode::OK, encoded(ImageFormat::WebP, 80, 40)),
        "/small" => (StatusCode::OK, encoded(ImageFormat::Png, 4, 2)),
        "/large" => (StatusCode::OK, vec![0; 16384]),
        "/unsupported" => (StatusCode::OK, b"not an image".to_vec()),
        "/decode" => (StatusCode::OK, b"\x89PNG\r\n\x1a\ntruncated".to_vec()),
        "/huge" => (StatusCode::OK, huge_png()),
        "/status" => (StatusCode::NOT_FOUND, Vec::new()),
        _ => (StatusCode::OK, encoded(ImageFormat::Png, 80, 40)),
    };
    Response::builder()
        .status(status)
        .header("content-type", "image/png")
        .body(Body::from(body))
        .unwrap()
}

fn huge_png() -> Vec<u8> {
    let mut bytes = encoded(ImageFormat::Png, 1, 1);
    bytes[16..20].copy_from_slice(&20000u32.to_be_bytes());
    bytes[20..24].copy_from_slice(&20000u32.to_be_bytes());
    let mut crc = !0u32;
    for byte in &bytes[12..29] {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    bytes[29..33].copy_from_slice(&(!crc).to_be_bytes());
    bytes
}
