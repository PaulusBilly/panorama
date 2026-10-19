use http::Request;
use serde_json::Value;
use stremio_core::runtime::{Env, EnvError};
use url::Url;

use super::{TestEnv, mock::MockApi, tls::TlsMock};
use crate::stremio::{
    env::PanoramaEnv,
    fetch::{client, redirect_error},
};

#[test]
fn declared_and_streamed_oversized_bodies_are_rejected_before_json_parsing() {
    let api = MockApi::start();
    let fixture = TestEnv::memory(Some(api.base.clone()));
    fixture.runtime.block_on(async {
        for path in ["oversized-declared", "oversized-streamed"] {
            let result = PanoramaEnv::fetch::<_, Value>(Request::get(
                format!("https://api.strem.io/api/{path}"),
            ).body(()).unwrap()).await;
            assert!(matches!(result, Err(EnvError::Fetch(message)) if message == "response exceeds 8 MiB"));
        }
    });
}

#[test]
fn fetch_rejects_http_cross_origin_redirects_and_direct_loopback() {
    let api = MockApi::start();
    let target = MockApi::start();
    let mut fixture = TestEnv::memory(Some(api.base.clone()));
    let https_target = fixture.runtime.block_on(TlsMock::start());
    fixture.trust_tls(&[&https_target]);
    fixture.runtime.block_on(async {
        let control = reqwest::Client::builder()
            .use_rustls_tls()
            .referer(false)
            .add_root_certificate(https_target.certificate.clone())
            .build()
            .unwrap();
        for (path, destination) in [
            ("api/redirect-http", &target.base),
            ("api/redirect-origin", &https_target.base),
        ] {
            *api.redirect_target.lock().unwrap() = Some(destination.clone());
            let body = control
                .get(api.base.join(path).unwrap())
                .send()
                .await
                .unwrap()
                .json::<Value>()
                .await
                .unwrap();
            assert!(body["result"]["success"] == true || body["ok"] == true);
        }
        assert_eq!(target.calls.lock().unwrap().len(), 1);
        assert_eq!(https_target.requests.lock().unwrap().len(), 1);
        target.calls.lock().unwrap().clear();
        https_target.requests.lock().unwrap().clear();
        api.calls.lock().unwrap().clear();
        for uri in [
            "https://api.strem.io/api/redirect-http",
            "https://api.strem.io/api/redirect-origin",
            "http://remote.example.test/manifest.json",
            api.base.as_str(),
        ] {
            *api.redirect_target.lock().unwrap() = Some(if uri.ends_with("redirect-http") {
                target.base.clone()
            } else {
                https_target.base.clone()
            });
            let result = PanoramaEnv::fetch::<_, Value>(Request::get(uri).body(()).unwrap()).await;
            assert!(matches!(result, Err(EnvError::Fetch(_))));
        }
        assert_eq!(api.calls.lock().unwrap().len(), 2);
        assert!(target.calls.lock().unwrap().is_empty());
        assert!(https_target.requests.lock().unwrap().is_empty());
    });
}

#[test]
fn addon_https_redirect_sends_no_source_url_or_secrets_in_request() {
    let mut fixture = TestEnv::memory(None);
    let (source, target) = fixture
        .runtime
        .block_on(async { (TlsMock::start().await, TlsMock::start().await) });
    fixture.trust_tls(&[&source, &target]);
    *source.redirect.lock().unwrap() = Some(target.base.join("manifest.json").unwrap());
    fixture.runtime.block_on(async {
        let url = source
            .base
            .join("debrid-path-key/manifest.json?token=debrid-query-key")
            .unwrap();
        let result = PanoramaEnv::fetch::<_, Value>(Request::get(url.as_str()).body(()).unwrap())
            .await
            .unwrap();
        assert_eq!(result["ok"], true);
        let requests = target.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let request = requests[0].to_ascii_lowercase();
        assert!(request.starts_with("get /manifest.json http/1.1\r\n"));
        assert!(
            !request.contains("referer:"),
            "destination received Referer"
        );
        for secret in [
            "debrid-path-key",
            "debrid-query-key",
            "token=",
            source.base.as_str(),
        ] {
            assert!(
                !request.contains(secret),
                "destination received source URL data"
            );
        }
    });
}

#[test]
fn api_cross_origin_https_redirect_is_blocked_with_reachable_control() {
    let mut fixture = TestEnv::memory(None);
    let (source, target) = fixture
        .runtime
        .block_on(async { (TlsMock::start().await, TlsMock::start().await) });
    fixture.replace(
        crate::store::Store::open_in_memory().unwrap(),
        Some(source.base.clone()),
    );
    fixture.trust_tls(&[&source, &target]);
    *source.redirect.lock().unwrap() = Some(target.base.clone());
    fixture.runtime.block_on(async {
        let result = PanoramaEnv::fetch::<_, Value>(
            Request::post("https://api.strem.io/api/login")
                .body(serde_json::json!({"authKey": "private-key"}))
                .unwrap(),
        )
        .await;
        assert!(matches!(result, Err(EnvError::Fetch(_))));
        assert_eq!(source.requests.lock().unwrap().len(), 1);
        assert!(target.requests.lock().unwrap().is_empty());
        let control = crate::stremio::fetch::client_builder(None)
            .unwrap()
            .add_root_certificate(source.certificate.clone())
            .add_root_certificate(target.certificate.clone())
            .build()
            .unwrap();
        let result = control
            .get(source.base.clone())
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap();
        assert_eq!(result["ok"], true);
        assert_eq!(target.requests.lock().unwrap().len(), 1);
    });
}

#[test]
fn redirect_policy_caps_hops_and_checks_every_target() {
    let original: Url = "https://api.strem.io/api/login".parse().unwrap();
    let same: Url = "https://api.strem.io/api/next".parse().unwrap();
    let other: Url = "https://other.example.test/api/next".parse().unwrap();
    let http: Url = "http://api.strem.io/api/next".parse().unwrap();
    assert!(redirect_error(&same, std::slice::from_ref(&original), None).is_none());
    assert_eq!(
        redirect_error(&other, std::slice::from_ref(&original), None),
        Some("API redirects must keep the same origin")
    );
    assert_eq!(
        redirect_error(&http, std::slice::from_ref(&original), None),
        Some("HTTPS is required for redirects")
    );
    for hops in [10, 11, 20] {
        assert_eq!(
            redirect_error(&same, &vec![original.clone(); hops], None),
            Some("redirect limit reached")
        );
    }
    let override_url: Url = "https://mock.example.test/api/login".parse().unwrap();
    assert_eq!(
        redirect_error(
            &other,
            std::slice::from_ref(&override_url),
            Some(&override_url.origin())
        ),
        Some("API redirects must keep the same origin")
    );
    let addon: Url = "https://addon.example.test/manifest.json".parse().unwrap();
    assert!(redirect_error(&other, &[addon], None).is_none());
}

#[test]
fn api_override_accepts_only_clean_https_or_literal_loopback_origins() {
    for base in [
        "http://example.test/",
        "http://localhost/",
        "https://user:secret@example.test/",
        "https://example.test/api/",
        "https://example.test/?token=secret",
        "https://example.test/#secret",
    ] {
        assert!(client(Some(&base.parse().unwrap())).is_err());
    }
    for base in [
        "https://example.test/",
        "http://127.0.0.1:8080/",
        "http://[::1]:8080/",
    ] {
        assert!(client(Some(&base.parse().unwrap())).is_ok());
    }
}
