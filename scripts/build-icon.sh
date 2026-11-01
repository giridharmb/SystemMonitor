#!/bin/bash
# Rebuild the checked-in macOS icon using only Apple's image tools.
set -euo pipefail
cd "$(dirname "$0")/.."
work=$(mktemp -d "${TMPDIR:-/tmp}/SystemMonitor-icon.XXXXXX")
trap 'rm -rf "$work"' EXIT
mkdir "$work/AppIcon.iconset"
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" assets/AppIcon.png \
        --out "$work/AppIcon.iconset/icon_${size}x${size}.png" >/dev/null
    retina=$((size * 2))
    sips -z "$retina" "$retina" assets/AppIcon.png \
        --out "$work/AppIcon.iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$work/AppIcon.iconset" -o "$work/AppIcon.icns"
iconutil -c iconset "$work/AppIcon.icns" -o "$work/check.iconset"
for size in 16 32 128 256 512; do
    test -s "$work/check.iconset/icon_${size}x${size}.png"
    test -s "$work/check.iconset/icon_${size}x${size}@2x.png"
done
cp "$work/AppIcon.icns" assets/AppIcon.icns
echo "Built assets/AppIcon.icns (16–1024 pixels, including Retina representations)."
