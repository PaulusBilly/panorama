# Playback libraries in Panorama for macOS

Panorama plays video through `libmpv.2.dylib` and the libraries next to it in
`Contents/Frameworks`. Panorama itself is licensed under the GNU General Public
License, version 3 or later; its licence is in `LICENSE` in this folder.

- `libmpv.2.dylib` is built by `scripts/build-macos-libmpv.py` from mpv 0.41.0 and
  FFmpeg 8.0 (FFmpeg without `--enable-gpl`), plus libass, libplacebo, glslang,
  FreeType, HarfBuzz, FriBidi and dav1d, all linked in statically. mpv is built
  with its GPL features, so the library is distributed under the **GNU GPL,
  version 2 or later**.
- MoltenVK is shipped as a separate library under the Apache License 2.0.
- Each library's own licence files are in `libmpv/`, one folder per library.

## Source code

Every source archive, with its download URL and SHA-256, is listed in
`libmpv/macos-libmpv.json`. Panorama's patches to them are the `.patch` files in
`libmpv/`. The build script and Panorama's own source are at
<https://github.com/PaulusBilly/panorama>. If any of those links stop working,
open an issue on that repository and the corresponding source will be provided,
for at least three years after this build was distributed.
