//! Background collection. All syscalls happen on this one thread, never on the UI thread, and each
//! source runs only as often as it is worth: 1 Hz for counters, slower for anything expensive.

use crate::ioreport::{IoReport, Power};
use crate::mac::{self, Battery, DiskCounters, GpuCounters, VmCounters};
use crate::util::{app_name, core_usage, parse_lsof};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};
use sysinfo::{
    Components, Disks, Networks, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind,
};

/// Lets the sampler skip work nobody is looking at.
#[derive(Default)]
pub struct Control {
    pub wants_procs: AtomicBool,
    pub wants_ports: AtomicBool,
    pub window_active: AtomicBool,
}

#[derive(Clone, Default)]
pub struct Info {
    pub model: String,
    pub cpu_name: String,
    pub os: String,
    pub host: String,
    /// 'E', 'P' or 'C' per logical core.
    pub core_kinds: Vec<char>,
    pub gpu_cores: u32,
    pub total_mem: u64,
}

#[derive(Clone, Default)]
pub struct ProcRow {
    pub pid: i32,
    pub name: String,
    pub app: String,
    pub cpu: f32,
    pub mem: u64,
    pub threads: u32,
    pub read_bps: f32,
    pub write_bps: f32,
    pub wakeups: f32,
    pub watts: f32,
}

#[derive(Clone, Default)]
pub struct Volume {
    pub name: String,
    pub mount: String,
    pub fs: String,
    pub total: u64,
    pub free: u64,
    pub read_only: bool,
    pub removable: bool,
}

#[derive(Clone, Default)]
pub struct NetIf {
    pub name: String,
    pub rx: f32,
    pub tx: f32,
    pub total_rx: u64,
    pub total_tx: u64,
    pub errors: u64,
}

#[derive(Clone, Default)]
pub struct Mem {
    pub vm: VmCounters,
    pub total: u64,
    pub free: u64,
    pub swap_used: u64,
    pub swap_total: u64,
    pub pressure: u64,
    pub free_pct: u64,
    pub pagein: f32,
    pub pageout: f32,
    pub swapin: f32,
    pub swapout: f32,
    pub compress: f32,
    pub decompress: f32,
}

impl Mem {
    pub fn used(&self) -> u64 {
        self.vm.app + self.vm.wired + self.vm.compressed
    }
}

#[derive(Clone, Copy, Default)]
pub struct DiskIo {
    pub read: f32,
    pub write: f32,
    pub read_iops: f32,
    pub write_iops: f32,
    pub read_ms: f32,
    pub write_ms: f32,
    pub errors: u64,
}

impl DiskIo {
    pub fn worst_latency(&self) -> f32 {
        self.read_ms.max(self.write_ms)
    }
}

#[derive(Clone, Default)]
pub struct Crash {
    pub name: String,
    pub age_secs: u64,
}

/// One tick. `Option` fields are absent on ticks where that slow source did not run.
#[derive(Default)]
pub struct Sample {
    pub core_user: Vec<f32>,
    pub core_sys: Vec<f32>,
    pub load: [f64; 3],
    pub uptime: u64,
    pub mem: Mem,
    pub gpu: GpuCounters,
    pub power: Option<Power>,
    pub disk: DiskIo,
    pub net: Vec<NetIf>,
    pub thermal: u64,
    pub procs: Option<Arc<Vec<ProcRow>>>,
    pub volumes: Option<Vec<Volume>>,
    pub temps: Option<Vec<(String, f32)>>,
    pub battery: Option<Option<Battery>>,
    pub ports: Option<Vec<(String, i32, String)>>,
    pub crashes: Option<Vec<Crash>>,
    pub self_cpu: f32,
    pub self_rss: u64,
    pub sample_ms: f32,
}

pub fn info() -> Info {
    let total = mac::sysctl_u64("hw.logicalcpu").unwrap_or(0) as usize;
    let core_kinds = mac::core_kinds(total);
    Info {
        model: mac::sysctl_string("hw.model").unwrap_or_default(),
        cpu_name: mac::sysctl_string("machdep.cpu.brand_string").unwrap_or_default(),
        os: System::long_os_version().unwrap_or_default(),
        host: System::host_name().unwrap_or_default(),
        core_kinds,
        gpu_cores: mac::gpu_core_count(),
        total_mem: mac::sysctl_u64("hw.memsize").unwrap_or(0),
    }
}

struct ProcPrev {
    started: u64,
    wakeups: u64,
    energy: u64,
}

pub struct Sampler {
    ctl: Arc<Control>,
    sys: System,
    disks: Disks,
    nets: Networks,
    comps: Components,
    report: Option<IoReport>,
    ticks: Vec<[u32; 4]>,
    vm: VmCounters,
    disk: DiskCounters,
    procs_prev: HashMap<i32, ProcPrev>,
    last_tick: Instant,
    last_procs: Instant,
    n: u64,
    me: sysinfo::Pid,
    total_mem: u64,
}

impl Sampler {
    pub fn new(ctl: Arc<Control>, total_mem: u64) -> Self {
        Self {
            ctl,
            sys: System::new(),
            disks: Disks::new_with_refreshed_list(),
            nets: Networks::new_with_refreshed_list(),
            comps: Components::new_with_refreshed_list(),
            report: IoReport::new(),
            ticks: mac::cpu_ticks(),
            vm: mac::vm_counters().unwrap_or_default(),
            disk: mac::disk_counters(),
            procs_prev: HashMap::new(),
            last_tick: Instant::now(),
            last_procs: Instant::now() - Duration::from_secs(60),
            n: 0,
            me: sysinfo::Pid::from_u32(std::process::id()),
            total_mem,
        }
    }

    pub fn tick(&mut self) -> Sample {
        let started = Instant::now();
        let dt = self.last_tick.elapsed().as_secs_f32().max(0.001);
        self.last_tick = started;
        // The first network refresh establishes the baseline; boot totals are not a rate.
        let n = self.n;
        self.n += 1;
        let every = |secs: u64| n.is_multiple_of(secs);

        let mut s = Sample {
            uptime: System::uptime(),
            thermal: mac::thermal_pressure(),
            gpu: mac::gpu_counters(),
            power: self.report.as_mut().and_then(|r| r.sample()),
            ..Default::default()
        };
        let la = System::load_average();
        s.load = [la.one, la.five, la.fifteen];

        let ticks = mac::cpu_ticks();
        for (prev, cur) in self.ticks.iter().zip(&ticks) {
            let (u, sy) = core_usage(*prev, *cur);
            s.core_user.push(u);
            s.core_sys.push(sy);
        }
        self.ticks = ticks;

        self.collect_mem(&mut s, dt);
        self.collect_disk_io(&mut s, dt);

        self.nets.refresh(true);
        s.net = self
            .nets
            .iter()
            .map(|(name, d)| NetIf {
                name: name.clone(),
                rx: if n == 0 { 0. } else { d.received() as f32 / dt },
                tx: if n == 0 {
                    0.
                } else {
                    d.transmitted() as f32 / dt
                },
                total_rx: d.total_received(),
                total_tx: d.total_transmitted(),
                errors: d.total_errors_on_received() + d.total_errors_on_transmitted(),
            })
            .collect();

        // Processes are the costliest source: fast only while their tab is on screen.
        let active = self.ctl.window_active.load(Ordering::Relaxed);
        let period = if active && self.ctl.wants_procs.load(Ordering::Relaxed) {
            2
        } else {
            10
        };
        if self.last_procs.elapsed() >= Duration::from_secs(period) - Duration::from_millis(100) {
            s.procs = Some(Arc::new(self.collect_procs()));
        }
        if let Some(me) = self.sys.process(self.me) {
            s.self_cpu = me.cpu_usage();
            s.self_rss = me.memory();
        }

        if every(5) {
            self.comps.refresh(false);
            let mut t: Vec<_> = self
                .comps
                .iter()
                .filter_map(|c| Some((c.label().to_string(), c.temperature()?)))
                .filter(|t| t.1 > 0.0)
                .collect();
            t.sort_by(|a, b| a.0.cmp(&b.0));
            s.temps = Some(t);
        }
        if every(10) {
            s.battery = Some(mac::battery());
        }
        if every(15) && self.ctl.wants_ports.load(Ordering::Relaxed) {
            s.ports = Some(listening_ports());
        }
        if every(30) {
            self.disks.refresh(true);
            s.volumes = Some(
                self.disks
                    .iter()
                    .filter(|d| is_user_volume(&d.mount_point().to_string_lossy()))
                    .map(|d| {
                        let mount = d.mount_point().to_string_lossy().into_owned();
                        let (total, free) = mac::volume_space(&mount)
                            .unwrap_or((d.total_space(), d.available_space()));
                        Volume {
                            name: d.name().to_string_lossy().into_owned(),
                            mount,
                            fs: d.file_system().to_string_lossy().into_owned(),
                            total,
                            free,
                            read_only: d.is_read_only(),
                            removable: d.is_removable(),
                        }
                    })
                    .collect(),
            );
            s.crashes = Some(recent_crashes());
        }
        s.sample_ms = started.elapsed().as_secs_f32() * 1000.0;
        s
    }

    fn collect_mem(&mut self, s: &mut Sample, dt: f32) {
        let vm = mac::vm_counters().unwrap_or(self.vm);
        let prev = self.vm;
        self.vm = vm;
        let swap = mac::swap_usage();
        let per = |cur: u64, old: u64| cur.saturating_sub(old) as f32 / dt;
        let used = vm.app + vm.wired + vm.compressed;
        s.mem = Mem {
            vm,
            total: self.total_mem,
            free: self.total_mem.saturating_sub(used + vm.cached),
            swap_used: swap.used,
            swap_total: swap.total,
            pressure: mac::sysctl_u64("kern.memorystatus_vm_pressure_level").unwrap_or(1)
                & 0xffff_ffff,
            free_pct: mac::sysctl_u64("kern.memorystatus_level").unwrap_or(0) & 0xffff_ffff,
            pagein: per(vm.pageins, prev.pageins),
            pageout: per(vm.pageouts, prev.pageouts),
            swapin: per(vm.swapins, prev.swapins),
            swapout: per(vm.swapouts, prev.swapouts),
            compress: per(vm.compressions, prev.compressions),
            decompress: per(vm.decompressions, prev.decompressions),
        };
    }

    fn collect_disk_io(&mut self, s: &mut Sample, dt: f32) {
        let cur = mac::disk_counters();
        s.disk = disk_rates(&self.disk, &cur, dt);
        self.disk = cur;
    }

    fn collect_procs(&mut self) -> Vec<ProcRow> {
        let dt = self.last_procs.elapsed().as_secs_f32().max(0.001);
        self.last_procs = Instant::now();
        let kind = ProcessRefreshKind::nothing()
            .with_cpu()
            .with_memory()
            .with_disk_usage()
            .with_exe(UpdateKind::OnlyIfNotSet);
        self.sys
            .refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
        let mut next = HashMap::with_capacity(self.procs_prev.len());
        let rows = self
            .sys
            .processes()
            .values()
            .map(|p| {
                let pid = p.pid().as_u32() as i32;
                let name = p.name().to_string_lossy().into_owned();
                let exe = p
                    .exe()
                    .map(|e| e.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let du = p.disk_usage();
                let mut row = ProcRow {
                    pid,
                    app: app_name(&exe, &name).to_string(),
                    name,
                    cpu: p.cpu_usage(),
                    mem: p.memory(),
                    read_bps: if self.n == 1 {
                        0.
                    } else {
                        du.read_bytes as f32 / dt
                    },
                    write_bps: if self.n == 1 {
                        0.
                    } else {
                        du.written_bytes as f32 / dt
                    },
                    ..Default::default()
                };
                if let Some(x) = mac::proc_extra(pid) {
                    // Footprint is what Activity Monitor calls Memory; RSS overcounts shared pages.
                    row.mem = x.footprint;
                    row.threads = x.threads;
                    if let Some(old) = self.procs_prev.get(&pid).filter(|p| p.started == x.started)
                    {
                        row.wakeups = x.wakeups.saturating_sub(old.wakeups) as f32 / dt;
                        row.watts = x.energy_nj.saturating_sub(old.energy) as f32 * 1e-9 / dt;
                    }
                    next.insert(
                        pid,
                        ProcPrev {
                            started: x.started,
                            wakeups: x.wakeups,
                            energy: x.energy_nj,
                        },
                    );
                }
                row
            })
            .collect();
        self.procs_prev = next;
        rows
    }
}

fn disk_rates(p: &DiskCounters, c: &DiskCounters, dt: f32) -> DiskIo {
    let d = |a: u64, b: u64| a.saturating_sub(b);
    let latency = |ns: u64, ops: u64| {
        if ops > 0 {
            ns as f32 / ops as f32 / 1e6
        } else {
            0.0
        }
    };
    let (r_ops, w_ops) = (d(c.read_ops, p.read_ops), d(c.write_ops, p.write_ops));
    DiskIo {
        read: d(c.read_bytes, p.read_bytes) as f32 / dt,
        write: d(c.write_bytes, p.write_bytes) as f32 / dt,
        read_iops: r_ops as f32 / dt,
        write_iops: w_ops as f32 / dt,
        read_ms: latency(d(c.read_ns, p.read_ns), r_ops),
        write_ms: latency(d(c.write_ns, p.write_ns), w_ops),
        errors: c.errors,
    }
}

// Keep the writable Data volume for disk health; hide APFS helpers and simulator runtimes.
fn is_user_volume(mount: &str) -> bool {
    mount == "/System/Volumes/Data"
        || !(mount.starts_with("/System/Volumes/")
            || mount.starts_with("/Library/Developer/CoreSimulator")
            || mount.starts_with("/private/"))
}

fn listening_ports() -> Vec<(String, i32, String)> {
    crate::util::command_output(
        std::process::Command::new("/usr/sbin/lsof").args([
            "-nP",
            "-iTCP",
            "-sTCP:LISTEN",
            "-F",
            "pcn",
        ]),
        Duration::from_secs(2),
    )
    .map(|bytes| parse_lsof(&String::from_utf8_lossy(&bytes)))
    .unwrap_or_default()
}

fn recent_crashes() -> Vec<Crash> {
    let home = std::env::var("HOME").unwrap_or_default();
    let now = SystemTime::now();
    let mut out = Vec::new();
    for dir in [
        format!("{home}/Library/Logs/DiagnosticReports"),
        "/Library/Logs/DiagnosticReports".to_string(),
    ] {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for e in entries.flatten() {
            let file = e.file_name().to_string_lossy().into_owned();
            let Some(stem) = file
                .strip_suffix(".ips")
                .or(file.strip_suffix(".crash"))
                .or(file.strip_suffix(".panic"))
            else {
                continue;
            };
            let Some(age) = e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|m| now.duration_since(m).ok())
            else {
                continue;
            };
            if age.as_secs() < 7 * 86400 {
                // "Safari-2026-10-31-120000" -> "Safari"
                let name = stem.split("-20").next().unwrap_or(stem).to_string();
                out.push(Crash {
                    name,
                    age_secs: age.as_secs(),
                });
            }
        }
    }
    out.sort_by_key(|c| c.age_secs);
    out
}

pub fn spawn(ctl: Arc<Control>, total_mem: u64, tx: smol::channel::Sender<Sample>) {
    std::thread::Builder::new()
        .name("sampler".into())
        .spawn(move || {
            let mut sampler = Sampler::new(ctl, total_mem);
            loop {
                let started = Instant::now();
                if tx.send_blocking(sampler.tick()).is_err() {
                    return;
                }
                std::thread::sleep(Duration::from_secs(1).saturating_sub(started.elapsed()));
            }
        })
        .expect("spawn sampler");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn disk_rates_from_deltas() {
        let p = DiskCounters {
            read_bytes: 100,
            read_ops: 10,
            read_ns: 0,
            ..Default::default()
        };
        let c = DiskCounters {
            read_bytes: 300,
            read_ops: 20,
            read_ns: 20_000_000,
            ..Default::default()
        };
        let d = disk_rates(&p, &c, 2.0);
        assert_eq!(
            (d.read, d.read_iops, d.read_ms, d.write_ms),
            (100.0, 5.0, 2.0, 0.0)
        );
        // A shrinking counter, as after a device detaches, must not underflow.
        assert_eq!(disk_rates(&c, &p, 1.0).read, 0.0);
    }

    #[::core::prelude::v1::test]
    fn hides_system_volumes() {
        assert!(is_user_volume("/"));
        assert!(is_user_volume("/Volumes/Backup"));
        assert!(is_user_volume("/System/Volumes/Data"));
        assert!(!is_user_volume("/System/Volumes/VM"));
        assert!(!is_user_volume(
            "/Library/Developer/CoreSimulator/Volumes/iOS_23A8464"
        ));
    }

    #[::core::prelude::v1::test]
    fn live_ticks_produce_sane_values() {
        let i = info();
        assert_eq!(
            i.core_kinds.len() as u64,
            mac::sysctl_u64("hw.logicalcpu").unwrap()
        );
        let mut s = Sampler::new(Arc::new(Control::default()), i.total_mem);
        let first = s.tick();
        assert!(first.procs.is_some() && first.volumes.is_some() && first.temps.is_some());
        std::thread::sleep(Duration::from_millis(1100));
        let t = s.tick();
        assert_eq!(t.core_user.len(), i.core_kinds.len());
        assert!(
            t.core_user
                .iter()
                .zip(&t.core_sys)
                .all(|(u, y)| (0.0..=100.01).contains(&(u + y)))
        );
        assert!(t.mem.used() > 0 && t.mem.used() <= t.mem.total);
        assert!(matches!(t.mem.pressure, 1 | 2 | 4), "{}", t.mem.pressure);
        assert!(t.mem.free_pct <= 100, "{}", t.mem.free_pct);
        assert!(t.gpu.device <= 100.0);
        let v = first.volumes.unwrap();
        assert!(v.iter().any(|v| v.mount == "/" && v.total > 0));
        let procs = first.procs.unwrap();
        assert!(
            procs
                .iter()
                .any(|p| p.pid == std::process::id() as i32 && p.mem > 0)
        );
    }
}
