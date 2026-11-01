#!/bin/bash
# Xcode owns the app bundle/signature; Cargo owns compiling and linking the Rust executable.
set -euo pipefail
: "${SRCROOT:?Run this script from the SystemMonitor Xcode target.}"
: "${TARGET_BUILD_DIR:?Missing Xcode build environment.}"
: "${EXECUTABLE_PATH:?Missing Xcode executable path.}"
: "${DERIVED_FILE_DIR:?Missing Xcode generated-file directory.}"
cd "$SRCROOT"
export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
command -v cargo >/dev/null || { echo "error: Install Rust with rustup before building this target." >&2; exit 1; }
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$SRCROOT/target}"
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-13.0}"
case "${CONFIGURATION:-Debug}" in
    Debug) profile=debug; cargo_options=() ;;
    Release) profile=release; cargo_options=(--release) ;;
    *) echo "error: Unsupported Xcode configuration: $CONFIGURATION" >&2; exit 1 ;;
esac
host=$(rustc -vV | sed -n 's/^host: //p')
architectures=${ARCHS:-$(uname -m)}
binaries=()
for architecture in $architectures; do
    case "$architecture" in
        arm64) target=aarch64-apple-darwin ;;
        x86_64) target=x86_64-apple-darwin ;;
        *) echo "error: Unsupported Mac architecture: $architecture" >&2; exit 1 ;;
    esac
    # Bash 3.2 treats an empty array as unset with -u; the + expansion supports Apple's shell.
    if [[ "$target" == "$host" ]]; then
        cargo build --locked ${cargo_options[@]+"${cargo_options[@]}"}
        binaries+=("$CARGO_TARGET_DIR/$profile/SystemMonitor")
    else
        rustup target list --installed | grep -qx "$target" || {
            echo "error: Install the requested architecture with: rustup target add $target" >&2; exit 1;
        }
        cargo build --locked ${cargo_options[@]+"${cargo_options[@]}"} --target "$target"
        binaries+=("$CARGO_TARGET_DIR/$target/$profile/SystemMonitor")
    fi
done
destination="$TARGET_BUILD_DIR/$EXECUTABLE_PATH"
mkdir -p "$(dirname "$destination")" "$DERIVED_FILE_DIR"
[[ ${#binaries[@]} -gt 0 ]] || { echo "error: Xcode supplied no architectures." >&2; exit 1; }
staged=$(mktemp "$(dirname "$destination")/.SystemMonitor.XXXXXX")
metadata=""
trap 'rm -f "$staged" "$metadata"' EXIT
if [[ ${#binaries[@]} == 1 ]]; then
    cp "${binaries[0]}" "$staged"
else
    lipo -create "${binaries[@]}" -output "$staged"
fi
chmod +x "$staged"
# Replacing the inode avoids modifying executable pages of a still-running previous build.
mv -f "$staged" "$destination"
metadata=$(mktemp "$DERIVED_FILE_DIR/cargo-metadata.XXXXXX")
cargo metadata --locked --no-deps --format-version 1 > "$metadata"
python3 - "$metadata" "$DERIVED_FILE_DIR/SystemMonitor-Info.plist" <<'PY'
import json, os, plistlib, sys
with open(sys.argv[1]) as f:
    version = next(p['version'] for p in json.load(f)['packages'] if p['name'] == 'SystemMonitor')
info = {
    'CFBundleName': 'SystemMonitor',
    'CFBundleDisplayName': 'System Monitor',
    'CFBundleIdentifier': os.environ.get('PRODUCT_BUNDLE_IDENTIFIER', 'dev.systemmonitor.desktop'),
    'CFBundleExecutable': os.environ.get('EXECUTABLE_NAME', 'SystemMonitor'),
    'CFBundleIconFile': 'AppIcon',
    'CFBundlePackageType': 'APPL',
    'CFBundleShortVersionString': version,
    'CFBundleVersion': version.split('-')[0].split('+')[0],
    'LSMinimumSystemVersion': os.environ['MACOSX_DEPLOYMENT_TARGET'],
    'NSHighResolutionCapable': True,
    'NSSupportsAutomaticGraphicsSwitching': True,
}
with open(sys.argv[2], 'wb') as f:
    plistlib.dump(info, f)
PY
echo "Built Cargo $profile executable for $architectures; Xcode will sign the app."
