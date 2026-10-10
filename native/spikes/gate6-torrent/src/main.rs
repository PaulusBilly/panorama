mod http;
mod mpv;

use std::{
    path::PathBuf,
    process::ExitCode,
    sync::{Arc, OnceLock, mpsc::Receiver},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail};
use axum::{Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ManagedTorrent, Session, SessionOptions,
};
use tokio::{net::TcpListener, sync::watch, time::timeout};

const BBB_HASH: &str = "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c";
const SINTEL_HASH: &str = "08ada5a7a6183aae1e09d831df6748d566095a10";
const NO_PEERS_HASH: &str = "0000000000000000000000000000000000000001";
const BBB: &str = "magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c&dn=Big+Buck+Bunny&tr=udp%3A%2F%2Fexplodie.org%3A6969&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337&tr=udp%3A%2F%2Ftracker.openbittorrent.com%3A80&ws=https%3A%2F%2Fwebtorrent.io%2Ftorrents%2F";
const SINTEL: &str = "magnet:?xt=urn:btih:08ada5a7a6183aae1e09d831df6748d566095a10&dn=Sintel&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337&tr=udp%3A%2F%2Fexplodie.org%3A6969";

struct Args {
    magnet: Option<String>,
    file_index: Option<usize>,
    keep: bool,
}

impl Args {
    fn parse() -> Result<Self> {
        let mut args = Self {
            magnet: None,
            file_index: None,
            keep: false,
        };
        let mut values = std::env::args().skip(1);
        while let Some(value) = values.next() {
            match value.as_str() {
                "--magnet" => args.magnet = Some(values.next().context("--magnet needs a URI")?),
                "--file-index" => {
                    args.file_index = Some(
                        values
                            .next()
                            .context("--file-index needs a number")?
                            .parse()?,
                    )
                }
                "--keep" => args.keep = true,
                _ => bail!(
                    "Usage: gate6-torrent [--magnet <magnet-uri>] [--file-index <n>] [--keep]"
                ),
            }
        }
        if let Some(uri) = &args.magnet {
            if !uri.starts_with("magnet:?") {
                bail!("--magnet must be a magnet URI");
            }
            let magnet = librqbit::Magnet::parse(uri)?;
            let hash = magnet
                .as_id20()
                .context("magnet needs a BTv1 info-hash")?
                .as_string();
            if ![BBB_HASH, SINTEL_HASH, NO_PEERS_HASH].contains(&hash.as_str()) {
                bail!(
                    "Only Big Buck Bunny, Sintel, or the documented no-peers test hash may be used"
                );
            }
        }
        Ok(args)
    }
}

#[derive(Debug)]
struct NoMetadata;

impl std::fmt::Display for NoMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("no metadata/peers within 60 s")
    }
}

impl std::error::Error for NoMetadata {}

async fn metadata(
    session: &Arc<Session>,
    args: &Args,
    start: Instant,
) -> Result<librqbit::ListOnlyResponse> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    let sources: Vec<(&str, Duration)> = match args.magnet.as_deref() {
        Some(uri) => vec![(uri, Duration::from_secs(60))],
        None => vec![
            (BBB, Duration::from_secs(30)),
            (SINTEL, Duration::from_secs(30)),
        ],
    };
    for (index, (source, budget)) in sources.into_iter().enumerate() {
        if index == 1 {
            println!(
                "Big Buck Bunny did not yield metadata; falling back to Sintel within the same 60 s budget"
            );
        }
        println!("metadata source: {source}");
        let attempt_deadline = (tokio::time::Instant::now() + budget).min(deadline);
        let result = tokio::time::timeout_at(
            attempt_deadline,
            session.add_torrent(
                AddTorrent::from_url(source),
                Some(AddTorrentOptions {
                    list_only: true,
                    ..Default::default()
                }),
            ),
        )
        .await;
        match result {
            Ok(Ok(AddTorrentResponse::ListOnly(list))) => {
                println!(
                    "metadata: PASS {:.3} s from start; seen peers={}",
                    start.elapsed().as_secs_f64(),
                    list.seen_peers.len()
                );
                return Ok(list);
            }
            Ok(Ok(_)) => bail!("unexpected metadata response"),
            Ok(Err(error)) => println!("metadata attempt failed: {error:#}"),
            Err(_) => println!(
                "metadata attempt timed out after {:.3} s from start",
                start.elapsed().as_secs_f64()
            ),
        }
    }
    Err(NoMetadata.into())
}

#[derive(Clone)]
struct StreamState {
    torrent: Arc<ManagedTorrent>,
    file_index: usize,
    size: u64,
    mime: &'static str,
    meter: Arc<http::Meter>,
    stop: watch::Receiver<()>,
}

async fn stream_file(
    State(state): State<StreamState>,
    method: axum::http::Method,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    let reader = match timeout(
        Duration::from_secs(10),
        state.torrent.stream(state.file_index),
    )
    .await
    {
        Ok(Ok(reader)) => reader,
        Ok(Err(error)) => {
            return (StatusCode::SERVICE_UNAVAILABLE, error.to_string()).into_response();
        }
        Err(_) => {
            return (StatusCode::SERVICE_UNAVAILABLE, "stream setup timed out").into_response();
        }
    };
    match http::serve_reader(
        reader,
        state.size,
        state.mime,
        method,
        headers,
        state.meter,
        state.stop,
    )
    .await
    {
        Ok(response) => response,
        Err(error) => (StatusCode::SERVICE_UNAVAILABLE, error.to_string()).into_response(),
    }
}

fn stats(torrent: &ManagedTorrent, size: u64, step: &str) -> u64 {
    let stats = torrent.stats();
    let (fetched, peers, speed) = stats
        .live
        .as_ref()
        .map(|live| {
            (
                live.snapshot.fetched_bytes,
                live.snapshot.peer_stats.live_tcp
                    + live.snapshot.peer_stats.live_utp
                    + live.snapshot.peer_stats.live_socks,
                live.download_speed.as_bytes(),
            )
        })
        .unwrap_or((0, 0, 0));
    println!(
        "{step}: downloaded={fetched}/{size} bytes ({:.2}%); verified={} bytes; live peers={peers}; download speed={speed} B/s",
        fetched as f64 / size as f64 * 100.0,
        stats.progress_bytes
    );
    fetched
}

fn wait_position(
    updates: &Receiver<mpv::Update>,
    duration: &mut Option<f64>,
    target: Option<f64>,
) -> Result<bool> {
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut previous = None;
    let mut seek_observed = target.is_none();
    while Instant::now() < deadline {
        match updates.recv_timeout(Duration::from_millis(100)) {
            Ok(mpv::Update::Duration(value)) if value.is_finite() && value > 0.0 => {
                *duration = Some(value)
            }
            Ok(mpv::Update::Time(value)) => {
                if seek_observed
                    && value.is_finite()
                    && value >= target.unwrap_or(0.0)
                    && target.is_none_or(|target| value <= target + 10.0)
                {
                    if previous.is_some_and(|before| value > before)
                        && target.is_none_or(|target| value >= target + 1.0)
                    {
                        return Ok(true);
                    }
                    previous = Some(value);
                }
            }
            Ok(mpv::Update::Seek) => {
                seek_observed = true;
                previous = None;
            }
            Ok(mpv::Update::Error(error)) => bail!("mpv: {error}"),
            Ok(mpv::Update::End { reason, error }) if error < 0 || reason == 0 => {
                bail!("mpv ended: reason={reason} error={error}")
            }
            Ok(mpv::Update::Log(message)) => println!("{message}"),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                bail!("mpv event thread disconnected")
            }
            _ => {}
        }
    }
    Ok(false)
}

fn measure(
    api: mpv::Api,
    url: String,
    torrent: Arc<ManagedTorrent>,
    size: u64,
    start: Instant,
) -> Result<bool> {
    let (mut player, updates) = mpv::Mpv::start(api, url).map_err(|e| anyhow!(e))?;
    let mut duration = None;
    let outcome = (|| -> Result<bool> {
        let started = wait_position(&updates, &mut duration, None)?;
        println!(
            "playback start: {} {:.3} s from start",
            if started {
                "PASS"
            } else {
                "FAIL (90 s timeout)"
            },
            start.elapsed().as_secs_f64()
        );
        stats(&torrent, size, "playback start");
        if !started {
            return Ok(false);
        }
        let duration_deadline = Instant::now() + Duration::from_secs(90);
        while duration.is_none() && Instant::now() < duration_deadline {
            match updates.recv_timeout(Duration::from_millis(100)) {
                Ok(mpv::Update::Duration(value)) if value.is_finite() && value > 0.0 => {
                    duration = Some(value);
                }
                Ok(mpv::Update::Error(error)) => bail!("mpv: {error}"),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    bail!("mpv event thread disconnected");
                }
                _ => {}
            }
        }
        let duration =
            duration.context("mpv did not provide a duration within 90 s; cannot measure seeks")?;
        println!("duration: {duration:.3} s");
        let mut passed = true;
        for fraction in [0.25, 0.60, 0.90] {
            for _ in updates.try_iter() {}
            let target = fraction * duration;
            let seek_start = Instant::now();
            player.command(&["seek", &target.to_string(), "absolute+exact"]);
            let success = wait_position(&updates, &mut Some(duration), Some(target))?;
            println!(
                "seek {:.0}% target={target:.3} s: {} {:.3} s",
                fraction * 100.0,
                if success {
                    "PASS"
                } else {
                    "FAIL (90 s timeout)"
                },
                seek_start.elapsed().as_secs_f64()
            );
            stats(
                &torrent,
                size,
                &format!("after seek {:.0}%", fraction * 100.0),
            );
            passed &= success;
        }
        Ok(passed)
    })();
    let downloaded = stats(&torrent, size, "final");
    println!(
        "partial download: {} ({:.2}% of file fetched)",
        if downloaded > 0 && downloaded < size {
            "PASS"
        } else {
            "FAIL"
        },
        downloaded as f64 / size as f64 * 100.0
    );
    player.command(&["stop"]);
    player.stop();
    println!("mpv stopped");
    outcome
}

async fn run(session: &Arc<Session>, api: mpv::Api, args: &Args, start: Instant) -> Result<bool> {
    let list = metadata(session, args, start).await?;
    let files: Vec<_> = list
        .info
        .iter_file_details()
        .map(|file| (file.filename.to_pathbuf(), file.len))
        .collect();
    let index = match args.file_index {
        Some(index) => index,
        None => files
            .iter()
            .enumerate()
            .filter(|(_, (name, _))| http::content_type(&name.to_string_lossy()).is_some())
            .max_by_key(|(_, (_, size))| size)
            .map(|(index, _)| index)
            .context("torrent contains no recognized video file")?,
    };
    let (name, size) = files.get(index).context("--file-index is out of range")?;
    if *size == 0 {
        bail!("selected file is empty");
    }
    let size = *size;
    let mime = http::content_type(&name.to_string_lossy()).unwrap_or("application/octet-stream");
    println!(
        "selected file: index={index} name={} size={size} bytes",
        name.display()
    );
    let torrent = timeout(
        Duration::from_secs(30),
        session.add_torrent(
            AddTorrent::from_bytes(list.torrent_bytes),
            Some(AddTorrentOptions {
                only_files: Some(vec![index]),
                initial_peers: Some(list.seen_peers),
                ..Default::default()
            }),
        ),
    )
    .await
    .context("torrent add timed out")??
    .into_handle()
    .context("missing torrent handle")?;
    timeout(Duration::from_secs(30), torrent.wait_until_initialized())
        .await
        .context("torrent initialization timed out")??;
    stats(&torrent, size, "after metadata");
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let url = format!("http://{}/stream", listener.local_addr()?);
    println!("HTTP: {url}");
    let meter = Arc::new(http::Meter {
        start,
        first_byte: OnceLock::new(),
    });
    let (shutdown, stop) = watch::channel(());
    let state = StreamState {
        torrent: torrent.clone(),
        file_index: index,
        size,
        mime,
        meter: meter.clone(),
        stop: stop.clone(),
    };
    let server = tokio::spawn(async move {
        let mut stop = stop;
        axum::serve(
            listener,
            Router::new()
                .route("/stream", get(stream_file))
                .with_state(state),
        )
        .with_graceful_shutdown(async move {
            let _ = stop.changed().await;
        })
        .await
    });
    let measurement =
        tokio::task::spawn_blocking(move || measure(api, url, torrent, size, start)).await;
    let _ = shutdown.send(());
    timeout(Duration::from_secs(15), server)
        .await
        .context("HTTP shutdown timed out")???;
    println!("HTTP stopped");
    if meter.first_byte.get().is_none() {
        println!("first byte served: FAIL (no bytes served)");
    }
    measurement.context("measurement thread panicked")?
}

async fn execute() -> Result<bool> {
    let start = Instant::now();
    let args = Args::parse()?;
    let api = mpv::Api::load().map_err(|e| anyhow!(e))?;
    let directory: PathBuf =
        std::env::temp_dir().join(format!("panorama-gate6-{}", std::process::id()));
    tokio::fs::create_dir(&directory)
        .await
        .context("cannot create fresh download directory")?;
    println!(
        "download directory: {} (keep={})",
        directory.display(),
        args.keep
    );
    let session = timeout(
        Duration::from_secs(30),
        Session::new_with_opts(
            directory.clone(),
            SessionOptions {
                dht: Some(librqbit::DhtSessionConfig {
                    persistence: None,
                    ..Default::default()
                }),
                listen: Some(librqbit::ListenerOptions::default()),
                disable_local_service_discovery: true,
                ..Default::default()
            },
        ),
    )
    .await;
    let result = match session {
        Ok(Ok(session)) => {
            println!(
                "session started: DHT on; random TCP listener; UPnP off; persistence off; LSD off"
            );
            let result = run(&session, api, &args, start).await;
            session.stop().await;
            drop(session);
            println!("session stopped");
            result
        }
        Ok(Err(error)) => Err(error),
        Err(_) => Err(anyhow!("session startup timed out after 30 s")),
    };
    if args.keep {
        println!("download directory retained: {}", directory.display());
    } else {
        tokio::fs::remove_dir_all(&directory)
            .await
            .context("cannot delete download directory")?;
        println!("download directory deleted");
    }
    println!(
        "clean shutdown: PASS; total elapsed {:.3} s",
        start.elapsed().as_secs_f64()
    );
    result
}

#[tokio::main]
async fn main() -> ExitCode {
    match execute().await {
        Ok(true) => {
            println!("gate 6: PASS");
            ExitCode::SUCCESS
        }
        Ok(false) => {
            println!("gate 6: FAIL");
            ExitCode::from(1)
        }
        Err(error) => {
            eprintln!("gate 6: FAIL: {error:#}");
            ExitCode::from(if error.downcast_ref::<NoMetadata>().is_some() {
                2
            } else {
                1
            })
        }
    }
}
