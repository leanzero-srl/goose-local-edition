//! What this Mac's memory and disk hold right now. The fit rule that judges a mount against
//! these figures is `crate::fit` — the one rule every mount, plan and preflight shares.
use std::path::Path;

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

/// Page counts in `host_statistics64(HOST_VM_INFO64)`'s shape, as `vm_stat` prints them and as
/// `darwin::measure` rebuilds them from the live `vm.page_*` counters.
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
    /// `inactive_count` ("Pages inactive" in vm_stat): anonymous AND file pages the kernel has
    /// aged out of use. `None` where the source did not report it (an older or odd vm_stat, or a
    /// failed `host_statistics64`): the reading then counts only free + file cache + purgeable.
    pub(crate) inactive: Option<u64>,
}

/// THE one macOS "available" rule. Every reader of this Mac's or a node's memory goes through it:
/// the single-engine mount gate (`measure`), the distributed preflight and split planner, the
/// discovery probe, compaction, and the supervisor's admission/hold and CRITICAL stop (all via
/// `probe::parse_vm_stat`). There is no second copy of it anywhere.
///
/// Available = the larger of two lower bounds on what a new allocation can take without pushing a
/// page in use out:
/// - the file-cache figure: truly-free + file-backed + purgeable pages (what Activity Monitor
///   draws as physical memory minus App Memory, Wired and Compressed);
/// - the aged-out figure: free (incl. speculative) + INACTIVE + purgeable pages.
/// Their true union (free + file cache + inactive ANONYMOUS + purgeable) cannot be built from the
/// counters (inactive mixes anonymous and file pages), so the larger of the two is taken: never
/// more than the union, and never counting a page twice.
///
/// Why inactive counts (2026-10-31): leaving inactive anonymous pages out made the plan read the
/// workhorse at ~69-70 GiB with 66.6 GB free + 12.2 GB inactive, `memory_pressure` 85% free and
/// the kernel NORMAL, so the MiMo split 18/30 at 262,144 (rank 0 ~75.9 GiB with its 5% reserve)
/// was refused for a 17/31 that put rank 1 over its Metal limit; the same miscount made the hold
/// flap 56 times at 09:18 (free 7,878 pages, inactive 562,027). Inactive anonymous pages are not
/// freed outright: the kernel COMPRESSES (or swaps) them as a wired allocation grows over them.
/// The margin for that is kept OUTSIDE this figure, never by shrinking it: the plan's reserve
/// (RAM x `plan_reserve_ratio`, floor = the watchdog WARN reserve, 4.8 GiB on 96 GiB) stays on
/// top of every plan, and the kernel's own pressure level (`kern.memorystatus_vm_pressure_level`)
/// stays the guard of last resort: preflight fails a node at CRITICAL and warns at WARN, and the
/// serving hold stops on CRITICAL whatever this figure says.
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
    let file_cache = truly_free
        .saturating_add(counts.external)
        .saturating_add(counts.purgeable);
    let aged_out = counts.inactive.map_or(0, |inactive| {
        counts
            .free
            .saturating_add(inactive)
            .saturating_add(counts.purgeable)
    });
    let available = file_cache
        .max(aged_out)
        .saturating_mul(page_size)
        .min(total_bytes);
    MemoryReading {
        available_bytes: available,
        total_bytes,
        reclaimable_cache_bytes: Some(
            available.saturating_sub(truly_free.saturating_mul(page_size)),
        ),
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
    use std::ffi::CStr;

    /// The page counts, read live from the kernel's `vm.page_*` counters — never through
    /// `host_statistics64`, which the kernel rate-limits for every caller that is not an Apple
    /// binary: past a random 2–10 calls in a one-second window, machine-wide, it answers
    /// KERN_SUCCESS with a cached copy (xnu `rate_limit_host_statistics`). Measured 2026-09-29 on
    /// the M4 Max: two processes polling every 50 ms read the same frozen copy for ~1 s at a time,
    /// and a loop of 400,000 calls got 2 live answers while goosed polled beside it; under a 4 GiB
    /// churn the frozen copy sat 1.3 GB off `vm_stat`'s live figure, and the parity test failed 27
    /// of 50 runs with the same value before and after. These counters are what host_statistics64
    /// copies: on its live answers `free_count` was `vm.page_free_count + vm.page_speculative_count`
    /// and speculative, external and purgeable were equal to the page.
    pub fn measure() -> anyhow::Result<MemoryReading> {
        let free = page_count(c"vm.page_free_count")?;
        let speculative = page_count(c"vm.page_speculative_count")?;
        let counts = VmPageCounts {
            free: free.saturating_add(speculative),
            speculative,
            external: page_count(c"vm.page_pageable_external_count")?,
            purgeable: page_count(c"vm.page_purgeable_count")?,
            inactive: inactive_count(),
        };
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        anyhow::ensure!(
            page_size > 0,
            "sysconf(_SC_PAGESIZE) failed: {}",
            std::io::Error::last_os_error()
        );
        Ok(darwin_reading(counts, page_size as u64, total_bytes()?))
    }

    extern "C" {
        fn mach_host_self() -> libc::mach_port_t;
    }

    /// One host send right for the process: every `mach_host_self()` call adds a user reference,
    /// and the status poll would otherwise leak one per read.
    fn host_port() -> libc::mach_port_t {
        static HOST: std::sync::OnceLock<libc::mach_port_t> = std::sync::OnceLock::new();
        *HOST.get_or_init(|| unsafe { mach_host_self() })
    }

    /// The inactive page count. No `vm.page_*` sysctl carries it (checked 2026-10-31 on macOS 27:
    /// `sysctl -a` has no inactive/active page counter), so it alone comes from
    /// `host_statistics64(HOST_VM_INFO64)`. That call is rate-limited (see `measure`): past the
    /// limit it answers with the copy from at most ~1 s earlier, so this one term may lag by that
    /// much while free, file cache and purgeable stay live. `None` when the call fails: the
    /// reading then keeps the file-cache figure alone.
    fn inactive_count() -> Option<u64> {
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
        (rc == libc::KERN_SUCCESS).then(|| u64::from(stat.inactive_count))
    }

    /// A `vm.page_*` counter: 4 bytes for most, 8 for `vm.page_purgeable_count` (measured).
    fn page_count(name: &CStr) -> anyhow::Result<u64> {
        let mut value = [0u8; 8];
        let mut len = value.len();
        let rc = unsafe {
            libc::sysctlbyname(
                name.as_ptr(),
                value.as_mut_ptr() as *mut libc::c_void,
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        anyhow::ensure!(
            rc == 0,
            "sysctl {} failed: {}",
            name.to_string_lossy(),
            std::io::Error::last_os_error()
        );
        match len {
            4 => Ok(u64::from(u32::from_ne_bytes([
                value[0], value[1], value[2], value[3],
            ]))),
            8 => Ok(u64::from_ne_bytes(value)),
            other => anyhow::bail!(
                "sysctl {} answered {other} bytes, not a 4- or 8-byte page count",
                name.to_string_lossy()
            ),
        }
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
    use crate::fit::{judge, Need, NodeMemoryFacts, Verdict, GIB};

    #[test]
    fn measure_returns_plausible_numbers() {
        let reading = measure().unwrap();
        assert!(reading.total_bytes > 4 * GIB);
        assert!(reading.available_bytes > 0 && reading.available_bytes < reading.total_bytes);
        if let Some(cache) = reading.reclaimable_cache_bytes {
            assert!(cache <= reading.available_bytes);
        }
    }

    /// A frozen copy is byte-identical from call to call; a live count moves when a GiB of this
    /// process's own memory is touched between two reads. Ten reads first spend the most calls the
    /// kernel's rate limiter ever lets through in one window, so a reader built on
    /// `host_statistics64` answers both of the last two from its cache.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_reading_is_live_never_the_kernels_rate_limited_copy() {
        for _ in 0..10 {
            measure().unwrap();
        }
        let before = measure().unwrap();
        let touched = vec![1u8; GIB as usize];
        let after = measure().unwrap();
        std::hint::black_box(&touched);
        assert_ne!(
            before, after,
            "touching 1 GiB left every count unchanged: the reading is a cached copy"
        );
    }

    const PAGE_16K: u64 = 16 * 1024;
    const M4_MAX_TOTAL: u64 = 128 * GIB;
    const M4_MAX_CEILING: u64 = 115_448_725_504;
    /// Qwen3.8-27B-Atlassian-Q8-mlx on disk: 31,989,932 KiB (`du -sk`, 2026-09-23).
    const QWEN_27B_Q8: u64 = 31_989_932 * 1024;

    /// The 2026-09-23 defect on the M4 Max 128 GB: the page read "0.0 GB free" and later
    /// "19.2 GB free" while vm_stat showed free 2,272,001 + speculative 1,117,292 with
    /// 1,610,894 file-backed pages (16 KiB) and memory_pressure said 88% free — a 105 GB
    /// download and a 31 GB rsync had filled the file cache. Purgeable was not recorded; it
    /// is taken as 0, which only understates what is available.
    #[test]
    fn the_recorded_file_cache_case_flips_the_gate_from_block_to_allow() {
        let fit = |available_bytes| {
            judge(
                Need::single_engine(QWEN_27B_Q8, Ok(0), 0),
                NodeMemoryFacts {
                    available_bytes,
                    total_bytes: M4_MAX_TOTAL,
                    ceiling_bytes: M4_MAX_CEILING,
                    other_engines_bytes: 0,
                    freed_by_switch_bytes: 0,
                },
            )
        };
        for sysinfo_read in [0, (19.2 * GIB as f64) as u64] {
            assert_eq!(
                fit(sysinfo_read).verdict,
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
                inactive: None,
            },
            PAGE_16K,
            M4_MAX_TOTAL,
        );
        assert_eq!(reading.available_bytes, (2_272_001 + 1_610_894) * PAGE_16K);
        assert_eq!(reading.reclaimable_cache_bytes, Some(1_610_894 * PAGE_16K));
        let gate = fit(reading.available_bytes);
        assert_eq!(gate.verdict, Verdict::Allow, "{}", gate.message);
    }

    #[test]
    fn file_cache_is_available_and_speculative_is_not_counted_twice() {
        let base = VmPageCounts {
            free: 1_000,
            speculative: 200,
            external: 500,
            purgeable: 50,
            inactive: None,
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
