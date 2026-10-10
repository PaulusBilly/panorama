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

## Packaging (Windows)

Run from the repository root with PowerShell 7 (packaging runs locally; there is no CI workflow):

```powershell
pwsh native/scripts/package-windows.ps1
pwsh native/scripts/verify-windows-package.ps1
```

`package-windows.ps1` builds `panorama-app` in release mode with `--locked`, reuses the staged libmpv from `.cache/panorama/windows-libmpv/` (running `desktop/scripts/stage-windows-libmpv.mjs` only if it is missing), and writes `native/target/package/panorama-<version>-windows-x64.zip` plus a `.sha256` file.

The zip contains `panorama.exe`, `libmpv-2.dll`, `BUILD.txt` (git commit, cargo version, libmpv manifest sha256) and `licenses/` (libmpv notices, plus `OFL.txt`, the Tabler icons licence and the repository `LICENSE` once those files exist in the repository; the script warns when one is absent).

`verify-windows-package.ps1` checks the exact member list and paths, that both binaries are x64 PE images, the libmpv DLL hash against the stage and the zip hash against the `.sha256` file. It then runs `panorama.exe --screenshot home <png>` from an extracted copy; this step is skipped with a note while the binary does not support `--screenshot`.
