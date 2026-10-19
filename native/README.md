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
- `sha2` (=0.10.9) and `getrandom` (=0.3.4): salted installation identities and OS-generated installation salt. Both reuse versions already in the workspace lockfile.

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
