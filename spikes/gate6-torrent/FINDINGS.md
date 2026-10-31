# Gate 6: in-process librqbit to headless mpv

## Verdict and measured results

**Streaming/seeking PASS; the complete question's “only what is watched” requirement FAIL with this configuration.** One real default run on Windows 10, 2026-10-10 (Asia/Bangkok), obtained BBB metadata, streamed it through loopback HTTP, and passed all three seeks. No Sintel fallback was needed. The sandbox process helper failed to start, so commands ran through reviewed execution; the real swarm was reachable. No networking/firewall workaround or rule change was made. The DLL was loaded from the existing `PANORAMA_LIBMPV_DIR`; the download began in a fresh directory. Workspace compilation and the independent negative probe were running concurrently, so these are functional observations rather than isolated latency benchmarks.

BBB selected file: index **1**, `Big Buck Bunny.mp4`, **276,134,947 bytes**, duration **634.533 seconds**. Metadata discovery saw 227 peer addresses; this is distinct from live connected peers.

| Item | Verdict | Measured time | Received payload / file size | Verified bytes | Live peers | Download B/s |
| --- | --- | --- | --- | --- | --- | --- |
| Metadata | PASS | 1.639 s from start | 0 / 276,134,947 (snapshot after initialization) | 0 | 0 | 0 |
| First HTTP payload | PASS | 5.282 s from start | Not sampled at this event | Not sampled | Not sampled | Not sampled |
| Playback start | PASS | 11.752 s from start | 164,380,672 / 276,134,947 (59.53%) | 145,489,920 | 26 | 28,229,243 |
| Seek 25%, target 158.633 s | PASS | 10.260 s after command | 277,544,960 / 276,134,947 (100.51%) | 250,609,664 | 47 | 13,205,258 |
| Seek 60%, target 380.720 s | PASS | 1.945 s after command | 311,476,224 / 276,134,947 (112.80%) | 274,989,056 | 56 | 26,010,717 |
| Seek 90%, target 571.080 s | PASS | 1.158 s after command | 324,206,592 / 276,134,947 (117.41%) | 275,775,488 | 56 | 8,110,966 |
| Partial download | FAIL | Final measurement snapshot | 324,206,592 / 276,134,947 (117.41%) | 275,775,488 (99.87% of video size) | 56 | 8,110,966 |
| Clean shutdown | PASS on both runs | Real run: 26.834 s total internal elapsed; 29.260 s process wall time | mpv destroyed, HTTP stopped, session stopped, directory deleted | — | — | — |
| Bounded no-metadata/peers failure | PASS | 62.713 s from start to timeout log; 64.023 s internal total; 65.426 s process wall time | No media download; exit 2 | — | — | — |

The negative test uses a 60-second **metadata wait**, after DLL/session startup; total process lifetime includes startup, the library's one-second stop delay, cleanup, and runtime exit. Its exact message was `gate 6: FAIL: no metadata/peers within 60 s`. Neither probe remained running afterward. The real run's HTTP listener on port 51542 and both temp directories were checked after exit.

The real run exited **0**, as requested for playback plus all three seek successes. Its printed `gate 6: PASS` is that exit criterion, not certification of the partial-download condition. Received payload exceeded the file size because that counter includes redundant/unverified piece transfers. Verified progress was already approximately 99.87% of the video size after playing only brief intervals. Together with the scheduler source below, this disproves a watched-ranges-only claim for the selected-file configuration.

Release executable: **12,199,936 bytes (11.635 MiB)**, `target/release/gate6-torrent.exe`, excluding PDB and the external libmpv DLL.

### Validation

All required commands passed from `native/`, using the final Rust sources and lockfile:

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | PASS |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | PASS, no warnings |
| `cargo test --workspace --locked` | PASS, 5 tests total: 3 gate6 HTTP tests and 2 panorama-core tests |
| `cargo build -p gate6-torrent --release --locked` | PASS |

The three gate6 tests cover 16 in-memory requests: full 200, exact/open-ended/suffix/clamped 206 ranges, malformed and unsupported ranges, out-of-bounds/zero-length 416 responses, empty files, and HEAD without a body. They verify response headers and actual byte slices, and require neither network access nor the libmpv DLL. The offline tests completed in 0.01 s after compilation. The workspace test build took 16m 53s on this busy host, primarily rebuilding and linking dependencies; the target cache was preserved. Changes are limited to `native/Cargo.toml`, `native/Cargo.lock`, and this new spike.

### Actual complete real-run console output

Command: `./target/release/gate6-torrent.exe`; stdout and stderr combined, exit **0**.

```text
download directory: C:\Users\billi\AppData\Local\Temp\panorama-gate6-15452 (keep=false)
session started: DHT on; random TCP listener; UPnP off; persistence off; LSD off
metadata source: magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c&dn=Big+Buck+Bunny&tr=udp%3A%2F%2Fexplodie.org%3A6969&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337&tr=udp%3A%2F%2Ftracker.openbittorrent.com%3A80&ws=https%3A%2F%2Fwebtorrent.io%2Ftorrents%2F
metadata: PASS 1.639 s from start; seen peers=227
selected file: index=1 name=Big Buck Bunny.mp4 size=276134947 bytes
after metadata: downloaded=0/276134947 bytes (0.00%); verified=0 bytes; live peers=0; download speed=0 B/s
HTTP: http://127.0.0.1:51542/stream
first byte served: 5.282 s from start
playback start: PASS 11.752 s from start
playback start: downloaded=164380672/276134947 bytes (59.53%); verified=145489920 bytes; live peers=26; download speed=28229243 B/s
duration: 634.533 s
seek 25% target=158.633 s: PASS 10.260 s
after seek 25%: downloaded=277544960/276134947 bytes (100.51%); verified=250609664 bytes; live peers=47; download speed=13205258 B/s
seek 60% target=380.720 s: PASS 1.945 s
after seek 60%: downloaded=311476224/276134947 bytes (112.80%); verified=274989056 bytes; live peers=56; download speed=26010717 B/s
seek 90% target=571.080 s: PASS 1.158 s
after seek 90%: downloaded=324206592/276134947 bytes (117.41%); verified=275775488 bytes; live peers=56; download speed=8110966 B/s
final: downloaded=324206592/276134947 bytes (117.41%); verified=275775488 bytes; live peers=56; download speed=8110966 B/s
partial download: FAIL (117.41% of file fetched)
mpv destroyed
mpv stopped
HTTP stopped
session stopped
download directory deleted
clean shutdown: PASS; total elapsed 26.834 s
gate 6: PASS
```

### Actual no-peers console output

Command: `./target/release/gate6-torrent.exe --magnet 'magnet:?xt=urn:btih:0000000000000000000000000000000000000001'`; stdout and stderr combined, exit **2**.

```text
download directory: C:\Users\billi\AppData\Local\Temp\panorama-gate6-7704 (keep=false)
session started: DHT on; random TCP listener; UPnP off; persistence off; LSD off
metadata source: magnet:?xt=urn:btih:0000000000000000000000000000000000000001
metadata attempt timed out after 62.713 s from start
session stopped
download directory deleted
clean shutdown: PASS; total elapsed 64.023 s
gate 6: FAIL: no metadata/peers within 60 s
```

## Implementation and interpretation

This throwaway Windows spike pins Apache-2.0 `librqbit =9.0.1`. It uses a custom Axum `/stream` endpoint backed by `ManagedTorrent::stream(file_id)`, rather than enabling librqbit's full HTTP control API. The same generic `serve_reader` function is exercised with a Tokio-compatible in-memory `Cursor`, without torrent networking or libmpv. The endpoint binds only `127.0.0.1` on an OS-assigned port. Each request gets its own seekable reader. Responses include MIME type, `Accept-Ranges`, `Content-Length`, and `Content-Range` for 206/416. Open-ended and suffix ranges work; malformed/unsupported multipart ranges are ignored with 200. HEAD and unvalidated If-Range use full representation headers. Body reads time out after 90 seconds and observe shutdown.

Metadata resolution uses `list_only` before selecting the largest recognized video or the supplied zero-based file index. The returned torrent bytes are then added with `only_files = [index]` and the discovered peers, so no unselected file is queued for normal download. Whole torrent pieces can overlap neighboring files. The download directory is newly created as `%TEMP%/panorama-gate6-<pid>`; existing directories are refused. Session/DHT persistence and LAN service discovery are disabled. DHT remains enabled; a random dual-stack TCP peer listener is enabled; UPnP port forwarding is disabled.

The default sources are the exact supplied Big Buck Bunny and Sintel magnets. There is one 60-second metadata deadline: BBB gets up to 30 seconds, and Sintel gets the remaining budget if BBB yields no metadata. An explicit `--magnet` gets 60 seconds. Failure prints `no metadata/peers within 60 s` and exits 2 after cleanup. This is an application timeout: it does not prove that the swarm is dead or that zero peers exist. Only the two legal film hashes and the fixed nonexistent hash `0000000000000000000000000000000000000001` for the requested negative test are accepted. The negative hash is never used as media.

The copied/trimmed gate1 DLL loader lives entirely in this crate. `PANORAMA_LIBMPV_DIR` is required before starting a session. Options include `vo=null`, `ao=null`, `hwdec=no`, `cache=yes`, `demuxer-max-bytes=64MiB`, and a 90-second network timeout. No video window or audio output is created. Playback start is the first pair of increasing nonnegative `time-pos` events. Duration discovery and playback start each have a 90-second bound. Seeks use `absolute+exact` at 25%, 60%, and 90%; a seek passes only after an mpv SEEK event and increasing `time-pos` observations within ten seconds of the target, reaching at least target + 1 second, within 90 seconds. Timeouts are reported separately and the next seek is still attempted. Exit 0 requires playback and all three seeks. The partial-download verdict is printed separately.

Reported downloaded bytes are `live.snapshot.fetched_bytes`, torrent payload received (including possible duplicate/unverified data), rather than sparse file logical size or HTTP bytes. Verified bytes are printed separately. Peer count sums TCP/uTP/SOCKS live connections; speed is the library's rolling estimate in B/s. First-byte time is observed when the HTTP body first produces payload, immediately before Hyper sends it, rather than a socket-level timestamp. Startup times use one monotonic origin after Tokio runtime creation; seek times use their own origins. The final snapshot precedes mpv/HTTP/session shutdown, so a few further in-flight bytes may arrive during teardown.

**The stock selected-file configuration is not strictly demand-only.** Streaming prioritizes a 32 MiB lookahead for every active reader, then the piece scheduler falls back to the selected file's remaining queue. mpv also reads ahead into its 64 MiB cache. Consequently a short run downloading less than the full file demonstrates partial transfer, but cannot prove that every downloaded byte belonged to a watched range. A fast swarm or a long session can fetch the entire selected file. Phase 4 must separately validate a policy that actually restricts background fetching; this spike does not alter library internals to hide that behavior.

## Exact source evidence

Registry root on this machine: `C:/Users/billi/scoop/persist/rustup/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`. All references below are installed 9.0.1 sources, not upstream HEAD.

| Source relative to registry root | API / behavior |
| --- | --- |
| `librqbit-9.0.1/src/session.rs:395`, `:418`, `:568` | `DhtSessionConfig`, `SessionOptions`, `Session::new_with_opts`; DHT persistence can be disabled independently. |
| `librqbit-9.0.1/src/session.rs:244`, `:306`, `:355`, `:359`, `:1087` | `AddTorrentOptions`, `AddTorrentResponse`, `AddTorrent::from_url`, `AddTorrent::from_bytes`, `Session::add_torrent`. |
| `librqbit-9.0.1/src/session.rs:1298`, `:1635` | `list_only` returns metadata/torrent bytes/seen peers without managing a download; magnets resolve metadata from peers. |
| `librqbit-9.0.1/src/torrent_state/mod.rs:567` | `ManagedTorrent::wait_until_initialized`; separate from magnet metadata acquisition. |
| `librqbit-9.0.1/src/torrent_state/streaming.rs:337`, `:167`, `:246` | `ManagedTorrent::stream(self: Arc<Self>, file_id)` returns `FileStream`, which implements Tokio `AsyncRead` and `AsyncSeek`. Reads wait for verified pieces; seeking updates stream position. |
| `librqbit-9.0.1/src/torrent_state/streaming.rs:29`, `:44`, `:73` | Fixed 32 MiB per-stream lookahead and interleaving active streams. |
| `librqbit-9.0.1/src/piece_tracker.rs:123`, `:136`, `:149` | Piece acquisition tries stream priorities, then ordinary queued pieces: priority is not exclusivity. |
| `librqbit-9.0.1/src/torrent_state/mod.rs:503`; `src/torrent_state/stats.rs:10`, `:72`; `src/torrent_state/live/stats/snapshot.rs:8`; `src/torrent_state/live/peers/stats/mod.rs:12` | `ManagedTorrent::stats`, speeds, received payload bytes, verified progress, live peer counters. |
| `librqbit-9.0.1/src/listen.rs:52`, `:63` | Listener mode/address/announce port/UPnP controls; defaults to random dual-stack TCP and UPnP off. |
| `librqbit-9.0.1/src/session.rs:1059` | `Session::stop` pauses torrents, cancels the session token, then sleeps one second. It does not join every task; shutdown is best effort at the library boundary. |
| `librqbit-9.0.1/Cargo.toml:24`, `:34`, `:38`, `:44`, `:50`, `:55`, `:68`, `:70` | Apache-2.0 package metadata and feature definitions. |
| `librqbit-core-9.0.1/src/magnet.rs:8`, `:63` | `Magnet` fields and parser recognize info hashes, trackers, name, and selection; unrecognized query fields including `ws` are ignored. |

Default features are disabled: `default-tls` and `http-api-client` are unnecessary. Only `rust-tls` is enabled to provide HTTPS tracker support and the ring SHA-1 implementation instead of the default native TLS/hash backend. `http-api`, `webui`, `postgres`, `upnp-serve-adapter`, watch, metrics exporter, and tracing subscriber utilities remain off. The library still depends unconditionally on its UPnP port-forwarding subcrate; that is distinct from the optional media-server adapter, and runtime forwarding stays off. No direct dependencies beyond the user-authorized list were added. Versions resolved here: Tokio 1.53.2, Axum 0.8.9, Hyper 1.12.0, http 1.5.0, Tower 0.5.3, libloading 0.8.9.

The workspace lockfile grew from 822 to 919 packages: **96 new dependency packages plus the gate6 crate**, zero removed packages. `cargo metadata --locked --filter-platform x86_64-pc-windows-msvc` resolves **320 dependency packages** reachable from gate6, counting build dependencies and dependencies shared with the existing workspace; this is not 320 additional crates. No target directory was deleted.

Unset-DLL-path probe (no network): exit **1**, `gate 6: FAIL: PANORAMA_LIBMPV_DIR is unset; set it to the directory containing libmpv-2.dll`. Both measurement temp directories were absent after exit, no gate6 process remained, and the real run's loopback port had no listening socket.

## Phase 4 production requirements

- Create the session lazily on the first torrent stream request, reuse it across requests, and stop it after a measured idle interval once mpv has closed its readers. Track ownership explicitly to avoid stopping a session during a seek or another playback. Propagate cancellation through metadata, reads, and player shutdown. Replace the library's one-second best-effort stop assumption with verified task/socket teardown. `mpv_terminate_destroy` and the copied thread join have no hard timeout; the 90-second measurement waits do not bound DLL destruction.
- Use a dedicated cache with an actual byte cap and an eviction policy for inactive torrents. Sparse file length is not downloaded occupancy. Protect active readers and pieces from eviction, account for concurrent sessions/in-flight writes, and verify free space before creating large files. The spike's whole-file queue does not enforce a disk cap or strictly watched-range fetching.
- Stremio stream objects carry `infoHash`, optional `fileIdx`, and optional `sources`. Build `magnet:?xt=urn:btih:<infoHash>` with encoded `tr` parameters from validated `tracker:` sources after removing that prefix; preserve the zero-based `fileIdx` as `AddTorrentOptions.only_files`. Do not forward `sources` blindly: `dht:` entries are not tracker URLs and need separate handling. Without an index, resolve metadata and pick the desired video. In 9.0.1 `AddTorrentOptions.trackers` is extended for torrent-byte/URL input, while the magnet branch reads trackers from the URI, so include them in the magnet itself. See the [Stremio stream contract](https://github.com/Stremio/stremio-addon-sdk/blob/master/docs/api/responses/stream.md).
- Keep HTTP strictly loopback. Choose and expose the peer/DHT ports separately: `ListenerOptions.listen_addr`, `announce_port`, and `DhtSessionConfig.port`. Decide whether inbound TCP/uTP and opt-in UPnP port mapping are needed. Listening on all interfaces can trigger Windows Firewall permission prompts; existing rules, privileges, and policy affect the result. This spike does not create firewall rules or grant prompts. Test outbound-only behavior and restrictive NAT/firewalls outside this sandbox. See [Microsoft's firewall application-rule behavior](https://learn.microsoft.com/en-us/windows/security/operating-system-security/network-security/windows-firewall/rules).
- Detect stalled discovery with a metadata deadline; after initialization combine sustained zero live/connecting peers, discovery errors, and lack of received-byte progress. Zero live peers at one instant is not proof of permanent unavailability. Preserve distinct causes (tracker failure, blocked UDP/DHT, unreachable peers, metadata timeout) and offer bounded retry/cancellation. This spike intentionally reports the honest combined `no metadata/peers` error.
- The user's public IP is visible to peers and trackers; DHT also exposes discovery activity. Default uploads remain enabled. Local HTTP does not anonymize BitTorrent. Production needs informed torrent-source activation, appropriate upload limits, and network-interface/proxy decisions; private torrents must not be announced publicly. Do not log authenticated tracker URLs in production.

## Top risks

1. **Download scope and cache growth:** stream priority plus background selected-file downloading, 32 MiB lookahead per reader, mpv caching, and piece overlap are incompatible with a strict “only watched bytes” promise without further policy work.
2. **Reachability and startup/seek variability:** UDP trackers/DHT, peer availability, NAT/firewall rules, and file/container layout can prevent metadata or timely seek completion. librqbit's magnet path does not turn this magnet's `ws` parameter into a guaranteed web-seed fallback.
3. **Lifecycle and privacy:** best-effort session cancellation and an unbounded mpv destroy/join need production hardening; peers see the user's IP and default sharing sends uploads.

## Reproduction

Run from `native/` in PowerShell:

```powershell
$env:PANORAMA_LIBMPV_DIR = 'D:\codeOS\panorama\.cache\panorama\windows-libmpv\current'
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build -p gate6-torrent --release --locked
./target/release/gate6-torrent.exe
```

Keep downloads for inspection: `./target/release/gate6-torrent.exe --keep`. Set a zero-based selection with `--file-index <n>`. To give BBB the full 60 seconds rather than the default fallback split, pass the complete BBB magnet printed in the console using `--magnet '<uri>'`.

Requested bounded negative test:

```powershell
./target/release/gate6-torrent.exe --magnet 'magnet:?xt=urn:btih:0000000000000000000000000000000000000001'
$LASTEXITCODE
```
