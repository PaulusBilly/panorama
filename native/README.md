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

- `stremio-core` (git revision `474ffaa42e0b7b9bcba0dd99368c41e828579dec`, `derive`, `env-future-send`): account models, authentication, official addons and saved-state migrations, matching the Phase 0 spike.
- `reqwest` (=0.12.28, rustls only): bounded JSON HTTP requests with HTTPS and origin-safe redirects.
- `tokio` (=1.53.2): background core execution, FIFO persistence and blocking SQLite workers.
- `futures` (=0.3.34): core futures and runtime event streams.
- `serde` (=1.0.229): typed core bucket serialization.
- `serde_json` (=1.0.151): opaque JSON bytes for saved core state and API payloads.
- `url` (=2.5.8): API origin validation and redirect checks.
- `http` (=1.5.0): the core Env's typed request contract.
- `chrono` (=0.4.45): UTC timestamps required by the core Env.
- `tracing` (=0.1.44): scoped disabled subscribers suppress upstream secret-bearing diagnostics even when the app enables tracing.
- `tiny_http` (=0.12.0, dev-dependency): local mock Stremio API without public network access.
- `rcgen` (=0.14.5, dev-dependency): ephemeral certificates for trusted local HTTPS redirect tests.
- `tokio-rustls` (=0.26.6, dev-dependency): local TLS mocks that exercise redirect origin and header rules over HTTPS.
- `rusqlite` (=0.40.2, `bundled`): the native key-value store; ships SQLite without a system dependency.
- `tempfile` (=3.27.0, dev-dependency): isolated directories for store tests.
