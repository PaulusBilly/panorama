//! Retry and integrity regressions demonstrated against `desktop/main/media-proxy.ts` behavior.
use super::*;
use crate::media::fetch::{FetchRequest, FetchResponse, MediaFetch};
use futures::{StreamExt, future::BoxFuture};
use std::sync::atomic::{AtomicBool, AtomicUsize};

#[derive(Clone, Copy)]
enum Fault {
    Overlong,
    Permanent503,
    PendingEnd,
    NoValidatorDrop,
    ChangedValidator,
}
struct Failing {
    fixture: Fixture,
    fault: Fault,
    first: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
}
impl MediaFetch for Failing {
    fn fetch(
        &self,
        source: MediaSource,
        request: FetchRequest,
    ) -> BoxFuture<'static, Result<FetchResponse, MediaError>> {
        let (fixture, fault, first, calls) = (
            self.fixture.clone(),
            self.fault,
            self.first.clone(),
            self.calls.clone(),
        );
        Box::pin(async move {
            let opening = crate::media::range::header(&request.headers, "range")
                .is_some_and(|value| value.starts_with("bytes=0-"));
            let resolving =
                crate::media::range::header(&request.headers, "range") == Some("bytes=0-0");
            let mut response = fixture.fetch(source, request).await?;
            if matches!(fault, Fault::Permanent503) && !opening {
                calls.fetch_add(1, Ordering::SeqCst);
                response.status = 503;
                response.body = Box::pin(futures::stream::empty());
            }
            if matches!(fault, Fault::Overlong) && first.swap(false, Ordering::SeqCst) {
                response.body = Box::pin(response.body.chain(futures::stream::once(async {
                    Ok(Bytes::from_static(b"overflow"))
                })));
            }
            if matches!(fault, Fault::PendingEnd) && first.swap(false, Ordering::SeqCst) {
                response.body = Box::pin(response.body.chain(futures::stream::pending()));
            }
            if matches!(fault, Fault::NoValidatorDrop) {
                response.headers.remove("etag");
                if !opening && first.swap(false, Ordering::SeqCst) {
                    response.body = Box::pin(futures::stream::iter([
                        Ok(Bytes::from_static(b"prefix")),
                        Err(MediaError::Transport),
                    ]));
                }
            }
            if matches!(fault, Fault::ChangedValidator) && (!opening || resolving) {
                response
                    .headers
                    .insert("etag", "\"fixture-v2\"".parse().unwrap());
            }
            Ok(response)
        })
    }
}
async fn failing(
    fault: Fault,
) -> (
    tempfile::TempDir,
    MediaProxy,
    SessionHandle,
    Arc<AtomicUsize>,
) {
    let dir = tempfile::tempdir().unwrap();
    let fixture = Fixture::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut options = ProxyOptions::default();
    options.cache.directory = dir.path().to_owned();
    options.cache.reserve_free_bytes = 0;
    options.stall_timeout = Duration::from_millis(300);
    let fetch = Failing {
        fixture,
        fault,
        first: Arc::new(AtomicBool::new(true)),
        calls: calls.clone(),
    };
    let mut proxy = MediaProxy::with_fetch(options, Arc::new(fetch), None)
        .await
        .unwrap();
    let handle = proxy.create_session(Fixture::source()).await.unwrap();
    handle.set_read_ahead(sample(300.0, false));
    (dir, proxy, handle, calls)
}
#[tokio::test(start_paused = true)]
async fn exhausted_chunk_retries_stop_instead_of_scheduler_restarting_the_failure() {
    let (_dir, mut proxy, handle, calls) = drive(failing(Fault::Permanent503)).await;
    drive(handle.session.prepare()).await.unwrap();
    let mut reader = handle.session.add_reader(0).unwrap();
    assert_eq!(
        drive(reader.read(CHUNK_BYTES - 1))
            .await
            .unwrap()
            .unwrap()
            .len(),
        65536
    );
    drive(async {
        while !handle.session.stopped() {
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert_eq!(calls.load(Ordering::SeqCst), 7);
    assert_eq!(handle.stats().error, Some(MediaError::Status(503)));
    tokio::time::advance(Duration::from_secs(600)).await;
    handle.set_read_ahead(sample(300.0, false));
    assert_eq!(calls.load(Ordering::SeqCst), 7);
    assert!(drive(reader.read(CHUNK_BYTES - 1)).await.is_err());
    drop(reader);
    drive(proxy.close()).await;
}
#[tokio::test(start_paused = true)]
async fn overlong_range_fails_before_last_bytes_can_complete_playback() {
    let (_dir, mut proxy, handle, _) = drive(failing(Fault::Overlong)).await;
    assert_eq!(
        drive(read(&handle, 0, CHUNK_BYTES - 1)).await,
        Err(MediaError::Representation("Overlong media range"))
    );
    assert!(handle.stats().representation_changed);
    drive(proxy.close()).await;
}
#[tokio::test(start_paused = true)]
async fn complete_prefix_without_eof_waits_for_validation_and_refetches_whole_chunk() {
    let (_dir, mut proxy, handle, _) = drive(failing(Fault::PendingEnd)).await;
    drive(handle.session.prepare()).await.unwrap();
    let mut reader = handle.session.add_reader(0).unwrap();
    for _ in 0..31 {
        assert_eq!(
            drive(reader.read(CHUNK_BYTES - 1))
                .await
                .unwrap()
                .unwrap()
                .len(),
            65536
        );
    }
    {
        let last = reader.read(CHUNK_BYTES - 1);
        tokio::pin!(last);
        assert!(futures::poll!(&mut last).is_pending());
        tokio::time::advance(Duration::from_secs(8)).await;
        assert_eq!(drive(last).await.unwrap().unwrap().len(), 65536);
    }
    assert!(lock(&handle.session.state).error.is_none());
    drop(reader);
    drive(proxy.close()).await;
}
#[tokio::test(start_paused = true)]
async fn cannot_resume_unverified_media_without_strong_validator() {
    let (_dir, mut proxy, handle, _) = drive(failing(Fault::NoValidatorDrop)).await;
    assert_eq!(
        drive(read(&handle, 0, 2 * CHUNK_BYTES - 1)).await,
        Err(MediaError::Representation("Cannot resume unverified media"))
    );
    drive(proxy.close()).await;
}
#[tokio::test(start_paused = true)]
async fn changed_validator_refreshes_then_rejects_unmatched_identity() {
    let (_dir, mut proxy, handle, _) = drive(failing(Fault::ChangedValidator)).await;
    assert!(drive(read(&handle, 0, 2 * CHUNK_BYTES - 1)).await.is_err());
    assert!(handle.stats().representation_changed);
    drive(proxy.close()).await;
}
#[test]
fn non_loopback_peers_and_absolute_form_requests_are_rejected() {
    let mut headers = http::HeaderMap::new();
    headers.insert("host", "127.0.0.1:1234".parse().unwrap());
    let uri = "/media/token".parse().unwrap();
    assert!(server::authorized(
        &headers,
        &uri,
        "127.0.0.1:9999".parse().unwrap(),
        "127.0.0.1:1234"
    ));
    assert!(!server::authorized(
        &headers,
        &uri,
        "192.0.2.1:9999".parse().unwrap(),
        "127.0.0.1:1234"
    ));
    assert!(!server::authorized(
        &headers,
        &"http://127.0.0.1:1234/media/token".parse().unwrap(),
        "127.0.0.1:9999".parse().unwrap(),
        "127.0.0.1:1234"
    ));
}
