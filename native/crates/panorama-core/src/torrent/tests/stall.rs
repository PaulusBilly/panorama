use super::super::*;
use super::fixture::Swarm;
use http_body_util::BodyExt;
use std::{
    io::SeekFrom,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeek, AsyncWriteExt, ReadBuf};

struct PendingRead(tokio::io::DuplexStream);

impl AsyncRead for PendingRead {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl AsyncSeek for PendingRead {
    fn start_seek(self: Pin<&mut Self>, _: SeekFrom) -> std::io::Result<()> {
        Ok(())
    }

    fn poll_complete(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<u64>> {
        Poll::Ready(Ok(0))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pending_seek_with_a_live_peer_outlasts_the_no_peers_window() {
    let swarm = Swarm::new().await;
    let (source, _) = swarm.video("pending.mp4", 20 * 1024 * 1024).await;
    let engine = swarm.engine(swarm.dir.path().join("cache"), 32 * 1024 * 1024);
    let stream = engine.open(source, CancellationToken::new()).await.unwrap();
    let entry = stream.playback.entry.clone();
    let mut reader = entry.reader(stream.playback.index).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while stream.stats().peers_live == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let (pending, mut writer) = tokio::io::duplex(100);
    reader.inner = Box::pin(PendingRead(pending));
    let playback = stream.playback.clone();
    tokio::task::spawn_blocking(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async move {
                let window = Duration::from_secs(5);
                let mut body = super::super::server::body(reader, 100, playback.clone());
                tokio::time::pause();
                let frame = body.frame();
                tokio::pin!(frame);
                assert!(futures::poll!(&mut frame).is_pending());
                tokio::time::advance(window + Duration::from_millis(1)).await;
                assert!(playback.entry.snapshot().peers_live > 0);
                assert!(
                    futures::poll!(&mut frame).is_pending(),
                    "healthy seek failed at the read deadline"
                );
                assert_eq!(playback.entry.snapshot().error, None);
                writer.write_all(&[17; 100]).await.unwrap();
                assert_eq!(
                    frame.await.unwrap().unwrap().into_data().unwrap(),
                    &[17; 100][..]
                );
                assert!(body.frame().await.is_none());
            });
    })
    .await
    .unwrap();
    engine.shutdown().await.unwrap();
    swarm.seeder.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_range_body_aborts_the_http_connection() {
    let swarm = Swarm::new().await;
    let (source, _) = swarm.video("abort.mp4", 64 * 1024).await;
    let engine = swarm.engine(swarm.dir.path().join("cache"), 1024 * 1024);
    let stream = engine.open(source, CancellationToken::new()).await.unwrap();
    for prefix in [0, 10] {
        let mut reader = stream
            .playback
            .entry
            .reader(stream.playback.index)
            .await
            .unwrap();
        let (pending, mut writer) = tokio::io::duplex(100);
        reader.inner = Box::pin(PendingRead(pending));
        let body = super::super::server::body(reader, 100, stream.playback.clone());
        let response = http::Response::builder()
            .status(206)
            .header("Content-Length", "100")
            .body(body)
            .unwrap();
        let response = std::sync::Mutex::new(Some(response));
        let (mut client, server) = tokio::io::duplex(4096);
        let connection = tokio::spawn(async move {
            hyper::server::conn::http1::Builder::new()
                .serve_connection(
                    hyper_util::rt::TokioIo::new(server),
                    hyper::service::service_fn(move |_| {
                        std::future::ready(Ok::<_, std::convert::Infallible>(
                            lock(&response).take().unwrap(),
                        ))
                    }),
                )
                .await
        });
        client
            .write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
            .await
            .unwrap();
        let mut headers = Vec::new();
        tokio::time::timeout(Duration::from_secs(2), async {
            while !headers.ends_with(b"\r\n\r\n") {
                headers.push(client.read_u8().await.unwrap());
            }
        })
        .await
        .unwrap();
        assert!(headers.starts_with(b"HTTP/1.1 206"));
        writer.write_all(&vec![17; prefix]).await.unwrap();
        let mut received = vec![0; prefix];
        client.read_exact(&mut received).await.unwrap();
        drop(writer);
        client.read_to_end(&mut received).await.unwrap();
        assert_eq!(received, vec![17; prefix]);
        let error = tokio::time::timeout(Duration::from_secs(2), connection)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert!(error.is_user(), "body failure was not propagated: {error}");
    }
    engine.shutdown().await.unwrap();
    swarm.seeder.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_peers_prevent_stalls_and_pauses_reset_the_window() {
    let swarm = Swarm::new().await;
    let (source, _) = swarm.video("clock.mp4", 20 * 1024 * 1024).await;
    let engine = swarm.engine(swarm.dir.path().join("cache"), 32 * 1024 * 1024);
    let stream = engine.open(source, CancellationToken::new()).await.unwrap();
    let entry = stream.playback.entry.clone();
    let reader = entry.reader(stream.playback.index).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while stream.stats().peers_live == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let live = entry.torrent.live().unwrap();
    let second = entry.reader(stream.playback.index).await.unwrap();
    entry.monitor(Duration::from_secs(5)).await;
    assert!(std::sync::Arc::ptr_eq(
        &live,
        &entry.torrent.live().unwrap()
    ));
    drop(second);
    let window = Duration::from_secs(1);
    lock(&entry.activity).last_progress = tokio::time::Instant::now() - window * 2;
    entry.monitor(window).await;
    assert_eq!(entry.snapshot().error, None);
    let session = entry.session.upgrade().unwrap();
    let guard = entry.control.lock().await;
    session.pause(&entry.torrent).await.unwrap();
    lock(&entry.activity).last_progress = tokio::time::Instant::now() - window * 2;
    assert_eq!(entry.snapshot().error, None);
    assert!(lock(&entry.activity).last_progress.elapsed() < window);
    drop(guard);
    drop(reader);
    engine.shutdown().await.unwrap();
    swarm.seeder.stop().await;
}
