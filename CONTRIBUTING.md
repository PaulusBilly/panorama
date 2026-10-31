# Contributing

Panorama keeps one shared `main` for macOS arm64 and Windows x64. Branch from it with short-lived `ft/` branches. Shared behavior lives in `panorama-core` and `panorama-app`. Platform code stays behind `cfg(target_os)` in the mpv host, the titlebar, and screenshot capture.

## Setup

Install rustup. `rust-toolchain.toml` selects Rust 1.99.0 with rustfmt and Clippy.

**macOS arm64.** Install Xcode Command Line Tools, Python 3.12 or later, and `cmake`, `ninja`, `nasm`, and `pkg-config` from Homebrew. `python3 scripts/build-macos-libmpv.py` verifies the pinned source archives in `libmpv/macos.json`, applies the patches in `libmpv/patches/`, and builds mpv and its codec dependencies under `.cache/panorama/macos-libmpv/`. The first build needs network access and takes a while; later builds reuse the cache. `scripts/stage-macos-libmpv.sh` then stages it in `target/libmpv`.

**Windows x64.** Install the MSVC Build Tools ("Desktop development with C++" and a Windows SDK), PowerShell 7, and 7-Zip. `pwsh scripts/stage-windows-libmpv.ps1` downloads the archive pinned in `libmpv/windows.json`, verifies its SHA-256, and stages it under `.cache/panorama/windows-libmpv/current/`. Do not substitute a different DLL without updating and validating its manifest.

## Required checks

Run from the repository root:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
git diff --check
```

Set `PANORAMA_LIBMPV_DIR` to the staged runtime to include the mpv integration tests. Add a focused regression test for any reproducible logic bug. GitHub Actions runs these checks on Windows x64 and stages libmpv; macOS checks run locally.

## Packaging

```sh
scripts/package-macos.sh                 # target/package/Panorama.app, ad-hoc signed
pwsh scripts/package-windows.ps1         # target/package/panorama-<version>-windows-x64.zip
pwsh scripts/verify-windows-package.ps1
```

Run each on its own OS. The Windows libmpv licence review is still open (`libmpv/windows-licenses/README.md`); do not share a Windows build outside testing until it is settled. Ad-hoc signing is not notarization.

## Packaged validation

Automated checks do not prove real playback. A successful build or fixture run is not proof of 4K, HDR, or codec support.

Changes to shared playback, dependencies, or the build need packaged checks on both OSes. A platform-only change needs packaged checks on the affected OS plus the workspace tests.

Verify launch, sign-in, playback from an HTTP and a torrent source, embedded and addon subtitles, seeking, source switching, fullscreen, resize, minimize and restore, and clean exit. For shared playback changes, also check at least 30 minutes of uninterrupted playback and that the display stays awake.

Note the commit, OS, architecture, GPU and driver, and the outcome with the change. Do not call a platform validated when its hardware, account, or network checks could not be run.

## Releases

Each platform is released separately, from a commit that passed its own packaged validation. If one platform fails, keep its previous release and let the other proceed. Keep the previous artifact for rollback. There is no publishing automation.
