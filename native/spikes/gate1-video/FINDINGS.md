# Gate 1: mpv under GPUI on Windows

## Outcome

Approach A answers the core composition question **yes**: desktop captures show hardware-decoded mpv video with GPUI controls above it, both windowed and fullscreen. Mouse delivery and clean shutdown also pass. **The complete gate is not yet certified:** criteria 2 and 3 still need observation of transient artifacts. They are marked FAIL (unverified), rather than claiming that a still image or a size log proves absence of flicker. No actual compositing failure was observed, so B was not attempted; C was not built.

Tested on 2026-10-10 (Asia/Bangkok), Windows 10 Pro 10.0.19045, with a 1920x1080 desktop. GUI launch and desktop capture were available through approved command execution after the sandbox's process helper failed to start. The captured player client was 1280x720 windowed. Media: Blender's `bbb_sunflower_1080p_30fps_normal.mp4` (276,134,947 bytes); mpv explicitly reported `H.264 / AVC / MPEG-4 AVC / MPEG-4 part 10`. The example Peach `.mov` URL returned HTTP 404; the current [Blender sample directory](https://download.blender.org/demo/movies/BBB/) supplied a ZIP, downloaded and extracted under ignored `media/`.

Installed display adapters: NVIDIA GeForce GTX 1650 (driver 32.0.15.9144) and Parsec Virtual Display Adapter (driver 0.45.0.0). `d3d11va` is mpv's measured result; the spike does not query which D3D11 adapter mpv selected.

The spike uses the workspace's exact gpui-pre/gpui-pre-platform 0.3.8 pins. It creates one disabled child HWND with `WS_CHILD | WS_CLIPSIBLINGS | WS_CLIPCHILDREN`, sets mpv's `wid` to that child, and keeps it at `HWND_BOTTOM`. GPUI's root has no fill; the two overlays are GPUI elements. The bottom bar is 64 logical pixels tall with `#0b0b0c` at 70% opacity. Win32 client dimensions are physical pixels and are read on GPUI bounds changes and every render, including DPI/fullscreen changes.

All playback operations and mpv events run on a dedicated thread. Copied property events cross a channel to a GPUI task, which calls `cx.notify()`. Close, screenshot completion, and playback errors stop and join the event thread before destroying the video HWND or unloading the DLL. The thread wakes at most every 50 ms when idle; `mpv_terminate_destroy` itself has no imposed timeout.

## Follow-up capture probe (supersedes the two unverified rows below)

Criteria 2 and 3 were re-checked with a capture probe: the client area was grabbed from the desktop about 20 times a second while the window was resized continuously for 10 seconds (190 distinct sizes, 238 frames) and then toggled fullscreen five times (241 frames).

- **2. Resize: PASS.** At most 0.8 % of the video area was near-black in any frame, and the strip above the control bar never went dark, so the video kept up with the window and the bar stayed drawn.
- **3. Fullscreen: PASS at this sampling rate.** Six frames were flagged, one per transition. Each was the probe reading the old window rectangle while the window had already changed size, so it captured the desktop behind it; the video itself was drawn in every one. A flash shorter than about 50 ms would not be caught by this probe.

Gate 1 passes on Windows with approach A.

## Results

| Criterion | Result | Evidence / remaining check |
| --- | --- | --- |
| 1. Video and GPUI controls overlap visibly | PASS | Inspected desktop BitBlt captures `evidence/approach-a.png` (1296x759 including chrome) and `evidence/fullscreen.png` (1920x1080). Video is visible through the 70% bar, and both overlays draw above it. Playback clock advances in the corresponding logs. |
| 2. Drag-resize for 10 seconds without flashes/stale frames | FAIL (unverified transient artifacts) | Desktop mouse input held and dragged the right edge for 50 steps x 200 ms. Client widths in `evidence/input-resize-fullscreen.log` changed from 1280 through 1130-1427 and settled at 1299; playback advanced throughout. The entire drag was not visually observed or captured frame by frame, so black flashes, stale exterior frames, and continuous control layering are not certified. |
| 3. Five fullscreen on/off cycles | FAIL (unverified flicker limit) | Ten actual F key presses completed five cycles. `evidence/interactive-probe.log` records every fullscreen client at 1920x1080 and every restored client at 1299x720. The separate fullscreen screenshot shows full-screen video and both overlays. No frame-by-frame transition record exists to measure the one-frame flicker limit. |
| 4. Hardware H.264/HEVC decode | PASS | `video-codec=H.264 / AVC / MPEG-4 AVC / MPEG-4 part 10` and repeated `hwdec-current=d3d11va` in all successful playback logs; decoder also appears in both screenshots. |
| 5. GPUI receives control-bar mouse clicks | PASS | Actual desktop mouse clicks on Pause/resume and the seek bar produced `GPUI input: toggle pause` twice, `pause=true` then `false`, and `GPUI input: seek 235.68381438652676 absolute+exact` followed by `time-pos=235.767`. This did not inject mouse messages directly into GPUI. |
| 6. Clean close with mpv destruction and process exit | PASS | WM_CLOSE after the five cycles and both timed screenshot runs logged `mpv destroyed`, then `Gate 1 process exiting`, and returned code 0 without hanging. A separate `Process.CloseMainWindow()` run also exited normally. No spike process remained at the end. |

Keyboard checks also delivered Space twice, Right/Left, F, Esc, F through desktop input. `evidence/fullscreen.log` records both pause actions, relative +/-10-second seek actions and matching clock changes; the final capture confirms fullscreen after leaving and re-entering it. Steady windowed/fullscreen GPUI completion counts were approximately 144 frames/s after compilation pressure subsided; the first capture run briefly measured 28-29 frames/s while dependencies were compiling. This is not a latency benchmark.

Evidence files and probe scripts are under ignored `evidence/`; they exist locally and are not committed. The temporary mouse/keyboard probe restores the original cursor position. Its first title lookup failed; obtaining the exact spike process's `MainWindowHandle` corrected the probe. No desktop-permission block was established.

An initial relative-media invocation failed because mpv resolved the path against the repository root rather than the invoking `native/` directory. The final CLI resolves local media and screenshot paths before application startup; subsequent identical relative-path commands succeeded. This startup path issue was fixed before the recorded successful captures.

## GPUI source evidence

Source root: `%CARGO_HOME%/registry/src/index.crates.io-1949cf8c6b5b557f/`. These references are to the exact installed 0.3.8 packages, not upstream HEAD.

| File and lines | Behavior relied on |
| --- | --- |
| `gpui-pre-platform-0.3.8/src/gpui_platform.rs:16-27, 83-90` | `application()` selects the real Windows platform with `headless=false`. |
| `gpui-pre-windows-0.3.8/src/window.rs:490-565` | Creates the HWND with `CreateWindowExW`; enables `WS_EX_NOREDIRECTIONBITMAP` when DirectComposition is enabled. |
| `gpui-pre-windows-0.3.8/src/directx_renderer.rs:159-183` | Creates the D3D11 renderer and DirectComposition target/visual, then attaches the swapchain. |
| `gpui-pre-windows-0.3.8/src/directx_renderer.rs:1014-1033` | `CreateTargetForHwnd(hwnd, true)` puts the composition visual above the HWND children; sets its content/root and commits. This is the key reason A is plausible. |
| `gpui-pre-windows-0.3.8/src/directx_renderer.rs:1298-1321` | Composition swapchain uses `DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL` and `DXGI_ALPHA_MODE_PREMULTIPLIED`. |
| `gpui-pre-windows-0.3.8/src/directx_renderer.rs:350-357` | Non-opaque backgrounds clear the render target to `[0,0,0,0]`. |
| `gpui-pre-windows-0.3.8/src/window.rs:915-940` | `WindowBackgroundAppearance::Transparent` also requests transparent window composition attributes. |
| `gpui-pre-windows-0.3.8/src/platform.rs:245-247`; `src/directx_renderer.rs:26` | `GPUI_DISABLE_DIRECT_COMPOSITION=1` or `true` selects the non-composition path, invalidating the central assumption of A. |
| `gpui-pre-windows-0.3.8/src/events.rs:99-106, 239-294, 878-943` | Windows size and DPI events resize the renderer and invoke the GPUI resize callback. |
| `gpui-pre-0.3.8/src/app/context.rs:402-420` | Bounds subscription updates the child after GPUI resize notification. |
| `gpui-pre-0.3.8/src/window.rs:2650-2672, 7434-7437` | Frame completion callback/animation requests, and `HasWindowHandle` delegation to the platform window for the raw HWND. |

Prior art read: `desktop/native/mpv-host/src/addon_win.cc`, including the runtime loader, window creation, options, bounds, and shutdown. The spike uses `libloading` with DLL-directory/default-directory search flags, resolves nine C functions, and reuses all the host's initialization options (network tuning remains best effort). FFI layouts and constants were checked against the staged `include/mpv/client.h`. No DLL, import library, or media is tracked. `windows =0.62.2` and `raw-window-handle =0.6.2` match GPUI's resolved dependencies. `png =0.18.1` is already in the lockfile and only encodes the GDI capture; no capture crate was added.

## Commands

Run from `native/` in PowerShell on an interactive Windows desktop:

```powershell
$env:PANORAMA_LIBMPV_DIR = 'D:\codeOS\panorama\.cache\panorama\windows-libmpv\current'
cargo build -p gate1-video --release
$sample = 'spikes/gate1-video/media/bbb_sunflower_1080p_30fps_normal.mp4'
./target/release/gate1-video.exe $sample
```

Space toggles pause; F toggles fullscreen; Esc leaves fullscreen; Left/Right seek -/+10 seconds. The seek bar responds to left clicks. Each control action logs `GPUI input:` so mouse delivery can be distinguished from merely seeing the controls.

```powershell
New-Item -ItemType Directory -Force spikes/gate1-video/evidence | Out-Null
./target/release/gate1-video.exe $sample --screenshot spikes/gate1-video/evidence/approach-a.png --after 15
```

`--after` starts at the first positive playback clock observation, so startup buffering does not consume the delay. Screenshot mode times out if playback does not start within 90 seconds of its requested delay. PNG capture reads the window's screen rectangle from the desktop DC with `BitBlt(SRCCOPY | CAPTUREBLT)` and saves opaque RGBA pixels. It includes whatever is actually on screen, including occluding windows and window chrome. Keep the window visible and unobscured. `PrintWindow`/GPUI-only scene snapshots would not establish child-video composition. Screenshot success alone does not prove the video is present: inspect the resulting PNG. Output directories must already exist; capture errors produce exit code 1.

Every second stdout includes `time-pos`, `duration`, `pause`, `hwdec-current`, physical client size, completed GPUI frame callbacks, and the actual measurement interval. The top-left diagnostic text updates every 500 ms. Continuous animation requests deliberately exercise GPUI; these counts describe GPUI redraws, not mpv swaps, decoded video frames, or measured DWM presentation latency.

Download the test media first if it is absent (ignored):

```powershell
New-Item -ItemType Directory -Force spikes/gate1-video/media | Out-Null
curl.exe -L --fail https://download.blender.org/demo/movies/BBB/bbb_sunflower_1080p_30fps_normal.mp4.zip -o spikes/gate1-video/media/bbb_sunflower_1080p_30fps_normal.mp4.zip
Expand-Archive -LiteralPath spikes/gate1-video/media/bbb_sunflower_1080p_30fps_normal.mp4.zip -DestinationPath spikes/gate1-video/media -Force
```

Required validation from `native/`:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build -p gate1-video --release
```

All four commands passed against the final source. Workspace tests: two panorama-core unit tests passed, no failures; app/spike binaries contain no unit tests. The release binary was exercised with the sample, rather than treating compilation as runtime proof. Neither `native/crates/*` nor any files outside `native/` were modified; the lockfile only adds the new workspace package entry, reusing existing dependency versions.

Startup guards were also exercised: an unset `PANORAMA_LIBMPV_DIR` prints the required directory instruction and exits 1; a missing runtime directory prints the exact missing `libmpv-2.dll` path and exits 1 before opening a GPUI window.

## Product risks

1. Composition correctness depends on GPUI's Windows backend using the alpha composition swapchain and above-child target. A non-composition backend, driver/device recovery, remote desktop, HDR, display changes, or GPUI updates may change the result. Resize/DPI and fullscreen transitions still require observation on real hardware, including mixed-DPI monitors.
2. GPUI and mpv own independent D3D11 devices, swapchains, and presentation clocks. This spike does not synchronize them. GPUI FPS does not establish video smoothness or a one-frame transition limit. Hardware decode must be observed on the target GPU; requesting `d3d11va,auto-safe` can fall back to software. Sustained GPUI redraws also have CPU/power cost.
3. HWND/input/lifecycle coordination is platform-specific. Disabled child input must be verified with actual clicks; close joins mpv on the UI thread and a stuck driver/demuxer could delay shutdown. DLL/header ABI compatibility and transitive DLL packaging remain release requirements.
4. Desktop capture is sensitive to occlusion, minimized/offscreen windows, desktop permissions, protected overlays, and timing. A single image establishes layering at one instant only. It does not measure flicker, frame staleness, focus, Alt-Tab behavior, or exclusive fullscreen.
5. The fixed control widths are intentionally crude and can overflow very narrow windows. File loading is single-source, with no production error/retry UI, subtitle integration, accessibility pass, drag seeking, or cross-platform support. End-file errors are logged and close the spike; keep-open can hold the last frame without an end-file event.
6. The event channel is unbounded and GPUI polls it every 16 ms; a stalled UI can accumulate updates. Commands wait for the event thread's next iteration. The spike also preserves the host's default mpv configuration behavior, so installed user configuration/scripts may influence startup or playback. Production should explicitly own its configuration and event delivery policy.

## Fallback decision

Do not infer that B or C failed from a blocked GUI launch. If A actually fails, implement B next and test ownership, move/resize tracking, Z-order, focus, Alt-Tab, and fullscreen. Only after both fail should C be investigated: mpv's OpenGL render API through ANGLE/WGL would need a compatible context, a render-update callback, a texture-sharing/synchronization bridge into GPUI's D3D11 device, and a GPUI image/scene path to composite that texture. GPUI currently keeps its renderer/device behind platform internals, so this may require upstream hooks. Neither a render-API bridge nor a second-window implementation is claimed here.
