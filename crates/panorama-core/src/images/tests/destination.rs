use super::*;

#[test]
fn internal_ip_literals_are_invalid_urls_and_private_lan_stays_allowed() {
    for host in [
        "127.0.0.1",
        "127.45.67.89",
        "[::1]",
        "[::ffff:127.0.0.1]",
        "0.0.0.0",
        "[::]",
        "169.254.169.254",
        "[fe80::1]",
        "[::ffff:169.254.169.254]",
    ] {
        let url = format!("https://{host}/image");
        assert_eq!(ImageUrl::parse(&url).unwrap_err(), ImageError::InvalidUrl);
    }
    for host in ["10.0.0.1", "172.16.0.1", "192.168.0.1", "[fd00::1]"] {
        assert!(ImageUrl::parse(&format!("https://{host}/image")).is_ok());
    }
}

#[tokio::test]
async fn production_resolver_rejects_hostname_resolving_only_to_loopback() {
    let server = server::Server::start().await;
    let dir = tempfile::tempdir().unwrap();
    let loader = ImageLoader::new(options(dir.path())).unwrap();
    let error = tokio::time::timeout(
        Duration::from_secs(3),
        loader.load(request(&server, "/png", 10, 10)),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert_eq!(error, ImageError::Network);
    assert_eq!(server.connections(), 0);
}

#[test]
fn dns_answers_drop_forbidden_addresses_and_allow_private_lan() {
    let addresses = [
        "127.0.0.1:0",
        "[::1]:0",
        "[::ffff:127.4.5.6]:0",
        "0.0.0.0:0",
        "[::]:0",
        "169.254.169.254:0",
        "[fe80::1]:0",
        "10.0.0.1:0",
    ];
    let parse = || {
        addresses
            .iter()
            .map(|address| address.parse::<std::net::SocketAddr>().unwrap())
    };
    use super::super::destination::filter_addresses as filter;
    assert_eq!(
        filter(parse()).unwrap().collect::<Vec<_>>(),
        ["10.0.0.1:0".parse().unwrap()]
    );
    assert!(matches!(filter(parse().take(7)), Err(ImageError::Network)));
    assert!(matches!(
        filter(std::iter::empty()),
        Err(ImageError::Network)
    ));
}

#[tokio::test]
async fn redirect_to_loopback_literal_is_rejected_before_connect() {
    let server = server::Server::start().await;
    let dir = tempfile::tempdir().unwrap();
    let loader = server.loader(options(dir.path()));
    assert_eq!(
        loader
            .load(request(&server, "/loopback", 10, 10))
            .await
            .unwrap_err(),
        ImageError::Network
    );
    assert_eq!(server.requests(), 1);
    assert_eq!(server.connections(), 1);
}
