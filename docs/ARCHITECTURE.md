# Architecture and metric definitions

## Data flow

A sampler thread owns sysinfo, IOReport, counter baselines and process deltas. It sends snapshots over a bounded channel of four items. The GPUI view receives them asynchronously, records bounded histories, and builds only the selected tab. The process vector is shared through `Arc` rather than copied per chart. Rendering and collector work never run concurrently against the same mutable collector.

`src/mac.rs` contains the unsafe boundary for Mach, libproc, sysctl and IOKit. Core Foundation values use create/get-rule wrappers. Mach processor-info arrays are deallocated after copying, IOKit iterators/services are released, and the host port is reused. `src/ioreport.rs` owns its subscription, channel dictionary and prior sample. It releases every replaced sample and the subscription on drop. These APIs are private or platform-specific; no claim is made that unsupported interfaces remain stable across OS versions.

## Sampling budget

| Source | Schedule | Implementation |
| --- | --- | --- |
| CPU, VM, disk, GPU, power, network, thermal | 1 second | Mach/sysctl/IOKit/IOReport/sysinfo |
| Processes, including per-process libproc details | 2 seconds on relevant active tabs; 10 seconds otherwise | Minimal sysinfo refresh set and libproc calls |
| Temperatures | 5 ticks | sysinfo components; no SMC kernel extension |
| Battery | 10 ticks | AppleSmartBattery properties |
| Listening ports | 15 ticks, Network tab only | `lsof -nP -iTCP -sTCP:LISTEN -F pcn`, bounded output/deadline |
| User volumes and crash reports | 30 ticks | sysinfo disks and directory metadata |
| Log health rate | 10 seconds | Error/fault count deltas per minute |
| Log list drain | 250 ms when foreground Logs is visible and unpaused | Bounded queue and virtual list |

The slow-source schedules are tick based, so a delayed sample delays subsequent ticks; they are approximate intervals, not real-time deadlines. Histories show the last 180 samples and label their usual one-second cadence. A suspended system or a stalled collector can make the actual history span longer. Samples record measured elapsed time for rate calculations.

## Formulas and sources

- **CPU:** `host_processor_info(PROCESSOR_CPU_LOAD_INFO)`. Per-core `(Δuser + Δnice)/Δtotal` and `Δsystem/Δtotal`; system usage is the mean across logical cores. Unsigned 32-bit deltas wrap, with 64-bit summation. Core labels use `logical-cpu-id` and `cluster-type` properties, with generic C labels if topology is unavailable.
- **Memory:** `host_statistics64(HOST_VM_INFO64)`, multiplied by the kernel page size. App = internal minus purgeable; wired = wired pages; compressed = compressor physical pages; cached = external plus purgeable; free = total minus used minus cached, saturating at zero. These snapshots are not atomic with every other system source. Swap uses `vm.swapusage`; pressure uses `kern.memorystatus_vm_pressure_level`.
- **GPU:** IOAccelerator `PerformanceStatistics`, with utilization clamped to 0–100%. The primary target is a single Apple Silicon integrated GPU; multi-GPU aggregation and per-core GPU counters are not implemented.
- **Disk:** IOBlockStorageDriver Statistics. Throughput = byte delta / measured seconds; IOPS = operation delta / seconds; average operation time = time delta / operation delta / 1,000,000. Cumulative aggregate counters saturate when devices disappear. Volume free space uses `statfs`, with Foundation/sysinfo as a fallback if the syscall fails. Both the sealed system volume and writable Data volume are shown; health checks ignore read-only volumes. An attached device's existing counters may cause a one-sample spike; device-identity tracking is a future refinement.
- **Network:** sysinfo's cumulative interface deltas / measured seconds. First-pass rates are zero until a baseline exists. Interfaces can appear/disappear between samples. Summing all non-loopback interfaces is useful for trends but is not guaranteed to equal external wire traffic with VPNs and bridges.
- **Processes:** sysinfo CPU/memory/disk fields, supplemented with libproc physical footprint, thread count, idle/interrupt wakeups, and `ri_billed_energy`. Energy/wakeup deltas are accepted only when PID and process start time match, preventing PID-reuse spikes. Process CPU follows sysinfo's one-core normalization. Protected data falls back to RSS or zero; table totals can be partial.
- **Power:** IOReport Energy Model channels convert mJ/µJ/nJ deltas to joules / elapsed seconds. CPU/GPU aggregate channels avoid double-counting subchannels; ANE and DRAM are included separately. The sum excludes display, charger losses, peripherals and several chip blocks, so it is **not wall power**. Apple's [Recount documentation](https://github.com/apple-oss-distributions/xnu/blob/main/doc/observability/recount.md) describes nanojoule task energy accounting.
- **Frequency:** IOReport state residency weighted by device-tree voltage-state frequencies. Average clock is weighted over active states; effective cluster frequency multiplies it by active residency. Device-tree tables use Hz/kHz conversions. New chips need hardware calibration; the UI does not assign IOReport channel order to individual Mach core IDs.
- **Thermal:** notify state for `com.apple.system.thermalpressurelevel`; 0 nominal, 1 moderate, 2 heavy, 3 trapping, 4 sleeping. Absent notification support currently defaults to nominal. Temperature sensors are opportunistic and may be entirely absent.

## Logs and resource bounds

The predicate is evaluated by `log stream` itself: errors/faults plus kernel, launchd, powerd, sudo/authd/loginwindow/securityd/syspolicyd/trustd. No debug/info-wide stream is requested. Two reader threads drain stdout and stderr so error output cannot block the child. JSON records over 64 KiB are discarded, messages are truncated at a UTF-8 boundary to 2,000 bytes, stderr is capped at 4 KiB, pending events are capped at 5,000, and the visible history is capped independently at 5,000. Log count limits do not bound CPU cost during an extreme OS log storm; the upstream producer and JSON parsing still do work for each event.

Capture runs only while the Logs tab is selected, to avoid a permanent `log`/logd workload. Leaving that tab kills/reaps the stream; reopening starts a new stream and preserves the displayed history, with an explicit capture gap. Log-rate health penalties apply only during capture. The child is held as a `Child`, not a reusable integer PID, and killed/reaped on quit or view teardown. A stream that exits is reported and does not automatically restart; switch away and back to Logs to reconnect. Pausing affects display, not the live producer. Clear removes queued and displayed entries; lifetime health counters remain intact. All monitoring is read-only; the application does not terminate other processes, delete logs, alter kernel settings or send data externally.

## Health score

Start at 100, subtract points, clamp at zero. Explain each deduction on Overview:

| Condition | Points |
| --- | ---: |
| Memory pressure warning / critical | 15 / 35 |
| Thermal pressure moderate / heavier | 10 / 30 |
| Current CPU ≥90% | 15 |
| Swap-out ≥100 pages/s and pressure warning/critical | 10 |
| A writable volume <15% / <5% free | 10 / 25 |
| Current aggregate disk operation time >50 ms | 10 |
| Unified-log errors/faults >600/min | 5 |
| At least 5 accessible diagnostic reports in 24h | 10 |
| Known battery design capacity below 80% | 5 |

Conditions reflect the latest readings; they are not sustained-load alerts. Log errors, diagnostic reports and high CPU can be normal. A missing source cannot lower the score by itself, and device-level availability is shown separately. This design deliberately keeps the score interpretable.
