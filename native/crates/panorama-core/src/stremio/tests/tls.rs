use std::sync::{Arc, Mutex};

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
    task: JoinHandle<()>,
}

impl TlsMock {
    pub(super) async fn start() -> Self {
        let certified = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
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
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    incoming = listener.accept() => {
                        let (socket, _) = incoming.unwrap();
                        let acceptor = acceptor.clone();
                        let requests = Arc::clone(&worker_requests);
                        let redirect = Arc::clone(&worker_redirect);
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
                                None => "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}".into(),
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
            task,
        }
    }
}

impl Drop for TlsMock {
    fn drop(&mut self) {
        self.task.abort();
    }
}
