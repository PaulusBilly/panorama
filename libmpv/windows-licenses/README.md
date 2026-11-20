# Playback libraries in Panorama for Windows

Panorama plays video through `libmpv-2.dll` and `vulkan-1.dll`, which sit next to
`panorama.exe`. Panorama itself is licensed under the GNU General Public License,
version 3 or later; its licence is in `licenses/LICENSE`.

## libmpv-2.dll

- mpv `v0.41.0-923-g7b8915bc1` with FFmpeg `N-126125-g1d7b14f61`, built by
  [shinchiro/mpv-winbuild-cmake](https://github.com/shinchiro/mpv-winbuild-cmake)
  (release 20260814, recipe commit `cd1edc11dc6887a50f705717619d879f5a93a488`).
- mpv is built with its GPL features and FFmpeg with `--enable-gpl --enable-version3`,
  so this DLL is distributed under the **GNU GPL, version 3**. No non-free FFmpeg
  components are enabled.
- It statically includes many other libraries (x264, x265, libass, dav1d, libplacebo,
  FreeType, HarfBuzz and more). Each one's licence is listed in `THIRD-PARTY.md`, with
  its text in `third-party/`.

## vulkan-1.dll

The Khronos Vulkan loader 1.4.350.0 (MSYS2 build), under the Apache License 2.0. Its
licence is in `licenses/vulkan-loader/LICENSE`. Panorama draws video with Direct3D 11;
the loader is shipped only because libmpv links to it.

## Source code

The exact binaries and the source they were built from are mirrored at
<https://github.com/PaulusBilly/panorama/releases/tag/libmpv-windows-20260814>:

- the libmpv build archive (SHA-256 `0af22b28e920620036d3ae08fd9283156dc9af0420bf4df84b0e02282094599c`)
- mpv at commit `7b8915bc1d04c7e1b61184e00c7fbfaab1911e75`
- FFmpeg at commit `1d7b14f61d66fdf18f15204c613df9d65396c319`
- the build recipe, which names the source of every other library it builds
- the Vulkan loader package (SHA-256 `808115a0818f3e7db74c8facb3f01fcbfac44d58767116f8653770a080b8dbf0`)

Panorama's own source is at <https://github.com/PaulusBilly/panorama>; the commit
of this build is in `BUILD.txt`. If any of these links stop working, open an issue
on that repository and the corresponding source will be provided, for at least
three years after this build was distributed.
