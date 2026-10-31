use super::*;

#[tokio::test]
async fn png_jpeg_webp_resize_preserves_aspect_and_rgba_channels() {
    let server = server::Server::start().await;
    let dir = tempfile::tempdir().unwrap();
    let loader = server.loader(options(dir.path()));
    for path in ["/png", "/jpeg", "/webp"] {
        let image = loader.load(request(&server, path, 30, 30)).await.unwrap();
        assert_eq!((image.width, image.height), (30, 15));
        assert_eq!(image.rgba.len(), 30 * 15 * 4);
        assert!(image.rgba[0] > 190);
        assert!(image.rgba[1] < 50);
        assert!(image.rgba[2] > 40 && image.rgba[2] < 80);
        assert_eq!(image.rgba[3], 255);
        let portrait = loader.load(request(&server, path, 100, 10)).await.unwrap();
        assert_eq!((portrait.width, portrait.height), (20, 10));
    }
    let image = loader
        .load(request(&server, "/small", 100, 100))
        .await
        .unwrap();
    assert_eq!((image.width, image.height), (4, 2));
    assert_eq!(server.requests(), 4);
}

#[tokio::test]
async fn body_caps_cover_length_and_streaming_without_length() {
    let server = server::Server::start().await;
    let dir = tempfile::tempdir().unwrap();
    let mut options = options(dir.path());
    options.max_body_bytes = 1024;
    let loader = server.loader(options);
    for path in ["/large", "/chunked"] {
        let error = loader
            .load(request(&server, path, 10, 10))
            .await
            .unwrap_err();
        assert_error(error, ImageError::TooLarge, &server.url(path));
    }
}

#[tokio::test]
async fn content_sniffing_limits_status_and_decode_errors_are_sanitized() {
    let server = server::Server::start().await;
    let dir = tempfile::tempdir().unwrap();
    let loader = server.loader(options(dir.path()));
    for (path, kind) in [
        ("/unsupported", ImageError::Unsupported),
        ("/huge", ImageError::TooLarge),
        ("/decode", ImageError::Decode),
        ("/status", ImageError::Network),
    ] {
        let error = loader
            .load(request(&server, path, 10, 10))
            .await
            .unwrap_err();
        assert_error(error, kind, &server.url(path));
    }
    let error = loader
        .load(request(&server, "/png", 0, 10))
        .await
        .unwrap_err();
    assert_error(error, ImageError::Decode, &server.url("/png"));
    assert_eq!(server.requests(), 4);
}

#[tokio::test]
async fn redirect_policy_rejects_http_and_six_hops_but_allows_five() {
    let server = server::Server::start().await;
    let dir = tempfile::tempdir().unwrap();
    let loader = server.loader(options(dir.path()));
    for path in ["/http", "/redirect/6"] {
        let error = loader
            .load(request(&server, path, 10, 10))
            .await
            .unwrap_err();
        assert_error(error, ImageError::Network, &server.url(path));
    }
    assert!(
        loader
            .load(request(&server, "/redirect/5", 10, 10))
            .await
            .is_ok()
    );
    assert_eq!(server.requests(), 13);
}

#[tokio::test]
async fn timeout_uses_paused_tokio_clock() {
    let server = server::Server::start().await;
    let dir = tempfile::tempdir().unwrap();
    let loader = server.loader(options(dir.path()));
    let request = request(&server, "/slow/timeout", 10, 10);
    let task = tokio::spawn(async move { loader.load(request).await });
    server.wait_requests(1).await;
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(21)).await;
    assert_error(
        task.await.unwrap().unwrap_err(),
        ImageError::Timeout,
        &server.url("/slow/timeout"),
    );
}
