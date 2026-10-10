use super::super::*;
use super::fixture::Swarm;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tracing::{
    Event, Metadata, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
};

// librqbit 9.0.1 always binds DHT on all interfaces, so each new test binary triggers a
// Windows Firewall prompt. Run on demand: `cargo test -p panorama-core -- --ignored`.
#[ignore = "binds DHT on 0.0.0.0; triggers a Windows Firewall prompt"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn peer_and_dht_sockets_are_released_after_join_without_bootstrap() {
    let dir = tempfile::tempdir().unwrap();
    let bootstrap = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let engine = TorrentEngine::offline_dht(
        TorrentOptions {
            cache_dir: dir.path().join("cache"),
            listen_port: Some(0),
            metadata_timeout: Duration::from_millis(200),
            ..Default::default()
        },
        bootstrap.local_addr().unwrap(),
    );
    let source =
        TorrentSource::from_stream("0000000000000000000000000000000000000001", None, &[]).unwrap();
    assert!(matches!(
        engine.open(source, CancellationToken::new()).await.err(),
        Some(TorrentError::MetadataTimeout | TorrentError::NoPeers)
    ));
    let host = engine.host_for_test().await;
    let (tcp, udp, session) = {
        let state = host.shared.state.lock().await;
        let session = state.session.as_ref().unwrap();
        (
            session.listen_addr().unwrap(),
            session.get_dht().unwrap().listen_addr(),
            Arc::downgrade(session),
        )
    };
    assert!(tokio::net::TcpListener::bind(tcp).await.is_err());
    assert!(
        std::net::UdpSocket::bind(udp).is_err(),
        "DHT exited before shutdown"
    );
    let mut packet = [0; 2048];
    tokio::time::timeout(Duration::from_secs(1), bootstrap.recv_from(&mut packet))
        .await
        .unwrap()
        .unwrap();
    engine.shutdown().await.unwrap();
    assert!(session.upgrade().is_none());
    assert!(tokio::net::TcpListener::bind(tcp).await.is_ok());
    assert!(std::net::UdpSocket::bind(udp).is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_during_startup_is_joined_by_shutdown() {
    for index in 0..3 {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join(format!("cache-{index}"));
        let engine = Arc::new(TorrentEngine::offline(
            TorrentOptions {
                cache_dir: cache.clone(),
                ..Default::default()
            },
            vec![],
        ));
        let cancel = CancellationToken::new();
        let task_engine = engine.clone();
        let task_cancel = cancel.clone();
        let request = tokio::spawn(async move {
            task_engine
                .open(
                    TorrentSource::from_stream(
                        "0000000000000000000000000000000000000001",
                        None,
                        &[],
                    )
                    .unwrap(),
                    task_cancel,
                )
                .await
        });
        tokio::task::yield_now().await;
        cancel.cancel();
        assert!(matches!(
            request.await.unwrap().err(),
            Some(TorrentError::Cancelled | TorrentError::NoPeers)
        ));
        tokio::time::timeout(Duration::from_secs(5), engine.shutdown())
            .await
            .unwrap()
            .unwrap();
        assert!(!cache.exists());
    }
}

#[derive(Clone)]
struct Capture(Arc<AtomicBool>);
impl Visit for Capture {
    fn record_debug(&mut self, _: &Field, value: &dyn std::fmt::Debug) {
        if format!("{value:?}").contains("passkey=panorama-test-secret") {
            self.0.store(true, Ordering::Release);
        }
    }
}
impl Subscriber for Capture {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.target() == "panorama_torrent_privacy_probe"
            || metadata.target().starts_with("librqbit_tracker_comms")
    }
    fn new_span(&self, attributes: &Attributes<'_>) -> Id {
        attributes.record(&mut self.clone());
        Id::from_u64(1)
    }
    fn record(&self, _: &Id, values: &Record<'_>) {
        values.record(&mut self.clone());
    }
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, event: &Event<'_>) {
        event.record(&mut self.clone());
    }
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn library_runtime_and_blocking_threads_suppress_private_tracker_tracing() {
    let captured = Arc::new(AtomicBool::new(false));
    tracing::subscriber::set_global_default(Capture(captured.clone())).unwrap();
    tracing::error!(target: "panorama_torrent_privacy_probe", "passkey=panorama-test-secret");
    assert!(captured.swap(false, Ordering::AcqRel));
    let swarm = Swarm::new().await;
    let (source, _) = swarm.video("private.mp4", 1024 * 1024).await;
    let source = TorrentSource::from_stream(
        source.info_hash(),
        None,
        &["tracker:https://tracker.test/?passkey=panorama-test-secret".into()],
    )
    .unwrap();
    let engine = swarm.engine(swarm.dir.path().join("cache"), 8 * 1024 * 1024);
    let stream = engine.open(source, CancellationToken::new()).await.unwrap();
    let host = engine.host_for_test().await;
    host.handle
        .spawn(async {
            tracing::error!(target: "panorama_torrent_privacy_probe", "passkey=panorama-test-secret");
        })
        .await
        .unwrap();
    host.handle
        .spawn_blocking(|| {
            tracing::error!(target: "panorama_torrent_privacy_probe", "passkey=panorama-test-secret");
        })
        .await
        .unwrap();
    engine.shutdown().await.unwrap();
    assert!(!captured.load(Ordering::Acquire));
    stream.close().await.unwrap();
    swarm.seeder.stop().await;
}
