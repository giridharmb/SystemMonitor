#!/bin/sh
# Create a locally signed, double-clickable macOS app. No installation or elevation.
set -eu
cd "$(dirname "$0")/.."
cargo build --release --locked
BUILD_DIR="${CARGO_TARGET_DIR:-target}/release"
APP="${BUNDLE_OUTPUT:-$BUILD_DIR/SystemMonitor.app}"
VERSION=$(cargo metadata --locked --no-deps --format-version 1 | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "SystemMonitor"))')
mkdir -p "$APP/Contents/MacOS"
mkdir -p "$APP/Contents/Resources"
cp assets/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
cp "$BUILD_DIR/SystemMonitor" "$APP/Contents/MacOS/SystemMonitor"
cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>SystemMonitor</string>
<key>CFBundleDisplayName</key><string>SystemMonitor</string>
<key>CFBundleIdentifier</key><string>dev.systemmonitor.desktop</string>
<key>CFBundleExecutable</key><string>SystemMonitor</string>
<key>CFBundleIconFile</key><string>AppIcon</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>CFBundleVersion</key><string>1</string>
<key>NSHighResolutionCapable</key><true/>
<key>LSMinimumSystemVersion</key><string>13.0</string>
<key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict></plist>
PLIST
python3 - "$APP/Contents/Info.plist" "$VERSION" "${MACOSX_DEPLOYMENT_TARGET:-13.0}" <<'PY'
import plistlib, sys
path, version, minimum = sys.argv[1:]
with open(path, 'rb') as f:
    info = plistlib.load(f)
info['CFBundleShortVersionString'] = version
info['CFBundleVersion'] = version.split('-')[0].split('+')[0]
info['LSMinimumSystemVersion'] = minimum
with open(path, 'wb') as f:
    plistlib.dump(info, f)
PY
plutil -lint "$APP/Contents/Info.plist"
codesign --force --sign - "$APP"
codesign --verify --strict "$APP"
printf '%s\n' "Built $APP"
