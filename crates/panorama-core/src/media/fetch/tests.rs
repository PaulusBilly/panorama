use super::*;

#[test]
fn address_classifier_blocks_local_destinations_and_preserves_lan_servers() {
    for address in [
        "127.0.0.1",
        "127.255.255.255",
        "0.0.0.0",
        "169.254.0.1",
        "169.254.169.254",
        "169.254.255.255",
        "::1",
        "::",
        "fe80::1",
        "febf:ffff::1",
        "::ffff:127.0.0.1",
        "::ffff:127.9.8.7",
        "::ffff:0.0.0.0",
        "::ffff:169.254.169.254",
    ] {
        assert!(forbidden_address(address.parse().unwrap()), "{address}");
    }
    for address in [
        "10.0.0.1",
        "172.16.0.1",
        "192.168.1.1",
        "126.255.255.255",
        "128.0.0.1",
        "169.253.255.255",
        "169.255.0.0",
        "8.8.8.8",
        "2001:db8::1",
        "fc00::1",
        "fe7f::1",
        "fec0::1",
        "::ffff:192.168.1.1",
    ] {
        assert!(!forbidden_address(address.parse().unwrap()), "{address}");
    }
}

#[test]
fn literal_ip_sources_reject_forbidden_destinations_without_exposing_urls() {
    for host in [
        "127.0.0.1",
        "127.9.8.7",
        "2130706433",
        "0x7f000001",
        "0.0.0.0",
        "169.254.169.254",
        "[::1]",
        "[::]",
        "[fe80::1]",
        "[::ffff:127.9.8.7]",
        "[::ffff:169.254.169.254]",
    ] {
        for scheme in ["http", "https"] {
            let url = format!("{scheme}://{host}:1234/private?secret=token");
            let error = MediaSource::new(&url).unwrap_err();
            assert_eq!(error, MediaError::InvalidDestination);
            assert!(!error.to_string().contains(&url));
            assert!(!format!("{error:?}").contains("secret"));
        }
    }
    for host in [
        "10.0.0.1",
        "172.16.1.1",
        "192.168.1.1",
        "[::ffff:192.168.1.1]",
        "media.example",
    ] {
        assert!(MediaSource::new(&format!("http://{host}/film")).is_ok());
    }
}

#[tokio::test]
async fn production_transport_rejects_loopback_dns_with_sanitized_error() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let contacted = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let accepted = contacted.clone();
    let source = MediaSource::new(&format!(
        "http://localhost:{}/private?secret=token",
        listener.local_addr().unwrap().port()
    ))
    .unwrap();
    let upstream = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        accepted.store(true, std::sync::atomic::Ordering::SeqCst);
        let service = hyper::service::service_fn(|_| async {
            Ok::<_, std::convert::Infallible>(http::Response::new(http_body_util::Full::new(
                Bytes::from_static(b"private service"),
            )))
        });
        let _ = hyper::server::conn::http1::Builder::new()
            .serve_connection(hyper_util::rt::TokioIo::new(socket), service)
            .await;
    });
    let result = HttpMediaFetch::new()
        .unwrap()
        .fetch(
            source,
            FetchRequest {
                method: Method::GET,
                headers: HeaderMap::new(),
            },
        )
        .await;
    upstream.abort();
    assert!(matches!(result, Err(MediaError::InvalidDestination)));
    assert!(!contacted.load(std::sync::atomic::Ordering::SeqCst));
}
