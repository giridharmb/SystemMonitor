//! Thin wrappers over macOS kernel, IOKit and libproc APIs. None of them need root.

use core_foundation::base::{CFType, TCFType, kCFAllocatorDefault};
use core_foundation::boolean::CFBoolean;
use core_foundation::data::CFData;
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use core_foundation_sys::base::CFTypeRef;
use core_foundation_sys::dictionary::CFMutableDictionaryRef;
use std::ffi::{CString, c_char, c_int, c_void};
use std::sync::OnceLock;

pub type CfDict = CFDictionary<CFString, CFType>;

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOServiceMatching(name: *const c_char) -> CFMutableDictionaryRef;
    fn IOServiceGetMatchingServices(
        port: u32,
        matching: CFMutableDictionaryRef,
        it: *mut u32,
    ) -> i32;
    fn IOIteratorNext(it: u32) -> u32;
    fn IOObjectRelease(obj: u32) -> i32;
    fn IORegistryEntryCreateCFProperty(
        e: u32,
        key: CFStringRef,
        alloc: *const c_void,
        opts: u32,
    ) -> CFTypeRef;
    fn IORegistryEntryFromPath(port: u32, path: *const c_char) -> u32;
}

unsafe extern "C" {
    fn notify_register_check(name: *const c_char, token: *mut c_int) -> u32;
    fn notify_get_state(token: c_int, state: *mut u64) -> u32;
}

pub fn sysctl_u64(name: &str) -> Option<u64> {
    let name = CString::new(name).ok()?;
    let mut buf = [0u8; 8];
    let mut len = buf.len();
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            buf.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    // Kernel integers here are 4 or 8 bytes wide; a little-endian read covers both.
    (rc == 0).then(|| u64::from_le_bytes(buf))
}

pub fn sysctl_string(name: &str) -> Option<String> {
    let name = CString::new(name).ok()?;
    let mut buf = [0u8; 256];
    let mut len = buf.len();
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            buf.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    (rc == 0).then(|| {
        String::from_utf8_lossy(&buf[..len])
            .trim_end_matches('\0')
            .to_string()
    })
}

fn host_port() -> libc::mach_port_t {
    // mach_host_self() adds a port right on every call, so keep just one.
    static PORT: OnceLock<libc::mach_port_t> = OnceLock::new();
    #[allow(deprecated)]
    *PORT.get_or_init(|| unsafe { libc::mach_host_self() })
}

/// Cumulative `[user, system, idle, nice]` ticks per logical core.
pub fn cpu_ticks() -> Vec<[u32; 4]> {
    let mut count = 0u32;
    let mut info: *mut i32 = std::ptr::null_mut();
    let mut info_count = 0u32;
    let rc = unsafe {
        libc::host_processor_info(
            host_port(),
            libc::PROCESSOR_CPU_LOAD_INFO,
            &mut count,
            (&mut info as *mut *mut i32).cast(),
            &mut info_count,
        )
    };
    if rc != 0 {
        return Vec::new();
    }
    let raw = unsafe { std::slice::from_raw_parts(info as *const u32, info_count as usize) };
    let out = raw
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| [c[0], c[1], c[2], c[3]])
        .collect();
    #[allow(deprecated)]
    unsafe {
        libc::vm_deallocate(
            libc::mach_task_self(),
            info as usize,
            info_count as usize * 4,
        );
    }
    out
}

#[derive(Clone, Copy, Default, Debug)]
pub struct VmCounters {
    pub app: u64,
    pub wired: u64,
    pub compressed: u64,
    pub cached: u64,
    pub pageins: u64,
    pub pageouts: u64,
    pub swapins: u64,
    pub swapouts: u64,
    pub compressions: u64,
    pub decompressions: u64,
}

/// Memory split the way Activity Monitor defines it; sizes in bytes, the rest are page counts.
pub fn vm_counters() -> Option<VmCounters> {
    let mut s: libc::vm_statistics64 = unsafe { std::mem::zeroed() };
    let mut count = libc::HOST_VM_INFO64_COUNT;
    let rc = unsafe {
        libc::host_statistics64(
            host_port(),
            libc::HOST_VM_INFO64,
            (&mut s as *mut libc::vm_statistics64).cast(),
            &mut count,
        )
    };
    if rc != 0 {
        return None;
    }
    let page = sysctl_u64("hw.pagesize").unwrap_or(16384);
    let purgeable = s.purgeable_count as u64;
    Some(VmCounters {
        app: (s.internal_page_count as u64).saturating_sub(purgeable) * page,
        wired: s.wire_count as u64 * page,
        compressed: s.compressor_page_count as u64 * page,
        cached: (s.external_page_count as u64 + purgeable) * page,
        pageins: s.pageins,
        pageouts: s.pageouts,
        swapins: s.swapins,
        swapouts: s.swapouts,
        compressions: s.compressions,
        decompressions: s.decompressions,
    })
}

#[derive(Clone, Copy, Default)]
pub struct SwapUsage {
    pub total: u64,
    pub used: u64,
}

pub fn swap_usage() -> SwapUsage {
    let mut x: libc::xsw_usage = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::xsw_usage>();
    let name = c"vm.swapusage";
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&mut x as *mut libc::xsw_usage).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return SwapUsage::default();
    }
    SwapUsage {
        total: x.xsu_total,
        used: x.xsu_used,
    }
}

/// 0 nominal, 1 moderate, 2 heavy, 3 trapping, 4 sleeping.
pub fn thermal_pressure() -> u64 {
    static TOKEN: OnceLock<Option<c_int>> = OnceLock::new();
    let token = TOKEN.get_or_init(|| {
        let mut t = 0;
        let rc = unsafe {
            notify_register_check(c"com.apple.system.thermalpressurelevel".as_ptr(), &mut t)
        };
        (rc == 0).then_some(t)
    });
    let Some(token) = *token else { return 0 };
    let mut state = 0;
    unsafe { notify_get_state(token, &mut state) };
    state
}

fn for_each_service(class: &str, mut f: impl FnMut(u32)) {
    let Ok(class) = CString::new(class) else {
        return;
    };
    unsafe {
        let mut it = 0;
        // IOServiceGetMatchingServices consumes the matching dictionary.
        if IOServiceGetMatchingServices(0, IOServiceMatching(class.as_ptr()), &mut it) != 0 {
            return;
        }
        loop {
            let e = IOIteratorNext(it);
            if e == 0 {
                break;
            }
            f(e);
            IOObjectRelease(e);
        }
        IOObjectRelease(it);
    }
}

fn prop(entry: u32, key: &str) -> Option<CFType> {
    let key = CFString::new(key);
    let r = unsafe {
        IORegistryEntryCreateCFProperty(
            entry,
            key.as_concrete_TypeRef(),
            kCFAllocatorDefault as _,
            0,
        )
    };
    (!r.is_null()).then(|| unsafe { CFType::wrap_under_create_rule(r) })
}

fn prop_i64(entry: u32, key: &str) -> Option<i64> {
    prop(entry, key)?.downcast::<CFNumber>()?.to_i64()
}

fn prop_bool(entry: u32, key: &str) -> Option<bool> {
    Some(prop(entry, key)?.downcast::<CFBoolean>()? == CFBoolean::true_value())
}

fn prop_dict(entry: u32, key: &str) -> Option<CfDict> {
    let dict = prop(entry, key)?.downcast::<CFDictionary>()?;
    Some(unsafe { CFDictionary::wrap_under_get_rule(dict.as_concrete_TypeRef()) })
}

fn num(d: &CfDict, key: &str) -> i64 {
    d.find(CFString::new(key))
        .and_then(|v| v.downcast::<CFNumber>())
        .and_then(|n| n.to_i64())
        .unwrap_or(0)
}

#[derive(Clone, Copy, Default, Debug)]
pub struct GpuCounters {
    pub available: bool,
    pub device: f32,
    pub renderer: f32,
    pub tiler: f32,
    pub mem_used: u64,
    pub mem_alloc: u64,
}

pub fn gpu_counters() -> GpuCounters {
    let mut out = GpuCounters::default();
    for_each_service("IOAccelerator", |e| {
        if let Some(d) = prop_dict(e, "PerformanceStatistics") {
            out.available = d.find(CFString::new("Device Utilization %")).is_some();
            out.device = (num(&d, "Device Utilization %") as f32).clamp(0., 100.);
            out.renderer = (num(&d, "Renderer Utilization %") as f32).clamp(0., 100.);
            out.tiler = (num(&d, "Tiler Utilization %") as f32).clamp(0., 100.);
            out.mem_used = num(&d, "In use system memory") as u64;
            out.mem_alloc = num(&d, "Alloc system memory") as u64;
        }
    });
    out
}

/// Read the actual logical CPU IDs rather than assuming an E/P ordering.
pub fn core_kinds(total: usize) -> Vec<char> {
    let mut kinds = vec!['C'; total];
    for_each_service("IOPlatformDevice", |entry| {
        let Some(id) = prop_i64(entry, "logical-cpu-id").and_then(|n| usize::try_from(n).ok())
        else {
            return;
        };
        let Some(kind) = prop(entry, "cluster-type").and_then(|v| v.downcast::<CFData>()) else {
            return;
        };
        if let (Some(slot), Some(&byte)) = (kinds.get_mut(id), kind.bytes().first())
            && matches!(byte, b'E' | b'P')
        {
            *slot = byte as char;
        }
    });
    kinds
}

pub fn gpu_core_count() -> u32 {
    let mut cores = 0;
    for_each_service("IOAccelerator", |e| {
        cores = prop_i64(e, "gpu-core-count").unwrap_or(0) as u32
    });
    cores
}

/// Cumulative counters summed over every block device.
#[derive(Clone, Copy, Default, Debug)]
pub struct DiskCounters {
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub read_ops: u64,
    pub write_ops: u64,
    pub read_ns: u64,
    pub write_ns: u64,
    pub errors: u64,
}

pub fn disk_counters() -> DiskCounters {
    let mut c = DiskCounters::default();
    for_each_service("IOBlockStorageDriver", |e| {
        if let Some(d) = prop_dict(e, "Statistics") {
            c.read_bytes += num(&d, "Bytes (Read)") as u64;
            c.write_bytes += num(&d, "Bytes (Write)") as u64;
            c.read_ops += num(&d, "Operations (Read)") as u64;
            c.write_ops += num(&d, "Operations (Write)") as u64;
            c.read_ns += num(&d, "Total Time (Read)") as u64;
            c.write_ns += num(&d, "Total Time (Write)") as u64;
            c.errors += (num(&d, "Errors (Read)") + num(&d, "Errors (Write)")) as u64;
        }
    });
    c
}

#[derive(Clone, Debug, Default)]
pub struct Battery {
    pub percent: i64,
    pub cycles: i64,
    pub design_mah: i64,
    pub max_mah: i64,
    pub temperature_c: f32,
    pub voltage_v: f32,
    pub amps: f32,
    pub charging: bool,
    pub on_ac: bool,
    pub minutes_left: i64,
}

impl Battery {
    /// Positive while charging, negative while discharging.
    pub fn watts(&self) -> f32 {
        self.voltage_v * self.amps
    }
    pub fn health_pct(&self) -> f32 {
        if self.design_mah > 0 {
            self.max_mah as f32 / self.design_mah as f32 * 100.0
        } else {
            0.0
        }
    }
}

pub fn battery() -> Option<Battery> {
    let mut out = None;
    for_each_service("AppleSmartBattery", |e| {
        out = Some(Battery {
            percent: prop_i64(e, "CurrentCapacity").unwrap_or(0),
            cycles: prop_i64(e, "CycleCount").unwrap_or(0),
            design_mah: prop_i64(e, "DesignCapacity").unwrap_or(0),
            max_mah: prop_i64(e, "AppleRawMaxCapacity")
                .or_else(|| prop_i64(e, "NominalChargeCapacity"))
                .unwrap_or(0),
            temperature_c: prop_i64(e, "Temperature").unwrap_or(0) as f32 / 100.0,
            voltage_v: prop_i64(e, "Voltage").unwrap_or(0) as f32 / 1000.0,
            amps: prop_i64(e, "InstantAmperage")
                .or_else(|| prop_i64(e, "Amperage"))
                .unwrap_or(0) as f32
                / 1000.0,
            charging: prop_bool(e, "IsCharging").unwrap_or(false),
            on_ac: prop_bool(e, "ExternalConnected").unwrap_or(false),
            minutes_left: prop_i64(e, "TimeRemaining").unwrap_or(0),
        });
    });
    out
}

/// Filesystem capacity, excluding purgeable/reclaimable estimates returned by Foundation.
pub fn volume_space(path: &str) -> Option<(u64, u64)> {
    let path = CString::new(path).ok()?;
    let mut stat: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(path.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    let block = stat.f_bsize as u64;
    Some((
        stat.f_blocks.saturating_mul(block),
        stat.f_bavail.saturating_mul(block),
    ))
}

/// A DVFS table from the power manager's device-tree node, in MHz.
pub fn dvfs_table_mhz(key: &str) -> Vec<u32> {
    let entry = unsafe { IORegistryEntryFromPath(0, c"IODeviceTree:/arm-io/pmgr".as_ptr()) };
    if entry == 0 {
        return Vec::new();
    }
    let table = prop(entry, key)
        .and_then(|v| v.downcast::<CFData>())
        .map(|d| {
            d.bytes()
                .as_chunks::<8>()
                .0
                .iter()
                .map(|c| hz_to_mhz(u32::from_le_bytes([c[0], c[1], c[2], c[3]])))
                .collect()
        });
    unsafe { IOObjectRelease(entry) };
    table.unwrap_or_default()
}

// M1-M3 tables are in Hz, M4 in kHz. No chip clocks above 10 GHz, so the magnitude tells them apart.
fn hz_to_mhz(raw: u32) -> u32 {
    if raw > 10_000_000 {
        raw / 1_000_000
    } else {
        raw / 1000
    }
}

#[derive(Clone, Copy, Default, Debug)]
pub struct ProcExtra {
    pub footprint: u64,
    pub threads: u32,
    pub wakeups: u64,
    pub energy_nj: u64,
    pub started: u64,
}

/// `None` for processes owned by another user, where the kernel refuses access.
pub fn proc_extra(pid: i32) -> Option<ProcExtra> {
    let mut ru: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
    let rc = unsafe {
        libc::proc_pid_rusage(
            pid,
            libc::RUSAGE_INFO_V4,
            (&mut ru as *mut libc::rusage_info_v4).cast(),
        )
    };
    if rc != 0 {
        return None;
    }
    let mut ti: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_taskinfo>() as c_int;
    let n = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTASKINFO,
            0,
            (&mut ti as *mut libc::proc_taskinfo).cast(),
            size,
        )
    };
    Some(ProcExtra {
        footprint: ru.ri_phys_footprint,
        threads: if n == size {
            ti.pti_threadnum as u32
        } else {
            0
        },
        wakeups: ru.ri_pkg_idle_wkups + ru.ri_interrupt_wkups,
        energy_nj: ru.ri_billed_energy,
        started: ru.ri_proc_start_abstime,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn counters_are_sane_on_this_machine() {
        let cores = cpu_ticks();
        assert_eq!(cores.len() as u64, sysctl_u64("hw.logicalcpu").unwrap());
        assert!(
            cores.iter().all(|c| c[2] > 0),
            "every core has idled at least once"
        );

        let total = sysctl_u64("hw.memsize").unwrap();
        let vm = vm_counters().unwrap();
        assert!(
            vm.wired > 0 && vm.app + vm.wired + vm.compressed <= total,
            "{vm:?}"
        );

        assert!(thermal_pressure() <= 4);
        let extra = proc_extra(std::process::id() as i32).unwrap();
        assert!(extra.footprint > 0 && extra.threads >= 1, "{extra:?}");
        // Cross-user access depends on the current account and must not be assumed.
        let _ = proc_extra(1);
        assert!(disk_counters().read_bytes > 0);
        let (total, free) = volume_space("/").unwrap();
        assert!(total > 0 && free <= total);
        let _ = sysctl_string("hw.model").unwrap();
    }

    #[::core::prelude::v1::test]
    fn dvfs_tables_convert_both_units() {
        assert_eq!(hz_to_mhz(3_228_000_000), 3228);
        assert_eq!(hz_to_mhz(4_512_000), 4512);
    }
}
