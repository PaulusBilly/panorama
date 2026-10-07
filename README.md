# Panorama

An unofficial Stremio client for browsing and watching films, with a calm, editorial layout. It runs in the browser and as a desktop app for macOS (Apple Silicon) and Windows (x64).

Panorama is not affiliated with Stremio. It does not host or provide any content: films come from the addons already installed on your own Stremio account.

## What it does

- **Browse.** A full-screen featured film, then TMDB Popular Movies in a landscape grid.
- **Search.** Films and people, with shareable result pages at `/search/films` and `/search/people`.
- **Sign in with Stremio.** Email and password go straight from your browser to Stremio. Panorama has no server-side login and never stores your password.
- **Watch.** Open a film at `/films/[tmdbId]`, and Panorama prepares the first playable source from your addons. Switch sources from the player.
- **Audio and subtitles.** Pick audio tracks, choose embedded or addon subtitles, and adjust how subtitles look.
- **Resume.** Progress syncs through your Stremio account, so you can continue or start over.
- **Desktop playback.** The desktop app plays through native MPV instead of the browser's video element, which covers far more codecs.
- **Discord activity (desktop, optional).** Show what you're watching. Off by default.

Not included: series and episodes, an addon manager, or a custom backend.

## Quick start

You need:

- Node.js 22 or newer (`.nvmrc`) and pnpm 11 or newer
- A [TMDB API key](https://www.themoviedb.org/settings/api) for the catalog and search
- [Stremio Service](https://www.stremio.com/) running locally for playback in the browser (default `http://127.0.0.1:11470`)

```bash
pnpm install
cp .env.example .env.local   # then set NEXT_PUBLIC_TMDB_API_KEY
pnpm dev
```

Open <http://127.0.0.1:3000>.

Browsing works without signing in and without Stremio Service. Sources need a signed-in account with addons; playback needs the service.

To look around without an account, a TMDB key, or the service, run the app on fixture data:

```bash
pnpm dev:preview
```

## Desktop app

The desktop app wraps the same interface in Electron and plays video through a native MPV surface. It bundles its own streaming server and only starts it when no Stremio Service is already running.

```bash
pnpm desktop:make
```

Run this on the OS you are building for. The result is a ZIP under `out/make/zip/<platform>/<arch>/`. The first build downloads and compiles native dependencies, so it needs network access and takes a while.

Build prerequisites, validation steps, and release rules are in [CONTRIBUTING.md](CONTRIBUTING.md). There are no published releases yet.

## Scripts

| Command | What it does |
| --- | --- |
| `pnpm dev` | Development server |
| `pnpm dev:preview` | Development server on fixture data, already signed in |
| `pnpm build` | Production build |
| `pnpm start` | Serve the production build |
| `pnpm lint` | ESLint |
| `pnpm typecheck` | Next.js type generation, then `tsc --noEmit` |
| `pnpm test` | Unit and component tests (Vitest) |
| `pnpm test:e2e` | End-to-end tests (Playwright) |
| `pnpm desktop:make` | Build and package the desktop app |

Development and builds use webpack (`--webpack`) because the Stremio core worker ships WASM that the webpack config handles as an asset.

## Configuration

| Variable | Purpose |
| --- | --- |
| `NEXT_PUBLIC_TMDB_API_KEY` | TMDB v3 key for the catalog, search, and film details. Bundled into the client, so it is visible in the browser. |
| `NEXT_PUBLIC_PANORAMA_RUNTIME` | Unset for the live Stremio runtime. `fake` for test fixtures. `preview` for fixtures that start signed in. |
| `PANORAMA_DIST_DIR` | Build output directory. Use `.next-validation` so a build does not overwrite a running dev server's `.next`. |

## How it works

```text
Next.js pages (App Router)
        |
client runtime boundary
        |
StremioRuntime interface
        |-- @stremio/stremio-core-web worker    account, addons, sources, progress
        |-- TMDB                                catalog, search, film details
        |-- Stremio Service (loopback only)     streaming
        `-- @stremio/stremio-video              browser: HTML video
                                                desktop: native MPV
```

- Components talk to the `StremioRuntime` interface and never import Stremio packages directly.
- TMDB provides what you browse. Stremio provides who you are and what you can play.
- Catalog, account, addons, and the streaming service fail independently, so the catalog stays usable when the others are down.
- There is no database and no backend. The session lives in your browser's localStorage until you log out.

## Project layout

```text
app/            Routes: home, search, film details, watch
components/     Interface components
runtime/        StremioRuntime, the Stremio core adapter, TMDB, subtitles, fake runtime
desktop/        Electron main process, preload, native MPV host, build scripts
tests/          Vitest unit and component tests, Playwright end-to-end tests
public/         Logo, icons, and fonts
patches/        pnpm patches for dependencies
```

## Testing

```bash
pnpm lint && pnpm typecheck && pnpm test && pnpm test:e2e
```

End-to-end tests build into `.next-playwright` and run against the fake runtime in Chromium, Firefox, and WebKit, so they need no account and do not disturb a running dev server.

Passing tests do not prove real playback. A real account, installed addons, live TMDB, the streaming service, codecs, and subtitles are checked by hand; see [CONTRIBUTING.md](CONTRIBUTING.md#packaged-validation).

In the browser, playback depends on what the browser and OS can decode. H.264 with AAC is the baseline; other codecs may fail, and should fail into a recoverable player state. The desktop app's MPV playback is the answer for everything else.

## Security and privacy

- Your Stremio password is sent to Stremio only, and is not stored. The session key stays in localStorage until you log out.
- Only loopback streaming-service addresses (`127.0.0.1`, `localhost`, `[::1]`) are accepted, and the address is never read from the URL.
- Addon metadata, URLs, and stream fields are treated as untrusted and never rendered as HTML.
- Source URLs, info hashes, and raw stream objects stay inside the runtime; the interface only sees opaque IDs and display fields.
- The desktop app's bundled streaming server binds to `127.0.0.1`, keeps its data under Panorama's own user-data directory, and stops when the app quits.

## Discord activity

In the desktop app's playback settings, **Share watching activity on Discord** is off by default. When it is on and the Discord desktop client is running, your activity shows "Watching {film title}" with artwork and playback timing. Pausing or buffering stops the timer; stopping playback, turning the setting off, or quitting clears it. No token or extra service is needed, and it never blocks playback. Browser builds do not share activity.

## Fonts

The interface and desktop subtitles use bundled DM Sans at weights 400, 500, and 700. Font files are distributed under the SIL Open Font License 1.1; the copyright notice and license are in [public/fonts/OFL.txt](public/fonts/OFL.txt).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for desktop setup, required checks, and packaged validation.
