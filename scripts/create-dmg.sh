#!/bin/bash
# Package an already signed app. This helper does not sign or notarize the DMG.
set -euo pipefail
if [[ $# != 2 ]]; then
    echo "Usage: $0 /path/SystemMonitor.app /path/output.dmg" >&2
    exit 2
fi
[[ -d "$1/Contents/MacOS" ]] || { echo "App bundle not found: $1" >&2; exit 2; }
[[ "$2" == *.dmg ]] || { echo "Output must end in .dmg" >&2; exit 2; }
[[ ! -e "$2" ]] || { echo "Refusing to overwrite: $2" >&2; exit 2; }
app=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
mkdir -p "$(dirname "$2")"
output=$(cd "$(dirname "$2")" && pwd)/$(basename "$2")
codesign --verify --deep --strict "$app"
work=$(mktemp -d "$(dirname "$output")/.dmg.XXXXXX")
trap 'rm -rf "$work"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
mkdir "$work/content"
ditto "$app" "$work/content/SystemMonitor.app"
ln -s /Applications "$work/content/Applications"
cat > "$work/content/Install.txt" <<'TXT'
SystemMonitor

Drag SystemMonitor.app onto Applications, then eject this disk image.
Open SystemMonitor from Applications. No Rust installation is needed.

To update, quit the running app before replacing it in Applications.
To uninstall, quit the app and move it from Applications to Trash.

Hardware support depends on the architecture named in the download.
GPU, power and frequency metrics depend on the Mac and macOS version.
TXT
hdiutil create -volname "SystemMonitor" -srcfolder "$work/content" \
    -fs HFS+ -format UDZO -imagekey zlib-level=9 "$work/image.dmg"
hdiutil verify "$work/image.dmg"
# Both paths are on the same filesystem; linking publishes atomically without overwriting a release.
ln "$work/image.dmg" "$output"
printf 'Packaged (DMG not yet signed/notarized): %s\n' "$output"
