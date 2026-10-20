# Panorama

An unofficial Stremio client for browsing and watching films, with a calm, editorial layout. It is a native desktop app for macOS (Apple Silicon) and Windows (x64), written in Rust with GPUI, and plays video through mpv.

Panorama is not affiliated with Stremio. It does not host or provide any content: films come from the addons already installed on your own Stremio account.

## What it does

- **Browse.** A featured film, then a grid from your addons' catalogs, with a switcher that remembers your choice. Signed out, it uses Cinemeta.
- **Search.** Films across your search-capable addons.
- **Sign in with Stremio.** Your password goes to Stremio only and is never stored.
- **Watch.** Film details and playable sources from every installed stream addon. Torrent sources play through a built-in engine, so no Stremio Service is needed.
- **Audio and subtitles.** Pick audio tracks, choose embedded or addon subtitles, and adjust how subtitles look.
- **Resume.** Progress syncs through your Stremio account.
- **Addons.** List, install by URL, configure, reorder, and remove addons, and choose the metadata source.
- **Discord activity (optional).** Shows what you're watching. Off by default.

Not included: series and episodes, or an addon discovery catalog.

## Quick start

Install [rustup](https://rustup.rs); the workspace selects its own toolchain. Then:

```sh
cd native
cargo run -p panorama-app
```

Playback needs a libmpv runtime. On macOS, build it once and point the app at it:

```sh
python3 native/scripts/build-macos-libmpv.py
native/scripts/stage-macos-libmpv.sh
cd native && PANORAMA_LIBMPV_DIR=$PWD/target/libmpv cargo run -p panorama-app
```

On Windows, `pwsh native/scripts/stage-windows-libmpv.ps1` downloads and verifies the pinned build (it needs 7-Zip).

To look around without an account, run on fixture data: `cargo run -p panorama-app -- --fixtures`.

## Project layout

```text
native/crates/panorama-app    GPUI interface: routes, theme, motion, playback UI
native/crates/panorama-core   UI-free logic: Stremio core, addons, store, images, media proxy, torrents, Discord
native/crates/panorama-mpv    libmpv host, loaded at runtime
native/libmpv                 Pinned libmpv sources, patches, and licence notices
native/scripts                libmpv build and stage, macOS and Windows packaging
native/spikes                 Phase 0 feasibility spikes
```

Module-level detail, dependency rationale, and test coverage are in [native/README.md](native/README.md).

## Security and privacy

- Your Stremio password is sent to Stremio only. The session lives in Panorama's local SQLite store until you sign out.
- Addon metadata, URLs, and stream fields are treated as untrusted, length-capped, and never rendered as markup.
- Source URLs and loopback session tokens are never logged. The media proxy and torrent engine bind to `127.0.0.1` and require a per-session token.

## Fonts

The interface uses bundled Aspekta at weights 400, 500, and 700, under the SIL Open Font License 1.1 ([licence](native/crates/panorama-app/assets/fonts/OFL.txt)).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).
