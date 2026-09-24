//! Memory gate for sidecar model mounts. Exact parity with the acting-path gate in
//! `local-edition/mlx/gates.py` (G1): same floor formula, same verdict bands — the Python
//! gate guards manual/bench mounts, this one guards every mount goose itself performs.
use std::path::Path;

pub const GIB: u64 = 1024 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Warn,
    Block,
}

#[derive(Debug, Clone)]
pub struct GateResult {
    pub verdict: Verdict,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct MemoryGate {
    pub floor_min_bytes: u64,
    pub floor_fraction: f64,
    pub warn_band_bytes: u64,
}

impl Default for MemoryGate {
    fn default() -> Self {
        Self {
            floor_min_bytes: 8 * GIB,
            floor_fraction: 0.10,
            warn_band_bytes: 4 * GIB,
        }
    }
}

impl MemoryGate {
    pub fn floor_bytes(&self, total_bytes: u64) -> u64 {
        self.floor_min_bytes
            .max((total_bytes as f64 * self.floor_fraction) as u64)
    }

    /// Could ANY amount of reclaimed memory let the model through? Not when the model plus the
    /// floor exceeds the Mac's whole RAM.
    pub fn could_ever_fit(&self, model_bytes: u64, total_bytes: u64) -> bool {
        model_bytes.saturating_add(self.floor_bytes(total_bytes)) <= total_bytes
    }

    pub fn evaluate(&self, model_bytes: u64, available_bytes: u64, total_bytes: u64) -> GateResult {
        let floor = self.floor_bytes(total_bytes);
        let needed = model_bytes.saturating_add(floor);
        if needed > available_bytes {
            let short = needed - available_bytes;
            return GateResult {
                verdict: Verdict::Block,
                message: format!(
                    "model {:.1} GiB + floor {:.1} GiB exceeds available {:.1} GiB (short {:.1} GiB)",
                    gib(model_bytes),
                    gib(floor),
                    gib(available_bytes),
                    gib(short)
                ),
            };
        }
        let leftover = available_bytes - needed;
        if leftover < self.warn_band_bytes {
            return GateResult {
                verdict: Verdict::Warn,
                message: format!(
                    "fits, but only {:.1} GiB above the floor — expect pressure under load",
                    gib(leftover)
                ),
            };
        }
        GateResult {
            verdict: Verdict::Allow,
            message: format!(
                "model {:.1} GiB fits with {:.1} GiB above the {:.1} GiB floor",
                gib(model_bytes),
                gib(leftover),
                gib(floor)
            ),
        }
    }
}

fn gib(bytes: u64) -> f64 {
    bytes as f64 / GIB as f64
}

/// Physical memory right now, as the mount gate and the status report read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryReading {
    /// Memory a new allocation can take without pushing anything in use out: truly free
    /// pages plus the reclaimable file cache the OS hands back on demand.
    pub available_bytes: u64,
    pub total_bytes: u64,
    /// The part of `available_bytes` that is file cache or purgeable memory. `None` where
    /// the platform source folds cache into its available figure without splitting it out
    /// (Linux's `MemAvailable`, via sysinfo).
    pub reclaimable_cache_bytes: Option<u64>,
}

/// Page counts from `host_statistics64(HOST_VM_INFO64)`, as the kernel reports them.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Debug, Clone, Copy)]
pub(crate) struct VmPageCounts {
    /// `free_count` — INCLUDES the speculative pages (vm_stat prints `free_count -
    /// speculative_count` as "Pages free"; measured: raw 3,347,169 = 2,719,606 + 626,850
    /// speculative on 2026-09-23).
    pub(crate) free: u64,
    pub(crate) speculative: u64,
    /// `external_page_count` — file-backed pages ("File-backed pages" in vm_stat); the
    /// speculative read-ahead pages are among them.
    pub(crate) external: u64,
    pub(crate) purgeable: u64,
}

/// macOS available memory the way Activity Monitor draws it: physical memory minus App
/// Memory, Wired and Compressed — which is truly-free pages plus file cache plus purgeable.
///
/// Why not sysinfo: its macOS `available_memory()` is `free + inactive + purgeable −
/// compressor`, which drops every active file-cache page and then subtracts the compressor's
/// footprint a second time; with a 41.8 GiB compressor it read 0.0–3.6 GiB while 41 GiB
/// was available (measured 2026-09-23, M4 Max 128 GB). Why not `kern.memorystatus_level`
/// (the % `memory_pressure` prints): holding 12 GiB of touched anonymous memory moved this
/// figure 41.4 → 28.0 GiB and left memorystatus_level at 46–47% throughout, so it cannot
/// guard a mount. Filling 11 GiB of file cache moved this figure by < 0.4 GiB.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn darwin_reading(
    counts: VmPageCounts,
    page_size: u64,
    total_bytes: u64,
) -> MemoryReading {
    let truly_free = counts.free.saturating_sub(counts.speculative);
    let reclaimable = counts.external.saturating_add(counts.purgeable);
    MemoryReading {
        available_bytes: truly_free
            .saturating_add(reclaimable)
            .saturating_mul(page_size),
        total_bytes,
        reclaimable_cache_bytes: Some(reclaimable.saturating_mul(page_size)),
    }
}

#[cfg(target_os = "macos")]
pub fn measure() -> anyhow::Result<MemoryReading> {
    darwin::measure()
}

#[cfg(not(target_os = "macos"))]
pub fn measure() -> anyhow::Result<MemoryReading> {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    let total_bytes = sys.total_memory();
    anyhow::ensure!(total_bytes > 0, "sysinfo reported 0 bytes of total memory");
    Ok(MemoryReading {
        available_bytes: sys.available_memory(),
        total_bytes,
        reclaimable_cache_bytes: None,
    })
}

#[cfg(target_os = "macos")]
mod darwin {
    use super::{darwin_reading, MemoryReading, VmPageCounts};
    use std::sync::OnceLock;

    extern "C" {
        fn mach_host_self() -> libc::mach_port_t;
    }

    /// One host send right for the process: every `mach_host_self()` call adds a user
    /// reference, and the status poll would otherwise leak one per read.
    fn host_port() -> libc::mach_port_t {
        static HOST: OnceLock<libc::mach_port_t> = OnceLock::new();
        *HOST.get_or_init(|| unsafe { mach_host_self() })
    }

    pub fn measure() -> anyhow::Result<MemoryReading> {
        let mut stat: libc::vm_statistics64 = unsafe { std::mem::zeroed() };
        let mut count = libc::HOST_VM_INFO64_COUNT;
        let rc = unsafe {
            libc::host_statistics64(
                host_port(),
                libc::HOST_VM_INFO64,
                &mut stat as *mut libc::vm_statistics64 as libc::host_info64_t,
                &mut count,
            )
        };
        anyhow::ensure!(
            rc == libc::KERN_SUCCESS,
            "host_statistics64(HOST_VM_INFO64) failed with kern_return {rc}"
        );
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        anyhow::ensure!(
            page_size > 0,
            "sysconf(_SC_PAGESIZE) failed: {}",
            std::io::Error::last_os_error()
        );
        let counts = VmPageCounts {
            free: u64::from(stat.free_count),
            speculative: u64::from(stat.speculative_count),
            external: u64::from(stat.external_page_count),
            purgeable: u64::from(stat.purgeable_count),
        };
        Ok(darwin_reading(counts, page_size as u64, total_bytes()?))
    }

    fn total_bytes() -> anyhow::Result<u64> {
        let mut total: u64 = 0;
        let mut len = std::mem::size_of::<u64>();
        let rc = unsafe {
            libc::sysctlbyname(
                c"hw.memsize".as_ptr(),
                &mut total as *mut u64 as *mut libc::c_void,
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        anyhow::ensure!(
            rc == 0 && len == std::mem::size_of::<u64>() && total > 0,
            "sysctl hw.memsize failed: {}",
            std::io::Error::last_os_error()
        );
        Ok(total)
    }
}

/// (available, total) bytes of the filesystem holding `path`, via statvfs on the
/// nearest existing ancestor (the models dir may not exist before the first download —
/// its future volume is still the ancestor's). `f_frsize` is the unit statvfs reports
/// blocks in (verified against `df` on macOS); `f_bavail` is what an unprivileged
/// writer can actually use.
#[cfg(unix)]
// The statvfs field widths differ across unix targets, so the casts are load-bearing
// on some and "unnecessary" on others.
#[allow(clippy::unnecessary_cast)]
pub fn disk_space(path: &Path) -> anyhow::Result<(u64, u64)> {
    use std::os::unix::ffi::OsStrExt;
    let target = path
        .ancestors()
        .find(|p| p.exists())
        .ok_or_else(|| anyhow::anyhow!("no existing ancestor for {}", path.display()))?;
    let c_path = std::ffi::CString::new(target.as_os_str().as_bytes())
        .map_err(|_| anyhow::anyhow!("path {} contains a NUL byte", target.display()))?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) };
    anyhow::ensure!(
        rc == 0,
        "statvfs({}) failed: {}",
        target.display(),
        std::io::Error::last_os_error()
    );
    let frsize = stat.f_frsize as u64;
    Ok((stat.f_bavail as u64 * frsize, stat.f_blocks as u64 * frsize))
}

#[cfg(not(unix))]
pub fn disk_space(path: &Path) -> anyhow::Result<(u64, u64)> {
    anyhow::bail!(
        "disk space measurement for {} is unix-only in this build",
        path.display()
    )
}

pub fn dir_size_bytes(path: &Path) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                stack.push(entry.path());
            } else if meta.is_file() {
                total += meta.len();
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOTAL: u64 = 96 * GIB;

    #[test]
    fn blocks_when_model_exceeds_available() {
        let g = MemoryGate::default();
        assert_eq!(
            g.evaluate(30 * GIB, 20 * GIB, TOTAL).verdict,
            Verdict::Block
        );
    }

    #[test]
    fn blocks_when_fit_would_eat_the_floor() {
        let g = MemoryGate::default();
        assert_eq!(
            g.evaluate(12 * GIB, 20 * GIB, TOTAL).verdict,
            Verdict::Block
        );
    }

    #[test]
    fn warns_in_the_thin_band() {
        let g = MemoryGate::default();
        assert_eq!(g.evaluate(8 * GIB, 20 * GIB, TOTAL).verdict, Verdict::Warn);
    }

    #[test]
    fn allows_with_headroom() {
        let g = MemoryGate::default();
        assert_eq!(g.evaluate(6 * GIB, 40 * GIB, TOTAL).verdict, Verdict::Allow);
    }

    #[test]
    fn floor_scales_with_total_on_big_machines() {
        let g = MemoryGate::default();
        assert_eq!(g.evaluate(GIB, 12 * GIB, 512 * GIB).verdict, Verdict::Block);
    }

    #[test]
    fn measure_returns_plausible_numbers() {
        let reading = measure().unwrap();
        assert!(reading.total_bytes > 4 * GIB);
        assert!(reading.available_bytes > 0 && reading.available_bytes < reading.total_bytes);
        if let Some(cache) = reading.reclaimable_cache_bytes {
            assert!(cache <= reading.available_bytes);
        }
    }

    const PAGE_16K: u64 = 16 * 1024;
    const M4_MAX_TOTAL: u64 = 128 * GIB;
    /// Qwen3.8-27B-Atlassian-Q8-mlx on disk: 31,989,932 KiB (`du -sk`, 2026-09-23).
    const QWEN_27B_Q8: u64 = 31_989_932 * 1024;

    /// The 2026-09-23 defect on the M4 Max 128 GB: the page read "0.0 GB free" and later
    /// "19.2 GB free" while vm_stat showed free 2,272,001 + speculative 1,117,292 with
    /// 1,610,894 file-backed pages (16 KiB) and memory_pressure said 88% free — a 105 GB
    /// download and a 31 GB rsync had filled the file cache. Purgeable was not recorded; it
    /// is taken as 0, which only understates what is available.
    #[test]
    fn the_recorded_file_cache_case_flips_the_gate_from_block_to_allow() {
        let g = MemoryGate::default();
        for sysinfo_read in [0, (19.2 * GIB as f64) as u64] {
            assert_eq!(
                g.evaluate(QWEN_27B_Q8, sysinfo_read, M4_MAX_TOTAL).verdict,
                Verdict::Block,
                "the old sysinfo reading {sysinfo_read} refused a mount that fits"
            );
        }
        let reading = darwin_reading(
            VmPageCounts {
                free: 2_272_001 + 1_117_292,
                speculative: 1_117_292,
                external: 1_610_894,
                purgeable: 0,
            },
            PAGE_16K,
            M4_MAX_TOTAL,
        );
        assert_eq!(reading.available_bytes, (2_272_001 + 1_610_894) * PAGE_16K);
        assert_eq!(reading.reclaimable_cache_bytes, Some(1_610_894 * PAGE_16K));
        let gate = g.evaluate(QWEN_27B_Q8, reading.available_bytes, M4_MAX_TOTAL);
        assert_eq!(gate.verdict, Verdict::Allow, "{}", gate.message);
    }

    #[test]
    fn file_cache_is_available_and_speculative_is_not_counted_twice() {
        let base = VmPageCounts {
            free: 1_000,
            speculative: 200,
            external: 500,
            purgeable: 50,
        };
        let r = darwin_reading(base, PAGE_16K, M4_MAX_TOTAL);
        assert_eq!(r.available_bytes, (800 + 500 + 50) * PAGE_16K);
        assert_eq!(r.reclaimable_cache_bytes, Some(550 * PAGE_16K));
        let cached = darwin_reading(
            VmPageCounts {
                free: 700,
                external: 800,
                ..base
            },
            PAGE_16K,
            M4_MAX_TOTAL,
        );
        assert_eq!(
            cached.available_bytes, r.available_bytes,
            "300 free pages turning into file cache must not move available"
        );
    }
}
