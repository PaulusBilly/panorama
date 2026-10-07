# Contributing

Panorama keeps one shared `main` for macOS arm64 and Windows x64. Branch from it with short-lived `ft/` branches. Shared UI and playback behavior live together; native rendering stays in the platform binding, which is selected once at startup rather than through platform checks spread across components.

For the web app alone, the [README](README.md) quick start is all you need. The rest of this guide covers the desktop app.

## Desktop setup

Use Node 24 and the pnpm version declared in `package.json`, then run `pnpm install --frozen-lockfile`.

**macOS arm64.** Install Xcode Command Line Tools, Python 3.12 or later, CMake, Ninja, and pkg-config. The native build verifies pinned source archives, builds MPV and its codec dependencies under `.cache/panorama/macos-libmpv/`, and downloads the pinned MoltenVK runtime. The first build needs network access and takes a while; later builds reuse the cache. Versions and checksums are in `desktop/native/mpv-host/macos-libmpv.json`.

**Windows x64.** Install Visual Studio Build Tools with "Desktop development with C++" and Python 3.12. The native build downloads and verifies the pinned libmpv archive. Do not substitute a different DLL without updating and validating its manifest.

## Building

```bash
pnpm desktop:make
```

Run it on the target OS. It builds the native host against the installed Electron version, builds the renderer and main process, and produces a ZIP under `out/make/zip/<platform>/<arch>/`. Native resources are staged under `desktop-resources/native/<platform>-<arch>/`.

On Windows, follow with `pnpm desktop:verify:windows` to check the renderer, fonts, native host, DLLs, and notices in the package.

### Bundled streaming server

The package bundles the Stremio streaming server. `pnpm desktop:package:resources` downloads the `server.js` pinned in `desktop/stremio-server/stremio-server.json`, verifies its SHA-256, caches it under `.cache/panorama/stremio-server/`, and stages it with `desktop/stremio-server/NOTICE.md`.

To change the version, update the version, URL, and checksum together, update the notice, and repeat the packaged checks. Never point the manifest at a mutable URL.

Redistribution rights for `server.js` are unresolved (see the notice), and the Windows libmpv license review is still open (`desktop/native/mpv-host/licenses/windows-libmpv/README.md`). Do not share a build outside testing until both are settled.

### Playback spike

`pnpm desktop:spike:start` builds and launches a minimal native playback window for the host OS. Pass a permitted test source through `PANORAMA_SPIKE_MEDIA_URL`. Never commit media URLs or account credentials.

## Required checks

```bash
pnpm lint
pnpm typecheck
pnpm exec tsc -p desktop/tsconfig.json --noEmit
pnpm test
pnpm test:e2e
git diff --check
```

- Run renderer builds one at a time: the shared TypeScript project includes generated types from each output directory.
- Build for validation with `PANORAMA_DIST_DIR=.next-validation` so you never overwrite a running dev server's `.next`.
- `pnpm test:e2e` builds into `.next-playwright` and runs against the fixture runtime.
- Add a focused regression test for any reproducible logic bug.

GitHub Actions runs these checks and packages Windows x64. macOS packaging is a local step and has not been exercised on CI.

## Packaged validation

Automated checks do not prove real playback. A successful build or fixture test is not proof of 4K, HDR, or codec support.

Changes to shared playback, renderer composition, dependencies, or the build need packaged checks on both OSes. A native-only change needs packaged checks on the affected OS plus the shared automated tests.

Before checking a package:

- Quit any running Panorama. Its renderer port, 11475, cannot be shared.
- To exercise the bundled streaming server, also quit Stremio and Stremio Service, since a running service takes precedence. Confirm with `lsof -nP -iTCP:11470-11474 -sTCP:LISTEN`.
- Use `PANORAMA_DESKTOP_USER_DATA_DIR` for an isolated profile. Use a signed-in profile only for checks that need an account.

Then verify launch, playback, embedded and addon subtitles, seeking, source switching, fullscreen, resize, minimize and restore, and clean exit. For shared playback changes, also check at least 30 minutes of uninterrupted playback and that the display stays awake.

Note the commit, OS, architecture, GPU and driver, and the outcome with the change. Do not call a platform validated when its hardware, account, or service checks could not be run.

**macOS signing.** Run `codesign --verify --deep --strict` against the built app and `unzip -t` against its ZIP. If the local package has an invalid signature, ad-hoc sign a separate copy with `codesign --force --deep --sign - <app>` and check that instead. Ad-hoc signing is not notarization.

**Packaged end-to-end tests.** Run Playwright with `PANORAMA_DESKTOP_E2E=1` and `PANORAMA_DESKTOP_EXECUTABLE` pointing at the tested executable. Tests skipped for lack of an account remain manual checks.

**Windows fixture.** `tests/e2e/desktop-windows-fixture-playback.spec.ts` serves a generated silent AVI and subtitle file on loopback port 11474, which must be free. Set `PANORAMA_DESKTOP_SOAK=1` for a 30-minute run with sleep-blocker assertions, and allow 120 seconds for a cold start. It covers native lifecycle and clock behavior, not hardware decoding, audio, real addons, or the streaming service.

## Playback notes

- macOS renders through gpu-next/MoltenVK by default and falls back to OpenGL if that fails to initialize. Force the legacy path with `PANORAMA_MPV_RENDERER=opengl`. Windows uses gpu-next/D3D11.
- Audio defaults to decoded system audio. Receiver passthrough needs explicit consent and an output that reports support for the codec.
- Adaptive buffering is experimental and opt-in through `PANORAMA_ADAPTIVE_BUFFERING=1`.
- Three harnesses cover the playback path: `desktop/scripts/verify-media-proxy.cjs` (network), `desktop/scripts/verify-native-subtitles.cjs` (subtitles), and `desktop/scripts/benchmark-playback.cjs` (startup, seek, and sustained playback). The fixture generator needs FFmpeg on the developer machine only; it is not a production dependency.

## Releases

Each platform is released separately, from a commit that passed its own packaged validation. If one platform fails, keep its previous release and let the other proceed. Keep the previous artifact for rollback. There is no publishing automation.
