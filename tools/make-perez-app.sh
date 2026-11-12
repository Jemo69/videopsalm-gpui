#!/bin/bash
# make-perez-app.sh — bundle Perez RS as a macOS Perez.app + optional DMG.
# Clean-room script, no proprietary bits. Requires a macOS host with Xcode
# command-line tools; fails fast elsewhere (CI runs it on macos-latest).
#
# Interface matches .github/workflows/build.yml (positional args):
#   ./tools/make-perez-app.sh <binary> <app-out> [dmg-out]
# Example:
#   ./tools/make-perez-app.sh target/release/perez-rs dist/Perez.app dist/Perez.dmg
#
# Icon: place a 1024px master at assets/perez-1024.png and convert once with
#   sips -s format icns assets/perez-1024.png --out assets/perez.icns
# (see assets/icons-dist/ for smaller PNG masters rasterized from
# assets/perez.svg). Without assets/perez.icns the bundle ships icon-less.
# Code signing / notarization are intentionally NOT wired — the unsigned
# bundle runs via right-click Open; add signing identities when ready.
set -euo pipefail

if [[ "$(uname)" != "Darwin" ]]; then
  echo "make-perez-app.sh runs on macOS only (found $(uname))." >&2
  exit 1
fi

if [[ $# -lt 2 ]]; then
  echo "Usage: $0 <binary> <app-out> [dmg-out]" >&2
  exit 1
fi

BIN="$1"
APP_OUT="$2"
DMG_OUT="${3:-}"
VERSION="${PEREZ_VERSION:-0.1.27}"
# Rust app closure lives in gpui-app/ (own directory): icons and masters
# resolve under it, while <binary>/<app-out> stay caller-relative.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../gpui-app" && pwd)"

if [[ ! -x "$BIN" ]]; then
  echo "Missing release binary: $BIN" >&2
  echo "Build first: cargo build --release -p perez-rs" >&2
  exit 1
fi

case "$APP_OUT" in
  /*) : ;;
  *) APP_OUT="$PWD/$APP_OUT" ;;
esac

rm -rf "$APP_OUT"
mkdir -p "$APP_OUT/Contents/MacOS" "$APP_OUT/Contents/Resources"

cp "$BIN" "$APP_OUT/Contents/MacOS/Perez RS"
if [[ -f "$ROOT/assets/perez.icns" ]]; then
  cp "$ROOT/assets/perez.icns" "$APP_OUT/Contents/Resources/perez.icns"
  ICON_KEY="  <key>CFBundleIconFile</key>
  <string>perez</string>"
else
  echo "NOTE: assets/perez.icns not found — bundling without dock icon." >&2
  ICON_KEY=""
fi

cat > "$APP_OUT/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>Perez RS</string>
  <key>CFBundleDisplayName</key>
  <string>Perez RS</string>
  <key>CFBundleIdentifier</key>
  <string>org.perez.perez-rs</string>
  <key>CFBundleVersion</key>
  <string>${VERSION}</string>
  <key>CFBundleShortVersionString</key>
  <string>${VERSION}</string>
  <key>CFBundleExecutable</key>
  <string>Perez RS</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>LSMinimumSystemVersion</key>
  <string>13.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
${ICON_KEY}
</dict>
</plist>
PLIST

echo "Built $APP_OUT (version $VERSION)."

if [[ -n "$DMG_OUT" ]]; then
  case "$DMG_OUT" in
    /*) : ;;
    *) DMG_OUT="$PWD/$DMG_OUT" ;;
  esac
  rm -f "$DMG_OUT"
  hdiutil create -volname "Perez RS" -srcfolder "$APP_OUT" -ov -format UDZO "$DMG_OUT"
  echo "DMG: $DMG_OUT"
fi
