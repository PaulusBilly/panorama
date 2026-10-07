# Stremio streaming server

`server.js` in this directory is the Stremio streaming server, version 4.21.2, published by Stremio (Smart Code OOD) at the URL pinned in `desktop/stremio-server/stremio-server.json`. It is downloaded unmodified at build time and verified against the pinned SHA-256; it is byte-identical to the copy shipped inside Stremio 5.1.28 for macOS.

Panorama starts it as a child process when no Stremio Service is already running, and stops it on quit.

## Licence status

The server is not part of Panorama's source and is not covered by Panorama's licence. Stremio publishes no licence for `server.js` itself: the `Stremio/stremio-service` wrapper is GPL-2.0 and `Stremio/stremio-shell` is GPL-3.0, but neither repository contains or licenses the server bundle, and no `server.js.LICENSE.txt` is published beside the download. Permission to redistribute it has therefore not been established (checked 2026-10-06). Resolve this before sharing a build that contains it.
