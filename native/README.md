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
- `tokio` (=1.53.2): asynchronous media requests, loopback sockets, cancellation, tracked tasks and virtual-time tests.
- `reqwest` (=0.12.28, defaults disabled, `rustls-tls`, `stream`): cookie-free streaming upstream HTTP with manual redirects; no native TLS.
- `hyper` (=1.12.0, `server`, `http1`): direct loopback HTTP server without a web framework.
- `hyper-util` (=0.1.21, `tokio`): adapts Tokio sockets to Hyper.
- `http-body-util` (=0.1.5): bounded streaming response bodies.
- `http` (=1.5.0): shared HTTP methods, status codes and headers.
- `bytes` (=1.12.1): chunk buffers and shared immutable cache data.
- `futures` (=0.3.34): injectable asynchronous transports and streaming bodies.
- `sha2` (=0.10.9): SHA-256 cache filenames; source URLs never identify cache entries.
- `getrandom` (=0.3.4): operating-system randomness for 128-bit session tokens and owner names.
- `httpdate` (=1.0.3): HTTP Retry-After and Last-Modified dates.
- `fs4` (=0.13.1, `sync`): safe free-space, filesystem block-size and actual file-allocation queries. Owner locks use stable `std::fs::File::try_lock`.

## Native media proxy

`panorama_core::media` ports the Electron range proxy, cache, validators, retries,
manual redirect transport and buffering policy. See
[`panorama-core/MEDIA_PORT.md`](crates/panorama-core/MEDIA_PORT.md) for the test
mapping, refresh contract and Phase 3.3 player integration.
