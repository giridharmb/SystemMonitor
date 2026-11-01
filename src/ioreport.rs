//! Power and DVFS readings from libIOReport, the unprivileged source `powermetrics` also uses.
//! The library is private API: every failure degrades to "no data" rather than an error.

use crate::mac::{CfDict, dvfs_table_mhz};
use core_foundation::array::CFArray;
use core_foundation::base::{CFType, TCFType, kCFAllocatorDefault};
use core_foundation::dictionary::{CFDictionary, CFMutableDictionary};
use core_foundation::string::{CFString, CFStringRef};
use core_foundation_sys::base::CFTypeRef;
use core_foundation_sys::dictionary::{
    CFDictionaryCreateMutableCopy, CFDictionaryGetCount, CFDictionaryRef, CFMutableDictionaryRef,
};
use std::ffi::c_void;
use std::time::Instant;

#[link(name = "IOReport", kind = "dylib")]
unsafe extern "C" {
    fn IOReportCopyChannelsInGroup(
        group: CFStringRef,
        sub: CFStringRef,
        a: u64,
        b: u64,
        c: u64,
    ) -> CFDictionaryRef;
    fn IOReportMergeChannels(a: CFDictionaryRef, b: CFDictionaryRef, nil: CFTypeRef);
    fn IOReportCreateSubscription(
        a: *const c_void,
        chans: CFMutableDictionaryRef,
        out: *mut CFMutableDictionaryRef,
        id: u64,
        b: CFTypeRef,
    ) -> *const c_void;
    fn IOReportCreateSamples(
        sub: *const c_void,
        chans: CFMutableDictionaryRef,
        a: CFTypeRef,
    ) -> CFDictionaryRef;
    fn IOReportCreateSamplesDelta(
        prev: CFDictionaryRef,
        cur: CFDictionaryRef,
        a: CFTypeRef,
    ) -> CFDictionaryRef;
    fn IOReportChannelGetGroup(ch: CFDictionaryRef) -> CFStringRef;
    fn IOReportChannelGetSubGroup(ch: CFDictionaryRef) -> CFStringRef;
    fn IOReportChannelGetChannelName(ch: CFDictionaryRef) -> CFStringRef;
    fn IOReportChannelGetUnitLabel(ch: CFDictionaryRef) -> CFStringRef;
    fn IOReportSimpleGetIntegerValue(ch: CFDictionaryRef, idx: i32) -> i64;
    fn IOReportStateGetCount(ch: CFDictionaryRef) -> i32;
    fn IOReportStateGetNameForIndex(ch: CFDictionaryRef, idx: i32) -> CFStringRef;
    fn IOReportStateGetResidency(ch: CFDictionaryRef, idx: i32) -> i64;
}

#[derive(Clone, Debug, Default)]
pub struct Cluster {
    pub name: String,
    pub mhz: f32,
    pub active: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Power {
    pub cpu_w: f32,
    pub gpu_w: f32,
    pub ane_w: f32,
    pub dram_w: f32,
    pub gpu_mhz: f32,
    pub clusters: Vec<Cluster>,
    /// Average MHz per core channel, `(channel name, MHz while active)`.
    pub core_mhz: Vec<(String, f32)>,
}

pub struct IoReport {
    sub: CFType,
    chans: CFMutableDictionary,
    prev: CFDictionaryRef,
    prev_at: Instant,
    ecpu_mhz: Vec<u32>,
    pcpu_mhz: Vec<u32>,
    gpu_mhz: Vec<u32>,
}

impl Drop for IoReport {
    fn drop(&mut self) {
        unsafe { core_foundation_sys::base::CFRelease(self.prev.cast()) };
    }
}

fn cf(s: CFStringRef) -> String {
    if s.is_null() {
        String::new()
    } else {
        unsafe { CFString::wrap_under_get_rule(s) }.to_string()
    }
}

impl IoReport {
    pub fn new() -> Option<Self> {
        unsafe {
            let group = |g: &str, s: Option<&str>| {
                let g = CFString::new(g);
                let s = s.map(CFString::new);
                let sref = s
                    .as_ref()
                    .map_or(std::ptr::null(), |s| s.as_concrete_TypeRef());
                IOReportCopyChannelsInGroup(g.as_concrete_TypeRef(), sref, 0, 0, 0)
            };
            let base = group("Energy Model", None);
            if base.is_null() {
                return None;
            }
            for (g, s) in [
                ("CPU Stats", "CPU Complex Performance States"),
                ("CPU Stats", "CPU Core Performance States"),
                ("GPU Stats", "GPU Performance States"),
            ] {
                let other = group(g, Some(s));
                if !other.is_null() {
                    IOReportMergeChannels(base, other, std::ptr::null());
                    core_foundation_sys::base::CFRelease(other.cast());
                }
            }
            let size = CFDictionaryGetCount(base);
            let chans = CFDictionaryCreateMutableCopy(kCFAllocatorDefault, size, base);
            core_foundation_sys::base::CFRelease(base.cast());
            if chans.is_null() {
                return None;
            }
            let chans = CFMutableDictionary::wrap_under_create_rule(chans);
            let mut subbed: CFMutableDictionaryRef = std::ptr::null_mut();
            let sub = IOReportCreateSubscription(
                std::ptr::null(),
                chans.as_concrete_TypeRef(),
                &mut subbed,
                0,
                std::ptr::null(),
            );
            if !subbed.is_null() {
                core_foundation_sys::base::CFRelease(subbed.cast());
            }
            if sub.is_null() {
                return None;
            }
            let sub = CFType::wrap_under_create_rule(sub);
            let prev = IOReportCreateSamples(
                sub.as_CFTypeRef(),
                chans.as_concrete_TypeRef(),
                std::ptr::null(),
            );
            if prev.is_null() {
                return None;
            }
            Some(Self {
                sub,
                chans,
                prev,
                prev_at: Instant::now(),
                // The device-tree keys are stable across M1-M4.
                ecpu_mhz: dvfs_table_mhz("voltage-states1-sram"),
                pcpu_mhz: dvfs_table_mhz("voltage-states5-sram"),
                gpu_mhz: gpu_table(dvfs_table_mhz("voltage-states9")),
            })
        }
    }

    pub fn sample(&mut self) -> Option<Power> {
        unsafe {
            let cur = IOReportCreateSamples(
                self.sub.as_CFTypeRef(),
                self.chans.as_concrete_TypeRef(),
                std::ptr::null(),
            );
            if cur.is_null() {
                return None;
            }
            let delta = IOReportCreateSamplesDelta(self.prev, cur, std::ptr::null());
            core_foundation_sys::base::CFRelease(self.prev.cast());
            self.prev = cur;
            let secs = self.prev_at.elapsed().as_secs_f32().max(0.001);
            self.prev_at = Instant::now();
            if delta.is_null() {
                return None;
            }
            let delta: CfDict = CFDictionary::wrap_under_create_rule(delta);
            let channels = delta
                .find(CFString::new("IOReportChannels"))?
                .downcast::<CFArray>()?;
            let mut p = Power::default();
            for raw in channels.iter() {
                let ch = *raw as CFDictionaryRef;
                let name = cf(IOReportChannelGetChannelName(ch));
                match cf(IOReportChannelGetGroup(ch)).as_str() {
                    "Energy Model" => {
                        let unit = cf(IOReportChannelGetUnitLabel(ch));
                        let w = energy_to_watts(IOReportSimpleGetIntegerValue(ch, 0), &unit, secs);
                        add_energy(&mut p, &name, w);
                    }
                    "CPU Stats" => {
                        let table = if name.contains("ECPU") || name.starts_with('E') {
                            &self.ecpu_mhz
                        } else {
                            &self.pcpu_mhz
                        };
                        let (mhz, active) = dvfs(&states(ch), table);
                        if cf(IOReportChannelGetSubGroup(ch)).contains("Complex") {
                            // ECPM/PCPM are the clusters' memory controllers, not cores.
                            if name.contains("CPU") {
                                p.clusters.push(Cluster { name, mhz, active });
                            }
                        } else {
                            p.core_mhz.push((name, mhz));
                        }
                    }
                    "GPU Stats" if name == "GPUPH" => {
                        p.gpu_mhz = dvfs(&states(ch), &self.gpu_mhz).0
                    }
                    _ => {}
                }
            }
            p.clusters.sort_by(|a, b| a.name.cmp(&b.name));
            Some(p)
        }
    }

    /// Scratch output for tuning channel names on a new chip: `SystemMonitor --dump-ioreport`.
    pub fn dump(&mut self) {
        unsafe {
            let cur = IOReportCreateSamples(
                self.sub.as_CFTypeRef(),
                self.chans.as_concrete_TypeRef(),
                std::ptr::null(),
            );
            if cur.is_null() {
                return;
            }
            let dict: CfDict = CFDictionary::wrap_under_create_rule(cur);
            let Some(channels) = dict
                .find(CFString::new("IOReportChannels"))
                .and_then(|c| c.downcast::<CFArray>())
            else {
                return;
            };
            for raw in channels.iter() {
                let ch = *raw as CFDictionaryRef;
                let names: Vec<String> = states(ch).into_iter().map(|s| s.0).collect();
                println!(
                    "{} / {} / {} [{}] {:?}",
                    cf(IOReportChannelGetGroup(ch)),
                    cf(IOReportChannelGetSubGroup(ch)),
                    cf(IOReportChannelGetChannelName(ch)),
                    cf(IOReportChannelGetUnitLabel(ch)),
                    names
                );
            }
        }
        println!(
            "ecpu {:?}\npcpu {:?}\ngpu {:?}",
            self.ecpu_mhz, self.pcpu_mhz, self.gpu_mhz
        );
    }
}

// The GPU table carries a 0 MHz "off" row (the OFF state is already skipped) and is not always ascending.
fn gpu_table(mut t: Vec<u32>) -> Vec<u32> {
    t.retain(|&f| f > 0);
    t.sort_unstable();
    t
}

unsafe fn states(ch: CFDictionaryRef) -> Vec<(String, i64)> {
    unsafe {
        (0..IOReportStateGetCount(ch))
            .map(|i| {
                (
                    cf(IOReportStateGetNameForIndex(ch, i)),
                    IOReportStateGetResidency(ch, i),
                )
            })
            .collect()
    }
}

fn energy_to_watts(value: i64, unit: &str, secs: f32) -> f32 {
    let joules = value as f32
        * match unit.trim() {
            "mJ" => 1e-3,
            "uJ" | "µJ" => 1e-6,
            "nJ" => 1e-9,
            _ => 0.0,
        };
    joules / secs
}

fn add_energy(p: &mut Power, name: &str, w: f32) {
    // Aggregate channels only: per-cluster and per-core ones would double count.
    match name {
        "CPU Energy" => p.cpu_w += w,
        "GPU Energy" => p.gpu_w += w,
        n if n.starts_with("ANE") => p.ane_w += w,
        n if n.starts_with("DRAM") => p.dram_w += w,
        _ => {}
    }
}

/// `(average MHz while active, percent of time active)` from a state-residency histogram.
fn dvfs(states: &[(String, i64)], table: &[u32]) -> (f32, f32) {
    let idle = |n: &str| matches!(n, "IDLE" | "DOWN" | "OFF" | "IDLE_OFF");
    let total: i64 = states.iter().map(|s| s.1).sum();
    let (mut active, mut weighted) = (0i64, 0f64);
    for (i, (_, residency)) in states.iter().filter(|s| !idle(&s.0)).enumerate() {
        active += residency;
        // Tables can be shorter than the state list; clamp to the top frequency.
        let mhz = table.get(i).or(table.last()).copied().unwrap_or(0);
        weighted += *residency as f64 * mhz as f64;
    }
    let mhz = if active > 0 {
        (weighted / active as f64) as f32
    } else {
        0.0
    };
    let pct = if total > 0 {
        active as f32 / total as f32 * 100.0
    } else {
        0.0
    };
    (mhz, pct)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn dvfs_weights_by_residency() {
        let s = [("IDLE", 50), ("V0", 25), ("V1", 25)].map(|(n, r)| (n.to_string(), r));
        let (mhz, pct) = dvfs(&s, &[1000, 2000]);
        assert_eq!((mhz, pct), (1500.0, 50.0));
        // Missing table must not panic, all idle must not divide by zero.
        assert_eq!(dvfs(&s, &[]), (0.0, 50.0));
        assert_eq!(dvfs(&[("IDLE".to_string(), 0)], &[1000]), (0.0, 0.0));
    }

    #[::core::prelude::v1::test]
    fn gpu_table_drops_off_row_and_sorts() {
        assert_eq!(gpu_table(vec![0, 338, 1182, 1062]), vec![338, 1062, 1182]);
    }

    #[::core::prelude::v1::test]
    fn energy_units() {
        assert_eq!(energy_to_watts(2000, "mJ", 1.0), 2.0);
        assert!((energy_to_watts(2_000_000_000, "nJ", 2.0) - 1.0).abs() < 1e-6);
        assert_eq!(energy_to_watts(5, "furlong", 1.0), 0.0);
    }

    #[::core::prelude::v1::test]
    fn live_sample_reports_power() {
        if !cfg!(target_arch = "aarch64") {
            return;
        }
        let Some(mut r) = IoReport::new() else {
            eprintln!("IOReport unavailable on this Mac; optional hardware test skipped");
            return;
        };
        std::thread::sleep(std::time::Duration::from_millis(300));
        let p = r.sample().expect("sample");
        assert!(p.cpu_w > 0.0, "{p:?}");
        assert!(
            !p.clusters.is_empty() && p.clusters.iter().all(|c| c.name.contains("CPU")),
            "{p:?}"
        );
        assert_eq!(
            p.core_mhz.len() as u64,
            crate::mac::sysctl_u64("hw.logicalcpu").unwrap()
        );
    }
}
