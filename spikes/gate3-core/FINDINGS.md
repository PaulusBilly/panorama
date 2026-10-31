# Gate 3: native stremio-core with Panorama's Env

## Update: shared-workspace blocker resolved

The flate2 conflict described below is fixed by vendoring `stremio-watched-bitfield` (one small MIT crate from the same repository) into `native/vendor/` with its requirement relaxed from `flate2 = "1.0.*"` to `flate2 = "1"`, and pointing the core at it with a `[patch]` entry in `native/Cargo.toml`. Nothing else in the core is changed. See `native/vendor/README.md`.

With that patch the spike is a normal workspace member and every required command passes in the shared workspace: fmt, clippy `-D warnings`, the workspace tests (including the mock sign-in test), and the release build. The release binary from the shared workspace prints the six default addons and five live Cinemeta catalog names. Where the sections below say "FAIL" or "isolated" for the shared workspace, read them as the state before this fix.

**Gate 3 passes**, with one item still open: sign-in against the real Stremio API has not been run (no credentials were supplied). The same code path passes against the mock API.

## Verdict (before the fix)

**Native feasibility: PASS. Requested shared-workspace gate: FAIL.** The unmodified core links into a native `x86_64-pc-windows-msvc` executable, authenticates against a local mock API through our Env, preserves installed-addon order, and fetches a real movie catalog through the core's addon transport. This does not establish real account authentication: both credential variables were absent.

The crate is added to `native/Cargo.toml`, but its dependency graph cannot resolve alongside the workspace's pinned GPUI dependencies. Therefore this is **not a merge-ready PR**: the required workspace Clippy, tests, and release build fail before compilation. `native/Cargo.lock` remains unchanged because Cargo cannot generate a valid merged lockfile. No core, companion, GPUI, gate1, or application source was patched. The successful runtime evidence below comes from a copy of this spike in an **ignored diagnostic workspace under `native/target/`**, not the requested shared workspace.

Tested on Windows with the workspace's Rust `1.99.0-x86_64-pc-windows-msvc`, on 2026-10-10 (Asia/Bangkok).

| Item | Verdict | Evidence |
| --- | --- | --- |
| Builds on `x86_64-pc-windows-msvc` in the requested workspace | FAIL | Incompatible `flate2` constraints; exact commands and dependency chain below. The isolated diagnostic release build passes. |
| Env implemented without patching the core | PASS, isolated | `src/lib.rs` implements `Env`; both native debug tests and release compilation succeed against the exact upstream revision. |
| Signed-out addon list | PASS, isolated | Release executable prints six default addons, capabilities, and hosts; actual output below. |
| Mock sign-in | PASS, isolated | One integration test passes; account order is `second`, then `first`. The test also rejects a failed collection fetch, checks host-only output, enforces HTTP restrictions and the streamed body cap, and exercises a mock catalog through the core transport. |
| Real sign-in | NOT RUN, no credentials | `PANORAMA_STREMIO_EMAIL` and `PANORAMA_STREMIO_PASSWORD` were both absent; their values were never inspected or printed. |
| `--catalog` | PASS, isolated | Real Cinemeta request follows an HTTPS redirect and prints five names; release executable exits 0. |

## Revision and dependency blocker

Pinned core revision: **`474ffaa42e0b7b9bcba0dd99368c41e828579dec`**. `git ls-remote --tags https://github.com/Stremio/stremio-core` reports annotated tag `stremio-core-web-v0.61.0` as `ecac80d6319f6be96b568cc05044db865126871b`, peeled to this commit. Its [web Cargo manifest](https://github.com/Stremio/stremio-core/blob/474ffaa42e0b7b9bcba0dd99368c41e828579dec/stremio-core-web/Cargo.toml) declares version `0.61.0`. This is stronger evidence than guessing from release dates.

The core dependency enables `derive` and `env-future-send`. `stremio-derive` and `stremio-watched-bitfield` are path companions inside the same git checkout, automatically pinned to the same revision. No direct derive dependency is necessary: `src/lib.rs:11-14` re-exports `Model` with `derive` enabled.

The unsatisfiable requirements are:

```text
gate3-core -> stremio-core -> stremio-watched-bitfield -> flate2 = "1.0.*"
gate1-video / panorama-app -> gpui-pre =0.3.8 -> usvg ^0.46.0 -> flate2 ^1.1
```

Source evidence: core `stremio-watched-bitfield/Cargo.toml:20`; installed `gpui-pre-0.3.8/Cargo.toml:500-502`; installed `usvg-0.46.0/Cargo.toml:71-74`. `1.0.*` only allows `1.0.x`, while `^1.1` requires at least `1.1.0`. Cargo does not resolve two semver-compatible versions from the same package source to satisfy these disjoint constraints. Its initial diagnostic mentions `png`'s `^1.0.35` selected version, but forcing the old version reveals the actual GPUI constraint:

```text
cargo update -p flate2 --precise 1.0.35
error: failed to select a version for the requirement `flate2 = "^1.1"`
candidate versions found which didn't match: 1.0.35
required by package `usvg v0.46.0`
    ... dependency `usvg = "^0.46.0"` of `gpui-pre v0.3.8`
    ... dependency `gpui = "=0.3.8"` of `gate1-video`
```

The permitted newest-commit fallback was checked: default branch `development`, HEAD **`fa22c9ca8b8574fe7a72460023d05beff2b74026`** (web tag `0.65.0`). `cargo fetch` at that exact revision fails with the same watched-bitfield constraint. The spike retains the matching 0.61.0 revision because newer HEAD does not resolve the blocker. A temporary crates.io patch pointing to the *unmodified* upstream flate2 1.0.35 git tag did not produce a resolved graph; the resolver was stopped after several minutes, and the patch was removed. No claim is made that that experiment succeeded.

Making the requested shared workspace pass needs a change to this dependency incompatibility, such as an upstream watched-bitfield requirement change. A separate spike workspace would demonstrate feasibility but would change the workspace-members requirement; it has not been substituted for the requested layout.

## Env contract and friction

All core line references in this document are to `%CARGO_HOME%/git/checkouts/stremio-core-9908b64dcbd7d998/474ffaa/` at the pinned revision. Registry references are under `%CARGO_HOME%/registry/src/index.crates.io-1949cf8c6b5b557f/`.

| Core source | Required behavior and implementation |
| --- | --- |
| `src/runtime/env.rs:139-145` | Generic `fetch(Request<IN>) -> TryEnvFuture<OUT>`, with serialized input and deserialized output. Our Env preserves method/headers, serializes non-GET/HEAD bodies to JSON, and parses response JSON. Reqwest uses rustls with default features disabled, a 30-second total request timeout, and an 8 MiB response limit checked both against declared length and every incoming chunk. HTTP failures, serialization errors, and network errors become sanitized categories; bodies, request URLs, and raw reqwest errors are never logged. |
| `src/runtime/env.rs:147-150` | Generic storage read and optional write/delete. A mutex-protected map stores JSON bytes. `None` deletes the key. Serialization of borrowed `T` happens before returning the future, so `set_storage` does not add a `Send` or `'static` requirement to `T`. Reads clone bytes and release the lock before deserialization. |
| `src/runtime/env.rs:151-152` | Concurrent and sequential task execution. Concurrent effects use `tokio::spawn`; sequential effects enter one FIFO Tokio queue whose worker awaits each future to completion. Spawning both classes independently would break ordered persistence. |
| `src/runtime/env.rs:153-156` | UTC time uses `chrono::Utc::now()`. The core's default `local_now()` correctly converts through `chrono::Local` to a fixed offset. |
| `src/runtime/env.rs:157-164` | Analytics flush is a completed future, analytics context is JSON null, debug-only `log` discards its argument. The analytics feature and a tracing subscriber are not enabled. No secret-bearing actions/events are formatted. |
| `src/runtime/env.rs:165-172` | Default addon transport selects core HTTP/legacy transport from the URL scheme. We use this implementation directly for the `ResourceRequest`/`ResourcePath` catalog request. |
| `src/runtime/env.rs:174-345`; `src/constants.rs:46` | Explicitly call `migrate_storage_schema()` before constructing the model. Schema 0 progresses through all migrations to 25. Fresh-storage v1 clears profile/library keys (`env.rs:349-357`); later migrations transform settings and clear selected buckets. Versions newer than 25 are rejected. A production Env must not skip this or merely stamp the latest version. |

Further friction observed:

- Futures default to `LocalBoxFuture` (`env.rs:93-110`). `env-future-send` switches to `BoxFuture` and makes `ConditionalSend: Send` (`env.rs:115-133`), which is necessary for Tokio's multi-thread executor. No WASM cfg flags, bindings, or core patches were required. Wasm dependencies appearing in cross-platform metadata are not linked into this Windows executable.
- Env methods are static; they receive no `&self`. This spike initializes one process-wide `OnceLock<State>` before using the core. It cannot create isolated environments for two accounts in the same process or reset Env between tests. Production needs a deliberate account/runtime ownership strategy.
- A poisoned mutex error contains a non-`Send` guard. The storage read maps that error to `EnvError` *before* capturing its result in the `Send` future. No lock guard is held over an await.
- `Runtime` requires `E: Send + 'static` and `M: Send + Sync + 'static` (`src/runtime/runtime.rs:34-37`); `Model` also requires `Clone` (`src/runtime/update.rs:8`). The derive needs a `ctx` field and `#[model(NativeEnv)]` (`stremio-derive/src/lib.rs:15-45`). The model derives both `Clone` and `Model`; `Ctx::new` needs seven buckets (`src/models/ctx/ctx.rs:94-131`). This fresh-memory spike constructs them explicitly rather than rehydrating a saved account.
- Login is not just an auth request: `src/models/ctx/ctx.rs:311-395` performs login, then joins addon collection and the entire library fetch. Requests are `/api/login`, `/api/addonCollectionGet` with `update=true`, and `/api/datastoreGet` with collection `libraryItem`, `all=true`. A large/slow library can delay the addon result or exceed the 8 MiB cap.
- Authentication updates profile/addons before emitting completion events (`src/models/ctx/update_profile.rs:303-325`). The login path emits `UserAuthenticated`, `UserAddonsLocked`, and `UserLibraryMissing` (`src/models/ctx/ctx.rs:198-278`), rather than `AddonsPulledFromAPI`. `sign_in` waits for those three success events, any error event, or 30 seconds. Waiting only for `UserAuthenticated` could misreport fallback official addons as installed account addons. Failed collection pulls are explicitly tested.
- Core events and serialized actions can contain passwords, auth keys, emails, or full transport URLs (`src/runtime/msg/event.rs:79-103, 130-133, 190-193`). The CLI only formats selected profile fields and hosts; it never dumps an event, action, profile, or error source. Core tracing also has detailed request/result fields, so a production tracing subscriber needs its own redaction policy.
- `ManifestResource::name()` is private (`src/types/addon/manifest.rs:262`); the spike matches the public short/full resource variants. Catalog presence is also determined from `manifest.catalogs`, since a catalog can be present without an explicit `catalog` resource entry. Other offered resources are reported from their manifest declarations.
- Cinemeta's first movie catalog returned HTTP 307. An initial no-redirect Env failed the catalog run. The final Env follows at most ten redirects, requires HTTPS for every redirect, and restricts API redirects to the original origin so auth bodies do not move to another origin. The final real catalog run passes.
- `GATE3_API_BASE` is absent from the default path. Only requests whose original origin equals the core's `API_URL` (`src/constants.rs:85-86`) have scheme/host/port rewritten, retaining path/query. Override origins reject userinfo, paths, queries, and fragments. HTTP is accepted only for an explicitly configured literal loopback IP; other HTTP requests, including direct loopback addon URLs, remain rejected. The default profile contains a local-files addon, but this does not cause a network request in the signed-out listing.
- Sequential tasks use an unbounded queue, the runtime event buffer is 1024, and the throwaway process does not await a persistence/shutdown barrier before returning. Production must drain events, finish durable writes, and supervise/cancel tasks deliberately.

## Verification and dependency weight

Required commands were actually attempted from `native/`:

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | PASS |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | FAIL: flate2 dependency resolution, before linting |
| `cargo test --workspace --locked` | FAIL: same dependency resolution, before running tests |
| `cargo build -p gate3-core --release --locked` | FAIL: same dependency resolution, before compilation |
| `cargo tree -p gate3-core --locked` | FAIL: same dependency resolution |

The diagnostic copy passes `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked`, and a clean `--release --locked` build on the native MSVC toolchain. Final mock output:

```text
running 1 test
test core_login_lists_account_addons_in_order_and_rejects_failed_pulls ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.13s
```

The streamed oversized-response assertion specifically requires `EnvError::Fetch("response exceeds 8 MiB")`, so a JSON parse failure would not satisfy it. Mock addon URLs deliberately contain fake userinfo, a secret-looking path, and a query token; the listing asserts none appear. Only fake credentials are used.

Since the required shared `cargo tree` cannot resolve, **merged dependency weight and merged clean build time are unavailable**. For the isolated diagnostic graph, `cargo tree -p gate3-core --no-dedupe --prefix none --format '{p}'` has **184 unique package/version/source entries including the spike and test dependencies**, or **180 with normal/build edges only**. Unique counts filter package lines and remove duplicates; repeated `(*)` lines and the dev-dependencies heading are not counted. There are 43 normal/build package versions absent from the existing workspace lockfile; this is a diagnostic comparison, not a proposed merged lockfile delta. The isolated lockfile resolves 253 packages across platforms, including packages not built on Windows.

The clean release build took **141.585 seconds** (Cargo reported `2m 21s`) in the previously nonexistent `target/gate3-clean-release`, with downloaded sources cached and about three seconds of package-cache contention from another diagnostic command. No existing target directory was deleted. Release executable size: **7,657,984 bytes**. This measures the spike's complete isolated dependency build, not incremental build time, the shipping app, or deployment size.

Notable licences verified from installed manifests and repository LICENSE files:

| Dependency | Licence |
| --- | --- |
| stremio-core and same-repository companions | MIT via repository `LICENSE.md`; their package manifests omit license metadata |
| localsearch (upstream git `main`, diagnostic lock revision `74fefe1d...`) | MIT via repository `LICENSE`; package manifest omits license metadata |
| stremio-official-addons 2.1.2 | MIT |
| reqwest 0.12.28, futures 0.3.34, flate2 1.0.35, tiny_http 0.12.0 | MIT OR Apache-2.0 |
| tokio 1.53.2 | MIT |
| rustls 0.23.45 | Apache-2.0 OR ISC OR MIT |
| ring 0.17.14 | Apache-2.0 AND ISC |
| fst 0.4.7 | Manifest declares `Unlicense/MIT` |

All direct added crates are on the permitted list; tiny_http is test-only. The core necessarily brings other transitive crates, including official addons, local search, crypto, serde helpers, and watched-bitfield. No extra direct crate or replacement implementation is introduced. The resolved feature tree contains zero `native-tls` or `openssl` feature lines. Pinning the top-level core does not by itself pin its `localsearch` branch dependency; a valid committed lockfile is necessary for reproducibility once the workspace conflict is addressed.

## Actual release console output

Signed out, exit 0:

```text
signed out: addon count=6
addon name="Cinemeta" id="com.linvo.cinemeta" version=3.0.14 host=v3-cinemeta.strem.io resources=[catalog,meta]
addon name="YouTube" id="com.linvo.stremiochannels" version=1.30.7 host=v3-channels.strem.io resources=[catalog,meta]
addon name="WatchHub" id="org.stremio.watchhub" version=1.15.0 host=watchhub.strem.io resources=[stream]
addon name="Public Domain Movies" id="org.stremio.pubdomainmovies" version=1.0.0 host=caching.stremio.net resources=[catalog,stream]
addon name="OpenSubtitles v3" id="org.stremio.opensubtitlesv3" version=1.0.0 host=opensubtitles-v3.strem.io resources=[subtitles]
addon name="Local Files (without catalog support)" id="org.stremio.local" version=1.10.0 host=127.0.0.1 resources=[meta,stream]
real sign-in: not run, no credentials
```

`--catalog`, exit 0 (the repeated signed-out prefix is included):

```text
signed out: addon count=6
addon name="Cinemeta" id="com.linvo.cinemeta" version=3.0.14 host=v3-cinemeta.strem.io resources=[catalog,meta]
addon name="YouTube" id="com.linvo.stremiochannels" version=1.30.7 host=v3-channels.strem.io resources=[catalog,meta]
addon name="WatchHub" id="org.stremio.watchhub" version=1.15.0 host=watchhub.strem.io resources=[stream]
addon name="Public Domain Movies" id="org.stremio.pubdomainmovies" version=1.0.0 host=caching.stremio.net resources=[catalog,stream]
addon name="OpenSubtitles v3" id="org.stremio.opensubtitlesv3" version=1.0.0 host=opensubtitles-v3.strem.io resources=[subtitles]
addon name="Local Files (without catalog support)" id="org.stremio.local" version=1.10.0 host=127.0.0.1 resources=[meta,stream]
real sign-in: not run, no credentials
first movie catalog: first 5 item names
item name="Unabomber"
item name="The Love Hypothesis"
item name="Backrooms"
item name="The Uprising"
item name="Obsession"
```

Catalog names are live data and will change. The mock catalog supplies six known items and asserts that only the first five are returned in order.

## Reproduce

The requested commands below currently reproduce the **shared-workspace blocker**, not a successful executable:

```powershell
cargo build -p gate3-core --release --locked
cargo run -p gate3-core --release --locked
cargo run -p gate3-core --release --locked -- --catalog
cargo test -p gate3-core --locked --test mock_sign_in
```

To reproduce the isolated diagnostic from the tracked source, run this from `native/`. It only writes inside ignored `target/`; it does not change workspace membership or the core source:

```powershell
$probe = Join-Path (Get-Location) 'target/gate3-isolated-probe'
New-Item -ItemType Directory -Force "$probe/src", "$probe/tests" | Out-Null
Copy-Item -LiteralPath 'spikes/gate3-core/src/lib.rs' -Destination "$probe/src/lib.rs"
Copy-Item -LiteralPath 'spikes/gate3-core/src/main.rs' -Destination "$probe/src/main.rs"
Copy-Item -LiteralPath 'spikes/gate3-core/tests/mock_sign_in.rs' -Destination "$probe/tests/mock_sign_in.rs"
$manifest = Get-Content 'spikes/gate3-core/Cargo.toml' -Raw
$manifest = $manifest.Replace('edition.workspace = true', 'edition = "2024"')
$manifest = $manifest.Replace('license.workspace = true', 'license = "MIT"')
$manifest = $manifest.Replace('rust-version.workspace = true', 'rust-version = "1.99"')
$manifest = $manifest.Replace("`r`n", "`n").Replace("[lints]`nworkspace = true", "[lints.rust]`nunsafe_code = ""deny""")
$manifest += "`n[workspace]`n"
[System.IO.File]::WriteAllText("$probe/Cargo.toml", $manifest)
cargo generate-lockfile --manifest-path "$probe/Cargo.toml"
cargo test --manifest-path "$probe/Cargo.toml" --locked --target-dir target/gate3-isolated-build
cargo clippy --manifest-path "$probe/Cargo.toml" --all-targets --locked --target-dir target/gate3-isolated-build -- -D warnings
cargo build --manifest-path "$probe/Cargo.toml" -p gate3-core --release --locked --target-dir target/gate3-clean-release
./target/gate3-clean-release/release/gate3-core.exe
./target/gate3-clean-release/release/gate3-core.exe --catalog
```

Generating a fresh diagnostic lockfile can choose newer transitive versions. The locally recorded diagnostic lockfile and build directories remain in `target/`; they are not committed. Set both credential variables privately before running the executable to exercise real sign-in. The executable itself does not echo them. `GATE3_API_BASE` is optional and should be unset for the real API; the integration test passes its override directly to Env without modifying process environment.

## Phase 2 requirements and top risks

1. **Shared dependency compatibility and reproducibility.** Resolve the incompatible compression requirements before integrating GPUI and the core into one workspace. Keep the exact core revision and all transitive git revisions locked; review upstream version drift and incomplete licence metadata. The successful native diagnostic does not make the requested PR buildable.
2. **Durable storage and sensitive account state.** Replace the in-memory map with SQLite-backed JSON buckets, using ordered writes, durable completion and an account-aware lifecycle. The serialized `profile` includes the auth key and potentially secret-bearing addon URLs; protect those at rest and exclude them from telemetry. Run core schema migration before loading saved buckets, and make crash recovery/backups compatible with its multiple-step migrations. This spike intentionally starts fresh and does not prove persistence across launches.
3. **Runtime/UI ownership and complete models.** Continuously consume runtime events on a background task and forward sanitized state changes to the UI thread through a bounded/coalesced channel. Never hold `runtime.model()`'s read guard across an await or UI callback. `Runtime::emit` uses `try_send(...).expect(...)` (`src/runtime/runtime.rs:76-77`), so a stalled/dropped consumer can panic core worker tasks. Supervise tasks and establish shutdown/write barriers rather than relying on the throwaway process's runtime teardown.

Storage keys/buckets Phase 2 must support are declared at `src/constants.rs:10-19`:

| Key | Usage / writes |
| --- | --- |
| `schema_version` | Core migration version, currently 25. |
| `profile` | Auth/user, ordered addons, settings; sequential write in `src/models/ctx/update_profile.rs:525-536`. |
| `library_recent`, `library` | Split library storage; up to 200 recent items (`src/constants.rs:32`). For small libraries, recent contains everything and the older bucket is deleted; large libraries split recent/older data (`src/models/ctx/update_library.rs:328-389`). SQLite must implement deletion and preserve ordering of these grouped effects. |
| `streams` | Last-stream choices, sequential write in `src/models/ctx/update_streams.rs:105-119`. |
| `search_history` | Sequential write in `src/models/ctx/update_search_history.rs:48-63`. |
| `streaming_server_urls` | Sequential write in `src/models/ctx/update_streaming_server_urls.rs:48-65`. |
| `notifications` | Sequential write in `src/models/ctx/update_notifications.rs:347-362`. |
| `dismissed_events` | Sequential write in `src/models/ctx/update_events.rs:140-153`. |
| `calendar` | Constant is declared, but the pinned calendar model has no `set_storage` use; do not infer a persisted calendar implementation from the key alone. |

`Ctx` alone is sufficient for this spike's authentication, account addons, raw library bucket, and storage effects. Later refreshes can dispatch `Action::Ctx(ActionCtx::PullAddonsFromAPI)` and `SyncLibraryWithAPI` (`src/runtime/msg/action.rs:91-92`), with `PullUserFromAPI` for user refresh (`action.rs:82`). Read `ctx.profile.addons` and `ctx.library.items` from short-lived model snapshots; listen for `AddonsPulledFromAPI`, library sync/pull events, and changed-state events rather than inventing separate API requests.

`Ctx` alone is **not** a full application model. Add `CatalogsWithExtra` (`src/models/catalogs_with_extra.rs:27`) / `CatalogWithFilters` (`catalog_with_filters.rs:129`) for catalog pages and filters, `MetaDetails` (`meta_details.rs:52`) for metadata/streams/subtitles, and `LibraryWithFilters` (`library_with_filters.rs:146`) or appropriate library/continue-watching models for UI views. Add `Player` (`player.rs:96`) and map native playback events to `ActionPlayer::TimeChanged`, `PausedChanged`, seek/end/unload actions. Player updates the library item (`player.rs:533-655, 659-691, 978-989`); `Ctx` persists and synchronizes it. Watch progress lives in library item state, including offset, duration, video ID, times watched and watched bitfields (`src/types/library/library_item.rs:55-71, 103-141`). Reusing only Ctx while displaying video would not reproduce the core's watch-progress behavior.
