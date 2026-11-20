use std::time::Instant;

use super::*;

async fn pair(network: Network) -> (PartyEndpoint, PartyEndpoint, PartyConn, PartyConn) {
    let (host, _) = PartyEndpoint::bind_on(None, network)
        .await
        .expect("bind host");
    let (member, _) = PartyEndpoint::bind_on(None, network)
        .await
        .expect("bind member");
    let secret = PartySecret::generate().expect("secret");
    let invite = parse_invite(&host.invite(&secret).await.expect("invite")).expect("parse");
    assert_eq!(invite.secret, secret);

    let joining = async {
        let mut conn = member.connect(&invite).await.expect("connect");
        conn.send(r#"{"t":"hello"}"#).await.expect("first line");
        conn
    };
    let accepting = async { host.accept().await.expect("endpoint open").expect("accept") };
    let (member_conn, host_conn) = tokio::join!(joining, accepting);
    (host, member, host_conn, member_conn)
}

#[tokio::test]
async fn loopback_exchanges_lines_both_ways() {
    let (host, member, mut at_host, mut at_member) = pair(Network::Loopback).await;

    assert_eq!(
        at_host.recv().await.unwrap().as_deref(),
        Some(r#"{"t":"hello"}"#)
    );
    assert_eq!(at_host.remote_id(), member.id());
    assert_eq!(at_member.remote_id(), host.id());

    at_host
        .send(r#"{"t":"welcome","member_id":1}"#)
        .await
        .unwrap();
    assert_eq!(
        at_member.recv().await.unwrap().as_deref(),
        Some(r#"{"t":"welcome","member_id":1}"#)
    );
    assert_eq!(at_member.path().map(|p| p.kind), Some(PathKind::Direct));
}

#[tokio::test]
async fn split_halves_read_and_write_together() {
    let (_host, _member, at_host, mut at_member) = pair(Network::Loopback).await;
    let (mut writer, mut reader) = at_host.split();
    assert!(reader.recv().await.unwrap().is_some());
    writer.send("one").await.unwrap();
    assert_eq!(at_member.recv().await.unwrap().as_deref(), Some("one"));
    at_member.send("two").await.unwrap();
    assert_eq!(reader.recv().await.unwrap().as_deref(), Some("two"));
    assert!(writer.path().is_some());
}

#[test]
fn an_internet_invite_names_only_the_relay() {
    let relay: iroh::RelayUrl = "https://euc1-1.relay.n0.iroh-canary.iroh.link./"
        .parse()
        .unwrap();
    let addr = EndpointAddr::new(SecretKey::generate().public())
        .with_relay_url(relay.clone())
        .with_ip_addr("192.168.1.20:51234".parse().unwrap())
        .with_ip_addr("[2001:db8::1]:51234".parse().unwrap())
        .with_ip_addr("203.0.113.9:40000".parse().unwrap());
    let secret = PartySecret::generate().unwrap();
    let invite = invite_for(addr.clone(), Network::Internet, &secret);
    assert!(invite.len() < 200, "{} chars", invite.len());
    let parsed = parse_invite(&invite).unwrap();
    assert_eq!(parsed.addr.id, addr.id);
    assert_eq!(parsed.addr.ip_addrs().count(), 0);
    assert_eq!(parsed.addr.relay_urls().collect::<Vec<_>>(), vec![&relay]);
    let local = invite_for(addr, Network::Loopback, &secret);
    assert_eq!(parse_invite(&local).unwrap().addr.ip_addrs().count(), 3);
}

#[tokio::test]
async fn bind_keeps_the_persisted_identity() {
    let (first, key) = PartyEndpoint::bind_on(None, Network::Loopback)
        .await
        .unwrap();
    let (second, same) = PartyEndpoint::bind_on(Some(key), Network::Loopback)
        .await
        .unwrap();
    assert_eq!(key, same);
    assert_eq!(first.id(), second.id());
}

#[tokio::test]
async fn recv_errors_on_line_over_64k() {
    let (_host, _member, mut at_host, mut at_member) = pair(Network::Loopback).await;
    assert!(at_host.recv().await.unwrap().is_some());

    at_member.send(&"a".repeat(MAX_LINE)).await.unwrap();
    assert_eq!(
        at_host.recv().await.unwrap().map(|l| l.len()),
        Some(MAX_LINE)
    );

    assert_eq!(
        at_member.send(&"a".repeat(MAX_LINE + 1)).await,
        Err(TransportError::InvalidLine)
    );
    let mut oversized = vec![b'a'; MAX_LINE + 1];
    oversized.push(b'\n');
    at_member.send.send.write_all(&oversized).await.unwrap();
    assert_eq!(at_host.recv().await, Err(TransportError::LineTooLong));
}

#[tokio::test]
async fn send_rejects_embedded_newline() {
    let (_host, _member, _at_host, mut at_member) = pair(Network::Loopback).await;
    assert_eq!(
        at_member.send("a\nb").await,
        Err(TransportError::InvalidLine)
    );
}

async fn sample_invite() -> String {
    let (host, _) = PartyEndpoint::bind_on(None, Network::Loopback)
        .await
        .unwrap();
    host.invite(&PartySecret::generate().unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn parse_invite_rejects_other_schemes() {
    let invite = sample_invite().await;
    let tail = invite.strip_prefix("panorama://party/").unwrap();
    for bad in [
        format!("https://party/{tail}"),
        format!("panorama://room/{tail}"),
        format!("PANORAMA://party/{tail}"),
        tail.to_string(),
        String::new(),
    ] {
        assert!(parse_invite(&bad).is_err(), "accepted {bad:?}");
    }
    let (ticket, secret) = tail.split_once('?').unwrap();
    assert_eq!(
        parse_invite(&format!("panorama://party/{ticket}")).unwrap_err(),
        InviteError::Shape
    );
    assert_eq!(
        parse_invite(&format!("panorama://party/endpointzzz?{secret}")).unwrap_err(),
        InviteError::Ticket
    );
}

#[tokio::test]
async fn parse_invite_rejects_bad_secret() {
    let invite = sample_invite().await;
    let (head, _) = invite.split_once("?s=").unwrap();
    for secret in [
        "",
        "abc",
        &"A".repeat(32),
        &"g".repeat(32),
        &"a".repeat(31),
        &"a".repeat(33),
    ] {
        assert_eq!(
            parse_invite(&format!("{head}?s={secret}")).unwrap_err(),
            InviteError::Secret,
            "accepted secret {secret:?}"
        );
    }
    assert_eq!(
        parse_invite(&format!("{head}?x={}", "a".repeat(32))).unwrap_err(),
        InviteError::Shape
    );
}

#[tokio::test]
async fn parse_invite_trims_whitespace() {
    let invite = sample_invite().await;
    let parsed = parse_invite(&format!("  \n{invite}\t \n")).unwrap();
    assert_eq!(parsed.secret, parse_invite(&invite).unwrap().secret);
}

#[test]
fn secret_round_trips_through_hex() {
    let secret = PartySecret::generate().unwrap();
    let hex = secret.to_hex();
    assert_eq!(hex.len(), 32);
    assert_eq!(PartySecret::from_hex(&hex), Some(secret.clone()));
    assert_ne!(PartySecret::generate().unwrap(), secret);
}

#[test]
fn secret_debug_is_redacted() {
    let secret = PartySecret::from_hex(&"ab".repeat(16)).unwrap();
    assert_eq!(format!("{secret:?}"), "PartySecret(..)");
    let invite = Invite {
        addr: EndpointAddr::new(SecretKey::generate().public()),
        secret,
    };
    assert!(!format!("{invite:?}").contains("abab"));
}

#[tokio::test]
#[ignore = "needs the network"]
async fn relay_path_exchanges_a_line() {
    let started = Instant::now();
    let (_host, _member, mut at_host, mut at_member) = pair(Network::RelayOnly).await;
    assert_eq!(
        at_host.recv().await.unwrap().as_deref(),
        Some(r#"{"t":"hello"}"#)
    );
    at_host.send("pong").await.unwrap();
    assert_eq!(at_member.recv().await.unwrap().as_deref(), Some("pong"));
    let path = at_member.path().expect("a selected path");
    assert_eq!(path.kind, PathKind::Relayed);
    eprintln!(
        "relayed exchange in {:?}, rtt {:?}",
        started.elapsed(),
        path.rtt
    );
}
