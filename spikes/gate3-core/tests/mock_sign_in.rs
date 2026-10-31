use gate3_core::{CoreSession, NativeEnv, addon_lines, first_movie_catalog};
use http::Request;
use serde_json::{Value, json};
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use stremio_core::runtime::{Env, EnvError};
use tiny_http::{Response, Server, StatusCode};
use url::Url;

struct MockApi {
    base: Url,
    stop: Arc<AtomicBool>,
    reject_addons: Arc<AtomicBool>,
    calls: Arc<Mutex<Vec<String>>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl MockApi {
    fn start() -> Self {
        let server = Server::http("127.0.0.1:0").expect("bind mock server");
        let base = format!("http://{}/", server.server_addr())
            .parse()
            .expect("mock origin");
        let stop = Arc::new(AtomicBool::new(false));
        let reject_addons = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let worker_stop = stop.clone();
        let worker_reject = reject_addons.clone();
        let worker_calls = calls.clone();
        let worker = thread::spawn(move || {
            while !worker_stop.load(Ordering::Relaxed) {
                let Some(mut request) = server
                    .recv_timeout(Duration::from_millis(50))
                    .expect("receive mock request")
                else {
                    continue;
                };
                let path = request.url().to_owned();
                worker_calls.lock().expect("calls lock").push(path.clone());
                if path == "/api/oversized" {
                    let response = Response::new(
                        StatusCode(200),
                        vec![],
                        Cursor::new(vec![b'x'; 8 * 1024 * 1024 + 1]),
                        None,
                        None,
                    );
                    let _ = request.respond(response);
                    continue;
                }
                let mut body = String::new();
                request
                    .as_reader()
                    .read_to_string(&mut body)
                    .expect("read mock body");
                let body: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                let reply = match path.as_str() {
                    "/fixture/catalog/movie/movies.json" => {
                        json!({"metas": (1..=6).map(|index| json!({"id": format!("tt{index}"), "type": "movie", "name": format!("Mock movie {index}")})).collect::<Vec<_>>()})
                    }
                    "/api/login" => {
                        assert!(
                            body["email"]
                                .as_str()
                                .is_some_and(|email| email.ends_with("@example.test")),
                            "login must carry an email"
                        );
                        assert!(
                            body["password"]
                                .as_str()
                                .is_some_and(|password| !password.is_empty()),
                            "login must carry a password"
                        );
                        json!({"result": {
                            "authKey": "mock-auth-key",
                            "user": {
                                "_id": "mock-user",
                                "email": "spike@example.test",
                                "lastModified": "2025-01-01T00:00:00Z",
                                "dateRegistered": "2020-01-01T00:00:00Z",
                                "gdpr_consent": {"tos": true, "privacy": true, "marketing": false}
                            }
                        }})
                    }
                    "/api/addonCollectionGet" => {
                        assert!(
                            body["authKey"].as_str().is_some(),
                            "addon request must carry an auth key"
                        );
                        if worker_reject.load(Ordering::Relaxed) {
                            json!({"error": {"code": 1, "message": "mock collection unavailable"}})
                        } else {
                            json!({"result": {
                                "addons": [fake_addon("second", "Second in alphabet, first in account"), fake_addon("first", "First in alphabet, second in account")],
                                "lastModified": "2025-01-01T00:00:00Z"
                            }})
                        }
                    }
                    "/api/datastoreGet" | "/api/datastoreMeta" => json!({"result": []}),
                    "/api/getModal" | "/api/getNotification" => json!({"result": null}),
                    _ => json!({"result": {"success": true}}),
                };
                let _ = request.respond(Response::from_string(reply.to_string()));
            }
        });
        Self {
            base,
            stop,
            reject_addons,
            calls,
            worker: Some(worker),
        }
    }
}

impl Drop for MockApi {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            worker.join().expect("mock server shutdown");
        }
    }
}

fn fake_addon(id: &str, name: &str) -> Value {
    json!({
        "transportUrl": format!("https://user:mock-secret@{id}.example.test/secret-path/manifest.json?token=mock-token"),
        "manifest": {
            "id": id,
            "version": "1.2.3",
            "name": name,
            "types": ["movie"],
            "resources": ["catalog", {"name": "meta", "types": ["movie"]}, "stream", "subtitles"],
            "catalogs": [{"id": "movies", "type": "movie"}]
        }
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn core_login_lists_account_addons_in_order_and_rejects_failed_pulls() {
    let api = MockApi::start();
    NativeEnv::initialize(Some(api.base.clone())).expect("initialize Env");
    let mut session = CoreSession::new().await.expect("initialize core");
    let profile = session
        .sign_in("spike@example.test".into(), "mock-password".into())
        .await
        .expect("mock sign-in");
    let ids = profile
        .addons
        .iter()
        .map(|addon| addon.manifest.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, ["second", "first"]);
    let lines = addon_lines(&profile.addons);
    assert!(lines[0].contains("host=second.example.test"));
    assert!(lines[1].contains("host=first.example.test"));
    assert!(
        lines
            .iter()
            .all(|line| line.contains("resources=[catalog,meta,stream,subtitles]"))
    );
    assert!(lines.iter().all(|line| !line.contains("mock-secret")
        && !line.contains("secret-path")
        && !line.contains("mock-token")
        && !line.contains("https://")));
    let calls = api.calls.lock().expect("calls lock").clone();
    assert_eq!(calls[0], "/api/login");
    assert!(calls.iter().any(|path| path == "/api/addonCollectionGet"));
    assert!(calls.iter().any(|path| path == "/api/datastoreGet"));
    let mut catalog_addon = profile.addons[0].clone();
    catalog_addon.transport_url = "https://api.strem.io/fixture/manifest.json"
        .parse()
        .expect("mock catalog URL");
    let names = first_movie_catalog(&[catalog_addon])
        .await
        .expect("mock catalog transport");
    assert_eq!(
        names,
        [
            "Mock movie 1",
            "Mock movie 2",
            "Mock movie 3",
            "Mock movie 4",
            "Mock movie 5"
        ]
    );
    let denied = NativeEnv::fetch::<_, Value>(
        Request::get("http://remote.example.test/manifest.json")
            .body(())
            .expect("request"),
    )
    .await;
    assert!(
        denied.is_err(),
        "plain HTTP must be rejected even with an override"
    );
    let direct_loopback =
        NativeEnv::fetch::<_, Value>(Request::get(api.base.as_str()).body(()).expect("request"))
            .await;
    assert!(
        direct_loopback.is_err(),
        "only rewritten core API requests may use loopback HTTP"
    );
    let oversized = NativeEnv::fetch::<_, Value>(
        Request::get("https://api.strem.io/api/oversized")
            .body(())
            .expect("request"),
    )
    .await;
    assert!(
        matches!(oversized, Err(EnvError::Fetch(message)) if message == "response exceeds 8 MiB"),
        "chunked responses must fail at the body cap before JSON parsing"
    );
    api.reject_addons.store(true, Ordering::Relaxed);
    let failed = session
        .sign_in("spike@example.test".into(), "mock-password".into())
        .await;
    assert!(
        failed.is_err(),
        "failed addon pulls must not report default addons as account addons"
    );
}
