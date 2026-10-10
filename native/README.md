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
- `librqbit` (=9.0.1, default features off, `rust-tls`): the spike-proven BTv1 engine;
  excludes its control HTTP API/client and native TLS backend.
- `tokio` (=1.53.2), `tokio-util` (=0.7.20): private asynchronous runtime,
  cancellable I/O, timers, and cancellation tokens; versions already in the lockfile.
- `hyper` (=1.12.0), `hyper-util` (=0.1.21), `http-body-util` (=0.1.5),
  `http` (=1.5.0), `bytes` (=1.12.1), `futures` (=0.3.34): streaming loopback
  HTTP/1 and Range responses; match the Phase 3 proxy's pins.
- `fs4` (=0.13.1, `sync`): free-space/allocation-granularity queries,
  physical preallocation, and allocated-byte measurement; matches the media cache.
- `cap-std` / `cap-primitives` (=4.0.3): safe directory-relative I/O and no-follow
  directory opens; cleanup stays anchored to verified handles.
- `same-file` (=1.0.6): retained directory identity (Windows volume serial/file
  index; Unix device/inode), already present transitively in the lockfile.
- `getrandom` (=0.3.4): 128-bit operating-system-random bearer tokens.
- `url` (=2.5.8): tracker validation and magnet query encoding.
- `anyhow` (=1.0.104): required by librqbit's storage interface; public errors
  discard library diagnostics, which can contain tracker credentials.
- `tracing` (=0.1.44): disable upstream tracing on every private runtime thread,
  including its blocking pool, because librqbit logs tracker URLs internally.

## Torrent engine (PR 4.1)

`panorama_core::torrent` is UI-free. `source.rs` validates addon hashes and trackers;
`engine.rs` owns the public lifecycle; `opening.rs` resolves metadata, chooses
files, reuses torrents, and evicts inactive entries; `directory.rs` retains cache
directory handles and verifies ownership; `cache.rs` implements librqbit's storage interface; `stream.rs` owns leases,
readers, and stats; `server.rs` serves authenticated Range requests; `runtime.rs`
owns the private runtime and teardown. Tests use real, locally created torrents
and a seeder on 127.0.0.1 with DHT, trackers, and LAN discovery disabled.

Public entrypoints are `TorrentSource::from_stream`, `TorrentEngine::new`,
`TorrentEngine::open(source, CancellationToken)`, `TorrentStream::stats`,
`TorrentStream::close().await`, and `TorrentEngine::shutdown().await`.
`new` is synchronous and performs no I/O; options are validated at `open`.
Invoke the asynchronous lifecycle methods on a Tokio runtime.
`TorrentOptions { cache_dir, ..Default::default() }` sets a dedicated **initially
absent** directory whose parent exists. Existing directories are refused and
never deleted. The engine owns this directory until shutdown or idle teardown.
Defaults are 10 GiB of allocated cache data, five minutes idle, 45 seconds for
metadata/initialization, 60 seconds for first payload, and 1 MiB/s upload.
The metadata and first-payload deadlines share the public open's origin,
including lazy startup and waiting for idle teardown, rather than adding
45 and 60 seconds. Cancellation applies throughout open and the returned lease.

HTTP binds 127.0.0.1 on an OS-assigned port. URLs use `/torrent/<32 hex digits>`;
requests require that token, an exact single Host header, origin-form URI, and a
loopback peer. GET/HEAD and single byte ranges (including suffixes) are supported;
multipart/malformed ranges and unvalidated If-Range fall back to the full file.
At most eight HTTP readers share one torrent. By default no incoming TCP/uTP
peer listener exists and UPnP/LAN discovery/persistence are disabled.
An explicit `listen_port` enables IPv4 TCP on that port (0 selects a port),
with UPnP still disabled. If both library sessions are needed, the first owns
this listener; the other is outbound-only to avoid binding the same port twice.
DHT-allowed discovery uses DHT without persistence and an
ephemeral UDP socket on 0.0.0.0. librqbit cannot express an outbound-only DHT
socket; absence of a peer listener is not a guarantee about Windows Firewall
prompt behavior. No firewall rules are installed.

### Source privacy and discovery

A `tracker:` source without a matching, valid `dht:<hash>` marker is possibly
private. Its unresolved magnet is resolved in a separate lazy Session with
`SessionOptions.dht = None`; that session remains DHT-disabled for playback,
even if metadata later proves public. An explicit matching DHT marker permits
DHT. With neither trackers nor a DHT marker, DHT is permitted because there is
no other discovery method. Invalid `tracker:` entries still suppress DHT; a
marker for another hash does not authorize this hash. Markers are scanned before
the 20-tracker cap. Existing resolved entries can be reused: librqbit then knows
the metadata's private flag. Explicitly authorizing DHT for a private magnet can
expose its hash and the caller's IP before metadata is learned. Trackers also
learn the hash and IP; tracker-only discovery does not provide anonymity.
Upload limits apply to each library session; the two sessions share the disk
budget and HTTP service and stop concurrently.
Tracker-only metadata discovery still needs a peer willing to serve metadata.
The bundled seeder refuses `ut_metadata` requests for private torrents
(`src/torrent_state/live/mod.rs:1083-1098`); such peers can yield a bounded
`MetadataTimeout`. The local tracker-only regression therefore uses public
metadata behind a possibly private source, and keeps DHT disabled after resolution.

Installed `librqbit-9.0.1` evidence (paths relative to that registry package):

- `src/session.rs:244-294`: `AddTorrentOptions` includes `disable_trackers` and
  `peer_opts`, but no per-torrent DHT override. `src/peer_connection.rs:71-81`
  restricts `peer_opts` to connection/read-write/keepalive timeouts.
- `src/session.rs:394-428`, `:606-634`: DHT is configured on the Session;
  `dht = None` creates no DHT instance. `disable_trackers` disables trackers,
  not DHT (`:1553-1564`).
- `src/session.rs:1230-1259`: an unresolved magnet has `private = false` and
  creates its peer stream before resolving metadata, including in list-only mode.
- `src/session.rs:1537-1543`: non-private discovery calls DHT `get_peers`;
  an announce port is passed when announcing. `:1511-1524` uses known metadata's
  private flag on managed-torrent resume.

The local tracker test supplies compact loopback peers with no injected peers,
reads the seeder's media, asserts a DHT-disabled session, and observes no UDP
bootstrap traffic. No public tracker or bootstrap endpoint is used in tests.

### Download policy and installed 9.0.1 source evidence

Paths below are relative to the Cargo registry's `librqbit-9.0.1/` directory,
not upstream HEAD:

| Source | Behavior used |
| --- | --- |
| `src/session.rs:244-294`, `:1298-1308` | Initialize paused with `only_files=[chosen]`; list-only metadata fetch writes no payload. |
| `src/session.rs:1617-1625`; `src/chunk_tracker.rs:323-365` | Public `update_only_files(empty)` clears the ordinary selected-file download queue. |
| `src/torrent_state/live/mod.rs:819-835` | Active streams of unselected files still need peers, even with no selected background files. |
| `src/torrent_state/streaming.rs:29-50`, `:337-381` | Every FileStream registers its position and prioritizes a fixed 32 MiB lookahead, rounded to pieces. |
| `src/piece_tracker.rs:136-160` | Streaming priorities precede the ordinary queue; clearing that queue avoids the spike's whole-file fallback. |
| `src/chunk_tracker.rs:254-270`; `src/piece_tracker.rs:104-111` | Broken/in-flight pieces can be requeued. Clear the background queue again before each new reader/resume. |
| `src/torrent_state/live/mod.rs:1320-1335`; `src/torrent_state/streaming.rs:361-374`; `src/session.rs:1522`, `:1610-1614` | Disconnects can leave peers marked not needed across the reader-registration gap. Public pause/resume rebuilds discovery with known peers. |
| `src/storage/mod.rs:142-191` | Public storage write/length/take hooks allow a write-time byte cap without forking. |
| `src/session.rs:1108-1118` | Magnet trackers come from the URI, so filtered addon trackers are encoded as `tr`. |
| `src/session.rs:1059-1075` | Stop cancels/pauses then sleeps one second; no join handles are exposed for all session tasks. |

The chosen file is initialized as selected, then the ordinary queue is cleared
before registering an HTTP reader and resuming. Other files are never opened for
playback unless another lease explicitly chooses them. The torrent pauses when
no reader is active (50 ms monitor interval), including between open's one-byte
availability probe and mpv's first request. Multiple readers have independent
positions and 32 MiB lookaheads. Reader changes retain a healthy live generation;
pausing during librqbit's piece verification can strand a received piece before
it is marked verified. Its piece requester observes the updated stream queues
on subsequent requests or its five-second idle wake. With no connected or connecting
peers, the monitor also retries at most every 500 ms without extending the
no-peers deadline. Pieces can overlap neighboring files, and seeks,
failed pieces, and in-flight requests retain work from earlier windows. This is
not a strict current-position-only download guarantee; the storage cap is the
unconditional bound on engine-owned cache payload allocation.

The lookahead regression keeps a direct reader stationary at 4 MiB in a
64 MiB local torrent, waits for the lookahead to fill and settle, and checks a
two-second plateau while the reader is still registered. Its ceiling is
4 MiB + 32 MiB + one 64 KiB boundary piece. The seeder's test limit is
32 MiB/s to shorten transfers without shortening the plateau observation.
Removing the ordinary-queue clearing is tested as a deliberate mutation; the
previous read-and-close check could pass because closing paused the torrent.

With a nonreading client keeping its reader alive and an **8,388,608-byte cap**,
the engine stopped at **8,388,608 allocated bytes** and **8,323,072 verified bytes**,
receiving **8,388,608 to 8,404,992 bytes** across two runs (0 to 16 KiB of network
payload beyond the cap, including redundant transfers), with **zero disk
overshoot** and `stats().error == Some(Disk(Full))`. These are local-seeder
measurements, not speed or peer-availability promises for public swarms.

### Cache and teardown guarantees

Media is stored as fixed 64 KiB blocks, identified by file index and block number;
a distant seek does not create a giant sparse file. A shared mutex reserves the
allocation-granularity-rounded charge **before** each new block. `fs4::allocate`
preallocates its storage, `allocated_size` verifies the charge, and writes only
overwrite that reserved extent. Unexpected allocation sizes are rejected;
compressed/sparse allocation schemes are not supported. The budget counts
allocated file data, including unfinished/unverified pieces and block padding,
not logical torrent length; filesystem bookkeeping is outside this data budget.
Free space is checked before adding a torrent and before every new block.
Before adding, the engine makes room for up to one 32 MiB window (or the smaller
file/cap) by evicting whole inactive torrents in LRU order when either the cache
budget or filesystem free space is insufficient. Open leases and active
readers cannot be evicted. A rejected write records `Disk(Full)` immediately,
and library fatal-error handling or the monitor stops fetching. Already received
network payload can exceed the disk cap; it cannot allocate another cache block.

Cache creation retains a no-follow directory handle immediately after creation;
no canonicalized replacement target is adopted. Reparse points (Windows file
attribute 0x400) and Unix symlinks are rejected. Before cleanup, the path entry
is opened without following links and compared with the still-open identity
handle (volume serial + file index on Windows; dev + ino on Unix). Contents are
removed relative to those verified directory handles; each nested directory is
verified before recursion and again before its final empty-directory removal.
No path-based `remove_dir_all` is used. Windows handles deny delete sharing,
preventing rename/replacement while owned; tests exercise that denial and reject
an inserted junction without touching its external sentinel.

A disposable Windows demonstration of the old sequence created `cache`, replaced
it with a junction to `external`, canonicalized it, then recursively deleted the
adopted path: the external sentinel disappeared. Identity replacement likewise
made the old cache-retirement regression incorrectly return success.
Residual risk: creation and first no-follow open are separate OS operations, so
an ordinary directory substitution before identity capture is not atomic.
Concurrent mutation can make cleanup fail and leave cache data behind. The final
empty-directory removal follows closing its handles; a replacement at that
point can only cause failure or removal of an empty directory, not recursive
deletion of the replacement's contents. Keep the cache parent application-owned.

Initialization is coordinated per hash. Metadata, initialization, and first
payload waits do not hold the global torrent-state lock, and initializing hashes
cannot be evicted. A cancelled initializer releases its gate and operation count.
An inactive entry with `NoPeers` clears that playback error and progress/retry
clocks on its next open; readers and active leases retain their current errors.
The local test pauses the seeder, records `NoPeers`, closes, resumes the seeder,
then reopens and reads a previously unavailable distant range.

HTTP reads wait through seeks while peers are live or connecting, or received
payload keeps progressing. `NoPeers` requires no live or connecting peers and a
full configured window without received-byte progress. Pauses, reader resumes,
new readers, and seeks reset that window; automatic peer retries do not. A body
failure is propagated to Hyper to abort the connection
with an incomplete Content-Length, so HTTP clients report a truncated response.
Tests that enable DHT are ignored because librqbit binds it on 0.0.0.0 and can
trigger Windows Firewall prompts. Default-run tests bind only to 127.0.0.1.

The old fixed 30 ms startup probe was reproduced with a held startup barrier:
it panicked on an absent Host. The pending-open test now waits for a local peer
connection (which proves startup and pending metadata discovery), asserts the
operation is still active, then aborts it and verifies idle cleanup.

Deliberately disabling the fixes failed the DHT-disabled-session assertion,
the cached-open deadline, reopening with `NoPeers`, the active-reader lookahead
ceiling, and the pre-shutdown UDP ownership assertion. The held-startup
demonstration also reproduced the old 30 ms probe's absent-Host panic.

Shutdown cancels opens/leases, drops the listener, gives tracked Hyper connection
tasks 300 ms to drain, then aborts and joins the remaining tasks. Library stop is
bounded at three seconds. All library work runs on a dedicated thread/runtime
with suppressed tracing, so detached async library tasks are disposed when that
runtime shuts down. Its blocking-pool shutdown has a two-second bound; librqbit
still does not provide individual task/socket join handles, and a stuck OS
filesystem operation cannot be forcibly terminated safely. The owner thread is
joined off the caller's thread. Tests verify HTTP and peer TCP port closure,
DHT UDP ownership before shutdown and rebinding after shutdown with a local
bootstrap endpoint (including receipt of its bootstrap request), release of the Session even while stream objects
remain, startup cancellation, bounded nonreading-client shutdown
(locally about one second), and removal of the directory. Drop signals teardown
without joining or blocking; explicit shutdown is needed to await its outcome.
Idle teardown also removes the cache; the next open starts a fresh session.

The player in PR 4.2 should retain the `TorrentStream`, give `url` to mpv, poll
`stats()` for peers/rates/state/error, and call `close().await` on source change.
Show `MetadataTimeout`/`NoPeers` from `open`, and `Stalled` plus `error` after
playback begins. Cancel the token to interrupt pending metadata/reads, and await
the engine's `shutdown()` on application exit. Tracker/magnet strings and token
URLs must not be logged. `TorrentStats.error` is an intentional API addition so
the player can distinguish `Disk(Full)` from `NoPeers` while both are stalled.
