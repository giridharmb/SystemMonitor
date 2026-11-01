#!/bin/bash
# Build, Developer ID sign, notarize, staple, verify and publish a local DMG.
set -euo pipefail
cd "$(dirname "$0")/.."
usage() {
    cat <<'TXT'
Usage: ./scripts/release-dmg.sh [--arch native|arm64|x86_64|universal] [--signed-only]

Required environment:
  DEVELOPER_ID     SHA-1 from `security find-identity -v -p codesigning`
  NOTARY_PROFILE  `notarytool store-credentials` Keychain profile (full release)

Optional environment: DIST_DIR (default dist), CARGO_TARGET_DIR,
MACOSX_DEPLOYMENT_TARGET (default 13.0), NOTARY_TIMEOUT (default 30m).

Default: notarize both the app and DMG, staple both, and check Gatekeeper.
--signed-only produces a clearly named signed-only DMG without uploading to Apple.
Architecture native means this Rust toolchain's host; universal includes arm64/x86_64.
TXT
}
fail() { echo "Error: $*" >&2; exit 1; }
arch=native
notarize=true
while [[ $# -gt 0 ]]; do
    case "$1" in
        --arch) [[ $# -ge 2 ]] || { usage >&2; exit 2; }; arch=$2; shift 2 ;;
        --signed-only) notarize=false; shift ;;
        --help|-h) usage; exit 0 ;;
        *) usage >&2; exit 2 ;;
    esac
done
case "$arch" in native|arm64|x86_64|universal) ;; *) fail "Unsupported architecture: $arch" ;; esac
[[ $(uname -s) == Darwin ]] || fail "Run this script on macOS."
for tool in cargo rustc python3 security codesign ditto hdiutil lipo otool shasum; do
    command -v "$tool" >/dev/null || fail "Missing tool: $tool"
done
[[ -n "${DEVELOPER_ID:-}" ]] || fail "Set DEVELOPER_ID to your Developer ID Application certificate SHA-1."
if $notarize; then
    [[ -n "${NOTARY_PROFILE:-}" ]] || fail "Set NOTARY_PROFILE, or explicitly use --signed-only."
    xcrun --find notarytool >/dev/null
    xcrun --find stapler >/dev/null
fi
# Resolve exactly one valid Developer ID certificate; never silently select a team/certificate.
identity=$(security find-identity -v -p codesigning | python3 -c '
import re, sys
requested = sys.argv[1]
matches = re.findall(r"^\s*\d+\)\s+([A-Fa-f0-9]{40})\s+\"(Developer ID Application:[^\"]+)\"", sys.stdin.read(), re.M)
matches = [(sha, name) for sha, name in matches if sha.upper() == requested.upper() or name == requested]
if len(matches) != 1:
    sys.exit("DEVELOPER_ID must match exactly one valid Developer ID Application certificate. Use its SHA-1 hash.")
print(matches[0][0])
' "$DEVELOPER_ID")
targets=()
case "$arch" in
    arm64) targets=(aarch64-apple-darwin) ;;
    x86_64) targets=(x86_64-apple-darwin) ;;
    universal) targets=(aarch64-apple-darwin x86_64-apple-darwin) ;;
esac
if [[ ${#targets[@]} -gt 0 ]]; then
    command -v rustup >/dev/null || fail "rustup is required for explicit architecture builds."
    for target in "${targets[@]}"; do
        rustup target list --installed | grep -qx "$target" || fail "Install the target first: rustup target add $target"
    done
fi
mkdir -p "${DIST_DIR:-dist}"
dist=$(cd "${DIST_DIR:-dist}" && pwd)
work=$(mktemp -d "$dist/.SystemMonitor-release.XXXXXX")
finished=false
cleanup() {
    if $finished; then rm -rf "$work";
    else printf 'Release incomplete. Logs and staging files retained: %s\n' "$work" >&2; fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-13.0}"
echo "Checking application… (details: $work/validation.log)"
if ! ./scripts/validate.sh > "$work/validation.log" 2>&1; then
    tail -n 60 "$work/validation.log" >&2
    fail "Application validation failed."
fi
app="$work/SystemMonitor.app"
BUNDLE_OUTPUT="$app" ./scripts/bundle.sh
build_dir="${CARGO_TARGET_DIR:-target}"
if [[ ${#targets[@]} -gt 0 ]]; then
    binaries=()
    for target in "${targets[@]}"; do
        cargo build --release --locked --target "$target"
        binaries+=("$build_dir/$target/release/SystemMonitor")
    done
    # Copy away from an existing signature before replacing the native executable.
    rm "$app/Contents/MacOS/SystemMonitor"
    if [[ ${#binaries[@]} == 1 ]]; then cp "${binaries[0]}" "$app/Contents/MacOS/SystemMonitor";
    else lipo -create "${binaries[@]}" -output "$app/Contents/MacOS/SystemMonitor"; fi
fi
binary="$app/Contents/MacOS/SystemMonitor"
architectures=$(lipo -archs "$binary")
case "$architectures" in
    arm64|x86_64) label=$architectures ;;
    'arm64 x86_64'|'x86_64 arm64') label=universal ;;
    *) fail "Unexpected executable architectures: $architectures" ;;
esac
[[ "$arch" == native || "$arch" == "$label" ]] || fail "Requested $arch but built $label."
# A local/Homebrew dylib would make the download fail on another Mac.
otool -L "$binary" > "$work/dependencies.txt"
while IFS= read -r dependency; do
    case "$dependency" in /usr/lib/*|/System/Library/*) ;; *) fail "Non-system dependency must be bundled and signed first: $dependency" ;; esac
done < <(awk '/^[[:space:]]/ {print $1}' "$work/dependencies.txt")
version=$(python3 -c 'import plistlib,sys; print(plistlib.load(open(sys.argv[1], "rb"))["CFBundleShortVersionString"])' "$app/Contents/Info.plist")
name="SystemMonitor-$version-$label"
$notarize || name="$name-signed-only"
[[ ! -e "$dist/$name.dmg" && ! -e "$dist/$name.dmg.sha256" && ! -e "$dist/$name.logs" ]] || fail "Release already exists: $dist/$name. Bump Cargo.toml version or choose a new DIST_DIR."

notarize_file() {
    local artifact=$1 stage=$2 result="$work/$2-submission.json" status id rc=0
    echo "Submitting $stage to Apple; waiting up to ${NOTARY_TIMEOUT:-30m}…"
    xcrun notarytool submit "$artifact" --keychain-profile "$NOTARY_PROFILE" \
        --wait --timeout "${NOTARY_TIMEOUT:-30m}" --output-format json > "$result" || rc=$?
    id=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("id", ""))' "$result" 2>/dev/null || true)
    status=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("status", ""))' "$result" 2>/dev/null || true)
    if [[ -n "$id" ]]; then
        echo "Submission ID ($stage): $id"
        xcrun notarytool log "$id" --keychain-profile "$NOTARY_PROFILE" "$work/$stage-notary-log.json" || echo "Notary log is not available yet; use the submission ID to retrieve it later." >&2
    fi
    [[ $rc == 0 && "$status" == Accepted && -n "$id" ]] || fail "$stage notarization did not finish as Accepted (status: ${status:-unknown}, exit: $rc). See $result."
}

codesign --force --sign "$identity" --options runtime --timestamp "$app"
codesign --verify --deep --strict --verbose=2 "$app"
if $notarize; then
    ditto -c -k --keepParent "$app" "$work/SystemMonitor.zip"
    notarize_file "$work/SystemMonitor.zip" app
    xcrun stapler staple "$app"
    xcrun stapler validate "$app"
    spctl --assess --type execute --verbose=2 "$app"
fi
./scripts/create-dmg.sh "$app" "$work/$name.dmg"
codesign --force --sign "$identity" --timestamp "$work/$name.dmg"
if $notarize; then
    notarize_file "$work/$name.dmg" dmg
    xcrun stapler staple "$work/$name.dmg"
    ./scripts/verify-dmg.sh "$work/$name.dmg"
else
    ./scripts/verify-dmg.sh "$work/$name.dmg" --signed-only
fi
# Publish only after every check passes; checksum covers the final, stapled image.
mkdir "$dist/$name.logs"
cp "$work/validation.log" "$dist/$name.logs/"
for log in "$work"/*submission.json "$work"/*notary-log.json; do
    [[ ! -f "$log" ]] || cp "$log" "$dist/$name.logs/"
done
ln "$work/$name.dmg" "$dist/$name.dmg"
(cd "$dist" && shasum -a 256 "$name.dmg" > "$name.dmg.sha256")
finished=true
printf '\nCreated: %s\nSHA-256: %s\n' "$dist/$name.dmg" "$dist/$name.dmg.sha256"
if $notarize; then echo "App and DMG signed, notarized, stapled and verified. Test on another Mac before publishing.";
else echo "SIGNED ONLY: no notarization was requested. This is not the Gatekeeper-ready release."; fi
