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
