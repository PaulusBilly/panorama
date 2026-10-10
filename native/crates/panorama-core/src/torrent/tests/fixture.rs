use super::super::*;
use librqbit::{
    AddTorrent, AddTorrentOptions, CreateTorrentOptions, ListenerOptions, Session, SessionOptions,
    create_torrent, spawn_utils::BlockingSpawner,
};
use std::{net::SocketAddr, num::NonZeroU32, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};

pub struct Swarm {
    pub dir: tempfile::TempDir,
    pub seeder: Arc<Session>,
    pub peer: SocketAddr,
}

impl Swarm {
    pub async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let seeder = Session::new_with_opts(
            dir.path().to_path_buf(),
            SessionOptions {
                dht: None,
                persistence: None,
                disable_trackers: true,
                disable_local_service_discovery: true,
                ipv4_only: true,
                listen: Some(ListenerOptions {
                    listen_addr: ([127, 0, 0, 1], 0).into(),
                    ipv4_only: true,
                    enable_upnp_port_forwarding: false,
                    ..Default::default()
                }),
                ratelimits: librqbit::limits::LimitsConfig {
                    upload_bps: NonZeroU32::new(32 * 1024 * 1024),
                    download_bps: None,
                },
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let peer = seeder.listen_addr().unwrap();
        Self { dir, seeder, peer }
    }

    pub async fn video(&self, name: &str, size: usize) -> (TorrentSource, Vec<u8>) {
        let data: Vec<_> = (0..size)
            .map(|idx| (idx.wrapping_mul(31) ^ (idx >> 16)) as u8)
            .collect();
        let path = self.dir.path().join(name);
        std::fs::write(&path, &data).unwrap();
        let meta = create_torrent(
            &path,
            CreateTorrentOptions {
                piece_length: Some(64 * 1024),
                ..Default::default()
            },
            &BlockingSpawner::new(2),
        )
        .await
        .unwrap();
        let handle = self
            .seeder
            .add_torrent(
                AddTorrent::from_bytes(meta.as_bytes().unwrap()),
                Some(AddTorrentOptions {
                    overwrite: true,
                    ..Default::default()
                }),
            )
            .await
            .unwrap()
            .into_handle()
            .unwrap();
        timeout(Duration::from_secs(10), handle.wait_until_initialized())
            .await
            .unwrap()
            .unwrap();
        assert!(handle.stats().finished);
        let source =
            TorrentSource::from_stream(&handle.info_hash().as_string(), None, &[]).unwrap();
        (source, data)
    }

    pub async fn bundle(&self) -> (TorrentSource, Vec<u8>, usize) {
        let path = self.dir.path().join("bundle");
        std::fs::create_dir(&path).unwrap();
        let large = vec![43; 2 * 1024 * 1024];
        std::fs::write(path.join("large.MKV"), &large).unwrap();
        std::fs::write(path.join("small.mp4"), vec![17; 512 * 1024]).unwrap();
        std::fs::write(path.join("notes.txt"), vec![99; 3 * 1024 * 1024]).unwrap();
        let meta = create_torrent(
            &path,
            CreateTorrentOptions {
                piece_length: Some(64 * 1024),
                ..Default::default()
            },
            &BlockingSpawner::new(2),
        )
        .await
        .unwrap();
        let bytes = meta.as_bytes().unwrap();
        let list = self
            .seeder
            .add_torrent(
                AddTorrent::from_bytes(bytes.clone()),
                Some(AddTorrentOptions {
                    list_only: true,
                    ..Default::default()
                }),
            )
            .await
            .unwrap();
        let librqbit::AddTorrentResponse::ListOnly(list) = list else {
            panic!("expected metadata");
        };
        let small = list
            .info
            .iter_file_details()
            .position(|file| file.filename.to_pathbuf().file_name().unwrap() == "small.mp4")
            .unwrap();
        let source = TorrentSource::from_stream(&meta.info_hash().as_string(), None, &[]).unwrap();
        let handle = self
            .seeder
            .add_torrent(
                AddTorrent::from_bytes(bytes),
                Some(AddTorrentOptions {
                    output_folder: Some(path.to_string_lossy().into_owned()),
                    overwrite: true,
                    ..Default::default()
                }),
            )
            .await
            .unwrap()
            .into_handle()
            .unwrap();
        timeout(Duration::from_secs(10), handle.wait_until_initialized())
            .await
            .unwrap()
            .unwrap();
        (source, large, small)
    }

    pub fn engine(&self, cache_dir: std::path::PathBuf, cap: u64) -> TorrentEngine {
        TorrentEngine::offline(
            TorrentOptions {
                cache_dir,
                max_cache_bytes: cap,
                metadata_timeout: Duration::from_secs(5),
                no_peers_timeout: Duration::from_secs(5),
                idle_stop_after: Duration::from_secs(30),
                ..Default::default()
            },
            vec![self.peer],
        )
    }
}

impl Drop for Swarm {
    fn drop(&mut self) {
        let session = self.seeder.clone();
        tokio::spawn(async move {
            session.stop().await;
        });
    }
}

pub async fn connect(url: &str, range: Option<&str>, host: Option<&str>) -> TcpStream {
    let url = url::Url::parse(url).unwrap();
    let address = format!("127.0.0.1:{}", url.port().unwrap());
    let mut socket = TcpStream::connect(&address).await.unwrap();
    let mut request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n",
        url.path(),
        host.unwrap_or(&address)
    );
    if let Some(range) = range {
        request.push_str(&format!("Range: {range}\r\n"));
    }
    request.push_str("\r\n");
    socket.write_all(request.as_bytes()).await.unwrap();
    socket
}

pub async fn get(url: &str, range: Option<&str>, host: Option<&str>) -> (u16, String, Vec<u8>) {
    let mut socket = connect(url, range, host).await;
    let mut response = Vec::new();
    timeout(Duration::from_secs(20), socket.read_to_end(&mut response))
        .await
        .unwrap()
        .unwrap();
    let split = response
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .unwrap();
    let headers = String::from_utf8(response[..split].to_vec()).unwrap();
    let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, headers, response[split + 4..].to_vec())
}
