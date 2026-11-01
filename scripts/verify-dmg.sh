#!/bin/bash
# Verify the exact deliverable and a copied-out app, without installing or launching it.
set -euo pipefail
if [[ $# -lt 1 || $# -gt 2 || ( $# == 2 && "$2" != --signed-only ) ]]; then
    echo "Usage: $0 file.dmg [--signed-only]" >&2
    exit 2
fi
[[ -f "$1" ]] || { echo "DMG not found: $1" >&2; exit 2; }
dmg=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
notarized=true
[[ "${2:-}" != --signed-only ]] || notarized=false
codesign --verify --strict --verbose=2 "$dmg"
signature=$(codesign -d --verbose=4 "$dmg" 2>&1)
# Capture first: grep -q can close a pipe early and make codesign fail under pipefail.
grep -q '^Authority=Developer ID Application:' <<< "$signature" || {
    echo "DMG requires a Developer ID Application signature." >&2; exit 1;
}
hdiutil verify "$dmg"
if $notarized; then
    xcrun stapler validate "$dmg"
    spctl --assess --type open --context context:primary-signature --verbose=2 "$dmg"
fi
work=$(mktemp -d "${TMPDIR:-/tmp}/SystemMonitor-verify.XXXXXX")
mounted=false
cleanup() {
    if $mounted && ! hdiutil detach "$work/mount"; then
        echo "Could not eject $work/mount; verification files retained at $work" >&2
        return
    fi
    rm -rf "$work"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
mkdir "$work/mount"
hdiutil attach -readonly -nobrowse -mountpoint "$work/mount" "$dmg" > "$work/attach.log"
mounted=true
[[ -d "$work/mount/SystemMonitor.app" && -L "$work/mount/Applications" ]] || {
    echo "DMG is missing the app or Applications link." >&2; exit 1;
}
[[ "$(readlink "$work/mount/Applications")" == /Applications ]] || {
    echo "Invalid Applications link." >&2; exit 1;
}
ditto "$work/mount/SystemMonitor.app" "$work/SystemMonitor.app"
codesign --verify --deep --strict --verbose=2 "$work/SystemMonitor.app"
signature=$(codesign -d --verbose=4 "$work/SystemMonitor.app" 2>&1)
grep -q '^Authority=Developer ID Application:' <<< "$signature"
grep -Eq '^CodeDirectory .*flags=.*runtime' <<< "$signature"
grep -q '^Timestamp=' <<< "$signature"
if $notarized; then
    xcrun stapler validate "$work/SystemMonitor.app"
    spctl --assess --type execute --verbose=2 "$work/SystemMonitor.app"
fi
printf 'Verified %s\n' "$dmg"
$notarized || echo "Signed-only: notarization and Gatekeeper acceptance were not checked."
