# Security policy

## Reporting a vulnerability

Use [GitHub's private vulnerability report form](https://github.com/giridharmb/SystemMonitor/security/advisories/new). Do not open a public issue containing an unpatched vulnerability, credentials or private system logs. Include the affected commit/version, Mac chip and macOS version, reproduction steps, expected impact and a minimal example with sensitive data removed.

Reports are handled by the project maintainer; no response-time or remediation SLA is promised. Security fixes target the current development version. There is no long-term support release line yet.

## Application boundaries

SystemMonitor reads local system/process metrics and starts a filtered macOS unified-log stream only while its Logs tab is selected. It has no server, telemetry uploader, process-termination feature or privileged helper. It respects the current account's access and OS privacy redaction. Monitoring does not prove that a Mac is secure or free of hardware faults.

Some collectors use undocumented Apple interfaces and native unsafe code. Bugs involving malformed log input, unbounded resource use, native memory ownership, subprocess lifecycle or accidental disclosure are relevant security reports.

## Distribution and credentials

Builds in `target/` and images in `dist/` are local artifacts, not source releases. Developer ID signing and notarization are separate checks; a signed-only DMG is not a notarized download. Verify release provenance and checksums. Follow [DISTRIBUTION.md](docs/DISTRIBUTION.md) to sign, notarize and verify a distributable image.

Store signing private keys and notary credentials in the macOS Keychain. Keep credentials out of Git, workflow logs, issue attachments and pull requests. Public CI needs no signing secrets and does not perform notarization.
