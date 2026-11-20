# Third-party notices for libmpv-2.dll

The `libmpv-2.dll` shipped with Panorama comes from shinchiro/mpv-winbuild-cmake release 20260814 (mpv v0.41.0-923-g7b8915bc1, FFmpeg N-126125-g1d7b14f61); this index lists every component in that build's dependency closure (`mpv` and `ffmpeg` DEPENDS, followed recursively, recipe commit cd1edc11dc68) and the licence files collected for it in `third-party/`.

| Component | Licence (SPDX id) | Upstream source URL | Version/ref used for the licence text | Licence file(s) |
| --- | --- | --- | --- | --- |
| mpv (libmpv) | GPL-2.0-or-later (LGPL-2.1-or-later only if built with -Dgpl=false; this build is GPL) | https://github.com/mpv-player/mpv | commit 7b8915bc1d04 (v0.41.0-923-g7b8915bc1) | `mpv-Copyright`, `mpv-LICENSE.GPL`, `mpv-LICENSE.LGPL` |
| FFmpeg (libavcodec/format/filter/...) | GPL-3.0-or-later (built --enable-gpl --enable-version3 per LICENSE.md) | https://github.com/FFmpeg/FFmpeg | commit 1d7b14f61d66 (N-126125-g1d7b14f61) | `ffmpeg-COPYING.GPLv2`, `ffmpeg-COPYING.GPLv3`, `ffmpeg-COPYING.LGPLv2.1`, `ffmpeg-COPYING.LGPLv3`, `ffmpeg-LICENSE.md` |
| AMF headers | MIT | https://github.com/GPUOpen-LibrariesAndSDKs/AMF | default branch HEAD 57ee0cfb0e1a (recipe unpinned) | `amf-headers-LICENSE.txt` |
| ANGLE headers | BSD-3-Clause | https://github.com/google/angle | default branch HEAD 76729448eb7d (recipe unpinned) | `angle-headers-LICENSE` |
| aom (libaom) | BSD-2-Clause; AOM patent grant in PATENTS (LicenseRef, not an SPDX id) | https://aomedia.googlesource.com/aom | branch main @ d03d2c63a132 (recipe GIT_TAG main) | `aom-LICENSE`, `aom-PATENTS` |
| AviSynth+ headers | GPL-2.0-or-later with AviSynth linking exception (license.rst) | https://github.com/AviSynth/AviSynthPlus | default branch HEAD 5da089a5889a (recipe unpinned) | `avisynth-headers-gpl.txt`, `avisynth-headers-license.rst` |
| brotli | MIT | https://github.com/google/brotli | default branch HEAD fed075f48eaf (recipe unpinned) | `brotli-LICENSE` |
| bzip2 | bzip2-1.0.6 style BSD licence (text in file) | https://gitlab.com/bzip2/bzip2 | default branch HEAD 66c46b8c9436 (recipe unpinned) | `bzip2-COPYING` |
| c-ares | MIT (LICENSE.md; LICENSES/ holds per-file texts: APSL-2.0, Autoconf-exception-3.0, BSD-3-Clause, FSFAP, GPL-3.0-or-later, LGPL-2.1-or-later, MIT, Unicode-3.0) | https://github.com/c-ares/c-ares | default branch HEAD 46266f4ae2d3 (recipe unpinned) | `c-ares-LICENSE.md`, `c-ares-LICENSES_APSL-2.0.txt`, `c-ares-LICENSES_Autoconf-exception-3.0.txt`, `c-ares-LICENSES_BSD-3-Clause.txt`, `c-ares-LICENSES_FSFAP.txt`, `c-ares-LICENSES_GPL-3.0-or-later.txt`, `c-ares-LICENSES_LGPL-2.1-or-later.txt`, `c-ares-LICENSES_MIT.txt`, `c-ares-LICENSES_Unicode-3.0.txt` |
| curl (libcurl) | curl (COPYING); also BSD-4-Clause-UC, ISC texts shipped in LICENSES/ | https://github.com/curl/curl | default branch HEAD 1becd936ad27 (recipe unpinned) | `curl-COPYING`, `curl-LICENSES_BSD-4-Clause-UC.txt`, `curl-LICENSES_ISC.txt`, `curl-LICENSES_curl.txt` |
| dav1d | BSD-2-Clause | https://code.videolan.org/videolan/dav1d | default branch HEAD 2a0775ca5ea0 (recipe unpinned) | `dav1d-COPYING` |
| davs2 | GPL-2.0* (text only) | https://github.com/saindriches/davs2 | default branch HEAD f50435051b72 (recipe unpinned) | `davs2-COPYING` |
| fast_float | Apache-2.0 OR BSL-1.0 OR MIT (three files) | https://github.com/fastfloat/fast_float | default branch HEAD 8ec59686a143 (recipe unpinned) | `fast_float-LICENSE-APACHE`, `fast_float-LICENSE-BOOST`, `fast_float-LICENSE-MIT` |
| Fontconfig | HPND-style permissive (no SPDX id stated in file) | https://gitlab.freedesktop.org/fontconfig/fontconfig | default branch HEAD 3e9035e505c6 (recipe unpinned) | `fontconfig-COPYING` |
| FreeType | FTL OR GPL-2.0* (LICENSE.TXT offers a choice; FTL.TXT fetched from docs/) | https://github.com/freetype/freetype | default branch HEAD 4fc22c56fca8 (recipe unpinned) | `freetype2-FTL.TXT`, `freetype2-LICENSE.TXT` |
| FriBidi | LGPL-2.1* (text only) | https://github.com/fribidi/fribidi | default branch HEAD 15393e4937aa (recipe unpinned) | `fribidi-COPYING` |
| glad | MIT AND Apache-2.0 (per sections of LICENSE) | https://github.com/Dav1dde/glad | default branch HEAD 0a7cf1af9e25 (recipe unpinned) | `glad-LICENSE` |
| glslang | BSD-3-Clause (LICENSE.txt) plus files under BSD-2-Clause, MIT, MIT-Khronos-old, Apache-2.0, GPL-3.0-or-later with Bison-exception-2.2, AML-glslang (LICENSES/) | https://github.com/KhronosGroup/glslang | default branch HEAD 5d525ee305af (recipe unpinned) | `glslang-LICENSE.txt`, `glslang-LICENSES_AML-glslang.txt`, `glslang-LICENSES_Apache-2.0.txt`, `glslang-LICENSES_BSD-2-Clause.txt`, `glslang-LICENSES_BSD-3-Clause.txt`, `glslang-LICENSES_Bison-exception-2.2.txt`, `glslang-LICENSES_GPL-3.0-or-later.txt`, `glslang-LICENSES_MIT-Khronos-old.txt`, `glslang-LICENSES_MIT.txt` |
| graphengine | WTFPL | https://github.com/sekrit-twc/graphengine | default branch HEAD 91c6af4c795c (recipe unpinned) | `graphengine-COPYING` |
| HarfBuzz | "Old MIT" (no SPDX id stated in file) | https://github.com/harfbuzz/harfbuzz | default branch HEAD 94715f6033f9 (recipe unpinned) | `harfbuzz-COPYING` |
| highway | Apache-2.0 OR BSD-3-Clause | https://github.com/google/highway | default branch HEAD a31b7f1fd029 (recipe unpinned) | `highway-LICENSE` |
| LAME (libmp3lame, shinchiro fork) | LGPL-2.0* (text is GNU Library GPL v2; LICENSE is an LGPL FAQ note) | https://gitlab.com/shinchiro/lame | default branch HEAD 9a5b35c430a2 (recipe unpinned) | `lame-COPYING`, `lame-LICENSE` |
| Little CMS 2 (lcms2) | MIT | https://github.com/mm2/Little-CMS | default branch HEAD 914ab50e951d (recipe unpinned) | `lcms2-LICENSE` |
| libarchive | BSD-2-Clause (COPYING summarises per-file exceptions incl. public domain) | https://github.com/libarchive/libarchive | default branch HEAD 432ab464725a (recipe unpinned) | `libarchive-COPYING` |
| libaribcaption | MIT | https://github.com/xqq/libaribcaption | default branch HEAD 0f2c41063bdf (recipe unpinned) | `libaribcaption-LICENSE` |
| libass | ISC | https://github.com/libass/libass | default branch HEAD a6613d2554da (recipe unpinned) | `libass-COPYING` |
| libbluray | LGPL-2.1* | https://code.videolan.org/videolan/libbluray | default branch HEAD 4613535451af (recipe unpinned) | `libbluray-COPYING` |
| libbs2b | MIT | https://github.com/alexmarsev/libbs2b | default branch HEAD 5ca2d59888df (recipe unpinned) | `libbs2b-COPYING` |
| libdvdcss | GPL-2.0* | https://code.videolan.org/videolan/libdvdcss | default branch HEAD 811ce97a1d4d (recipe unpinned) | `libdvdcss-COPYING` |
| libdvdnav | GPL-2.0* | https://code.videolan.org/videolan/libdvdnav | default branch HEAD 8147ccd35e5a (recipe unpinned) | `libdvdnav-COPYING` |
| libdvdread | GPL-2.0* | https://code.videolan.org/videolan/libdvdread | default branch HEAD 0f30df53125b (recipe unpinned) | `libdvdread-COPYING` |
| libiconv | LGPL-2.1* (COPYING.LIB, library); COPYING is GPL-3.0 (programs) | https://ftp.gnu.org/pub/gnu/libiconv/libiconv-1.18.tar.gz | libiconv-1.18 tarball (recipe URL, SHA256 verified) | `libiconv-COPYING`, `libiconv-COPYING.LIB` |
| libjpeg-turbo | IJG AND BSD-3-Clause AND Zlib (per LICENSE.md) | https://github.com/libjpeg-turbo/libjpeg-turbo | default branch HEAD ab2cb1a97868 (recipe unpinned) | `libjpeg-LICENSE.md`, `libjpeg-README.ijg` |
| libjxl | BSD-3-Clause; patent grant in PATENTS | https://github.com/libjxl/libjxl | default branch HEAD 12e4bf4087bc (recipe unpinned) | `libjxl-LICENSE`, `libjxl-PATENTS` |
| libmodplug | Public domain (statement in COPYING; no SPDX id) | https://github.com/Konstanty/libmodplug | default branch HEAD a52647673746 (recipe unpinned) | `libmodplug-COPYING` |
| libmysofa | BSD-3-Clause (numbered-clause variant) | https://github.com/hoene/libmysofa | default branch HEAD 648eed03472e (recipe unpinned) | `libmysofa-LICENSE` |
| libopenmpt | BSD-3-Clause (LICENSE); src/mpt also BSL-1.0 | https://lib.openmpt.org/files/libopenmpt/src/libopenmpt-0.7.12+release.autotools.tar.gz | 0.7.12 tarball (recipe URL, SHA256 verified) | `libopenmpt-LICENSE`, `libopenmpt-LICENSE.BSD-3-Clause.txt`, `libopenmpt-LICENSE.BSL-1.0.txt` |
| libplacebo | LGPL-2.1* | https://github.com/haasn/libplacebo | default branch HEAD 89b04663135b (recipe unpinned) | `libplacebo-LICENSE` |
| libpng | libpng-2.0 (LICENSE.md); LICENSES/ adds MIT, MIT-0 | https://github.com/glennrp/libpng | default branch HEAD 5408ee7a2e56 (recipe unpinned) | `libpng-LICENSE.md`, `libpng-LICENSES_MIT-0.txt`, `libpng-LICENSES_MIT.txt` |
| libpsl | MIT | https://github.com/rockdaboot/libpsl | default branch HEAD 88331dae18f1 (recipe unpinned) | `libpsl-LICENSE` |
| libsamplerate | BSD-2-Clause | https://github.com/libsndfile/libsamplerate | default branch HEAD 0844c208f683 (recipe unpinned) | `libsamplerate-COPYING` |
| SDL2 | Zlib | https://github.com/libsdl-org/SDL | default branch HEAD 803ca329e4cc (recipe unpinned) | `libsdl2-LICENSE.txt` |
| libsixel | MIT (LICENSE); per-component notes in LICENSE.* (images, mesa, pngsuite, pnmcolormap, sdump, sixel, stb) | https://github.com/saitoha/libsixel | default branch HEAD 72ed7d6b4a22 (recipe unpinned) | `libsixel-LICENSE`, `libsixel-LICENSE.images`, `libsixel-LICENSE.mesa`, `libsixel-LICENSE.pngsuite`, `libsixel-LICENSE.pnmcolormap`, `libsixel-LICENSE.sdump`, `libsixel-LICENSE.sixel`, `libsixel-LICENSE.stb` |
| SoX Resampler (libsoxr, shinchiro fork) | LGPL-2.1-or-later | https://gitlab.com/shinchiro/soxr | default branch HEAD 945b592b7047 (recipe unpinned) | `libsoxr-COPYING.LGPL`, `libsoxr-LICENCE` |
| SRT (libsrt) | MPL-2.0 | https://github.com/Haivision/srt | default branch HEAD 98ac9998f9ba (recipe unpinned) | `libsrt-LICENSE` |
| libssh | LGPL-2.1* | https://gitlab.com/libssh/libssh-mirror | default branch HEAD ae59cf0785d3 (recipe unpinned) | `libssh-COPYING` |
| libudfread | LGPL-2.1* | https://code.videolan.org/videolan/libudfread | default branch HEAD b0bc6957e7e0 (recipe unpinned) | `libudfread-COPYING` |
| libunibreak | Zlib | https://github.com/adah1972/libunibreak | default branch HEAD ff24f284a567 (recipe unpinned) | `libunibreak-LICENCE` |
| libva | MIT | https://github.com/intel/libva | default branch HEAD f052e984e0c8 (recipe unpinned) | `libva-COPYING` |
| oneVPL (libvpl) | MIT | https://github.com/intel/libvpl | default branch HEAD ec718e8ede85 (recipe unpinned) | `libvpl-LICENSE` |
| libvpx | BSD-3-Clause; patent grant in PATENTS | https://chromium.googlesource.com/webm/libvpx | branch main @ 18656c27cf3b (recipe GIT_TAG main) | `libvpx-LICENSE`, `libvpx-PATENTS` |
| libwebp | BSD-3-Clause; patent grant in PATENTS | https://chromium.googlesource.com/webm/libwebp | branch main @ ef1ef5a8d2a9 (recipe GIT_TAG main) | `libwebp-COPYING`, `libwebp-PATENTS` |
| libxml2 | MIT | https://github.com/GNOME/libxml2 | default branch HEAD 21c8223ac091 (recipe unpinned) | `libxml2-Copyright` |
| zimg (libzimg) | WTFPL | https://github.com/sekrit-twc/zimg | default branch HEAD 67e0603271c0 (recipe unpinned) | `libzimg-COPYING` |
| zvbi (libzvbi) | Mixed per COPYING.md: GPL-2.0-or-later (default), LGPL-2.0-or-later (src/), BSD-2-Clause (examples), MIT, others | https://github.com/zapping-vbi/zvbi | default branch HEAD 34bb6f660c5c (recipe unpinned) | `libzvbi-COPYING.md` |
| LuaJIT (openresty luajit2) | MIT | https://github.com/openresty/luajit2 | default branch HEAD 2769af8852a4 (recipe unpinned) | `luajit-COPYRIGHT` |
| LZO | GPL-2.0* | https://www.oberhumer.com/opensource/lzo/download/lzo-2.10.tar.gz | 2.10 tarball (recipe URL, SHA1 verified) | `lzo-COPYING` |
| MuJS | ISC | https://codeberg.org/ccxvii/mujs | default branch HEAD e9090f1829b3 (recipe unpinned) | `mujs-COPYING` |
| nghttp2 | MIT | https://github.com/nghttp2/nghttp2 | default branch HEAD 7262774042b8 (recipe unpinned) | `nghttp2-COPYING` |
| nghttp3 | MIT | https://github.com/ngtcp2/nghttp3 | default branch HEAD e06a63ac6a02 (recipe unpinned) | `nghttp3-COPYING` |
| ngtcp2 | MIT | https://github.com/ngtcp2/ngtcp2 | default branch HEAD 9595a0dda799 (recipe unpinned) | `ngtcp2-COPYING` |
| nv-codec-headers (ffnvcodec) | MIT (header comment) | https://github.com/FFmpeg/nv-codec-headers | default branch HEAD f2053071ea93 (recipe unpinned); no licence file, MIT notice is the header comment | `nvcodec-headers-dynlink_cuda.h` |
| libogg | BSD-3-Clause | https://github.com/xiph/ogg | default branch HEAD 06a5e0262cdc (recipe unpinned) | `ogg-COPYING` |
| OpenAL Soft | LGPL-2.0* (COPYING is GNU Library GPL v2); also Apache-2.0, BSD-3-Clause, pffft notice | https://github.com/kcat/openal-soft | default branch HEAD 2a6c1a6a7fad (recipe unpinned) | `openal-soft-COPYING`, `openal-soft-LICENSE-pffft`, `openal-soft-LICENSE.Apache-2.0.txt`, `openal-soft-LICENSE.BSD-3-Clause.txt` |
| OpenSSL | Apache-2.0 | https://github.com/openssl/openssl | default branch HEAD 3b8973cfc45d (recipe unpinned) | `openssl-LICENSE.txt` |
| Opus (libopus) | BSD-3-Clause (COPYING); royalty-free patent statements referenced | https://github.com/xiph/opus | default branch HEAD 31b4b3628439 (recipe unpinned) | `opus-COPYING`, `opus-LICENSE_PLEASE_READ.txt` |
| Opus DNN model data | No licence file in tarball; same project as Opus, see opus-COPYING | https://media.xiph.org/opus/models/opus_data-8a07d57c4fce6fb30f23b3e0d264004e04f1d7b421f5392ef61543d021a439af.tar.gz | model-data tarball (recipe URL, SHA256 verified) | none (not fetched: no licence file in tarball; see Opus row) |
| Rubber Band | GPL-2.0* (text only; upstream also sells commercial licence) | https://github.com/breakfastquay/rubberband | default branch HEAD e4296ac80b11 (recipe unpinned) | `rubberband-COPYING` |
| shaderc | Apache-2.0 | https://github.com/google/shaderc | default branch HEAD 05814952134d (recipe unpinned) | `shaderc-LICENSE` |
| Speex | BSD-3-Clause | https://github.com/xiph/speex | default branch HEAD 09b569d1b437 (recipe unpinned) | `speex-COPYING` |
| SPIRV-Cross | Apache-2.0 (LICENSE); LICENSES/ adds MIT, CC-BY-4.0, LicenseRef-KhronosFreeUse | https://github.com/KhronosGroup/SPIRV-Cross | default branch HEAD 57e4efcc8411 (recipe unpinned) | `spirv-cross-LICENSE`, `spirv-cross-LICENSES_Apache-2.0.txt`, `spirv-cross-LICENSES_CC-BY-4.0.txt`, `spirv-cross-LICENSES_LicenseRef-KhronosFreeUse.txt`, `spirv-cross-LICENSES_MIT.txt` |
| SPIRV-Headers | MIT (LICENSE: Khronos MIT, plus jsoncpp public-domain notice); LICENSES/ adds MIT, CC-BY-4.0 | https://github.com/KhronosGroup/SPIRV-Headers | default branch HEAD 50184103b190 (recipe unpinned) | `spirv-headers-LICENSE`, `spirv-headers-LICENSES_CC-BY-4.0.txt`, `spirv-headers-LICENSES_MIT.txt` |
| SPIRV-Tools | Apache-2.0 | https://github.com/KhronosGroup/SPIRV-Tools | default branch HEAD 3c8fd3529731 (recipe unpinned) | `spirv-tools-LICENSE` |
| subrandr | MPL-2.0 | https://github.com/afishhh/subrandr | default branch HEAD af0803fb5d11 (recipe unpinned) | `subrandr-LICENSE` |
| SVT-AV1 | BSD-3-Clause-Clear (LICENSE.md) and BSD-2-Clause (LICENSE-BSD2.md); AOM patent grant in PATENTS.md | https://gitlab.com/AOMediaCodec/SVT-AV1 | default branch HEAD e3d63405f9ba (recipe unpinned) | `svtav1-LICENSE-BSD2.md`, `svtav1-LICENSE.md`, `svtav1-PATENTS.md` |
| uavs3d | BSD-3-Clause (custom organisation names) | https://github.com/uavs3/uavs3d | default branch HEAD 0e20d2c29185 (recipe unpinned) | `uavs3d-COPYING` |
| uchardet | MPL-1.1 OR GPL-2.0* OR LGPL-2.1* (COPYING contains all three texts) | https://gitlab.freedesktop.org/uchardet/uchardet | default branch HEAD 06029ec3340c (recipe unpinned) | `uchardet-COPYING` |
| VapourSynth (headers/import lib) | LGPL-2.1* | https://github.com/vapoursynth/vapoursynth | default branch HEAD 3b2c049876f6 (recipe unpinned) | `vapoursynth-COPYING.LESSER` |
| libvorbis | BSD-3-Clause | https://github.com/xiph/vorbis | default branch HEAD 1b75110b5a27 (recipe unpinned) | `vorbis-COPYING` |
| Vulkan-Loader | Apache-2.0 (LICENSE.txt); LICENSES/ adds MIT, MIT-Khronos-old, CC-BY-4.0, HPND-Kevlin-Henney | https://github.com/KhronosGroup/Vulkan-Loader | default branch HEAD bdeb6dc40fe4 (recipe unpinned) | `vulkan-LICENSE.txt`, `vulkan-LICENSES_Apache-2.0.txt`, `vulkan-LICENSES_CC-BY-4.0.txt`, `vulkan-LICENSES_HPND-Kevlin-Henney.txt`, `vulkan-LICENSES_MIT-Khronos-old.txt`, `vulkan-LICENSES_MIT.txt` |
| Vulkan-Headers | Apache-2.0 OR MIT | https://github.com/KhronosGroup/Vulkan-Headers | default branch HEAD d277a0a3cda9 (recipe unpinned) | `vulkan-header-LICENSE.md`, `vulkan-header-LICENSES_Apache-2.0.txt`, `vulkan-header-LICENSES_MIT.txt` |
| x264 | GPL-2.0* (text only; upstream also sells commercial licence) | https://code.videolan.org/videolan/x264 | default branch HEAD 0480cb05fa18 (recipe unpinned) | `x264-COPYING` |
| x265 | GPL-2.0* (COPYING also states a commercial licence is available) | https://github.com/Multicorewareinc/x265 | default branch HEAD fab244873135 (recipe unpinned) | `x265-COPYING` |
| xxHash | BSD-2-Clause (library) | https://github.com/Cyan4973/xxHash | default branch HEAD 2ec67caeb18e (recipe unpinned) | `xxhash-LICENSE` |
| XZ Utils (liblzma) | 0BSD (liblzma, per COPYING); other texts shipped: GPL-2.0, GPL-3.0, LGPL-2.1 | https://github.com/tukaani-project/xz | default branch HEAD f54febc54060 (recipe unpinned) | `xz-COPYING`, `xz-COPYING.0BSD`, `xz-COPYING.GPLv2`, `xz-COPYING.GPLv3`, `xz-COPYING.LGPLv2.1` |
| zlib-ng | Zlib | https://github.com/zlib-ng/zlib-ng | default branch HEAD c6bb771d3476 (recipe unpinned) | `zlib-LICENSE.md` |
| zstd | BSD-3-Clause OR GPL-2.0* (LICENSE / COPYING) | https://github.com/facebook/zstd | default branch HEAD 1658a42d787d (recipe unpinned) | `zstd-COPYING`, `zstd-LICENSE` |
| GCC runtime (libgcc, libstdc++) | GPL-3.0-or-later WITH GCC-exception-3.1 (COPYING.RUNTIME) | https://github.com/gcc-mirror/gcc | master @ b55cf3fda452 (toolchain version not known) | `gcc-runtime-COPYING.RUNTIME`, `gcc-runtime-COPYING3` |
| winpthreads | MIT AND BSD-3-Clause (per COPYING) | https://github.com/mingw-w64/mingw-w64 | master @ 7095c702af78 (mingw-w64-libraries/winpthreads) | `winpthreads-COPYING` |
| mingw-w64 CRT | ZPL-2.1 and other permissive parts; COPYING.MinGW-w64-runtime.txt is the binary-distribution notice | https://github.com/mingw-w64/mingw-w64 | master @ 7095c702af78 (repo root) | `mingw-w64-crt-COPYING`, `mingw-w64-crt-COPYING.MinGW-w64-runtime.txt`, `mingw-w64-crt-COPYING.MinGW-w64.txt`, `mingw-w64-crt-DISCLAIMER.PD` |

## Notes

- `*` after a GPL/LGPL id means only the licence text was shipped in the file fetched, so whether "or later" applies is not stated there (it is set per source file upstream).
- The release tag is gone and almost every recipe is unpinned (no GIT_TAG, or a branch name). The refs above are the upstream default-branch heads at collection time (2026-11-21), not the commits actually built; licence texts rarely change but may differ from the build in copyright years or SPDX layout. Tarball recipes (libiconv, libopenmpt, lzo, opus-dnn) are pinned by URL and hash.
- Components assumed linked for an x86_64 build: x265, uavs3d, davs2, libva and the GCC/mingw-w64 runtime items (the task brief, not the DLL itself, was the source for the runtime pieces). If the DLL was built with the clang toolchain, libc++/libunwind (Apache-2.0 WITH LLVM-exception) replace the GCC runtime row, and it was not verified which toolchain was used.
- Vendored third-party code inside a package is covered only where the package's top-level licence files list it. subrandr is a Rust project: its crate dependencies are not enumerated here.
- No non-free or non-redistributable licence was found. The resulting binary is GPL-3.0-or-later as a whole (FFmpeg built with --enable-gpl --enable-version3, plus GPL-only components such as x264, x265, davs2, libdvdread, libdvdnav, Rubber Band).
