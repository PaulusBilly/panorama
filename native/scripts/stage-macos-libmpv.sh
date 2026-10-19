#!/bin/sh
# Copies the libmpv runtime built by desktop/scripts/build-macos-libmpv.py into DEST
# (default native/target/libmpv), thinned to arm64 and loading its dependencies from
# its own directory. An optional second argument receives the runtime's licenses.
# Run the app against it with PANORAMA_LIBMPV_DIR=DEST.
set -eu

native=$(cd "$(dirname "$0")/.." && pwd)
repo=$(dirname "$native")
dest=${1:-"$native/target/libmpv"}
# A worktree has no build cache of its own; fall back to the main checkout's.
main=$(dirname "$(git -C "$repo" rev-parse --path-format=absolute --git-common-dir)")
for prefix in "$repo/.cache/panorama/macos-libmpv/current" "$main/.cache/panorama/macos-libmpv/current"; do
  [ -f "$prefix/lib/libmpv.2.dylib" ] && break
done
[ -f "$prefix/lib/libmpv.2.dylib" ] || { echo "Missing libmpv; run python3 desktop/scripts/build-macos-libmpv.py" >&2; exit 1; }

mkdir -p "$dest"
cp -L "$prefix"/lib/*.dylib "$dest/"
rm -f "$dest/libmpv.dylib"
for lib in "$dest"/*.dylib; do
  [ "$(lipo -archs "$lib")" = arm64 ] || lipo "$lib" -thin arm64 -output "$lib"
  install_name_tool -id "@loader_path/$(basename "$lib")" "$lib" 2>/dev/null
  for dep in $(otool -L "$lib" | tail -n +2 | awk '{print $1}'); do
    case "$dep" in /System/* | /usr/lib/* | @loader_path/*) continue ;; esac
    [ -f "$dest/$(basename "$dep")" ] || { echo "Unbundled dependency $dep in $lib" >&2; exit 1; }
    install_name_tool -change "$dep" "@loader_path/$(basename "$dep")" "$lib" 2>/dev/null
  done
  for rpath in $(otool -l "$lib" | awk '/cmd LC_RPATH/ { getline; getline; print $2 }'); do
    case "$rpath" in /*) install_name_tool -delete_rpath "$rpath" "$lib" ;; esac
  done
  codesign --force --sign - "$lib" 2>/dev/null
done
if [ -n "${2:-}" ]; then
  mkdir -p "$2"
  cp -R "$prefix/licenses/." "$2/"
fi
echo "Staged libmpv in $dest"
