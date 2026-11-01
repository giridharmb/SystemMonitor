#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
python3 scripts/test_release.py
python3 scripts/test_xcode.py
cargo build --release --locked
# Exercise real collectors without opening a window. Output remains local.
"${CARGO_TARGET_DIR:-target}/release/SystemMonitor" --diagnostics 3
