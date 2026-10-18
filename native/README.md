# Panorama native

This workspace is the foundation for Panorama's Rust and GPUI desktop app.
`panorama-core` contains shared logic without UI dependencies.
`panorama-app` provides the native shell, navigation and shared theme.

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
- `sha2` (=0.10.9) and `getrandom` (=0.3.4): salted installation identities and OS-generated installation salt. Both reuse versions already in the workspace lockfile.
- `image` (=0.25.10, defaults off; `jpeg`, `png`, `webp`): content sniffing, bounded decoding (4096×4096, 64 MiB), and aspect-preserving downscaling.
- `axum` (=0.8.9, dev-dependency, defaults off; `http1`, `tokio`): deterministic HTTP test server on 127.0.0.1 with counters and explicit release gates, without public internet.
- `windows` (=0.62.2): Win32 HWND and GDI desktop capture, matching GPUI's resolved version.
- `raw-window-handle` (=0.6.2): obtain the GPUI window's HWND without platform assumptions.
- `png` (=0.18.1): encode debug window captures on a background executor.

mpv host (PR 3.1):

- `libloading` (=0.8.9): Gate 1's runtime loader; no link-time mpv dependency.
- `windows` (=0.62.2): matches GPUI and Gate 1; only Foundation, GDI (window
  class/background), LibraryLoader, Threading (window owner validation), keyboard
  window disabling, and WindowsAndMessaging features are enabled here.
- `raw-window-handle` (=0.6.2): matches GPUI's `HasWindowHandle` contract.
- `panorama-mpv` uses standard-library mpsc channels; no channel dependency.
- `serde_json` (=1.0.151, Windows dev-dependency): parses explicit mpv IPC clock
  readings in playback tests; reuses the version already in the lockfile.

Media proxy (PR 3.2):

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

Torrent engine (PR 4.1):

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

## Addon data for PR 2.4

`panorama_core::addons` contains `mod.rs` (client, installation identity, blocking storage and cancellable delivery), `catalog.rs` (Home selection and fallback), `search.rs` (concurrent search and enrichment), `details.rs` (metadata chain and stream groups), `transport.rs` (core transport adapter), `sanitize.rs` (untrusted response validation), and `types.rs` (screen data and events). Tests live under `addons/tests/`; the production transport also has a local HTTPS test under `stremio/tests/addons.rs`.

After installing `PanoramaEnv`, construct `AddonClient::new(Arc::clone(&store), CoreAddonTransport).await?` on Tokio. Construction is async and fallible because the salt is initialized atomically on a blocking worker. The client retains that runtime handle so subsequent screen calls can run on a UI thread without a Tokio context; keep the runtime alive. For each screen request, pass `CoreSession::installed_addons()` when signed in, or `&[]` when signed out to use Cinemeta alone. The client never retains the installation list between calls.

- Home: `catalogs(&addons)` lists movie catalogs without required extras in account order, considering at most 100 declared catalogs per addon. Descriptors share their manifests through `Arc`; copied catalog IDs are limited to 512 bytes and display names to 300 characters. `catalog(&addons, remembered)` emits `ResourceEvent::CacheHit(Page)` before network work when a usable cached page exists, followed by `Fresh(Page)` or `Failed(FailureKind)`. It tries the selected catalog first (a removed selection uses the first current catalog), then the other catalogs in account order, then Cinemeta `top`. Each source has an 8-second deadline; failures, malformed responses and zero usable items advance the chain. Cache read failures act as misses. A cached fallback page is also available offline. `Page.catalog` identifies the actual source.
- Search: `search(&addons, query)` trims and caps queries at 100 Unicode characters; an empty query emits an empty result without requests. Planning considers at most 100 declared catalogs per addon and stops after four eligible catalogs, respecting required extras and the 512-byte catalog ID limit. Those catalogs run concurrently. Results merge in account order and de-duplicate by opaque film ID. Cards missing a name, poster or release information among the first 20 results receive metadata through `details()` with at most four active chains. Each arriving cached or fresh detail updates its original position; unnamed cards are withheld until resolved. Search snapshots are `ResourceEvent::Fresh(Vec<FilmDetails>)`; queries themselves are uncached.
- Film: `details(&addons, id, preferred)` emits cached metadata, then tries the explicit preferred installation (or JSON-serialized `AddonKey` in `pref:metaAddon`), matching installed movie `meta` resources in account order, then Cinemeta for IDs starting with `tt`. Each request has an 8-second deadline and must return the requested ID and a nonempty name. `FilmDetails` includes bounded display fields, HTTPS images, HTTP(S) links and credits. Legacy director/cast fields are captured within the request before core parsing discards them.
- Playback choices: `streams(&addons, id)` queries every matching movie `stream` resource concurrently with individual 15-second deadlines. `StreamsEvent::Groups` contains account-ordered `StreamGroup`s with `Ready`, `Empty` or `Failed` states; failures are isolated. `StreamSource.stream` retains the core playback source and behavior hints, with bounded display fields and redacted debug output. Links are uncached. An unrecognized film emits `NoSources`, independently of metadata. If every stream source is offline, the only event is `Failed(Offline)`.

Consume streams with `futures::StreamExt`. Keep cache hits visible when a later failure arrives, display retry on `Failed`, and retry by calling the same method again. Cache read errors allow Home and details to continue fetching; cache write errors follow fresh data as `Failed(Storage)`. Dropping a stream cancels its background request task and any outstanding enrichment chains. Drop screen request streams before signing out. `store.clear_on_sign_out()` advances a shared cache generation under the store lock: pending writes from earlier addon requests, including search enrichment, are discarded. Already-running writes finish before clearing, and subsequent screen requests can cache normally. No separate addon invalidation hook is needed. Transport connection errors are reported as `Offline`; fallback continues through all candidates because one unreachable addon does not establish that every source is offline.

Catalog keys are `catalog:<manifest id>#<16 lowercase hex SHA-256 chars>|<catalog id>`, hashing the persisted 32-byte random `pref:installSalt` followed by the transport URL. Transport URLs never enter keys or failure categories. Metadata uses `meta:<opaque film id>` and stores the source `AddonKey` alongside `FilmDetails`; entries from removed installations are ignored (the built-in Cinemeta source remains available for `tt` IDs). `Key::meta` accepts 200 Unicode characters, including up to 800 UTF-8 bytes. Film IDs reject `/`, `?`, `#`, control characters and whitespace; the core encodes resource paths.

The salt survives `store.clear_on_sign_out()` as a preference: it contains only random bytes, stabilizes local identities across launches and accounts, and stores no URL or account information. Sign-out deletes all catalog and metadata entries. Preference values should be written on a blocking worker, as with the rest of the store API.

The production adapter bounds encoded request URIs before invoking the core's infallible request builder. All addon fake tests run with Tokio paused time and no public network. Review coverage includes:

| Review focus | Tests |
| --- | --- |
| 1: Home fallback | `home_falls_back_when_first_addon_is_slow_down_or_junk` (slow/down/invalid JSON), `home_never_blank_while_cinemeta_works` (wrong shape/empty/unusable) |
| 2: unknown film | `unknown_film_shows_details_with_no_sources_state` |
| 3: offline launch | `offline_launch_shows_cached_catalog_then_offline_error`, `offline_launch_with_empty_cache_reports_offline_only`, metadata/search/stream offline tests |
| 6: removed selection | `removed_remembered_catalog_falls_back_to_first` |
| Search | `search_queries_at_most_four_search_capable_catalogs`, `search_fills_first_twenty_thin_results_four_at_a_time`, ordering/de-duplication/query-bound and cancellation tests |
| Metadata/streams | `details_chain_prefers_meta_addon_then_matching_addons_then_cinemeta`, `streams_grouped_by_addon_in_account_order_with_failures_isolated` |
| Security | `spoofed_addon_id_cannot_poison_another_addons_cache`, `cache_keys_and_errors_never_contain_transport_urls`, `non_https_images_dropped_and_oversized_fields_truncated`, `film_id_validation_rejects_path_injection`, byte/list-cap and sign-out/salt tests |
| Production adapter | `core_addon_transport_encodes_opaque_ids_preserves_credits_and_enforces_fetch_cap` (local HTTPS only) |

## App shell

Run `cargo run -p panorama-app -- --reduced-motion` to disable route transitions.
Light is the shipping default; dark is available only for debug captures:

```sh
target/release/panorama.exe --screenshot home home.png
target/release/panorama.exe --screenshot search:arrival search.png --size 1280x800 --theme dark --reduced-motion
```

Screenshot sizes use logical pixels and respect the 960x600 window minimum.
On Windows capture maps the client rectangle to screen coordinates for desktop BitBlt;
keep the interactive desktop visible and the window unobscured. Capture and PNG
writing run off the UI thread, after a settled frame and at least 1600 ms.

Every `--screenshot` invocation checks the shell's inherited GPUI text style at
400/500/700. It fails with a non-zero exit if the resolved family or the bundled
faces' M advances differ. On Windows the registered family is `DM Sans 14pt`.

The app modules are `main` (startup), `args`, `app`, `router`, `titlebar`,
`header`, `routes/{home,search,film,player,addons}`, `theme`, `motion`, `assets`,
and `debug/screenshot`. History stores 100 entries; the eight most recently visited
views retain their entities and scroll handles. Older entries remount on revisit.
Interrupted motion resumes from the sampled opacity and offset. The macOS
38px caption keeps system traffic lights and native titlebar dragging; it has
not been run on macOS.

PR 2.4b adds `search_band`/`app_search`/`app_view` (header search disclosure, 320 ms height tween, query handoff), `routes/search` (Home's grid with a Films tab; Cast & Crew is hidden until a TMDB key ships), `routes/film{,_data,_actions,_view,_images}` (details + sources start together; first ready URL/info-hash stream is the Play target and is handed to Player through `AppState::playback`), `film_display` (primary-button table, quality badges, rating rule), `film_overlay`, `a11y` (polite status nodes) and fixture search/details/streams. New capture flags: `--open-search`, `--signed-out`, `--no-sources`, `--film-loading`; routes `search:<q>`, `film:<id>`. `CoreSession::set_watchlisted`/`is_watchlisted` toggle the account library. The original (native) film title is not shown because addon metadata sanitising does not carry it.

The component theme is synchronized through `Theme::change` then `Theme::update`
so its solid colors, renderable tokens and Base projection agree. Mapped tokens:
background/foreground/border, accent/foreground, muted/foreground,
popover/foreground, ring, selection, scrollbar/thumb/hover, button/foreground/
hover/active, primary/foreground, secondary/foreground/hover/active,
danger/foreground, input, caret and skeleton. Component fonts use DM Sans;
component radii are zero because the CSS supplies no general radius.

GPUI 0.3.8 provides Windows caption hit-test areas (including snap layouts) and
keyboard click activation for focused controls. Raw SVG elements require an explicit
text color; parent text color is not inherited, so caption icon hover uses group hover.
It has no exit animation primitive; the shell retains outgoing views during their fades.
Windows application activation is a no-op, so debug captures activate the window
explicitly. Frame callbacks have no current view; animation requests must be made
during rendering. Its text-input focus query is component-owned, so future custom
text inputs should register with that facility or extend the app's Backspace guard.
Tracked focus handles own their Tab eligibility; element tab_stop settings do not
update those handles. Bound Tab actions bypass raw key handlers, so a keystroke
observer updates focus visibility. GPUI clears keyboard modality on mouse movement;
the shell keeps focus rings visible until pointer-down, matching Electron. Rings use
border overlays because GPUI drop shadows fill transparent control interiors.
The CSS drift tests fall back to the fixture when the Electron stylesheet is absent.

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

## Home and account services

`--fixtures` uses the nine Electron preview films and `preview@panorama.local`,
without network or persistent storage. Screenshot mode implies fixtures. Capture
options are `--scroll <delta|end>` (repeatable), `--hover-first-card`,
`--focus-first-card`, `--open-account-menu`, and `--open-sign-in`.
`--fixtures --bench-scroll` measures five seconds of scrolling through 200 cards;
frame-completion results go to the background diagnostics channel.

`services` creates two named Tokio workers before GPUI starts. Storage and image
cache initialization run on blocking workers. `app_state` consumes futures mpsc
channels from Tokio catalog, authentication and CoreSession subscription jobs;
dropping a GPUI consumer aborts its producer. Profile/addon changes refresh Home,
retaining cached cards until fresh data arrives. Startup errors expose Reload
Panorama; store lock contention opens the single-instance error window.

`header_state`, `header`, `account_menu`, `login`, and `transition` own fixed chrome,
modal focus, and reversible motion. `routes/home` retains scroll and keyboard
state; `home_view` virtualizes fixed-height rows; `hero` and `film_card` paint the
Electron layouts. `home_layout` owns the breakpoint calculations.

`image_cache` requests physical sizes rounded up to 64 px, allows six visible
loads, and cancels futures that leave the viewport. Its shared 160 MiB pixel LRU
calls `cx.drop_image` on replacement, eviction, and teardown. Disk originals use
`<store directory>/cache/images` with a 256 MiB budget. RGBA pixels are converted
to **straight BGRA8**, retaining alpha: GPUI 0.3.8 `src/assets.rs:42` documents
BGRA and `src/elements/img.rs:678-681` swaps R/B without premultiplication;
`gpui-pre-windows-0.3.8/src/directx_renderer.rs:1500-1502` uses SRC_ALPHA blending.
The hero's radial-over-linear gradient is generated once per physical size on a
blocking worker and uploaded as a RenderImage. Poster saturation is omitted
because GPUI has no saturation filter. Explicit `--reduced-motion` disables
transforms and makes transitions instant; this GPUI version exposes no OS reduced
motion preference.

Home paging requires the core `catalog_next` API and page cursor fields. Core also
preserves bounded legacy catalog director/country fields that Stremio's typed
preview drops. Paging tests verify raw skip offsets, terminal empty pages, and
preservation of the first-page cache; fixture/layout/header/error/cache tests live
in the app crate.

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

## Native media proxy

`panorama_core::media` ports the Electron range proxy, cache, validators, retries,
manual redirect transport and buffering policy. See
[`panorama-core/MEDIA_PORT.md`](crates/panorama-core/MEDIA_PORT.md) for the test
mapping, refresh contract and Phase 3.3 player integration.

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

## Progress and presence for PR 3.4

`panorama_core::stremio::progress` (re-exported from `stremio`) ports the Electron watch-progress behaviour onto the core's own `Player` model, added to the core model as a field next to `Ctx`. Calls only dispatch core actions and never block; the core creates library items, applies the watched threshold, persists through the Env storage and syncs with the API. Like the TS runtime, everything is a no-op while signed out (`resume` is `None`, `in_library` is `false`, the others return `NotSignedIn` or do nothing).

- `resume(film_id)`: `ResumePoint { offset_secs, duration_secs }` from the library item, only when at least 30 s in with at least 60 s left (`normalizeResumeState`).
- `begin_playback(PlaybackTarget, start_secs)` loads the player (`beginPlayback`); the target carries the chosen core `Stream`, the meta addon URL (the core loads the film's meta to create a temporary library item when none exists) and the stream addon URL.
- `report_progress(film_id, PlaybackSample)` sends `PausedChanged` when the pause state changes (also flushing, unless it is the first sample) and `TimeChanged` once the position is 10 s or more from the last push (`PROGRESS_SYNC_INTERVAL_SECONDS`, `flushPlaybackProgress`). Samples without a positive duration are not pushed. `report_seek` sends `Seek` and restarts the window. The core itself throttles library pushes to one per 90 s and pushes on pause and unload.
- `finish(film_id)` pushes the final position at the duration and sends `Ended`; the core's 70 % watched threshold then marks the film watched. `stop_playback(film_id)` flushes the last position and unloads the player (`stopPlayback`), which pushes the library item.
- `set_in_library(&FilmMeta, saved)` / `in_library(film_id)`: `AddToLibrary` / `RemoveFromLibrary`; saved means neither removed nor temporary (`normalizeWatchlisted`). The change lands asynchronously; observe `CoreChange::LibraryChanged`.

`panorama_core::discord` ports `desktop/main/discord-presence.ts` and `desktop/shared/discord-presence.ts`. `DiscordPresence::new(&runtime_handle, settings_file, discord_paths(Platform::current(), env), SystemConnector)` starts a task; `update`/`update_value`/`clear`/`set_enabled` only queue work. The transport is a `Connector` trait: `SystemConnector` uses `tokio::net::windows::named_pipe` on `\\?\pipe\discord-ipc-0..9` and Unix sockets in `$XDG_RUNTIME_DIR`/`$TMPDIR`/`$TMP`/`$TEMP`/`/tmp` elsewhere; tests use in-process duplex streams. Presence is opt-in (default off, persisted atomically with owner-only permissions), coalesced to one `SET_ACTIVITY` per 5 s, walks the ten paths on failure then retries every 15 s, drops oversized (over 64 KiB) or error frames, answers PING and fails silently when Discord is not running. Text limits count Unicode scalar values where the TS counted UTF-16 units. Tokio gained the `net` and `io-util` features (no new crate).

| TS test (`tests/unit/discord-presence.test.ts`) | Rust test (`discord::tests`) |
| --- | --- |
| maps watching, artwork and seek timing without advancing paused or buffering activities | `maps_watching_artwork_and_seek_timing_without_advancing_paused_or_buffering_activities` |
| defaults off, persists opt-in, handles split frames, coalesces updates and clears immediately | `defaults_off_persists_opt_in_handles_split_frames_coalesces_updates_and_clears_immediately` |
| retries disconnected clients after 15 seconds and cancels retries on clear | `retries_disconnected_clients_after_15_seconds_and_cancels_retries_on_clear` |
| rejects oversized frames and discovers platform socket paths | `rejects_oversized_frames_and_discovers_platform_socket_paths` |
## Addon management for PR 5.1

`addons/manage/` contains URL validation, manifest validation, display summaries,
opaque confirmation tokens and sanitized `InstallError` kinds. `stremio/manage.rs`
implements management on `CoreSession`; `preferences.rs` handles selections;
`events.rs` correlates core collection push events. `model.rs` supplies the two
operations missing from the pinned core: explicit installation replacement across
transport URLs and reordering by AddonKey. Both use the core's `PushAddonsToAPI` and `ProfileChanged`
effects, including `addonCollectionSet` and FIFO profile persistence.

PR 5.2 should call:

- `AddonUrl::parse(pasted_text)`, then `session.preview(url).await`. Display the
  summary, `already_installed`, `replaces` and configuration hints. After explicit
  confirmation, move `preview.token` into `session.install(token).await`.
- `installed_addons()` and `addon_key(&descriptor).await` for account order and
  salted identities; `remove(key).await` and `reorder(all_keys).await` for changes.
- `configure_url(&key).await` for the system browser. It is async because salt
  initialization runs off the caller's thread. Returned URLs may contain secrets;
  do not log them. A new manifest URL must go through preview and confirmation.
- `set_meta_source(Some(key)).await` for an addon supporting movie metadata;
  `None` means Automatic. `meta_source().await` clears stale selections.
- `set_home_catalog(Some(catalog)).await` and `home_catalog().await` for Home;
  `None` follows the first available movie catalog. Pass the returned selection
  to the resource client's catalog call. Subscribe to `CoreChange` to refresh.

All mutations and preview require sign-in. Signed-out defaults are read-only.
Preference getters repair references after external collection changes as well
as local removal/replacement. Removal deletes that installation's catalog rows,
advances the cache generation to discard pending cache writes, resets stale
metadata to Automatic and Home to the first available catalog. Store work runs
on blocking workers. Confirmation tokens are random 256-bit, single-use,
session-bound capabilities expiring at ten minutes. The session retains the
validated descriptor, resolved replacement AddonKey and their SHA-256 binding.
Install rejects confirmations whose resolved target has changed, checks current
protection flags and uses that target for mutation and cache cleanup; it never
fetches the addon again. Sign-in/sign-out discard confirmations.

URL parsing trims outer whitespace, accepts case-insensitive HTTPS/Stremio schemes
and rewrites Stremio to HTTPS. It preserves configuration path/query text, rejects
userinfo, fragments, internal whitespace/control characters, backslashes, relative
URLs, non-HTTPS schemes, paths without `/manifest.json`, dot traversal (including
percent-encoded dots), and input over 2,048 characters. IPv4/IPv6 loopback,
link-local and unspecified literals, including IPv4-mapped IPv6, are rejected.
Percent-encoded `@` in opaque configuration paths remains unchanged; an encoded
`@` in the authority is invalid. Manifest fetch uses the Env client, no Referer,
HTTPS-only redirects and a 256 KiB declared/streamed cap. Public addon redirects
cannot target forbidden IP literals. Manifest validation uses the core's semver
and Manifest types, bounds IDs/names/catalog counts, truncates descriptions and
drops non-HTTPS images. Debug output for previews, URLs, tokens and errors is
redacted; never format explicit core manifest or transport access for logging.

The pinned core matches install/upgrade/uninstall by transport URL, not manifest
ID. Preview prioritizes the installed transport URL (update in place). Otherwise,
exactly one matching manifest ID proposes replacement at that installation's
account position; several matching IDs install an additional installation with
no replacement or existing cache deletion. Manifest ID alone is never used as
an installation identity. Updated installations invalidate only the resolved
target's catalog cache; an unchanged reinstall keeps it. Model replacement
rejects a transport URL belonging to any other installation. Reorder resolves
AddonKeys to complete descriptors and preserves their manifests and flags. Core's
`protected` flag prohibits removal/upgrade; `official` alone does not prohibit
removal. Both flags prohibit cross-URL identity impersonation here. The pinned
core has no reorder action or protected-position rule, so full permutations may
move protected addons. Core applies and persists changes before the API push
completes and does not roll them back on push failure. Management waits for the
matching collection event and storage drain, returning `ApiPush` on API rejection
or transport failure. The local list remains changed; resnapshot it after an
error. Repeating an unchanged install still pushes the collection, allowing retry.
Cancelled mutation futures retain their completion task; a later operation waits
for it before dispatching another change.

Tests use local mock API/TLS addon servers only. Coverage includes URL boundaries,
encoded tricks, declared/streamed caps, bad/missing fields, image filtering,
redirect policy, exact preview installation, forged/replayed/expired/cross-session
tokens, manifest-binding tampering, replacement position, protected/official rules,
API failures, cache invalidation, stale preference repair and restart order.
Token expiry uses paused Tokio time. Review-focus tests are
`removed_meta_source_falls_back_to_automatic` and
`removed_home_catalog_falls_back_to_first`.

## Addons screen and catalog switcher (PR 5.2)

`Route::Addons` (`addons`) is reached from the account menu. Modules under
`panorama-app/src/addons_screen/`: `model.rs` (pure `InstallFlow`, `InstalledRow`,
`move_row`, `error_copy`, `catalog_groups`, meta-source fallback), `backend.rs`
(core calls on the services runtime, bridged back over mpsc), `actions.rs`
(preview, install, remove, reorder, configure, source selection), `install_view.rs`,
`installed_view.rs`, `source_view.rs`, `widgets.rs`, `fixture.rs`. Home's switcher
lives in `catalog_switcher.rs` and `routes/home_catalog.rs`. Addon URLs are never
logged; manifest text is shown as plain text and length-capped. Fixture flags:
`--addons-preview`, `--addons-confirm-remove`, `--addons-error`,
`--open-catalog-menu`, `--signed-out`.
