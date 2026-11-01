//! Headless sampling for reproducible collector checks and local snapshots.
use crate::{mac, sampler};
use serde_json::json;
use std::sync::{Arc, atomic::Ordering};
use std::time::{Duration, Instant};

pub fn run(seconds: u64) {
    let info = sampler::info();
    let control = Arc::new(sampler::Control::default());
    control.window_active.store(true, Ordering::Relaxed);
    control.wants_procs.store(true, Ordering::Relaxed);
    let mut sampler = sampler::Sampler::new(control, info.total_mem);
    // Establish delta baselines before emitting rates.
    sampler.tick();
    for _ in 0..seconds {
        std::thread::sleep(Duration::from_secs(1));
        let at = Instant::now();
        let s = sampler.tick();
        let cpu: Vec<_> = s
            .core_user
            .iter()
            .zip(&s.core_sys)
            .map(|(u, k)| u + k)
            .collect();
        let power = s.power.as_ref().map(|p| json!({
            "cpu_w": p.cpu_w, "gpu_w": p.gpu_w, "ane_w": p.ane_w, "dram_w": p.dram_w,
            "gpu_mhz": p.gpu_mhz,
            "clusters": p.clusters.iter().map(|c| json!({"name": c.name, "mhz": c.mhz, "active_pct": c.active})).collect::<Vec<_>>()
        }));
        let own = mac::proc_extra(std::process::id() as i32);
        println!(
            "{}",
            json!({
                "schema": 1,
                "unix_seconds": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(),
                "chip": info.cpu_name, "model": info.model, "os": info.os,
                "cpu_pct": cpu.iter().sum::<f32>() / cpu.len().max(1) as f32,
                "cores_pct": cpu, "core_kinds": info.core_kinds.iter().collect::<String>(),
                "gpu_available": s.gpu.available, "gpu_pct": s.gpu.available.then_some(s.gpu.device), "gpu_cores": info.gpu_cores,
                "memory_used_bytes": s.mem.used(), "memory_total_bytes": s.mem.total,
                "memory_pressure": s.mem.pressure, "swap_used_bytes": s.mem.swap_used,
                "disk_read_bps": s.disk.read, "disk_write_bps": s.disk.write,
                "disk_read_iops": s.disk.read_iops, "disk_read_ms": s.disk.read_ms,
                "interfaces": s.net.iter().map(|n| json!({"name": n.name, "rx_bps": n.rx, "tx_bps": n.tx})).collect::<Vec<_>>(),
                "power": power, "thermal_pressure": s.thermal,
                "self_cpu_pct": s.self_cpu, "self_rss_bytes": s.self_rss,
                "self_footprint_bytes": own.map(|p| p.footprint),
                "collector_ms": at.elapsed().as_secs_f64() * 1000.0,
                "process_count": s.procs.as_ref().map(|p| p.len())
            })
        );
    }
}
