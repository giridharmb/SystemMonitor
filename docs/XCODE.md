# Build and run in Xcode

Open `SystemMonitor.xcodeproj`, select the shared **SystemMonitor** scheme and **My Mac**, then press **Cmd+R**. **Cmd+B** builds without launching. Rust and Python 3 must already be installed; the project finds the standard rustup installation even when Xcode is opened from Finder.

The generated `.xcodeproj` is included. **XcodeGen is not required to open or build it.** The project was generated with XcodeGen 2.46.0 and validated with Xcode 27.1, using the standard Xcode 16 project format. Older Xcode versions have not been tested.

## How the project works

This is a native macOS application target with a shared scheme. The Rust source, Cargo files, scripts and documentation appear in Xcode's navigator. The build phase runs `scripts/xcode-build.sh`, which delegates compilation and linking to Cargo. Xcode processes the app's Info.plist, signs the app and launches it through LLDB. There is no Swift launcher or duplicate implementation.

The Resources phase copies `assets/AppIcon.icns`; the generated Info.plist references it using `CFBundleIconFile`. The same icon is included by the command-line bundler. See [artwork and regeneration instructions](../assets/README.md).

The build script runs on each build so Cargo can track its own dependency graph and newly added Rust files. Cargo reuses its normal incremental cache under `target/`; Xcode build products live in DerivedData. The script declares its executable and generated Info.plist outputs. Script sandboxing is disabled for this project because Cargo manages dynamic dependencies and its registry/toolchain caches outside Xcode's declared files. The monitoring app itself remains unsandboxed, matching the command-line build's need to inspect system processes and start the OS log watcher.

The app's version is generated from `Cargo.toml`. Its identifier comes from the target's `PRODUCT_BUNDLE_IDENTIFIER` setting, and its minimum macOS version follows `MACOSX_DEPLOYMENT_TARGET` (13.0 by default).

## Configurations and debugging

- **Debug:** Cargo's development build, including debug information, with LLDB enabled and `RUST_BACKTRACE=1`. Use this to inspect stack frames and set Rust source/function breakpoints.
- **Release:** Cargo's existing optimized, stripped release profile, with Hardened Runtime enabled. Use this for performance measurements, Instruments and archives.

Xcode does not provide rust-analyzer completion or Rust-native XCTest integration through this project. The scheme's Test action has no XCTest bundles. Run Rust tests and the full validation workflow from Terminal:

```sh
# From the repository root
cargo test --locked
./scripts/validate.sh
```

Xcode's Analyze action does not run Clippy; `validate.sh` does. Rust's debugger types/pretty-printing can be less complete than Swift or Objective-C support. Release stripping limits symbol-level profiling; use Debug for source-level inspection. To pass launch options such as `--tab cpu`, edit **Product > Scheme > Edit Scheme > Run > Arguments**.

Document version debugging is disabled in the shared Run/Profile scheme. Otherwise Xcode injects `-NSDocumentRevisionsDebugMode YES`, which the Rust command-line parser correctly rejects. The generation hook in `project.yml` preserves this setting when regenerating.

## Command-line builds and archive

```sh
# From the repository root
xcodebuild -project SystemMonitor.xcodeproj -scheme SystemMonitor \
  -configuration Debug -destination 'platform=macOS,name=My Mac' \
  -derivedDataPath target/xcode build

xcodebuild -project SystemMonitor.xcodeproj -scheme SystemMonitor \
  -configuration Release -destination 'platform=macOS,name=My Mac' \
  -derivedDataPath target/xcode build

xcodebuild -project SystemMonitor.xcodeproj -scheme SystemMonitor \
  -destination 'generic/platform=macOS' -derivedDataPath target/xcode \
  -archivePath target/xcode/SystemMonitor.xcarchive archive
```

With these commands, ordinary build products are under `target/xcode/Build/Products/Debug` or `Release`; the archive contains `Products/Applications/SystemMonitor.app`. Product > Archive in Xcode opens its Organizer normally.

The default architecture is the local machine's native architecture. For a universal archive, install both Rust targets and override Xcode's architectures:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
xcodebuild -project SystemMonitor.xcodeproj -scheme SystemMonitor \
  -destination 'generic/platform=macOS' 'ARCHS=arm64 x86_64' \
  -derivedDataPath target/xcode -archivePath target/xcode/SystemMonitor-universal.xcarchive archive
```

Missing cross-compilation targets fail with an installation instruction. Intel/universal compilation and execution have not been validated on hardware here. The app's existing OS/chip compatibility limits still apply.

## Signing and distribution

The project preserves the **automatic Apple Development signing and team selected in Xcode** on this workstation. The same settings are recorded in `project.yml`. Debug builds support debugger attachment; Release enables Hardened Runtime. On a different development Mac, select your own team in Signing & Capabilities, or build locally with ad hoc signing:

```sh
xcodebuild -project SystemMonitor.xcodeproj -scheme SystemMonitor \
  -configuration Debug -destination 'platform=macOS,name=My Mac' \
  -derivedDataPath target/xcode CODE_SIGN_STYLE=Manual CODE_SIGN_IDENTITY=- \
  DEVELOPMENT_TEAM= build
```

For ad hoc signing in the Xcode UI, set Code Signing Style to Manual, Code Signing Identity to Sign to Run Locally, and clear Development Team in the target's Build Settings. Developer ID signing and notarization still need your own certificate and account credentials.

An Apple Development or ad hoc Xcode archive is not a notarized public release. Use the [DMG distribution workflow](DISTRIBUTION.md) to build, Developer ID sign, notarize, staple and verify the distributable image. That workflow remains independent of Xcode's Organizer and does not require project regeneration.

## Maintaining the project

`project.yml` is the reproducible source for the generated project. After changing project settings, adding files you want shown in the navigator, or adding a scheme, regenerate with:

```sh
xcodegen generate --spec project.yml
```

XcodeGen is needed only for regeneration. Direct changes in Xcode can be used normally, but transfer persistent settings changes back to `project.yml` before regenerating. Cargo discovers new Rust modules independently of navigator entries. User-specific Xcode state and DerivedData are ignored by Git; the project and shared scheme are included.
