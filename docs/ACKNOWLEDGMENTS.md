# Third-party acknowledgments

SystemMonitor uses [GPUI Kit](https://github.com/longbridge/gpui-kit) by Longbridge and its GPUI-based rendering/component ecosystem. The application lifecycle, chart and virtualized table examples informed this implementation; the exact example links are in the [README](../README.md#gpui-kit-examples-studied).

Direct Rust dependencies include `gpui-kit`, `sysinfo`, `smol`, `libc`, `core-foundation`, `core-foundation-sys`, `serde` and `serde_json`. Their versions and transitive dependencies are recorded in `Cargo.lock`; their upstream packages retain their own licenses and notices. The application's MIT license does not replace those terms. Review the resolved dependency licenses when distributing a binary.

Direct dependency declarations reported by Cargo for the current lockfile:

| Package | Version | Declared license |
| --- | --- | --- |
| gpui-kit | 0.8.1 | Apache-2.0 |
| sysinfo | 0.37.2 | MIT |
| smol | 2.0.2 | Apache-2.0 OR MIT |
| libc | 0.2.191 | MIT OR Apache-2.0 |
| core-foundation | 0.10.1 | MIT OR Apache-2.0 |
| core-foundation-sys | 0.8.7 | MIT OR Apache-2.0 |
| serde | 1.0.229 | MIT OR Apache-2.0 |
| serde_json | 1.0.154 | MIT OR Apache-2.0 |

This table is not a complete inventory of transitive dependencies or a substitute for their license texts. Recheck the resolved packages when updating `Cargo.lock`.

The app icon was produced with the built-in image-generation tool. Its master, macOS representations, generation prompt and rebuild instructions are documented in [assets/README.md](../assets/README.md).

Apple's system frameworks, Xcode tools and operating-system APIs are used on macOS. No Apple framework, signing private key or notarization credential is bundled in this repository.
