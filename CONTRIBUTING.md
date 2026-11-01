# Contributing to SystemMonitor

Bug reports, hardware compatibility reports, documentation fixes and focused pull requests are welcome. For a larger feature, open an issue first so its scope and collection overhead can be discussed.

## Development setup

Use macOS, Rust 1.99 or newer, Python 3, and Xcode Command Line Tools. Full Xcode is needed for the Xcode project and archive workflow. Start from a fresh clone:

```sh
git clone https://github.com/giridharmb/SystemMonitor.git
cd SystemMonitor
cargo run --release --locked
```

The [Xcode guide](docs/XCODE.md) covers debugging and local signing. Signing keys, a paid developer account, notarization credentials and XcodeGen are not required to contribute Rust code. XcodeGen is needed only when regenerating the checked-in project from `project.yml`.

## Before opening a pull request

```sh
./scripts/validate.sh
```

This runs formatting, Clippy, Rust tests, offline packaging tests, a release build and a short real collector run. Optional hardware tests can skip unavailable sources; report your chip and macOS version. GUI changes also need a manual check in the affected tab. Use a release build when comparing CPU or memory use and follow the measurement procedure in [VALIDATION.md](docs/VALIDATION.md).

Keep changes focused. Explain the concrete problem and resulting behavior, and describe what you verified. A bug fix should include a useful regression check when practical. Follow the existing Rust style and `cargo fmt`. Update the metric-source documentation when changing formulas, availability or sampling schedules. Update both `project.yml` and the generated project when changing Xcode configuration.

## Collector and UI conventions

- Keep collection off the UI thread and bound histories, queues and subprocess output.
- Handle optional Apple interfaces and protected process data without inventing readings.
- Preserve PID/start-time checks, counter baselines and measured sample intervals.
- Explain unsafe code's ownership and lifetime requirements, and release native resources.
- Document CPU normalization, physical footprint/RSS differences and hardware limitations.
- Add no privileged helper, telemetry or new permission request without discussing the change first.

## Reports and attachments

The issue forms ask for environment details and a minimal reproduction. Screenshots and diagnostics may expose process names, file paths, hostnames, network details and private OS logs. Inspect and redact attachments before posting; never attach Keychain exports, certificates with private keys, app-specific passwords or API keys. Use [private vulnerability reporting](SECURITY.md) for security issues.

Be respectful, address technical ideas rather than people, and give maintainers enough detail to reproduce the problem. Maintainers may close abusive, unrelated or duplicate submissions.

## License

Contributions are provided under this repository's [MIT license](LICENSE). Dependencies and any imported code or assets retain their original notices; identify their source and license in the pull request.
