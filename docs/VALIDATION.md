# Validation record

Validated locally on **October 31, 2026**, on a **Mac16,7 / Apple M4 Pro**, macOS **26.6.2**, Rust **1.99.1**, with **14 logical CPU cores (4 efficiency, 10 performance), 20 GPU cores, and 48 GiB RAM**. This was a busy personal workstation with roughly 2,400–2,550 processes and memory pressure warning, rather than a clean benchmark host.

## Checks performed

| Check | Result |
| --- | --- |
| `cargo fmt --check` | Passed |
| `cargo clippy --all-targets --locked -- -D warnings` | Passed for application code |
| `cargo test --locked` | **28 passed**, including live Mach/VM/IOKit/IOReport checks |
| `python3 scripts/test_release.py` | **7 passed**, covering the offline release workflow, failure cases, and verbose signature inspection under pipefail |
| `python3 scripts/test_xcode.py` | **2 passed**, covering Debug/Release packaging, atomic executable replacement, plist metadata, and invalid configuration/missing architecture failures |
| Xcode 27.1 native arm64 target | Debug and Release builds passed; shared scheme launched the GUI under LLDB; Rust function breakpoint resolved to `src/main.rs` |
| Xcode archive | Passed; application registered in archive metadata, strict signature verification passed, Release Hardened Runtime confirmed; signed with the selected Apple Development identity |
| Xcode local signing and regeneration | Ad hoc build/signature verification passed without a team; isolated XcodeGen regeneration preserved Run/Profile launch settings and the selected development team |
| Local DMG packaging | Created, mounted read-only, app signature/binary and Applications link verified, then ejected; ad hoc distribution signature rejected |
| Developer ID signed DMG | Real arm64 signed-only release created; app/image signatures, timestamps, Hardened Runtime, mounted contents and SHA-256 verified. Not notarized; no notary profile configured |
| Custom app icon | Transparent 1024px master; all ten macOS ICNS representations round-tripped. Xcode Debug build and signed DMG under `dist/with-icon/` contain the exact icon and correct plist metadata; small-size preview inspected and signatures/checksum verified |
| `cargo build --release --locked` | Passed |
| App bundle plist validation, ad hoc signing and strict signature verification | Passed |
| Six invalid CLI cases: unknown/missing tab, unknown flag, and invalid diagnostic durations | Exit code 2, with useful errors |
| 20 headless NDJSON samples | All parsed; 14 valid core percentages, sane memory sizes, available GPU and CPU power |
| All nine tabs | Opened and inspected through the native UI |
| App filtering and grouped/per-process switching | Verified; matching table shrank from hundreds of rows to the two running monitor processes |
| Table sorting | Column sort control exercised; comparator and stable tie order covered by unit tests |
| Logs | Live OS events observed; source/search filters, pause/resume label, and selected message detail inspected |
| Log lifecycle after optimization | No child on Overview; exactly one child after opening Logs; no child after leaving Logs; child also absent after Cmd+Q |
| Filesystem space | Final Disk page reported about 28 GiB available on both APFS system/Data volumes, agreeing with `df`; writable Data volume participates in health |
| Keyboard navigation | Cmd+number switched tabs; Cmd+Q closed the preview |

The transitive `block 0.1.6` dependency emits a Rust future-incompatibility notice. Current compilation and strict application lint checks pass. Upstream updates should be revalidated before changing the pinned Kit version.

## Public repository preparation — November 1, 2026

The full local validation script passed again with the MIT license and public repository metadata: formatting, strict Clippy, 28 Rust tests, seven release-workflow tests, two Xcode-bridge tests, release compilation and three headless collector samples. GitHub issue-form/workflow YAML and relative documentation/image links were checked. The staged source was reviewed for private keys, credential patterns, diagnostic captures and local build folders; generated app bundles, DMGs, Keychain material and Xcode user state are excluded from Git.

The public CI workflow pins the validation toolchain and third-party actions, uses read-only repository permissions, and additionally builds/verifies the Xcode Release app with ad hoc signing and checks the packaged icon. Its current hosted result is available in the repository's Actions tab.

## Observed resource use

A 20.01-second interval measured from process CPU-time deltas, with Overview selected, no deliberate interaction, and no log subprocess, used **3.50% of one CPU core**. RSS went from **112.62 MB to 112.33 MB** (about 107 MiB). This is an observed run, **not a foreground-only performance claim**: another active automation session could change window focus. Other chart pages during interactive checks displayed approximately 4–10% single-core CPU and 105–128 MiB RSS, with transient higher values during navigation.

A separate 20-sample **headless** collector run observed:

| Metric | Median | Maximum |
| --- | ---: | ---: |
| Collection time | 35.26 ms | 111.37 ms |
| Reported process CPU, one-core convention | 2.30% | 3.28% |
| RSS | 25.28 MB | 25.30 MB |
| Physical footprint | 11.29 MB | 13.94 MB |

Process enumeration happens only on its scheduled ticks, so collection time varies strongly between ticks. Headless numbers exclude UI rendering and log streaming and must not be presented as whole-app performance. The GUI's physical footprint can exceed RSS (roughly 250 MB was visible in the applications table during checks) because Metal/shared memory and accounting differ. The `log` child and logd also add cost while Logs is selected, especially during a log storm. Screen resolution and hover interactions affect chart-rendering cost.

## Reproduce

```sh
./scripts/validate.sh
./scripts/bundle.sh
open target/release/SystemMonitor.app
# Identify the intended app process; more than one instance may be running.
pgrep -fl 'SystemMonitor.app/Contents/MacOS/SystemMonitor'
python3 scripts/measure.py PID 30
```

For meaningful comparison, keep the chosen tab and foreground/background state constant, move the pointer away from chart tooltips, let startup settle, avoid compiling, and record display resolution, process count, log activity and memory pressure. Measure Logs separately and include its child/producer costs. Repeat longer tests on target hardware before setting a performance budget.

## Visual examples

[Overview screenshot](screenshots/overview.png) · [GPU screenshot](screenshots/gpu.png)

The GPU image was captured before the final percentage-axis padding adjustment; its data and layout remain representative. Local screenshots are examples from this workstation, not synthetic metric fixtures.

## Limits of validation

Not tested: Intel/dGPU or multi-GPU Macs, older macOS versions, non-admin denial paths, an actual thermal-throttling or hardware-failure event, sustained multi-hour leak/stress behavior, full assistive-technology navigation, notarization or installation on another Mac, or the GitHub Actions workflow on a hosted runner. Optional-source fallbacks are implemented but were not all forced in the interactive session. Hardware-dependent tests may skip optional IOReport checks when unsupported. The app does not implement per-core GPU utilization because the required counters are not exposed by this collector.

The repository contains source, lockfile, documentation, validation scripts and screenshots. App bundles and DMGs are generated locally in the ignored `target/` and `dist/` directories; they are not included in a fresh clone.
