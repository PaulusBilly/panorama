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
- `libloading` (=0.8.9): Gate 1's runtime loader; no link-time mpv dependency.
- `windows` (=0.62.2): matches GPUI and Gate 1; only Foundation, GDI (window
  class/background), LibraryLoader, Threading (window owner validation), keyboard
  window disabling, and WindowsAndMessaging features are enabled here.
- `raw-window-handle` (=0.6.2): matches GPUI's `HasWindowHandle` contract.
- `panorama-mpv` uses standard-library mpsc channels; no channel dependency.
- `serde_json` (=1.0.151, Windows dev-dependency): parses explicit mpv IPC clock
  readings in playback tests; reuses the version already in the lockfile.

## panorama-mpv (PR 3.1)

`panorama-core` remains independent and unsafe-free. The new library contains:

| Module | Responsibility |
| --- | --- |
| `player.rs` | Safe command API, event worker, asynchronous close and teardown deadline |
| `types.rs`, `error.rs` | Copied public values and static sanitized errors |
| `options.rs` | Electron defaults and source-drift test |
| `events.rs` | Owned node parsing, track/device lists and buffering snapshots |
| `subtitle.rs` | mpv fallback subtitle preference batch |
| `ffi.rs`, `ffi/{nodes,receive}.rs` | Dynamic C ABI and copying event payloads; unsafe confined here |
| `win32.rs`, `win32/preview.rs` | Retained child HWND and standalone preview; unsafe confined here |

The crate inherits workspace lints, allows unsafe only in the FFI and Win32
module trees, and denies `unsafe_op_in_unsafe_fn`. Every unsafe block explains
its safety conditions. No DLL, import library, or generated media is tracked.

### Public API and PR 3.3 integration

1. On an application background thread, call
   `Mpv::load_library(Some(install_dir)) -> Result<MpvLibrary, MpvError>`.
   `None` selects the executable directory. The full canonical
   `libmpv-2.dll` path is loaded with `LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR |
   LOAD_LIBRARY_SEARCH_DEFAULT_DIRS`; PATH is never searched for that DLL.
   Required symbols are resolved once, and client API major 2 is required.
   DLL dependencies must also be packaged appropriately; no global DLL-search
   configuration is changed.
2. On GPUI's window thread, call `VideoSurface::from_window(&window)` (or
   `create(HWND)`), set its client-relative physical `PhysicalRect`, then show
   it. Update bounds on resize/DPI/fullscreen and hide it when appropriate.
   The surface is disabled and uses `WS_CHILD | WS_CLIPSIBLINGS |
   WS_CLIPCHILDREN`, at `HWND_BOTTOM`, underneath the DirectComposition controls.
   Keep DirectComposition enabled and GPUI's window/root transparent over video;
   opaque fills should be confined to the controls.
3. Call `library.create(PlayerOptions { wid: Some(surface.raw_window()), extra })`.
   This returns a `Player` immediately; `Ready` confirms initialization, and
   `Error` followed by `Shutdown` reports startup failure. This asynchronous
   readiness is intentional: synchronous initialization can block on mpv.
4. Take `player.events() -> Result<Receiver<PlayerEvent>, MpvError>` once, poll
   from a background task, update player state, then call `cx.notify()`.
   The Result reflects std mpsc's single-consumer receiver (second call is
   `EventsTaken`). `load`, `stop`, `set_pause`, `seek` (Absolute/Relative exact),
   `set_property<T: MpvValue>`, `command`, `set_audio_track`, `set_subtitle_track`,
   `set_volume` (0..=100), and `set_subtitle_style` enqueue owned arguments.
   Track IDs are positive i64; `None` disables the stream. Property scalars
   support bool, i64, finite f64, String and &str. Queue acceptance is synchronous;
   mpv operation failures arrive as sanitized `Error` events. Requests wait for
   async replies before the next request, including subtitle preference batches.
5. Feed the proxy policy from `DemuxerCacheState`: `buffered_seconds`,
   `buffering`, `paused`, and `playback_speed` map directly to PR 3.2's
   `BufferingSample`. `input_bytes_per_second * 8 / 1_000_000` supplies an mpv
   download-rate fallback. Prefer the proxy's measured `download_mbps`; the
   proxy supplies `source_mbps`, `transfer_demanded`, `throttled`, and `now_ms`.
   Forward bytes and cache-end time are also available for diagnostics.
6. Stop playback before closing the proxy session, then call `player.close()`.
   Poll its completion off GPUI and retain the parent until actual `Shutdown`.
   Release the surface and continue pumping posted child-window destruction.

Events include time-pos/duration (Option for unavailable values), pause,
paused-for-cache, cache-buffering-state, cache measurements, hwdec-current,
complete typed track lists, audio-device lists, subtitle cue properties,
end-file (EOF/stop/quit/error/redirect/other with playlist entry ID), sanitized
errors, readiness and shutdown. C event memory is copied before the next wait;
no pointer escapes. The event channel is unbounded; PR 3.3 must drain it promptly.
Never log load URLs, especially debrid URLs and loopback session tokens.
Error and end-file diagnostics contain only static kinds, never raw mpv error
strings or log messages. The crate does not subscribe to mpv logs.

### Exact Electron options

Source: `desktop/native/mpv-host/src/addon_win.cc`; applied in this exact order
before initialization. `extra` overrides follow `wid`. The unit test extracts
every literal `SetOption`/`TrySetOption` pair from the C++ file and checks the
entire ordered Rust list, including which options are best effort.

| C++ line | Option | Value | Requirement |
| --- | --- | --- | --- |
| 173 | terminal | no | required |
| 174 | msg-level | all=warn | required |
| 175 | keep-open | yes | required |
| 176 | hr-seek | default | required |
| 177 | vo | gpu-next | required |
| 178 | gpu-api | d3d11 | required |
| 179 | gpu-context | d3d11 | required |
| 180 | target-colorspace-hint | auto | required |
| 181 | hwdec | d3d11va,auto-safe | required |
| 182 | cache | yes | required |
| 183 | demuxer-max-bytes | 512MiB | required |
| 184 | demuxer-max-back-bytes | 64MiB | required |
| 185 | cache-pause | yes | required |
| 186 | cache-pause-wait | 2 | required |
| 187 | cache-on-disk | no | required |
| 188 | demuxer-cache-wait | no | required |
| 191 | cache-pause-initial | yes | best effort |
| 192 | network-timeout | 60 | best effort |
| 193 | osc | no | required |
| 194 | input-default-bindings | no | required |
| 195 | input-vo-keyboard | no | required |
| 196 | audio-client-name | Panorama | required |
| 197 | wid | retained child HWND decimal value, if present | required |

The best-effort options tolerate older compatible runtimes. Defaults deliberately
retain Electron's `keep-open=yes`; EOF integration tests override it to `no`.

### Threads and shutdown

| Thread | mpv calls |
| --- | --- |
| App startup background thread | `mpv_client_api_version` after DLL/symbol loading |
| Dedicated event thread | `mpv_create`, `mpv_set_option_string`, `mpv_initialize`, `mpv_observe_property`, `mpv_command_async`, `mpv_set_property_async`, `mpv_wait_event` |
| Command caller, including GPUI | Only `mpv_wakeup`, documented thread-safe and non-blocking in packaged `include/mpv/client.h` |
| Dedicated teardown thread | `mpv_terminate_destroy`, after joining the event thread |

Idle `mpv_wait_event` timeout is 50 ms. Wakeup uses a retained weak handle under a
try-lock; it never waits on the caller. Teardown disables wakeups while retaining
the final handle, so no wakeup can race destruction. Every playback command and
property set uses mpv's async API; the caller never waits for the playback core.

`close(self)` and Drop signal the event thread and wake it. The event thread
queues `stop`, exits, and is joined by the teardown thread. That thread calls
`mpv_terminate_destroy` with the library and child-window lease retained, then
releases the child lease. Its last release destroys the HWND on its owner thread
or posts a private destruction message to that thread. A surviving surface
wrapper intentionally retains the HWND until its own Drop.
The child procedure records `WM_NCDESTROY` in shared lease state, so destroying
the parent invalidates surviving leases. Posted destruction carries a unique
lease token to protect against HWND reuse before message dispatch.

`close` returns a completion receiver immediately. A separate deadline worker
waits at most two seconds for join plus teardown. On expiration it reports
`Err(ShutdownTimeout)` and an `Error(ShutdownTimeout)` event; it never kills a
thread, unloads an in-use DLL, or destroys a window underneath mpv. The detached
teardown thread retains those resources until mpv returns (possibly until process
exit for a stuck driver). Actual `Shutdown` arrives only after teardown returns;
keep the parent and its message loop alive until then. Drop uses the same path
and discards the completion receiver. No GPUI thread joins or waits.

### Subtitles

`PanoramaSubtitleBridge` (`desktop/native/mpv-host/src/subtitle-cue.h`) extracts
cues; `runtime/subtitle-renderer.ts` draws ordinary text with renderer preferences.
Bitmap and authored ASS subtitles fall back to mpv. Renderer CSS padding, radius,
line height, font size and opacity have no direct mpv preference port. PR 3.3
owns custom cue styling and must explicitly choose `sub-visibility`; this host
keeps mpv subtitles visible by default so unsupported tracks retain their layout.
Only the cue properties `sub-text`, `sub-start/full` and `sub-end/full` are exposed
for a future bridge. This is not a port of the Electron bridge's authored-ASS
classification or playback/selection/seek generation filtering; PR 3.3 must retain
mpv rendering until it can handle those cases safely.

`set_subtitle_style(SubtitleStyle)` supplies the mpv fallback options from
`desktop/main/main.ts:123-126`: `sub-font=DM Sans`,
`sub-border-style=background-box`, `sub-border-size=0`,
`sub-shadow-offset=10`; optional `sub-fonts-dir` comes from line 122. It also
accepts the shared protocol's scale, position, delay, text/background/outline
colors and bold flag. Renderer-only preferences belong to the player screen.
Colors use [mpv's alpha-first hex syntax](https://mpv.io/manual/stable/#options-sub-color):
the default background is `#AD000000` (black, approximately 68% opaque).

### Validation and playback example

Run from `native/`:

```powershell
$env:PANORAMA_LIBMPV_DIR = 'D:\codeOS\panorama\.cache\panorama\windows-libmpv\current'
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo run -p panorama-mpv --example play -- <file-or-url>
```

The example creates a plain Win32 window with no GPUI dependency, prints active
hwdec and sanitized end/error kinds, and pumps messages throughout asynchronous
close. No input path or URL is printed. Other platforms compile the library
and report `UnsupportedPlatform` when loading; Windows surfaces are cfg-gated.

Integration tests use the environment variable explicitly, with null audio/video
outputs. They print a skip note if it is absent. Generated lavfi media requires
no file or network ([mpv's av protocol documentation](https://mpv.io/manual/stable/#protocols)).
Tests exercise clock, pause, absolute/relative seeks, typed video/audio tracks,
EOF, redacted unreachable-URL errors, and close both after error and during load.
Pause checks require prior clock advancement and two explicit IPC clock reads
at least 500 ms apart. Pending-load shutdown waits for a local server to receive
mpv's HTTP request, then keeps the connection unanswered through close; completion
must arrive within two seconds plus 250 ms scheduling tolerance, with `Shutdown`
and no URL or session token in event/error text.

Verified on Windows on 2026-10-11: workspace fmt, strict Clippy and locked tests
passed (56 tests total, including 16 panorama-mpv unit tests and all three libmpv
integration tests; none of those integration tests were skipped). The final
library and all targets also passed a Linux `cargo check` with
`--target x86_64-unknown-linux-gnu`. The standalone example created its Win32
window with generated lavfi input and exited 0 after WM_CLOSE. This hidden-window
probe did not visually assess playback or certify H.264/HEVC hardware decoding.
