use std::{
    io::Cursor,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use serde_json::{Value, json};
use tiny_http::{Header, Response, Server, StatusCode};
use url::Url;

pub(super) const AUTH_KEY: &str = "PANORAMA_MOCK_AUTH_KEY_274519628";
pub(super) const EMAIL: &str = "panorama@example.test";
pub(super) const PASSWORD: &str = "mock-password-267198";

pub(super) struct MockApi {
    pub(super) base: Url,
    stop: Arc<AtomicBool>,
    pub(super) reject_addons: Arc<AtomicBool>,
    pub(super) login_error: Arc<AtomicU64>,
    pub(super) network_error: Arc<AtomicBool>,
    pub(super) stall_login: Arc<AtomicBool>,
    pub(super) released_logins: Arc<AtomicUsize>,
    pub(super) stall_logout: Arc<AtomicBool>,
    pub(super) reject_logout: Arc<AtomicBool>,
    pub(super) redirect_target: Arc<Mutex<Option<Url>>>,
    pub(super) calls: Arc<Mutex<Vec<String>>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl MockApi {
    pub(super) fn start() -> Self {
        let server = Server::http("127.0.0.1:0").unwrap();
        let base = format!("http://{}/", server.server_addr()).parse().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let reject_addons = Arc::new(AtomicBool::new(false));
        let login_error = Arc::new(AtomicU64::new(0));
        let network_error = Arc::new(AtomicBool::new(false));
        let stall_login = Arc::new(AtomicBool::new(false));
        let worker_stall = stall_login.clone();
        let released_logins = Arc::new(AtomicUsize::new(0));
        let worker_released = released_logins.clone();
        let stall_logout = Arc::new(AtomicBool::new(false));
        let worker_stall_logout = stall_logout.clone();
        let reject_logout = Arc::new(AtomicBool::new(false));
        let worker_reject_logout = reject_logout.clone();
        let redirect_target: Arc<Mutex<Option<Url>>> = Arc::new(Mutex::new(None));
        let worker_redirect = redirect_target.clone();
        let calls = Arc::new(Mutex::new(vec![]));
        let (worker_stop, worker_reject, worker_login, worker_network, worker_calls) = (
            stop.clone(),
            reject_addons.clone(),
            login_error.clone(),
            network_error.clone(),
            calls.clone(),
        );
        let worker = thread::spawn(move || {
            let mut stalled: Vec<tiny_http::Request> = vec![];
            while !worker_stop.load(Ordering::Relaxed) {
                let released = stalled.iter().position(|request| match request.url() {
                    "/api/login" => !worker_stall.load(Ordering::Relaxed),
                    "/api/logout" => !worker_stall_logout.load(Ordering::Relaxed),
                    _ => false,
                });
                let mut request = match released {
                    Some(index) => stalled.remove(index),
                    None => match server.recv_timeout(Duration::from_millis(2)).unwrap() {
                        Some(request) => request,
                        None => continue,
                    },
                };
                let path = request.url().to_owned();
                if released.is_none() {
                    worker_calls.lock().unwrap().push(path.clone());
                }
                if (path == "/api/login" && worker_stall.load(Ordering::Relaxed))
                    || (path == "/api/logout" && worker_stall_logout.load(Ordering::Relaxed))
                {
                    stalled.push(request);
                    continue;
                }
                if path.starts_with("/api/oversized") {
                    let declared =
                        (path == "/api/oversized-declared").then_some(8 * 1024 * 1024 + 1);
                    let response = Response::new(
                        StatusCode(200),
                        vec![],
                        Cursor::new(vec![b'x'; 8 * 1024 * 1024 + 1]),
                        declared,
                        None,
                    );
                    let _ = request.respond(response);
                    continue;
                }
                if path == "/api/redirect-http" || path == "/api/redirect-origin" {
                    let target = if let Some(target) = &*worker_redirect.lock().unwrap() {
                        target.to_string()
                    } else if path.ends_with("http") {
                        "http://127.0.0.1:1/secret-path?token=mock-token".into()
                    } else {
                        "https://127.0.0.1:1/secret-path?token=mock-token".into()
                    };
                    let response = Response::empty(307)
                        .with_header(Header::from_bytes("Location", target).unwrap());
                    let _ = request.respond(response);
                    continue;
                }
                if path == "/api/login" && worker_network.load(Ordering::Relaxed) {
                    let _ = request.respond(Response::empty(503));
                    continue;
                }
                let mut body = String::new();
                request.as_reader().read_to_string(&mut body).unwrap();
                let body: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                let reply = match path.as_str() {
                    "/api/login" if worker_login.load(Ordering::Relaxed) != 0 => {
                        json!({"error": {"code": worker_login.load(Ordering::Relaxed),
                            "message": format!("{AUTH_KEY} {EMAIL} {PASSWORD} mock-secret mock-token")}})
                    }
                    "/api/login" => {
                        assert!(body["email"].as_str().is_some_and(|email| email == EMAIL));
                        assert!(
                            body["password"]
                                .as_str()
                                .is_some_and(|password| password == PASSWORD)
                        );
                        json!({"result": {"authKey": AUTH_KEY, "user": {
                            "_id": "mock-user", "email": EMAIL,
                            "lastModified": "2025-01-01T00:00:00Z",
                            "dateRegistered": "2020-01-01T00:00:00Z",
                            "gdpr_consent": {"tos": true, "privacy": true, "marketing": false}
                        }}})
                    }
                    "/api/addonCollectionGet" => {
                        assert!(body["authKey"].as_str().is_some_and(|key| key == AUTH_KEY));
                        assert_eq!(body["update"], true);
                        if worker_reject.load(Ordering::Relaxed) {
                            json!({"error": {"code": 1, "message": "mock collection unavailable"}})
                        } else {
                            json!({"result": {"addons": [addon("second"), addon("first")],
                                "lastModified": "2025-01-01T00:00:00Z"}})
                        }
                    }
                    "/api/datastoreGet" => {
                        assert_eq!(body["collection"], "libraryItem");
                        assert_eq!(body["all"], true);
                        json!({"result": []})
                    }
                    "/api/datastoreMeta" => json!({"result": []}),
                    "/api/logout" if worker_reject_logout.load(Ordering::Relaxed) => {
                        json!({"error": {"code": 500, "message": "mock logout failure"}})
                    }
                    "/api/getModal" | "/api/getNotification" => json!({"result": null}),
                    _ => json!({"result": {"success": true}}),
                };
                let _ = request.respond(Response::from_string(reply.to_string()));
                if path == "/api/login" && released.is_some() {
                    worker_released.fetch_add(1, Ordering::Relaxed);
                }
            }
        });
        Self {
            base,
            stop,
            reject_addons,
            login_error,
            network_error,
            stall_login,
            released_logins,
            stall_logout,
            reject_logout,
            redirect_target,
            calls,
            worker: Some(worker),
        }
    }
}

impl Drop for MockApi {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}

pub(super) fn addon(id: &str) -> Value {
    json!({
        "transportUrl": format!("https://user:mock-secret@{id}.example.test/secret-path/manifest.json?token=mock-token"),
        "manifest": {"id": id, "version": "1.2.3", "name": id, "types": ["movie"],
            "resources": ["catalog", "meta", "stream", "subtitles"],
            "catalogs": [{"id": "movies", "type": "movie"}]}
    })
}
