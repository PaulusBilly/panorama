use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Instant,
};

use super::*;

#[test]
fn clear_worker() {
    let Some(root) = std::env::var_os("PANORAMA_IMAGES_TEST_CACHE") else {
        return;
    };
    let loader = ImageLoader::new(ImageLoaderOptions {
        cache_dir: root.into(),
        ..Default::default()
    })
    .unwrap();
    println!("PANORAMA_IMAGES_INITIALIZED");
    std::io::stdout().flush().unwrap();
    let (command_tx, command_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut command = String::new();
        std::io::stdin().read_line(&mut command).unwrap();
        command_tx.send(command).unwrap();
    });
    assert_eq!(
        command_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .trim(),
        "CLEAR"
    );
    loader.on_blocked_lock(Box::new(|| {
        println!("PANORAMA_IMAGES_BLOCKED");
        std::io::stdout().flush().unwrap();
    }));
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(loader.clear())
        .unwrap();
    println!("PANORAMA_IMAGES_CLEARED");
    std::io::stdout().flush().unwrap();
}

struct Worker(Child);

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let deadline = Instant::now() + Duration::from_secs(2);
        while matches!(self.0.try_wait(), Ok(None)) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn signal(lines: &mpsc::Receiver<String>, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let line = lines
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
        if line.contains("PANORAMA_IMAGES_") {
            assert!(line.contains(expected), "unexpected worker signal: {line}");
            return;
        }
    }
}

#[test]
fn second_process_clears_only_after_reader_releases_lock() {
    let dir = tempfile::tempdir().unwrap();
    let config = options(dir.path());
    let cache = super::super::cache::Cache::new(&config).unwrap();
    let url = ImageUrl::parse("https://example.test/process").unwrap();
    let key = super::super::cache::Cache::key(&url);
    let bytes = encoded(ImageFormat::Png, 80, 40);
    cache.insert(&key, &bytes, 0, 0).unwrap();
    let mut lock_path = fs::canonicalize(&config.cache_dir)
        .unwrap()
        .into_os_string();
    lock_path.push(".images.lock");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(lock_path)
        .unwrap();
    let mut child = Worker(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "images::tests::process::clear_worker",
                "--nocapture",
            ])
            .env("PANORAMA_IMAGES_TEST_CACHE", &config.cache_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let stdout = child.0.stdout.take().unwrap();
    let (lines_tx, lines_rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if lines_tx.send(line.unwrap()).is_err() {
                break;
            }
        }
    });
    signal(&lines_rx, "PANORAMA_IMAGES_INITIALIZED");
    lock.lock().unwrap();
    let mut reader = fs::File::open(config.cache_dir.join(&key[..2]).join(&key)).unwrap();
    writeln!(child.0.stdin.as_mut().unwrap(), "CLEAR").unwrap();
    child.0.stdin.as_mut().unwrap().flush().unwrap();
    signal(&lines_rx, "PANORAMA_IMAGES_BLOCKED");
    assert!(matches!(
        lines_rx.recv_timeout(Duration::from_millis(150)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    assert!(child.0.try_wait().unwrap().is_none());
    let mut read = Vec::new();
    reader.read_to_end(&mut read).unwrap();
    assert_eq!(read, bytes);
    drop(reader);
    drop(lock);
    signal(&lines_rx, "PANORAMA_IMAGES_CLEARED");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < deadline, "clear worker did not exit");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(fs::read_dir(config.cache_dir).unwrap().count(), 0);
}
