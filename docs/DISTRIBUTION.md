# Distribute SystemMonitor as a signed DMG

Run these commands from the cloned repository root. The default release workflow produces a Developer ID signed and Apple-notarized DMG containing the app and an Applications shortcut. Recipients drag the app into Applications; they do not need Rust, Xcode, a terminal, or a privileged helper.

## One-time setup

You need macOS, Xcode with the `notarytool` and `stapler` tools, Rust 1.99+, Python 3, and an Apple Developer Program account. Install your **Developer ID Application** certificate and its private key in your signing Keychain. An Apple Development, Apple Distribution, or ad hoc identity is not a substitute for Developer ID distribution. The release script never exports your private key.

List your available signing identities:

```sh
security find-identity -v -p codesigning
```

Copy the 40-character SHA-1 fingerprint from a valid **Developer ID Application** entry. A fingerprint identifies a certificate; it is not a password. This Mac has more than one matching Developer ID certificate, so use the fingerprint rather than the potentially ambiguous display name.

Create a Keychain profile for notarization:

```sh
xcrun notarytool store-credentials "SystemMonitor-notary"
```

Follow Apple's interactive prompts for your Apple ID, developer Team ID and an **app-specific password**. You can alternatively configure the profile with an App Store Connect API key using `notarytool store-credentials --help`. Keep passwords and private keys out of scripts, Git, shell command arguments, and release artifacts. The scripts use only the Keychain profile's name.

## Create the release

```sh
# From the repository root
export DEVELOPER_ID="YOUR_40_CHARACTER_DEVELOPER_ID_CERTIFICATE_SHA1"
export NOTARY_PROFILE="SystemMonitor-notary"
./scripts/release-dmg.sh
```

The default builds for your Rust toolchain's host architecture. On the development M4 Mac, that is Apple Silicon (`arm64`). The script prints the resulting absolute path. A version 0.1.0 Apple Silicon release produces:

```text
dist/SystemMonitor-0.1.0-arm64.dmg
dist/SystemMonitor-0.1.0-arm64.dmg.sha256
dist/SystemMonitor-0.1.0-arm64.logs/
```

Set the next version in `Cargo.toml` before a new release; the app's version and DMG filename are derived from it. Existing release images, checksums and log directories are not overwritten. To repeat a test release of the same version, choose another output directory:

```sh
DIST_DIR="dist/retry" ./scripts/release-dmg.sh
```

The workflow runs application and offline release checks, builds an isolated app bundle, and rejects non-system dynamic-library dependencies. It signs the app with Hardened Runtime and a secure timestamp, submits its ZIP to Apple's notary service, checks for `Accepted`, downloads the notary log, and staples/validates the app ticket. It then creates and signs the DMG, notarizes and staples that image, and verifies both the image and the copied-out app with Gatekeeper. The separate app submission ensures the installed app also carries its own offline ticket. No permissions or Hardened Runtime exceptions are added.

The final verification mounts the image read-only, checks its layout, copies the app to a temporary directory, checks its signature/ticket, and ejects the image. It does **not** install or launch the app. The SHA-256 checksum is generated after stapling, covering the actual final image. Review the saved Apple logs for warnings before publishing. Signing and notarization do not replace compatibility testing on recipient hardware.

## Intel and universal builds

For an explicit Apple Silicon build:

```sh
rustup target add aarch64-apple-darwin
./scripts/release-dmg.sh --arch arm64
```

For both Apple Silicon and Intel in a single app:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
./scripts/release-dmg.sh --arch universal
```

For an Intel-only image, use `--arch x86_64` after installing that target. Explicit architectures require `rustup`; missing targets fail before the build instead of silently producing the wrong architecture. `lipo` combines universal executables before the final signature is applied.

**Apple Silicon on macOS 26.6.2 is the tested app configuration.** The universal/Intel build path is provided but has not been compiled or run on Intel hardware here. Optional GPU/power/frequency collectors may be unavailable there. The release build and bundle default to a macOS 13.0 deployment floor, which can be raised with `MACOSX_DEPLOYMENT_TARGET`; this is a build setting, not evidence of testing every older macOS version. Private Apple APIs such as IOReport need testing on each supported OS/chip family.

## Signed-only images

To build a Developer ID signed image without submitting anything to Apple:

```sh
./scripts/release-dmg.sh --signed-only
```

`NOTARY_PROFILE` is not required in this mode. The output includes `-signed-only` in its filename. It still requires a valid Developer ID identity, Hardened Runtime and an online timestamp; it is **not the normal notarized, Gatekeeper-ready download**. The script does not suggest bypassing Gatekeeper.

## Verify and troubleshoot

Verify a complete release independently:

```sh
./scripts/verify-dmg.sh dist/SystemMonitor-0.1.0-arm64.dmg
(cd dist && shasum -a 256 -c SystemMonitor-0.1.0-arm64.dmg.sha256)
```

Use `verify-dmg.sh image.dmg --signed-only` only for the signed-only variant. It checks the Developer ID signatures and image contents while explicitly skipping ticket/Gatekeeper checks. Ad hoc signatures are rejected by the distribution verifier.

If a release fails, no success is reported and the staging directory under `dist/.SystemMonitor-release.*` is retained. Apple submission responses include IDs and statuses, even for many timeout/rejection cases. The default wait is 30 minutes; Apple may still be processing after a timeout. Inspect the existing submission before starting another upload:

```sh
xcrun notarytool info SUBMISSION_ID --keychain-profile "SystemMonitor-notary"
xcrun notarytool wait SUBMISSION_ID --keychain-profile "SystemMonitor-notary" --timeout 30m
xcrun notarytool log SUBMISSION_ID --keychain-profile "SystemMonitor-notary" notary-log.json
```

Set `NOTARY_TIMEOUT=60m` for a longer wait on a subsequent release run. The script does not automatically resume the remainder of a timed-out release; rerunning creates a fresh staged build and new submissions. Resolve rejected signatures or entitlement issues before retrying. An unavailable/locked Keychain or certificate-access prompt needs to be handled locally by the certificate owner. No credentials are embedded in the output.

`scripts/create-dmg.sh APP OUTPUT.dmg` is the low-level packaging helper. It preserves the app signature and creates the Applications link and installation text. It does **not** sign or notarize the resulting DMG; use the full release script for distribution.

## Validation performed

- Offline workflow tests cover accepted app/DMG submissions, JSON rejection and timeout handling, signed-only mode without uploads, ambiguous certificates, absent profiles/architecture targets, external-library inspection failures, paths with spaces, preserved failure logs, and refusal to overwrite releases.
- A real local DMG was created with `hdiutil`, mounted read-only and inspected. The app signature and binary contents were intact; the Applications symlink and installation text were present. The image was ejected after inspection. An ad hoc signed image was correctly rejected by the distribution verifier.
- On October 31, 2026, the real Apple Silicon release `dist/SystemMonitor-0.1.0-arm64-signed-only.dmg` was created using Developer ID Application identity `383C006D3FCCD36F88042300E9CD7C8FFD9FED7E` (team `2XXV2J4DUX`). Both app and image have verified signatures and secure timestamps; the app has Hardened Runtime. Image integrity, read-only mounting, the Applications link, and the copied-out app signature passed verification. The release includes a SHA-256 checksum and validation log.
- An updated release containing the [custom app icon](../assets/README.md) is at `dist/with-icon/SystemMonitor-0.1.0-arm64-signed-only.dmg`. The earlier image remains intact. The updated image passed the same release checks, and its mounted icon resource was compared byte-for-byte with the checked-in icon. Its checksum and validation log are beside it.
- The real signing run exposed an early-closed-pipe error in signature inspection under `pipefail`. The verifier now captures `codesign` output before checking it; a regression test covers verbose signature output. All 28 Rust tests, seven release-workflow tests, and two Xcode-bridge tests passed during this release.
- No Apple upload, notarization, stapling, or cross-Mac installation was performed. No `NOTARY_PROFILE` was configured, and `SystemMonitor-notary` was absent from the Keychain. This signed-only artifact is not the notarized, Gatekeeper-ready download. Configure the Keychain profile above and run the default release workflow for that deliverable. Apple service calls in the offline tests remain simulated.

Run the offline tests with `python3 scripts/test_release.py`; they are also included in `scripts/validate.sh`. `dist/` is ignored by Git.

References: Apple's [notarization requirements](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution) and [custom notarization workflow](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow).
