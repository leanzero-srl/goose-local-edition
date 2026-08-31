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
    pub fn evaluate(&self, model_bytes: u64, available_bytes: u64, total_bytes: u64) -> GateResult {
        let floor = self
            .floor_min_bytes
            .max((total_bytes as f64 * self.floor_fraction) as u64);
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

/// (available, total) physical memory right now.
pub fn measure() -> (u64, u64) {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    (sys.available_memory(), sys.total_memory())
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
        let (available, total) = measure();
        assert!(total > 4 * GIB);
        assert!(available > 0 && available < total);
    }
}
