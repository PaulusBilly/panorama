#!/bin/sh
# Builds panorama in release mode and wraps it in target/package/Panorama.app.
# A bare binary opens Terminal and has no icon; the bundle fixes both.
set -eu

repo=$(cd "$(dirname "$0")/.." && pwd)
icon="$repo/assets/panorama.icns"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$repo/crates/panorama-app/Cargo.toml" | head -n 1)
commit=$(git -C "$repo" rev-parse --short HEAD)

[ -f "$icon" ] || { echo "Missing icon: $icon" >&2; exit 1; }

(cd "$repo" && cargo build -p panorama-app --release --locked)

app="$repo/target/package/Panorama.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$repo/target/release/panorama" "$app/Contents/MacOS/panorama"
cp "$icon" "$app/Contents/Resources/panorama.icns"

licenses="$app/Contents/Resources/licenses"
"$repo/scripts/stage-macos-libmpv.sh" "$app/Contents/Frameworks" "$licenses/libmpv"
cp "$repo/LICENSE" "$licenses/LICENSE"
cp "$repo/libmpv/macos-SOURCE.md" "$licenses/SOURCE.md"
cp "$repo/crates/panorama-app/assets/fonts/OFL.txt" "$licenses/OFL.txt"
cp "$repo/crates/panorama-app/assets/icons/LICENSE" "$licenses/tabler-icons-LICENSE"

cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Panorama</string>
  <key>CFBundleDisplayName</key><string>Panorama</string>
  <key>CFBundleIdentifier</key><string>dev.pbilly.panorama.native</string>
  <key>CFBundleExecutable</key><string>panorama</string>
  <key>CFBundleIconFile</key><string>panorama</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$commit</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.entertainment</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>CFBundleURLTypes</key>
  <array>
    <dict>
      <key>CFBundleURLName</key><string>Panorama watch party</string>
      <key>CFBundleURLSchemes</key>
      <array><string>panorama</string></array>
    </dict>
  </array>
</dict>
</plist>
EOF

# ponytail: ad-hoc signature only, enough to run locally; distribution needs a Developer ID and notarization.
codesign --force --deep --sign - "$app"
codesign --verify --deep --strict "$app"
plutil -lint -s "$app/Contents/Info.plist"

echo "Built $app"
