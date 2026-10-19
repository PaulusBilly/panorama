use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::{JoinHandle, JoinSet},
};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{ServerConfig, crypto::ring, pki_types::PrivatePkcs8KeyDer},
};
use url::Url;

pub(super) struct TlsMock {
    pub(super) base: Url,
    pub(super) certificate: reqwest::Certificate,
    pub(super) requests: Arc<Mutex<Vec<String>>>,
    pub(super) redirect: Arc<Mutex<Option<Url>>>,
    pub(super) body: Arc<Mutex<Vec<u8>>>,
    pub(super) chunked: Arc<AtomicBool>,
    task: JoinHandle<()>,
}

impl TlsMock {
    pub(super) async fn start() -> Self {
        let certified =
            rcgen::generate_simple_self_signed(vec!["127.0.0.1".into(), "addon.test".into()])
                .unwrap();
        let certificate = reqwest::Certificate::from_der(certified.cert.der()).unwrap();
        let config = ServerConfig::builder_with_provider(Arc::new(ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![certified.cert.der().clone()],
                PrivatePkcs8KeyDer::from(certified.signing_key.serialize_der()).into(),
            )
            .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("https://{}/", listener.local_addr().unwrap())
            .parse()
            .unwrap();
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let redirect: Arc<Mutex<Option<Url>>> = Arc::new(Mutex::new(None));
        let worker_requests = Arc::clone(&requests);
        let worker_redirect = Arc::clone(&redirect);
        let body = Arc::new(Mutex::new(b"{\"ok\":true}".to_vec()));
        let worker_body = Arc::clone(&body);
        let chunked = Arc::new(AtomicBool::new(false));
        let worker_chunked = Arc::clone(&chunked);
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    incoming = listener.accept() => {
                        let (socket, _) = incoming.unwrap();
                        let acceptor = acceptor.clone();
                        let requests = Arc::clone(&worker_requests);
                        let redirect = Arc::clone(&worker_redirect);
                        let body = Arc::clone(&worker_body);
                        let chunked = Arc::clone(&worker_chunked);
                        connections.spawn(async move {
                            let Ok(mut socket) = acceptor.accept(socket).await else {
                                return;
                            };
                            let mut request = Vec::new();
                            let mut buffer = [0; 1024];
                            while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                                let read = socket.read(&mut buffer).await.unwrap();
                                if read == 0 {
                                    return;
                                }
                                request.extend_from_slice(&buffer[..read]);
                                assert!(request.len() < 64 * 1024);
                            }
                            requests.lock().unwrap().push(String::from_utf8(request).unwrap());
                            let target = redirect.lock().unwrap().clone();
                            let response = match target {
                                Some(target) => format!(
                                    "HTTP/1.1 307 Temporary Redirect\r\nLocation: {target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                                ),
                                None => {
                                    let body = body.lock().unwrap();
                                    if chunked.load(Ordering::Relaxed) {
                                        format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{}\r\n0\r\n\r\n", body.len(), String::from_utf8_lossy(&body))
                                    } else {
                                        format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), String::from_utf8_lossy(&body))
                                    }
                                },
                            };
                            let _ = socket.write_all(response.as_bytes()).await;
                            let _ = socket.shutdown().await;
                        });
                    }
                    result = connections.join_next(), if !connections.is_empty() => {
                        result.unwrap().unwrap();
                    }
                }
            }
        });
        Self {
            base,
            certificate,
            requests,
            redirect,
            body,
            chunked,
            task,
        }
    }
}

impl Drop for TlsMock {
    fn drop(&mut self) {
        self.task.abort();
    }
}
