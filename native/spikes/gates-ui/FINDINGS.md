# Gates 2, 4, 5: GPUI 0.3.8 on Windows

This is a disposable `gates-ui` binary, separate from the shipping app and native app crate. All source references below refer to the installed crates under `%CARGO_HOME%/registry/src/index.crates.io-1949cf8c6b5b557f/`, not upstream HEAD.

## Gate 2: poster grid

**Gate 2 still FAILS in the latest 96 MiB run; the latest 8 MiB run passes.** The isolated follow-up below identifies and removes synchronous benchmark stdout stalls, but remaining shorter frame-rate variation is unresolved. The measurements use the frame-completion proxy described below. Required thresholds: average FPS >= 95% of display refresh, 1% low FPS >= 50%, and end-of-pass-2 minus end-of-pass-1 memory < 16 MiB. Both working set and private bytes are checked. A 1% low is the reciprocal of the mean of the slowest ceil(1% of frame intervals), not the FPS at a single percentile.

Transport is `reqwest =0.12.28` with rustls, using its blocking client on GPUI's background executor. It does not block the UI for HTTP or decoding. The client rejects HTTP, including redirects to HTTP. Catalog responses are streamed through a 2 MiB + 1 byte reader and rejected before JSON parsing if oversized. Pages advance by raw item count; IDs are deduplicated and missing/invalid/non-HTTPS posters are ignored. An empty catalog or fewer than 500 qualifying items is an error.

`uniform_list` virtualizes rows, with columns recomputed from viewport width. Its renderer builds only the requested rows; it also measures a row to establish height. The exact implementation computes and renders `visible_range` in `gpui-pre-0.3.8/src/elements/uniform_list.rs:479-494`. Tiles have fixed layout bounds, so hover changes only the absolute poster rectangle. Hover state is discarded for tiles that leave the visible range.

The decoded cache is a hand-written byte LRU, default 96 MiB. Visible entries are pinned; least-recently-used offscreen entries are evicted first. If a configured cap cannot accommodate the visible set, the cache refuses the insertion rather than exceeding the cap; benchmark startup fails visibly. Six concurrent loads at most are scheduled, only for visible tiles. Each compressed image body is capped at 8 MiB, decoder dimensions at 4096x4096 and allocation limit at 64 MiB. Posters are resized on the background executor to tile device dimensions plus the 3% hover margin, and converted to GPUI's BGRA order. These transient decode allocations, in-flight HTTP bodies, current frame references, text caches and GPU textures are outside the decoded LRU byte counter.

GPUI does **not** supply a byte-bounded default image cache in this pin:

| Exact source | Finding |
| --- | --- |
| `gpui-pre-0.3.8/src/elements/img.rs:535-552` | URL/path sources use an explicit ImageCache or the application asset loader; `Arc<RenderImage>` bypasses both. |
| `gpui-pre-0.3.8/src/app.rs:812, 2890-2925` | `loading_assets` retains pending and completed results until explicit `remove_asset`; there is no automatic byte budget. |
| `gpui-pre-0.3.8/src/asset_cache.rs:15-72` | The loaded value lives in `CachedLoadState::Loaded`; dropping an `img()` element does not remove the application's cache entry. |
| `gpui-pre-0.3.8/src/elements/image_cache.rs:222, 251-283` | Despite the adjacent LRU doc comment, `RetainAllImageCache` is a HashMap with insert/clear/remove and no capacity/eviction policy. |
| `gpui-pre-0.3.8/src/app.rs:2983-2994` | `drop_image(image, Some(window))` removes renderer entries from the current and other windows. |
| `gpui-pre-0.3.8/src/window.rs:5037-5049` | `drop_image` removes every frame's sprite atlas key. |
| `gpui-pre-windows-0.3.8/src/directx_atlas.rs:86-88, 108-130` | Removal deallocates the atlas tile; an unreferenced texture is released and its slot reused. |

The spike supplies its own `Arc<RenderImage>` values to `img()` and explicitly calls `cx.drop_image` for every eviction. Merely bounding a separate map while continuing to call `img(url)` would leave GPUI's decoded asset cache growing.

Each scroll pass takes 20 seconds: ten seconds down and ten back, at constant pixel speed. The second pass begins immediately after the first, with the same cache. Startup waits for all visible posters and their 220 ms fades. The benchmark retargets hover to the first visible tile as rows change, independently of desktop cursor position; it exercises the same 160ms reversible tween and drawing path as mouse hover. Logs include visible loaded/total counts, so placeholders during network loading are identifiable. Subsequent scrolling does not wait for network completion.

Frame counts use `window.on_next_frame`, as gate1 does. `gpui-pre-0.3.8/src/window.rs:2650-2672` defines this as completion of a rendered GPUI frame. The Windows renderer calls `Present(0, ...)` at `gpui-pre-windows-0.3.8/src/directx_renderer.rs:251`. This is a GPUI frame-completion proxy, **not** per-window physical scanout telemetry or DWM latency. The app requests continuous frames deliberately. Refresh comes from `EnumDisplaySettingsW(..., ENUM_CURRENT_SETTINGS, ...)`; these runs must use the primary display. Working set/private bytes come from `GetProcessMemoryInfo(PROCESS_MEMORY_COUNTERS_EX)` and peaks are sampled each frame.

### PR 0.4 isolated investigation

The supplied `evidence/scroll-isolated.log` rules out competing builds: pass 1 averaged **36.961 FPS**, 1% low **0.603 FPS**, worst **2951.348 ms**; pass 2 averaged **10.034 FPS**, 1% low **0.401 FPS**, worst **3831.658 ms**. Memory grew **24.941 MiB working / 61.023 MiB private**. At seconds 11.399 and 13.092, the cache remained at 246 entries, all 18 visible images were loaded, and each sample contained only one frame. That failed run is retained.

Hypotheses and predictions, tested before changing the image pipeline:

1. A slow app callback (including cache removal or telemetry) should appear in tick/render timings, with Windows message dispatch unresponsive during the call.
2. Paint/upload/presentation blocking should leave the app callback timings short while the main thread remains unresponsive between callbacks. A broken or delayed frame chain should instead leave Windows messages responsive during that gap.
3. Shared background-executor starvation from blocking HTTP/decode should improve when network/images are removed, and should not produce a long synchronous call inside `Samples::log`.

Instrumentation measured every tick/render with `Instant` guards, then narrowed tick into image scheduling/completion, eviction, memory sampling, scroll offset, and finally log memory/output. An independent OS watchdog woke every 50 ms and recorded tick age, active scope, and whether `SendMessageTimeoutW(WM_NULL, 10 ms)` reached the window. The early probe emitted watchdog data to stdout too; its samples cannot establish continuous watchdog coverage while stdout is blocked. A follow-up wrote the watchdog to its own buffered file, eliminating that contention. The temporary probes were archived in ignored `evidence/instrumented-source/` and removed from the final binary.

All rows are two 20-second passes at 144 Hz; pairs are pass 1 / pass 2. Memory columns are end-pass-2 minus end-pass-1, in MiB. No other Cargo/rustc build was running during these measurements, and every benchmark followed a completed locked release build. The 500 downloaded originals and frozen catalog live in ignored `evidence/local-posters/`. The placeholder control used that same catalog and titles, with no image loading. The disk control used the original compressed bodies and the same decode/resize/upload/cache paths, with no HTTP.

| Experiment / raw log under `evidence/` | Cache MiB | Average FPS | 1% low FPS | Worst ms | Working / private delta MiB |
| --- | --- | --- | --- | --- | --- |
| Supplied isolated failure: `scroll-isolated.log` | 96 | 36.961 / 10.034 | 0.603 / 0.401 | 2951.348 / 3831.658 | +24.941 / +61.023 |
| Fresh uninstrumented baseline: `scroll-followup-baseline.log` | 96 | 140.552 / 138.448 | 52.744 / 26.994 | 35.284 / 656.547 | -1.887 / -1.816 |
| Fresh uninstrumented baseline: `scroll-followup-baseline-8mib.log` | 8 | 128.199 / 123.932 | 11.682 / 9.957 | 713.856 / 1207.318 | -1.602 / -1.629 |
| Remote images, first timing probe: `scroll-probe-remote.log` | 96 | 133.854 / 143.756 | 17.160 / 101.231 | 578.275 / 16.555 | +4.723 / +4.781 |
| No images, frozen titles: `scroll-probe-placeholders.log` | 96 | 136.097 / 143.905 | 21.196 / 123.198 | 480.211 / 20.896 | +0.129 / +0.121 |
| Disk images: `scroll-probe-local.log` | 96 | 131.456 / 131.508 | 13.774 / 13.791 | 738.511 / 1008.122 | +0.133 / +4.188 |
| Disk images, independent watchdog: `scroll-probe-independent.log`, `watchdog-15708.log` | 96 | 142.603 / 143.402 | 67.354 / 91.356 | 128.613 / 35.845 | +0.492 / +0.477 |
| Deferred reports, disk images, independent watchdog: `scroll-buffered-probe-local.log`, `watchdog-1040.log` | 96 | 143.505 / 143.354 | 98.083 / 93.682 | 28.052 / 29.368 | +3.395 / +3.375 |

**Root cause proven by timing:** `Gate::tick -> Gate::scroll_tick -> Samples::log -> println!` synchronously writes redirected stdout on the UI thread. With placeholders, the log-output scope took **478.762 ms**, the entire tick **478.773 ms**, and log-memory **0.016 ms**. With disk images, these were **1006.468 / 1006.483 / 0.004 ms**. The independent-file watchdog confirmed the same call: at elapsed 15449 ms, tick age was **82.584 ms**, scope `log-write`, main response **false**; that call finished after **126.112 ms**, versus **126.127 ms** for the whole tick. Maximum render was **0.367 ms**, image completion **0.162 ms**, eviction **0.141 ms**, per-frame memory **0.137 ms**, and scroll offset **0.021 ms**. These stalls were inside the app callback, not idle gaps awaiting a frame. Network is not necessary to reproduce them, and no-image runs exclude decode/upload/atlas as their cause. The underlying reason the redirected stdout consumer delays individual writes was not separately sampled; the measured blocking call is the spike's synchronous `println!`.

The shared executor and GPU paths were also inspected in the installed 0.3.8 sources:

| Exact source under `%CARGO_HOME%/registry/src/index.crates.io-1949cf8c6b5b557f/` | Finding |
| --- | --- |
| `gpui-pre-windows-0.3.8/src/platform.rs:247`, `src/dispatcher.rs:54-69, 106-117, 175-181` | BackgroundExecutor dispatches work through `TrySubmitThreadpoolCallback`; the callback environment does not configure a private pool or a fixed thread count. Blocking HTTP/decode occupies Windows default-pool callbacks, but the measured stalled tick is not waiting for these results. |
| `gpui-pre-windows-0.3.8/src/platform.rs:433-495`, `src/vsync.rs:39-61` | VSync runs on its own `std::thread`, waits through `DwmFlush` (or an interval sleep), and invalidates windows. It is not a background-executor job. |
| `gpui-pre-windows-0.3.8/src/events.rs:1328-1380` | `draw_window` calls the frame callback synchronously, then validates the window region. A blocked tick keeps that draw callback from returning. |
| `gpui-pre-0.3.8/src/window.rs:4943-4955`; `gpui-pre-windows-0.3.8/src/directx_atlas.rs:72, 92, 133, 159, 279` | RenderImage is inserted lazily by image ID/frame into an atlas, uploaded with `UpdateSubresource`. Full atlases allocate another texture (minimum 1024x1024); existing textures are not resized/copied and images do not each get a private texture. |
| `gpui-pre-0.3.8/src/app.rs:2983-2994`, `src/window.rs:5037-5049`; `gpui-pre-windows-0.3.8/src/directx_atlas.rs:108-130` | `drop_image` removes atlas tiles, deallocates slots and releases unreferenced textures; observed eviction timings were short. |
| `gpui-pre-windows-0.3.8/src/directx_renderer.rs:245-254, 1027-1031` | Present is a synchronous `Present(0, DXGI_PRESENT(0))` call, so native/driver waits are possible; DirectComposition Commit occurs when binding the swap chain. Neither call explains a stall timed entirely inside `Samples::log`. |

**Fix:** scroll `Samples` enqueue their progress and SUMMARY lines in a shared report vector; the pass-boundary MEMORY line is queued too. `main` drains it only after GPUI's `run` returns, so neither periodic stdout writes nor the first-pass summary can perturb the second pass. Report strings retain their original measured timestamps/counters and ordering; live progress is now delivered at benchmark exit. The finite two-pass benchmark queues about 41 lines (under 16 KiB in measured output). Memory sampling, HTTP, the six-worker limit, decode/resize, byte LRU, eviction, hover, fade, virtualization, frame requests and threshold calculations are unchanged. No dependency was added.

The independent watchdog after the fix measured maximum tick **0.233 ms**, render **0.340 ms**, log-memory **0.016 ms**, report formatting/enqueue **0.023 ms** and maximum sampled tick age **7.270 ms**. This is the same disk-image workload as the preceding failed control. The end-to-end benchmark is the regression check: a unit test for formatting or queue insertion would not exercise the stdout stall and frame chain.

Reproduction commands from `native/` (each benchmark runs alone after the build finishes):

```powershell
cargo build -p gates-ui --release --locked
python spikes/gates-ui/evidence/download-posters.py
./spikes/gates-ui/evidence/bench-probe.ps1 -Output scroll-placeholders -Cache 96 -NoImages -LocalPosters spikes/gates-ui/evidence/local-posters
./spikes/gates-ui/evidence/bench-probe.ps1 -Output scroll-local -Cache 96 -LocalPosters spikes/gates-ui/evidence/local-posters
./spikes/gates-ui/evidence/bench-probe.ps1 -Output scroll-fixed-96 -Cache 96
./spikes/gates-ui/evidence/bench-probe.ps1 -Output scroll-fixed-8 -Cache 8
```

The diagnostic build also accepted `--diagnose` (`-Diagnose` in the archived runner). Its source and runner snapshot is in `evidence/instrumented-source/`; watchdog timings were removed after the controlled comparison. The placeholder and disk controls remain available as `--no-images` and `--local-posters <dir>`.

The supplied memory failure is consistent with incomplete warm-up caused by the stalls: its decoded cache was still **61.763 MiB / 312 entries** at pass-1 second 19.407, versus **88.488 MiB / 447 entries** at pass-2 second 19.456. This explains why pass 2 can still add decoded/GPU allocations; it does not by itself establish a leak. The fresh baselines and disk controls already had flat memory. Final remote results are recorded below without removing any failed runs.

### Cache-stress follow-up after deferring stdout

The first probe-free remote runs passed at 96 MiB, but the 8 MiB second-pass 1% low was **69.534 FPS**, below 72. Its worst interval was **17.923 ms**, rather than a seconds-long freeze. These results triggered additional controls instead of a speculative image-pipeline change.

| Experiment / raw log under `evidence/` | Cache MiB | Average FPS, passes 1 / 2 | 1% low FPS, passes 1 / 2 | Worst ms, passes 1 / 2 | Working / private delta MiB |
| --- | --- | --- | --- | --- | --- |
| Deferred output, remote, six loaders: `scroll-fixed-96.log` | 96 | 143.553 / 144.000 | 86.792 / 127.050 | 26.524 / 10.675 | +0.172 / +0.262 |
| Deferred output, remote, six loaders: `scroll-fixed-8.log` | 8 | 142.654 / 141.802 | 72.369 / 69.534 | 18.008 / 17.923 | -0.035 / +0.125 |
| Deferred output, disk, six loaders: `scroll-fixed-local-8.log` | 8 | 143.752 / 144.003 | 113.262 / 135.983 | 27.391 / 7.749 | -0.035 / -0.039 |
| Remote, two loaders: `scroll-two-workers-8.log` | 8 | 143.303 / 144.003 | 85.411 / 133.161 | 18.209 / 7.906 | +0.242 / +0.215 |
| Remote async HTTP, six loaders: `scroll-async-8.log` | 8 | 143.953 / 138.504 | 120.402 / 23.876 | 18.389 / 161.356 | +0.051 / +0.008 |

The concurrency prediction was that fewer simultaneous loads would reduce remaining missed frames. Two loaders passed FPS, but the ratio of loaded-visible counts to total-visible counts across the 19 once-per-second samples fell from **82.4% / 81.9%** with six loaders to **17.3% / 18.3%**. That control mostly rendered placeholders, so it was rejected and the temporary `--image-workers` control removed. The disk control achieved **95.0% / 93.9%** coverage and passed with six loaders, so eviction/decode/upload alone did not reproduce the remote stress failure in this control.

The shared-pool prediction was tested by moving only poster HTTP to async reqwest on a two-I/O-worker Tokio 1.53.2 runtime. Six loads remained allowed; completed bodies were decoded/resized on GPUI's executor, with the same body/dimension/allocation limits, GPU path and cache. Coverage was **80.7% / 76.4%**, but the second pass failed badly. This does not support adopting async HTTP as the fix for this failure; that trial, its Tokio dependency, and lockfile addition were removed. The retained implementation contains only the proven deferred-report fix and the requested placeholder/disk controls. The failed trials remain listed.

Final measurements use the retained six-loader implementation, with no commands or commentary during each benchmark. This removes our own mid-run activity as another variable; it does not establish why the shorter remote-only misses occurred. The original stdout stall is proved independently by call timings and the controlled buffered-output comparison.

### Latest results after all probes and experimental changes were removed

All four checks passed on the retained code: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked` (7 tests), and `cargo build -p gates-ui --release --locked`. The final locked release build completed before either run. The final launch commands checked for Cargo/rustc/link processes and refused to start if any were present; none were present. The two benchmarks ran sequentially with no commands/commentary during either measurement. No Git commit or push was made. No new dependency remains; the pre-existing workspace/lockfile changes were preserved.

| Cache / pass | Frames / seconds | Average FPS | 1% low FPS | Worst interval ms | FPS result |
| --- | --- | --- | --- | --- | --- |
| 96 MiB / 1 | 2827 / 20.005 | 141.313 | 58.170 | 33.230 | FAIL |
| 96 MiB / 2 | 2606 / 20.000 | 130.298 | 27.690 | 41.731 | FAIL |
| 8 MiB / 1 | 2879 / 20.006 | 143.904 | 100.037 | 13.588 | PASS |
| 8 MiB / 2 | 2876 / 20.007 | 143.747 | 90.550 | 15.671 | PASS |

All memory figures below are MiB, start / peak / end:

| Cache / pass | Working set | Private bytes |
| --- | --- | --- |
| 96 MiB / 1 | 59.793 / 165.164 / 158.891 | 107.289 / 343.027 / 337.918 |
| 96 MiB / 2 | 158.891 / 162.223 / 162.223 | 337.918 / 341.242 / 341.242 |
| 8 MiB / 1 | 61.496 / 75.629 / 68.664 | 108.789 / 134.922 / 123.688 |
| 8 MiB / 2 | 68.625 / 75.828 / 67.758 | 123.648 / 136.820 / 128.266 |

Memory passes at both sizes: **+3.332 MiB working / +3.324 MiB private** at 96 MiB, **-0.906 / +4.578 MiB** at 8 MiB. Decoded-cache plateaus remain **95.813 MiB / 484 entries** and **7.918 MiB / 40 entries**. The latest 96 MiB warm-pass once-per-second samples all have every visible poster loaded. The final 8 MiB run's loaded-visible sample ratios are **75.4% / 75.5%**; passing callback FPS is not proof of full poster coverage or physical scanout cadence.

Raw logs: `evidence/scroll-final-96.log`, `evidence/scroll-final-8.log`, and their `.stderr.log` files (empty). Exact final commands from `native/`:

```powershell
./spikes/gates-ui/evidence/bench-probe.ps1 -Output scroll-final-96 -Cache 96
./spikes/gates-ui/evidence/bench-probe.ps1 -Output scroll-final-8 -Cache 8
```

**Conclusion:** the original measured synchronous-output stall is fixed, and pass-to-pass memory is flat in the final runs. Gate 2 is **not passed overall**: the latest 96 MiB run fails average/1% low cadence, and the retained 8 MiB history also includes a 69.534 FPS low failure. Earlier passing post-fix runs are retained as controls, not substituted for the latest failure. The remaining shorter cadence stalls have not been assigned a proven call site; neither changing loader count nor async HTTP supplied an acceptable evidence-backed fix. Further diagnosis is required for a reliable Gate 2 pass.

## Gate 4: bundled fonts

The three static TTFs and `OFL.txt` are exact copies of `public/fonts/`. The variable TTF is downloaded from `https://raw.githubusercontent.com/google/fonts/main/ofl/dmsans/DMSans%5Bopsz%2Cwght%5D.ttf` (240,164 bytes; SHA256 `8CD08D97E89C24D0AA92EDD2F0F4C8EE6195EEE9B7C9F154865A58B02F0C1C0D`). All four files are bundled with `include_bytes!` and registered with `cx.text_system().add_fonts`.

Both distributions declare DM Sans. For a valid simultaneous comparison, the variable font's name table is given the equal-length **in-memory-only** alias `DM Vari`, with SFNT checksums recalculated. The committed upstream file is untouched. The aliased bytes have process-long lifetime. This prevents variable/static family collisions from making one column silently use the other's file. No installed DM Sans is needed. Both columns show requested 400/500/700 at all seven requested sizes/line heights, with mixed punctuation and the Japanese/Latin fallback line. A DM Sans/Segoe UI comparison appears above them.

The probe logs GPUI's resolved family and M advance at 52 px, and independently repeats GPUI's exact DirectWrite font-set matching path to report the actual face family, weight and axes. `TextSystem::get_font_for_id` alone maps back to the requested font (`gpui-pre-0.3.8/src/text_system.rs:354-365`), so it cannot prove that DirectWrite instantiated the requested variable axis.

| Exact source | Finding |
| --- | --- |
| `gpui-pre-windows-0.3.8/src/direct_write.rs:360-398` | Registers in-memory files through IDWriteFactory5 / IDWriteFontSetBuilder1 and builds a custom collection. |
| `gpui-pre-windows-0.3.8/src/direct_write.rs:465-500` | Selects by family, enum weight/stretch/style using `GetMatchingFonts`, then `GetFontFaceReference` / parameterless `CreateFontFace`. No explicit axis values or font size are passed. |
| `gpui-pre-0.3.8/src/text_system.rs:1292-1315` | `Font` has family, weight, style, features, and fallbacks; it has no optical-size/variation-axis configuration. |

**PASS for both variable and static files at 400/500/700.** Runtime DirectWrite axes are `wght=400/500/700`, `opsz=14` for both distributions. The variable file therefore works for the requested weights through DirectWrite's font-set/face-reference selection, even though GPUI never passes explicit axis values. This is not proof of arbitrary axes or automatic optical-size interpolation; `opsz` stays 14 at 52px too, matching the existing static fonts.

The first run exposed a real silent fallback: asking for `DM Sans` returned Segoe UI. `all_font_names()` instead advertised `DM Sans 14pt` and `DM Vari 14pt`; requesting those names resolves the bundled faces. GPUI obtains names from the custom collection in `direct_write.rs:1285-1293, 1759-1778` and uses the supplied string directly in `GetMatchingFonts` at lines 488-493. The static medium file's legacy name is `DM Sans Medium`, but the resulting collection groups all three weights under `DM Sans 14pt`. Both columns use the advertised families, and the probe prints the actual DirectWrite face family as well as GPUI's resolved family.

Phase 1 should use the three existing **static files**, with the Windows family name `DM Sans 14pt`, for this scale and these weights. They total 170,632 bytes versus 240,164 for the variable file, match the existing web assets at optical size 14, and avoid relying on variable named-instance selection. This recommendation is not a variable-font failure: the single variable file also passes all three requested weights in this exact pin. If Phase 1 needs automatic `opsz` or custom variation axes, the GPUI `Font` API/backend needs additional work.

## Gate 5: hand-built motion

`motion.rs` implements the two mock pages, presence, the modal and focus handling without gpui-component. State retained: two (opacity, Y) tween pairs; target page; panel and scrim tweens; dialog target and presence flags; animation revision; opener/Close/Stay focus handles; and the focus handle to restore. A tween keeps start value, target, start instant, duration and easing choice. Reversing samples the existing tween before retargeting, so there is no discontinuity in opacity, scale or translation. A fully absent incoming page starts at +16px; an interrupted visible page continues from its current Y. The outgoing page stays mounted until its 160ms fade ends; the incoming page takes 320ms.

GPUI `Animation` / `with_animation` wrap the elements; the hand-built tween clock supplies current values and the specified cubic-bezier. Presence removal is explicit after the closing tween finishes. The dialog's scrim covers the entire window, including tabs. Focus is moved to Close on opening, Tab/Shift-Tab cycle between two controls, and focus restoration happens after close presence ends. Screen changes are blocked while modal presence exists.

The automated sequence runs 20 A->B->A cycles and 20 dialog open/close cycles, logging each of the 80 individual directions (count, elapsed time, worst interval). There is a 100ms settled interval between directions. Recording uses a single in-flight desktop capture on a background executor, requested approximately every 33ms; no unbounded queue of frames is created. Capture mode runs the same sequence. Capture-influenced timing is kept separate from the benchmark without recording.

**PASS for the specified mock transition and dialog.** `motion.rs` is 261 lines (including layout, controls and state); the shared cubic-bezier and reversible tween add 72 lines in `logic.rs:131-202`. The automation driver in `main.rs` is separate measurement code. Phase 1 should extract a `Presence`/transition wrapper that owns current tween values, retargeting, outgoing lifetimes, completion and focus restoration. A modal helper also needs keyboard trapping and scrim hit testing. These are manual state/lifecycle costs, not merely two animation declarations.

## Limitations to account for in Phase 1

- **Scale:** this pin's `Style` has opacity but no general subtree transform (`gpui-pre-0.3.8/src/style.rs:301-302`). Posters expand absolute bounds within fixed tiles. The dialog scales bounds, padding, text sizes and internal spacing explicitly. This reruns layout and text shaping; it is not a compositor transform. Native text can rerasterize during scale, rather than scaling one cached surface.
- **Opacity:** subtree opacity is implemented through `Interactivity::paint` (`gpui-pre-0.3.8/src/elements/div.rs:2544`) and `Window::with_element_opacity` (`src/window.rs:4078`). Overlapping descendants blend individually; this is not a CSS-style offscreen group surface.
- **Image corners:** set radius on `img()` itself. `img.rs:487-504` passes it to `paint_image`; `window.rs:4917-4926` describes rounded image masking. Parent overflow masking alone is rectangular. Poster outlines are separate rounded rectangles.
- **Focus/presence:** retained elements and manual key trapping are necessary; this spike has two controls, not a general focusable-descendant traversal. Scaling/layout may need special treatment for production dialogs with complex text and controls.
- **Measurement:** GPUI frame callbacks cannot certify physical presentation cadence. HTTPS loading means fast scrolling can show placeholders even while callback FPS meets the threshold. Decoded cache bytes do not bound total process/GPU memory.
- **Desktop capture:** GDI captures the actual screen rectangle, including chrome and occluding windows. Keep the window visible and unobscured; minimized capture fails. A still image does not establish animation quality or absence of transient artifacts.

## Exact commands

From `native/`, on an interactive Windows desktop:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build -p gates-ui --release --locked
New-Item -ItemType Directory -Force spikes/gates-ui/evidence | Out-Null
./target/release/gates-ui.exe
./target/release/gates-ui.exe --bench-scroll > spikes/gates-ui/evidence/scroll.log 2>&1
./target/release/gates-ui.exe --bench-scroll --image-cache-mib 8 > spikes/gates-ui/evidence/scroll-8mib.log 2>&1
./target/release/gates-ui.exe --bench-motion > spikes/gates-ui/evidence/motion.log 2>&1
./target/release/gates-ui.exe --screenshot spikes/gates-ui/evidence/grid.png --screen 1 --after 12 > spikes/gates-ui/evidence/grid.log 2>&1
./target/release/gates-ui.exe --screenshot spikes/gates-ui/evidence/type.png --screen 2 --after 3 > spikes/gates-ui/evidence/type.log 2>&1
./target/release/gates-ui.exe --screenshot spikes/gates-ui/evidence/motion.png --screen 3 --after 3 > spikes/gates-ui/evidence/motion-screen.log 2>&1
./target/release/gates-ui.exe --frames spikes/gates-ui/evidence/frames --screen 3 > spikes/gates-ui/evidence/frames.log 2>&1
./target/release/gates-ui.exe --frames spikes/gates-ui/evidence/frames-fast --screen 3 > spikes/gates-ui/evidence/frames-fast.log 2>&1
```

Interactive keys: 1/2/3 choose screens; Enter/click toggles mock pages; Backspace/Esc returns to A; D opens/reverses the modal; Esc, scrim click or Close closes it. Pure-logic tests cover byte-cap/pinned eviction/replacement/oversize refusal, known cubic-bezier values and monotonicity, reversal in both directions, and capped JSON/reader parsing with HTTPS-only filtering. Evidence captures/logs are ignored, as in gate1; fonts and source are tracked. No extra unapproved crate was added.

## Measured results and validation

Measured on 2026-10-10, Asia/Bangkok: Windows 10 Pro 10.0.19045, NVIDIA GeForce GTX 1650, primary display 1920x1080 at **144 Hz**, 16 GiB RAM. The sandbox process helper failed before PowerShell started. Approved command execution outside that helper allowed real window launch, Win32 counters and desktop captures; these are measured values, not sandbox placeholders. This spike's compilation had finished before the performance runs. Other worktrees' Cargo/rustc processes were active during the final scroll runs, including builds in `p2-store` and `p0-gate6`. They were left running. This is an observed competing workload, not a proven cause of the stalls. All benchmark/capture processes exited 0.

### Gate 2 numbers

The thresholds at 144 Hz are 136.8 average FPS and 72 FPS for the 1% low.

| Cache / pass | Frames / seconds | Average FPS | 1% low FPS | Worst interval ms | Result |
| --- | --- | --- | --- | --- | --- |
| 96 MiB / 1 | 2724 / 20.007 | 136.156 | 22.520 | 196.779 | FAIL |
| 96 MiB / 2 | 2171 / 20.007 | 108.514 | 16.114 | 133.138 | FAIL |
| 8 MiB / 1 | 2735 / 20.001 | 136.741 | 25.224 | 613.923 | FAIL |
| 8 MiB / 2 | 2746 / 20.005 | 137.263 | 30.325 | 180.346 | FAIL |

All memory figures below are MiB, reported as start / peak / end for each pass.

| Cache / pass | Working set | Private bytes |
| --- | --- | --- |
| 96 MiB / 1 | 60.203 / 165.578 / 160.469 | 107.996 / 341.320 / 336.203 |
| 96 MiB / 2 | 160.473 / 165.602 / 163.629 | 336.203 / 341.363 / 339.348 |
| 8 MiB / 1 | 60.219 / 72.672 / 68.109 | 108.113 / 136.375 / 129.523 |
| 8 MiB / 2 | 68.113 / 72.684 / 69.570 | 129.523 / 134.652 / 130.703 |

- Default-cache pass-2 delta: **+3.160 MiB working set, +3.145 MiB private**, both <16: PASS.
- 8 MiB stress pass-2 delta: **+1.461 MiB working set, +1.180 MiB private**, both <16: PASS.
- Default decoded cache plateau: **95.813 MiB / 484 entries**. Stress plateau: **7.918 MiB / 40 entries**. The latter forces repeated offscreen eviction and reloading.
- Network coverage is not hidden: default-cache second-level samples reached a minimum 0/18 loaded visible tiles during the cold pass; the warm-pass samples have all 18-24 visible tiles loaded. The smaller cache also shows placeholders during cold reloads. FPS alone would not establish that every cold-scroll tile was already downloaded.
- `grid.png` shows real posters, the rounded outlines, one-line 500-weight titles, and a hovered poster expanded inside its fixed tile. The final scroll benchmark also drives hover deterministically as the visible rows change.

Raw logs: `evidence/scroll.log`, `evidence/scroll-8mib.log`.

A supplementary 96 MiB run raised only the spike window above other windows to remove occlusion as a variable. It also failed: pass 1 averaged **87.693 FPS**, 1% low **2.174 FPS**, worst **2112.696ms**; pass 2 averaged **39.236 FPS**, 1% low **0.548 FPS**, worst **7029.925ms**. Working-set start/peak/end was **61.703/166.738/156.602 MiB**, then **156.605/166.918/161.227 MiB**; private bytes were **109.422/340.848/330.016 MiB**, then **330.016/344.352/338.664 MiB**. Memory still passed: **+4.625 MiB working set / +8.648 MiB private**. Other compilations remained active. Raw log: `evidence/scroll-visible.log`. This failed run is retained, not replaced by earlier passing measurements from a different workload. The result does not establish refresh-rate scrolling on an isolated machine.

Exact supplementary command, from `native/`:

```powershell
./spikes/gates-ui/evidence/bench-probe.ps1
```

### Gate 4 numbers and captures

| Weight | Static M advance at 52px | Variable M advance at 52px | Segoe UI M advance at 52px |
| --- | --- | --- | --- |
| 400 | 43.732px | 43.732px | 46.693px |
| 500 | 44.460px | 44.460px | 48.064px |
| 700 | 46.072px | 46.072px | 49.766px |

GPUI and DirectWrite both report `DM Sans 14pt` for the static bundled family; the isolated variable family reports `DM Vari 14pt`. The independent probe reports the requested actual weights and axes, rather than inferring correctness from the requested descriptor. Both columns visibly match at small and large sizes. Japanese glyphs, accented Latin and Turkish letters render without tofu in the inspected captures; fallback selection for individual Japanese glyphs was not separately identified.

Inspected `evidence/type.png` (small sizes and the Segoe UI comparison) and `evidence/type-large.png` (52px medium/bold and non-Latin fallback). The first type capture was occluded and was replaced by an unobscured capture. The temporary `evidence/capture-probe.ps1` raises only the test window during capture and restores the cursor. Exact additional capture commands:

```powershell
./spikes/gates-ui/evidence/capture-probe.ps1
./spikes/gates-ui/evidence/capture-probe.ps1 -Output type-large.png -ScrollBottom
```

Probe stdout is in `evidence/type.log` and `evidence/type.png.stdout.log`. The two final type columns use their advertised families; a successful startup resolution alone would not have validated incorrectly named element-level fonts.

### Gate 5 numbers and interaction checks

The non-recording benchmark completes 20 runs of each direction, 80 directions total:

| Direction | Runs | Frames per run | Mean elapsed ms | Maximum worst-frame interval ms |
| --- | --- | --- | --- | --- |
| A -> B | 20 | 47 | 326.364 | 7.896 |
| B -> A | 20 | 47 | 326.358 | 9.747 |
| Dialog open | 20 | 32 | 222.233 | 9.353 |
| Dialog close | 20 | 24 | 166.662 | 8.207 |

Elapsed times include sampling to the next display frame; the specified tween durations remain 320 / 220 / 160ms. The worst interval across all directions is **9.747ms**. Raw per-run log: `evidence/motion.log`.

The HWND-targeted keyboard probe records `D` with modal=false, then Tab with modal=true and Close focused, then `1` with modal=true and Stay focused. Two more Tabs cycle Stay -> Close -> Stay. `motion-focus.png` confirms the motion screen remains selected and Stay has the focus outline. Another sequence interrupts dialog open/close using D/D/D/Esc, waits for closing presence to end, then interrupts page navigation using Enter/Backspace/Enter. The input logs confirm restored navigation after Esc, and `motion-reversal.png` shows the final details page beneath an open dialog. Exact tween continuity in both directions is also unit-tested. These probes inject `WM_KEYDOWN`/`WM_KEYUP` into the spike's HWND; they do not depend on another application allowing foreground activation. The earlier global-key attempt was inconclusive because a foreground window intercepted keys; it is not used as focus evidence.

```powershell
./spikes/gates-ui/evidence/key-probe.ps1 -Screen 3 -Output motion-focus.png -Mode focus
./spikes/gates-ui/evidence/key-probe.ps1 -Screen 3 -Output motion-reversal.png -Mode reverse
```

The standalone `motion.png`, selected page-transition frames and opening/fully-open dialog frames were inspected. In the final recording, `frames-fast/00008.png` shows both pages during the transition, `00009.png` shows the settled details page, and `00402.png` / `00403.png` show partial dialog presence. The recording contains both outgoing and incoming pages, a growing/fading panel, the full-window scrim, and settled endpoints. There is no compositor subtree scale: text is relaid out at scaled sizes, as described above.

The final recording is **679 numbered PNGs** under `evidence/frames-fast/`, covering all 80 automated directions over 30.790s. File-creation interval median is **35.000ms**, mean **45.413ms**, maximum **450.315ms**. The first recording used PNG's default encoding and produced 635 files with a 47.723ms median interval. The final encoder uses `Compression::Fast` and the Up filter. Captures stay on the background executor, with one in flight. File-creation intervals are an approximate capture-cadence proxy; GDI, encoding and filesystem delays can make gaps longer than the requested 33ms. This is an approximately 33ms capture request, not a guaranteed 30fps recording. Recording timings are not substituted for the non-recording motion benchmark. Both recordings remain available locally; the faster one is the review artifact.

### Validation and scope

All four required commands passed against the final Rust source: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, `cargo build -p gates-ui --release --locked`. Workspace tests: **7 passed**, including all four new pure-logic tests; no failures. `git diff --check` also passed. The lockfile adds the new package and reqwest's already-resolved futures-channel dependency for its blocking feature; no package version changed. Changes are confined to `native/Cargo.toml`, `native/Cargo.lock`, and this new spike. No existing app crate or other spike was edited, no target directory was deleted, and no commit or push was made.

## Gate 2 verification by Opus (2026-10-10, machine idle, no cargo/rustc/codex running)

`bench-probe.ps1 -Cache 96`, two consecutive runs after the deferred-report fix:

| Run | Pass 1 avg / 1% low FPS | Pass 2 avg / 1% low FPS | Worst ms (p1 / p2) | Memory pass2-pass1 (working / private) |
|---|---|---|---|---|
| 1 | 143.602 / 85.728 | 144.005 / 129.109 | 21.546 / 8.543 | -0.641 / -0.641 MiB |
| 2 | 143.553 / 85.608 | 143.953 / 125.415 | 16.462 / 12.539 | +0.605 / +4.727 MiB |

Both runs pass every threshold (avg >= 136.8, 1% low >= 72, growth < 16 MiB). The last failing 96 MiB run above was taken while the implementing agent's own processes were active. **Gate 2 passes on Windows.** Phase 1 carries one lesson: nothing on the UI thread may write synchronously to stdout/stderr or a file; logging goes through a background sink.
