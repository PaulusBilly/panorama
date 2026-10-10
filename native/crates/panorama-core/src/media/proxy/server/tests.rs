use super::*;
use std::{
    collections::VecDeque,
    sync::atomic::{AtomicUsize, Ordering},
};
use tokio::sync::watch;

async fn until(mut condition: impl FnMut() -> bool) {
    for _ in 0..100_000 {
        if condition() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("accept loop did not make progress");
}

#[tokio::test(start_paused = true)]
async fn accept_errors_back_off_resume_serving_and_stop_without_waiting() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let client = tokio::net::TcpStream::connect(address).await.unwrap();
    let connection = listener.accept().await.unwrap();
    let (stop, _) = watch::channel(false);
    let shared = Arc::new(Shared {
        sessions: Default::default(),
        host: address.to_string(),
        stop: stop.clone(),
    });
    let mut results = VecDeque::from([
        Err(std::io::Error::from(std::io::ErrorKind::ConnectionReset)),
        Ok(connection),
        Err(std::io::Error::from(std::io::ErrorKind::Other)),
    ]);
    let attempts = Arc::new(AtomicUsize::new(0));
    let count = attempts.clone();
    let task = tokio::spawn(accept_with(
        move || {
            count.fetch_add(1, Ordering::SeqCst);
            std::future::ready(results.pop_front().unwrap())
        },
        shared,
        Duration::ZERO,
    ));
    until(|| attempts.load(Ordering::SeqCst) == 1).await;
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    assert!(!task.is_finished());
    tokio::time::advance(Duration::from_millis(49)).await;
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    tokio::time::advance(Duration::from_millis(1)).await;
    until(|| attempts.load(Ordering::SeqCst) == 3).await;
    let request = format!("GET / HTTP/1.1\r\nHost: {address}\r\n\r\n");
    let mut written = 0;
    until(|| {
        if let Ok(count) = client.try_write(&request.as_bytes()[written..]) {
            written += count;
        }
        written == request.len()
    })
    .await;
    let mut response = Vec::new();
    until(|| {
        let mut buffer = [0; 1024];
        if let Ok(count) = client.try_read(&mut buffer) {
            response.extend_from_slice(&buffer[..count]);
        }
        response.ends_with(b"\r\n\r\n")
    })
    .await;
    assert!(response.starts_with(b"HTTP/1.1 404"));
    let stopped_at = tokio::time::Instant::now();
    stop.send_replace(true);
    task.await.unwrap();
    assert_eq!(tokio::time::Instant::now(), stopped_at);
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
    drop(client);
}
