# Panorama native

This workspace is the foundation for Panorama's Rust and GPUI desktop app.
`panorama-core` contains shared logic without UI dependencies.
`panorama-app` opens the initial native window.

Install rustup; this workspace selects Rust 1.99.0 with rustfmt and Clippy.
On Windows, install the MSVC Build Tools with Desktop development with C++
and a Windows SDK for the `x86_64-pc-windows-msvc` target.

Run these commands from `native/`:

```sh
cargo run -p panorama-app
cargo test --workspace
```

The Electron app remains the shipping app.

## Dependencies

- `rusqlite` (=0.40.2, `bundled`): the native key-value store; ships SQLite without a system dependency.
- `tempfile` (=3.27.0, dev-dependency): isolated directories for store tests.
- `image` (=0.25.10, defaults off; `jpeg`, `png`, `webp`): content sniffing, bounded decoding (4096×4096, 64 MiB), and aspect-preserving downscaling.
- `reqwest` (=0.12.28, defaults off; `rustls-tls`, `stream`): async HTTPS requests with capped streamed bodies and restricted redirects; no cookie store.
- `tokio` (=1.53.2; `rt-multi-thread`, `macros`, `sync`, `time`, `net`): async image downloads, concurrency limits, timeouts, and blocking workers for disk/CPU work. Tests additionally enable `test-util` and `io-util` for paused time and local transport.
- `futures` (=0.3.34): shared download futures whose final waiter cancels interest, and response stream consumption.
- `sha2` (=0.10.9): normalized-URL SHA-256 cache names without metadata in filenames.
- `axum` (=0.8.9, dev-dependency, defaults off; `http1`, `tokio`): deterministic HTTP test server on 127.0.0.1 with counters and explicit release gates, without public internet.
- `tokio-rustls` (=0.26.6, dev-dependency, defaults off; `ring`): TLS around the local test server, exercising the production HTTPS and redirect policies.
- `rcgen` (=0.14.7, dev-dependency, defaults off; `ring`, `crypto`): ephemeral local TLS certificates trusted only by test clients; production certificate verification stays enabled.

## Images

`panorama_core::images` exports `ImageUrl`, `ImageTarget`, `ImageRequest`,
`ImageLoaderOptions`, `ImageLoader`, `DecodedImage`, and sanitized `ImageError` kinds.
`ImageLoader::new` synchronously rebuilds/prunes its disk index; construct it off the
UI thread, then call async `load` and `clear` in a Tokio runtime. Decoding, resizing,
and cache I/O run on blocking workers. Same-URL loads share acquisition and decode;
dropping the last interested future cancels a pending download.

The dedicated cache stores original encoded bytes at
`<cache_dir>/<first two hash digits>/<SHA-256(normalized URL)>`. Same-directory
temporary writes are flushed and atomically renamed. An in-memory index is rebuilt
on startup and refreshed under a cross-process lock to include other processes'
inserts and reads. Modified times, touched on reads, provide restart-safe LRU order.
Overflow evicts oldest files until at most 90% of the disk budget remains; startup
also enforces a lowered budget. Originals larger than the budget are decoded without
retention. A corrupt disk hit is deleted and fetched once.

The persistent sibling `<cache_dir>.images.lock` contains a clear generation and
serializes cache reads, publication, eviction, and clearing across processes.
Readers close image handles before releasing it, so Windows deletion waits for
cooperating readers and cannot invalidate their reads. `clear` empties the cache
directory and prevents downloads that observed an earlier generation from inserting;
the sibling coordination file stays in place. Abandoned temporary files are cleaned
under that lock. Use a dedicated directory with the user's normal profile ACLs.

Image tests cover URL validation/redaction; generated PNG/JPEG/WebP decoding,
RGBA channels, aspect ratio and no upscaling; original-byte reuse and offline restart;
Content-Length and chunked streaming caps; content sniffing, decoder limits, malformed
images and HTTP status; HTTPS-only redirects and the five-hop boundary; single-flight,
download concurrency and cancellation; LRU budget/read touches/restart; concurrent
loaders and a second-process Windows-safe read/clear; corrupt-hit recovery; paused-clock
timeout; clear during downloads; zero-budget retention and abandoned temporary cleanup.

The GPUI adapter remains separate: own a byte-bounded memory LRU of `RenderImage`s,
convert straight RGBA8 to BGRA, and call `cx.drop_image` on eviction. Avoid synchronous
logging or file work on the UI thread, as established by Phase 0 Gate 2.
