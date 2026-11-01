# Changelog

This file records user-visible changes. Versions follow the application's `Cargo.toml`.

## 0.1.0 — Initial public source release

- Native Rust/GPUI Kit application with nine monitoring tabs and live charts.
- Per-core CPU usage, process grouping, memory pressure/footprints, disk IOPS/timings and network interfaces/listeners.
- Optional GPU utilization, component power/frequency, battery and thermal sources.
- Bounded, on-demand OS log watcher and recent diagnostic-report summary.
- Background sampling, bounded histories and slower schedules for expensive queries.
- Headless diagnostic sampling and local overhead measurement tools.
- Xcode project, custom Retina app icon, application bundling and Developer ID DMG scripts.
- Architecture/accuracy documentation, validation records, contribution guides and GitHub templates.

The validated DMG is signed-only pending notarization. Intel/dGPU/multi-GPU Macs and older macOS versions have not been validated on hardware. Per-GPU-core utilization is unavailable; the UI reports device/pipeline-stage counters instead.
