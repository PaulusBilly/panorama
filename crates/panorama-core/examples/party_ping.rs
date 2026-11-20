//! Manual check of the watch party link across networks.
//!
//! Host:   cargo run -p panorama-core --example party_ping
//! Member: cargo run -p panorama-core --example party_ping -- '<invite>'
//!
//! The member pings once a second and the host echoes. Both print whether the path is direct or
//! relayed; the member also prints the line round trip.

use std::time::{Duration, Instant};

use panorama_core::party::transport::{PartyConn, PartyEndpoint, PartySecret, parse_invite};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (endpoint, _) = PartyEndpoint::bind(None).await?;
    let Some(invite) = std::env::args().nth(1) else {
        println!("{}", endpoint.invite(&PartySecret::generate()?).await?);
        let mut conn = endpoint
            .accept()
            .await
            .ok_or("endpoint closed")?
            .handshake()
            .await?;
        println!("connected to {}", conn.remote_id());
        let started = Instant::now();
        while let Some(line) = conn.recv().await? {
            conn.send(&line).await?;
            report(&conn, started, None);
        }
        return Ok(());
    };
    let mut conn = endpoint.connect(&parse_invite(&invite)?).await?;
    println!("connected to {}", conn.remote_id());
    let started = Instant::now();
    loop {
        let sent = Instant::now();
        conn.send("ping").await?;
        if conn.recv().await?.is_none() {
            return Ok(());
        }
        report(&conn, started, Some(sent.elapsed()));
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

fn report(conn: &PartyConn, started: Instant, round_trip: Option<Duration>) {
    let path = conn.path().map_or("no path".to_string(), |p| {
        format!("{:?}, quic rtt {:?}", p.kind, p.rtt)
    });
    let line = round_trip.map_or(String::new(), |rt| format!("line round trip {rt:?}, "));
    println!("{:>5.1}s  {line}{path}", started.elapsed().as_secs_f64());
}
