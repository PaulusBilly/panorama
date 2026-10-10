# Vendored crates

## stremio-watched-bitfield

Copied unmodified from https://github.com/Stremio/stremio-core at revision
`474ffaa42e0b7b9bcba0dd99368c41e828579dec` (MIT, see `LICENSE.md` beside it), except for one line in
`Cargo.toml`: `flate2 = "1.0.*"` became `flate2 = "1"`.

Upstream's requirement only allows flate2 1.0.x. GPUI needs flate2 1.1 or newer through `usvg`, and
Cargo will not select two 1.x versions of one crate, so `stremio-core` and GPUI cannot share a
workspace without this. The workspace `[patch]` section in `native/Cargo.toml` points
`stremio-core`'s dependency here. Remove this directory once upstream relaxes the requirement.
