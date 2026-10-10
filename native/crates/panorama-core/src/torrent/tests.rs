use super::*;

mod fixture;
mod policy;
mod stall;
mod swarm;
mod teardown;

#[test]
fn hashes_normalize_and_invalid_sources_are_rejected() {
    for (value, expected) in [
        (
            "0123456789ABCDEF0123456789abcdef01234567",
            "0123456789abcdef0123456789abcdef01234567",
        ),
        (
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "0000000000000000000000000000000000000000",
        ),
        (
            "77777777777777777777777777777777",
            "ffffffffffffffffffffffffffffffffffffffff",
        ),
        (
            "aerukz4jvpg66ajdivtytk6n54asgrlh",
            "0123456789abcdef0123456789abcdef01234567",
        ),
    ] {
        assert_eq!(
            TorrentSource::from_stream(value, Some(2), &[])
                .unwrap()
                .info_hash(),
            expected
        );
    }
    for value in [
        "",
        "abc",
        "0123456789abcdef0123456789abcdef0123456g",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA1",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        " 0000000000000000000000000000000000000000",
    ] {
        assert_eq!(
            TorrentSource::from_stream(value, None, &[]).unwrap_err(),
            TorrentError::InvalidSource
        );
    }
}

#[test]
fn tracker_filter_caps_encoding_round_trip_and_debug_redaction() {
    let mut sources: Vec<String> = [
        "tracker:udp://127.0.0.1:80/announce",
        "tracker:https://tracker.test/a?passkey=secret&x=a b",
        "tracker:http://tracker.test/announce",
        "tracker:file:///secrets",
        "dht:0000000000000000000000000000000000000000",
        "https://tracker.test/bare",
        "tracker:udp://",
        "tracker:https://tracker.test/\nsecret",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    sources.push(format!("tracker:https://tracker.test/{}", "x".repeat(2048)));
    let source =
        TorrentSource::from_stream("0000000000000000000000000000000000000001", None, &sources)
            .unwrap();
    let decoded: Vec<_> =
        url::form_urlencoded::parse(source.magnet_uri().split_once('?').unwrap().1.as_bytes())
            .filter(|(key, _)| key == "tr")
            .map(|(_, value)| value.into_owned())
            .collect();
    assert_eq!(
        decoded,
        sources[..3]
            .iter()
            .map(|source| source.strip_prefix("tracker:").unwrap().to_owned())
            .collect::<Vec<_>>()
    );
    let debug = format!("{source:?}");
    assert!(!debug.contains("secret"));
    assert!(!debug.contains("tracker.test"));
    assert!(debug.contains("tracker_count: 3"));
    sources.extend((0..30).map(|idx| format!("tracker:udp://tracker.test:{}/", idx + 1000)));
    let capped = TorrentSource::from_stream(source.info_hash(), None, &sources).unwrap();
    assert_eq!(
        librqbit::Magnet::parse(capped.magnet_uri())
            .unwrap()
            .trackers
            .len(),
        20
    );
}

#[test]
fn ranges_and_peer_host_checks() {
    use super::server::{authorized, client_range};
    assert_eq!(client_range(None, 10), Ok((0, 10, false)));
    for (range, expected) in [
        ("bytes=0-3", (0, 4, true)),
        ("bytes=5-", (5, 5, true)),
        ("bytes=-3", (7, 3, true)),
        ("bytes=9-999", (9, 1, true)),
        ("bytes=4-2", (0, 10, false)),
        ("bytes=0-1,2-3", (0, 10, false)),
        ("wat", (0, 10, false)),
    ] {
        assert_eq!(client_range(Some(range), 10), Ok(expected));
    }
    for range in ["bytes=10-", "bytes=-0", "bytes=999-"] {
        assert_eq!(client_range(Some(range), 10), Err(()));
    }
    assert_eq!(client_range(None, 0), Ok((0, 0, false)));
    assert_eq!(client_range(Some("bytes=0-"), 0), Err(()));
    let mut headers = http::HeaderMap::new();
    headers.insert("host", "127.0.0.1:1234".parse().unwrap());
    let uri = "/torrent/token".parse().unwrap();
    assert!(authorized(
        &headers,
        &uri,
        "127.0.0.1:1".parse().unwrap(),
        "127.0.0.1:1234"
    ));
    assert!(!authorized(
        &headers,
        &uri,
        "192.0.2.1:1".parse().unwrap(),
        "127.0.0.1:1234"
    ));
    assert!(!authorized(
        &headers,
        &uri,
        "127.0.0.1:1".parse().unwrap(),
        "evil.test"
    ));
    assert!(!authorized(
        &headers,
        &"http://127.0.0.1:1234/torrent/token".parse().unwrap(),
        "127.0.0.1:1".parse().unwrap(),
        "127.0.0.1:1234"
    ));
    headers.append("host", "127.0.0.1:1234".parse().unwrap());
    assert!(!authorized(
        &headers,
        &uri,
        "127.0.0.1:1".parse().unwrap(),
        "127.0.0.1:1234"
    ));
}

#[tokio::test]
async fn construction_and_unused_shutdown_have_no_io() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache");
    let engine = TorrentEngine::new(TorrentOptions {
        cache_dir: path.clone(),
        ..Default::default()
    });
    assert!(!path.exists());
    engine.shutdown().await.unwrap();
    assert!(!path.exists());
    assert_eq!(
        engine
            .open(
                TorrentSource::from_stream("0000000000000000000000000000000000000001", None, &[])
                    .unwrap(),
                CancellationToken::new()
            )
            .await
            .err(),
        Some(TorrentError::Cancelled)
    );
}
mod concurrency;
mod review;

#[test]
fn addon_sources_control_unresolved_magnet_dht() {
    let hash = "0000000000000000000000000000000000000001";
    for (sources, allowed) in [
        (vec![], true),
        (vec!["tracker:http://127.0.0.1:1/announce".into()], false),
        (vec!["tracker:file:///invalid".into()], false),
        (vec![format!("dht:{hash}")], true),
        (
            vec![
                "tracker:http://127.0.0.1:1/announce".into(),
                format!("dht:{hash}"),
            ],
            true,
        ),
        (
            vec![
                "tracker:http://127.0.0.1:1/announce".into(),
                "dht:0000000000000000000000000000000000000002".into(),
            ],
            false,
        ),
    ] {
        assert_eq!(
            TorrentSource::from_stream(hash, None, &sources)
                .unwrap()
                .dht,
            allowed
        );
    }
    let mut capped: Vec<_> = (0..21)
        .map(|_| "tracker:http://127.0.0.1:1/announce".into())
        .collect();
    capped.push(format!("dht:{hash}"));
    assert!(TorrentSource::from_stream(hash, None, &capped).unwrap().dht);
}
