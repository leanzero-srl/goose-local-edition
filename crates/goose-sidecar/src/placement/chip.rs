//! Which Apple chip a node is, and the memory bandwidth Apple publishes for it.
//!
//! Decode speed on Apple Silicon is memory-bandwidth-bound, so the planner needs each node's
//! bandwidth. It comes from Apple's own tech-spec pages (fetched 2026-09-24, cited per row), keyed
//! by the chip's brand string and — where Apple sells one chip with two bandwidths — its GPU core
//! count. A chip the table does not carry is a NAMED gap, never a neighbour's figure.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

/// Prints `hw.model`, the chip's brand string and the GPU's core count (IOKit, no Python, ~20 ms;
/// `system_profiler SPDisplaysDataType` gives the same count in ~400 ms). Section body of `@@chip`.
pub const CHIP_PROBE_SCRIPT: &str = "/usr/sbin/sysctl -n hw.model machdep.cpu.brand_string; /usr/sbin/ioreg -rc AGXAccelerator -d1 | /usr/bin/grep '\"gpu-core-count\"'";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChipIdentity {
    /// `hw.model` (Mac15,14).
    pub hw_model: String,
    /// `machdep.cpu.brand_string` (Apple M3 Ultra).
    pub brand: String,
    /// IOKit's `gpu-core-count` on the AGX accelerator; `None` when IOKit did not print one.
    pub gpu_cores: Option<u32>,
}

impl ChipIdentity {
    pub fn label(&self) -> String {
        match self.gpu_cores {
            Some(cores) => format!("{} · {cores}-core GPU", self.brand),
            None => self.brand.clone(),
        }
    }
}

/// Parse the `@@chip` answer: `hw.model`, the brand string, then IOKit's `"gpu-core-count" = N`.
pub fn parse_chip(text: &str) -> Result<ChipIdentity> {
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    let hw_model = lines
        .next()
        .context("the chip probe printed nothing")?
        .to_string();
    let brand = lines
        .next()
        .context("the chip probe printed no brand string")?
        .to_string();
    if !brand.starts_with("Apple ") {
        bail!("the brand string {brand:?} is not an Apple chip");
    }
    let gpu_cores = lines
        .filter(|l| l.contains("\"gpu-core-count\""))
        .find_map(|l| l.rsplit('=').next()?.trim().parse::<u32>().ok());
    Ok(ChipIdentity {
        hw_model,
        brand,
        gpu_cores,
    })
}

/// One row of Apple's published memory bandwidth.
struct SpecRow {
    brand: &'static str,
    /// `Some` where Apple sells this chip with two bandwidths by GPU size.
    gpu_cores: Option<u32>,
    gb_per_s: f64,
    source: &'static str,
}

/// Apple's own tech-spec pages, fetched 2026-09-24 (curl, text extracted; every figure below was
/// read off the page named beside it). The base M1 is absent on purpose: its spec page lists no
/// bandwidth, so it stays a named gap.
const SPEC_TABLE: &[SpecRow] = &[
    SpecRow { brand: "Apple M1 Pro", gpu_cores: None, gb_per_s: 200.0, source: "support.apple.com/en-us/111902" },
    SpecRow { brand: "Apple M1 Max", gpu_cores: None, gb_per_s: 400.0, source: "support.apple.com/en-us/111900" },
    SpecRow { brand: "Apple M1 Ultra", gpu_cores: None, gb_per_s: 800.0, source: "support.apple.com/en-us/111900" },
    SpecRow { brand: "Apple M2", gpu_cores: None, gb_per_s: 100.0, source: "support.apple.com/en-us/111867" },
    SpecRow { brand: "Apple M2 Pro", gpu_cores: None, gb_per_s: 200.0, source: "support.apple.com/en-us/111340" },
    SpecRow { brand: "Apple M2 Max", gpu_cores: None, gb_per_s: 400.0, source: "support.apple.com/en-us/111835" },
    SpecRow { brand: "Apple M2 Ultra", gpu_cores: None, gb_per_s: 800.0, source: "support.apple.com/en-us/111835" },
    SpecRow { brand: "Apple M3", gpu_cores: None, gb_per_s: 100.0, source: "support.apple.com/en-us/117735" },
    SpecRow { brand: "Apple M3 Pro", gpu_cores: None, gb_per_s: 150.0, source: "support.apple.com/en-us/117737" },
    SpecRow { brand: "Apple M3 Max", gpu_cores: Some(30), gb_per_s: 300.0, source: "support.apple.com/en-us/117736" },
    SpecRow { brand: "Apple M3 Max", gpu_cores: Some(40), gb_per_s: 400.0, source: "support.apple.com/en-us/117736" },
    SpecRow { brand: "Apple M3 Ultra", gpu_cores: None, gb_per_s: 819.0, source: "support.apple.com/en-us/122211" },
    SpecRow { brand: "Apple M4", gpu_cores: None, gb_per_s: 120.0, source: "support.apple.com/en-us/121552" },
    SpecRow { brand: "Apple M4 Pro", gpu_cores: None, gb_per_s: 273.0, source: "support.apple.com/en-us/121553" },
    SpecRow { brand: "Apple M4 Max", gpu_cores: Some(32), gb_per_s: 410.0, source: "support.apple.com/en-us/121553" },
    SpecRow { brand: "Apple M4 Max", gpu_cores: Some(40), gb_per_s: 546.0, source: "support.apple.com/en-us/122211" },
    SpecRow { brand: "Apple M5", gpu_cores: None, gb_per_s: 153.0, source: "apple.com/macbook-pro/specs" },
    SpecRow { brand: "Apple M5 Pro", gpu_cores: None, gb_per_s: 307.0, source: "apple.com/macbook-pro/specs" },
    SpecRow { brand: "Apple M5 Max", gpu_cores: Some(32), gb_per_s: 460.0, source: "apple.com/macbook-pro/specs" },
    SpecRow { brand: "Apple M5 Max", gpu_cores: Some(40), gb_per_s: 614.0, source: "apple.com/macbook-pro/specs" },
    SpecRow { brand: "Apple M5 Ultra", gpu_cores: None, gb_per_s: 1200.0, source: "apple.com/mac-studio/specs (\"1.2TB/s\")" },
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bandwidth {
    pub gb_per_s: f64,
    pub source: String,
}

/// Apple's published bandwidth for `chip`, or why there is none.
pub fn spec_bandwidth(chip: &ChipIdentity) -> Result<Bandwidth, String> {
    let rows: Vec<&SpecRow> = SPEC_TABLE.iter().filter(|r| r.brand == chip.brand).collect();
    if rows.is_empty() {
        return Err(format!(
            "{} is not in goose's bandwidth table (Apple's spec pages, 2026-09-24)",
            chip.brand
        ));
    }
    let chosen = rows
        .iter()
        .find(|r| r.gpu_cores.is_none() || r.gpu_cores == chip.gpu_cores);
    match chosen {
        Some(row) => Ok(Bandwidth {
            gb_per_s: row.gb_per_s,
            source: row.source.to_string(),
        }),
        None => {
            let listed: Vec<String> = rows
                .iter()
                .filter_map(|r| r.gpu_cores.map(|c| format!("{c}-core {} GB/s", r.gb_per_s)))
                .collect();
            Err(format!(
                "{} is sold with more than one bandwidth ({}); this one's GPU core count is {}",
                chip.brand,
                listed.join(", "),
                chip.gpu_cores
                    .map(|c| format!("{c}, which Apple does not list"))
                    .unwrap_or_else(|| "unknown".to_string())
            ))
        }
    }
}

/// This Mac's chip, read with the same probe a peer answers.
pub async fn local_chip() -> Result<ChipIdentity> {
    let out = tokio::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(CHIP_PROBE_SCRIPT)
        .output()
        .await
        .context("running the chip probe")?;
    parse_chip(&String::from_utf8_lossy(&out.stdout))
}

/// This Mac's GPU ceiling: Metal's `recommendedMaxWorkingSetSize` — the figure MLX reads as
/// `max_recommended_working_set_size` (measured equal on the M4 Max: 115,448,725,504 both ways).
#[cfg(target_os = "macos")]
pub fn local_gpu_ceiling() -> Result<u64> {
    let device = metal::Device::system_default().context("Metal reports no GPU on this Mac")?;
    let bytes = device.recommended_max_working_set_size();
    anyhow::ensure!(bytes > 0, "Metal reported a working-set ceiling of 0 bytes");
    Ok(bytes)
}

#[cfg(not(target_os = "macos"))]
pub fn local_gpu_ceiling() -> Result<u64> {
    bail!("the GPU ceiling is read from Metal, which exists only on macOS")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both Macs' real answers (2026-09-24).
    const MACBOOK: &str = "Mac16,5\nApple M4 Max\n      \"gpu-core-count\" = 40\n";
    const WORKHORSE: &str = "Mac15,14\nApple M3 Ultra\n      \"gpu-core-count\" = 60\n";

    #[test]
    fn the_two_real_macs_parse_to_their_published_bandwidth() {
        let mb = parse_chip(MACBOOK).unwrap();
        assert_eq!(mb.brand, "Apple M4 Max");
        assert_eq!(mb.gpu_cores, Some(40));
        assert_eq!(spec_bandwidth(&mb).unwrap().gb_per_s, 546.0);
        let wh = parse_chip(WORKHORSE).unwrap();
        assert_eq!(wh.hw_model, "Mac15,14");
        assert_eq!(spec_bandwidth(&wh).unwrap().gb_per_s, 819.0);
        assert_eq!(wh.label(), "Apple M3 Ultra · 60-core GPU");
    }

    #[test]
    fn the_gpu_size_picks_between_two_bandwidths_and_an_unlisted_size_is_a_gap() {
        let small = parse_chip("Mac16,6\nApple M4 Max\n\"gpu-core-count\" = 32\n").unwrap();
        assert_eq!(spec_bandwidth(&small).unwrap().gb_per_s, 410.0);
        let odd = parse_chip("Mac16,6\nApple M4 Max\n\"gpu-core-count\" = 36\n").unwrap();
        let gap = spec_bandwidth(&odd).unwrap_err();
        assert!(gap.contains("36, which Apple does not list"), "{gap}");
        let unknown = parse_chip("Mac16,6\nApple M4 Max\n").unwrap();
        assert!(spec_bandwidth(&unknown).unwrap_err().contains("unknown"));
    }

    #[test]
    fn a_chip_the_table_lacks_is_named_never_a_neighbours_figure() {
        let m1 = parse_chip("MacBookAir10,1\nApple M1\n\"gpu-core-count\" = 8\n").unwrap();
        assert!(spec_bandwidth(&m1)
            .unwrap_err()
            .contains("Apple M1 is not in goose's bandwidth table"));
        assert!(parse_chip("").is_err());
        assert!(parse_chip("MacPro7,1\nIntel(R) Xeon(R) W-3245\n").is_err());
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn this_macs_chip_and_ceiling_are_readable() {
        let chip = local_chip().await.unwrap();
        assert!(chip.brand.starts_with("Apple M"), "{chip:?}");
        assert!(local_gpu_ceiling().unwrap() > 0);
    }
}
