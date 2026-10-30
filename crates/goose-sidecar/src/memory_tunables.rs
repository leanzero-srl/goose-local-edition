//! The engine's memory tunables, set per machine (`mlx_engine.memory` in this Mac's goose config):
//! every number the Rapid-MLX memory planner uses (fork lz/metal-tunables, v0.14.3-lz.20, contract
//! in the fork's INTERFACE.md), each with its own engine flag. Unset = the engine's own default
//! (lz.19 behaviour), except the free-memory floor, whose goose default is derived per machine
//! ([`default_memory_floor_gb`]).

use serde::{Deserialize, Serialize};

use crate::engine_profile;

/// Bytes in the GB every memory tunable is entered in (the engine's `--metal-limit-gb` unit).
pub const GB: f64 = 1e9;
/// Bytes in the MB `--cache-memory-mb` takes (Rapid-MLX memory_cache.py multiplies by 1024 x 1024).
pub const ENGINE_MB: f64 = 1024.0 * 1024.0;

/// The goose default free-memory floor as a share of physical RAM. Not the engine's 8 % soft
/// reserve: the engine counts as "others" the memory the GPU driver wires for it plus file-cache side
/// effects (~24.5 GB measured on the workmule's 128 GiB), so an 8 % floor (11 GB) would cap that Mac
/// at 137.4 - 24.5 - 11 = 101.9 GB, below the 103.26 GB the lz.18 Gauntlet peaked at with ~9.7 GB
/// free and swap flat (1.36 GB). 5 % keeps that headroom: 128 GiB -> 7 GB.
pub const DEFAULT_FLOOR_SHARE: f64 = 0.05; // measured: workmule others 24.5 GB vs the lz.18 Gauntlet peak 103.26 GB (2026-10-30)

/// The engine's defaults for the fractions goose validates against when the user leaves one side
/// of a pair unset (INTERFACE.md receipts: #2858 for the auto plan, RC10 for the reserve).
pub const ENGINE_AUTO_FLOOR: f64 = engine_profile::ENGINE_CAP_MIN_SHARE;
pub const ENGINE_AUTO_CEILING: f64 = engine_profile::ENGINE_CAP_MAX_SHARE;
pub const ENGINE_AUTO_HEADROOM: f64 = engine_profile::ENGINE_CAP_HEADROOM_SHARE;
pub const ENGINE_SYSTEM_RESERVE: f64 = 0.08; // ratio: Rapid-MLX system_reserve_fraction default (RC10, INTERFACE.md)

/// Valid ranges, the same the engine enforces at start (INTERFACE.md "Unit / range").
pub const AUTO_FLOOR_RANGE: (f64, f64) = (0.1, 1.0);
pub const AUTO_CEILING_RANGE: (f64, f64) = (0.1, 1.0);
pub const AUTO_HEADROOM_RANGE: (f64, f64) = (0.0, 1.0);
pub const SYSTEM_RESERVE_RANGE: (f64, f64) = (0.0, 0.5);

/// Every engine memory number a person may set. `None` everywhere = the engine decides
/// (today's behaviour), except `memory_floor_gb` (see [`MemoryTunables::effective_floor_gb`]).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryTunables {
    /// `--metal-limit-gb`: the engine's Metal cap in GB; `None` = auto (the context-aware plan).
    pub metal_limit_gb: Option<f64>,
    /// `--memory-floor-gb`: memory the engine always leaves free for macOS and other apps.
    pub memory_floor_gb: Option<f64>,
    /// `--cache-memory-mb`: the prefix-cache budget, entered in GB.
    pub prefix_cache_gb: Option<f64>,
    /// `--prefill-step-size`: the largest chunk one prefill step takes, in tokens.
    pub prefill_step_size: Option<u32>,
    /// `--max-model-len`: the context window the engine admits, in tokens. Wins over the mounted
    /// model's profile `context_limit`.
    pub max_context_tokens: Option<u32>,
    /// `--memory-guard-margin-gb`: the guard's safety margin under its ceiling.
    pub guard_margin_gb: Option<f64>,
    /// `--max-concurrent-requests`: the engine's admission cap (goose's default is
    /// `engine::MAX_CONCURRENT_REQUESTS`).
    pub max_concurrent_requests: Option<u32>,
    /// `--auto-utilization-floor`: the auto cap's lowest share of the Metal working set.
    pub auto_utilization_floor: Option<f64>,
    /// `--auto-utilization-ceiling`: the auto cap's highest share of the Metal working set.
    pub auto_utilization_ceiling: Option<f64>,
    /// `--auto-headroom-fraction`: the auto plan's headroom over the weights.
    pub auto_headroom_fraction: Option<f64>,
    /// `--system-reserve-fraction`: the soft reserve of physical RAM optional memory leaves free.
    pub system_reserve_fraction: Option<f64>,
}

/// round(5 % x physical GB): 128 GiB -> 7, 96 GiB -> 5, 64 GiB -> 3.
pub fn default_memory_floor_gb(physical_bytes: u64) -> f64 {
    (DEFAULT_FLOOR_SHARE * physical_bytes as f64 / GB).round()
}

impl MemoryTunables {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// The floor the engine is given: the person's, else the per-machine default (needs the
    /// machine's physical RAM; `None` when it is unknown).
    pub fn effective_floor_gb(&self, physical_bytes: Option<u64>) -> Option<f64> {
        self.memory_floor_gb
            .or_else(|| physical_bytes.map(default_memory_floor_gb))
    }

    /// The engine flags these tunables add. `context_tokens` is the window to admit (the
    /// person's `max_context_tokens`, else the mounted model's configured context). `supports`
    /// says whether the launcher's engine knows the lz.20 flags; the three older flags
    /// (`--cache-memory-mb`, `--prefill-step-size`, and `--max-concurrent-requests`, which
    /// `engine::prefix_cache_flags` emits) work on every engine.
    pub fn engine_flags(
        &self,
        context_tokens: Option<u32>,
        physical_bytes: Option<u64>,
        supports: bool,
    ) -> Vec<String> {
        let mut argv = Vec::new();
        let mut push = |flag: &str, value: String| {
            argv.push(flag.to_string());
            argv.push(value);
        };
        if let Some(gb) = self.prefix_cache_gb {
            push(
                "--cache-memory-mb",
                ((gb * GB / ENGINE_MB).round() as u64).to_string(),
            );
        }
        if let Some(n) = self.prefill_step_size {
            push("--prefill-step-size", n.to_string());
        }
        if !supports {
            return argv;
        }
        if let Some(gb) = self.metal_limit_gb {
            push("--metal-limit-gb", fmt_num(gb));
        }
        if let Some(gb) = self.effective_floor_gb(physical_bytes) {
            push("--memory-floor-gb", fmt_num(gb));
        }
        if let Some(n) = context_tokens {
            push("--max-model-len", n.to_string());
        }
        if let Some(gb) = self.guard_margin_gb {
            push("--memory-guard-margin-gb", fmt_num(gb));
        }
        let fractions = [
            ("--auto-utilization-floor", self.auto_utilization_floor),
            ("--auto-utilization-ceiling", self.auto_utilization_ceiling),
            ("--auto-headroom-fraction", self.auto_headroom_fraction),
            ("--system-reserve-fraction", self.system_reserve_fraction),
        ];
        for (flag, value) in fractions {
            if let Some(v) = value {
                push(flag, fmt_num(v));
            }
        }
        argv
    }

    /// The tunables set that only an lz.20+ engine reads: an older engine would refuse the flag,
    /// so a mount on one fails with this list instead of silently ignoring them.
    pub fn needs_new_engine(&self) -> Vec<&'static str> {
        let mut set = Vec::new();
        let named = [
            ("metal_limit_gb", self.metal_limit_gb.is_some()),
            ("memory_floor_gb", self.memory_floor_gb.is_some()),
            ("max_context_tokens", self.max_context_tokens.is_some()),
            ("guard_margin_gb", self.guard_margin_gb.is_some()),
            (
                "auto_utilization_floor",
                self.auto_utilization_floor.is_some(),
            ),
            (
                "auto_utilization_ceiling",
                self.auto_utilization_ceiling.is_some(),
            ),
            (
                "auto_headroom_fraction",
                self.auto_headroom_fraction.is_some(),
            ),
            (
                "system_reserve_fraction",
                self.system_reserve_fraction.is_some(),
            ),
        ];
        for (name, is_set) in named {
            if is_set {
                set.push(name);
            }
        }
        set
    }

    /// Every impossible value or combination, each named by its tunable. `physical_bytes` is this
    /// Mac's RAM; `kv_bytes_per_token` is the running engine's own figure for the model the
    /// context would be admitted for (unknown while no engine runs).
    pub fn validate(
        &self,
        physical_bytes: Option<u64>,
        kv_bytes_per_token: Option<u64>,
    ) -> Result<(), String> {
        let mut problems: Vec<String> = Vec::new();
        let physical_gb = physical_bytes.map(|b| b as f64 / GB);
        let finite = |name: &str, v: Option<f64>, problems: &mut Vec<String>| {
            if let Some(v) = v {
                if !v.is_finite() {
                    problems.push(format!("{name} must be a number"));
                }
            }
        };
        for (name, v) in [
            ("metal_limit_gb", self.metal_limit_gb),
            ("memory_floor_gb", self.memory_floor_gb),
            ("prefix_cache_gb", self.prefix_cache_gb),
            ("guard_margin_gb", self.guard_margin_gb),
            ("auto_utilization_floor", self.auto_utilization_floor),
            ("auto_utilization_ceiling", self.auto_utilization_ceiling),
            ("auto_headroom_fraction", self.auto_headroom_fraction),
            ("system_reserve_fraction", self.system_reserve_fraction),
        ] {
            finite(name, v, &mut problems);
        }
        if !problems.is_empty() {
            return Err(problems.join("; "));
        }
        if let Some(cap) = self.metal_limit_gb {
            if cap <= 0.0 {
                problems.push("metal_limit_gb must be above 0 GB".into());
            }
            if let Some(p) = physical_gb {
                if cap > p {
                    problems.push(format!(
                        "metal_limit_gb {} GB is more than this Mac's {} GB of RAM",
                        fmt_num(cap),
                        fmt_gb(p)
                    ));
                }
            }
        }
        for (name, v) in [
            ("memory_floor_gb", self.memory_floor_gb),
            ("prefix_cache_gb", self.prefix_cache_gb),
            ("guard_margin_gb", self.guard_margin_gb),
        ] {
            if let Some(v) = v {
                if v < 0.0 {
                    problems.push(format!("{name} cannot be negative"));
                } else if let Some(p) = physical_gb {
                    if v >= p {
                        problems.push(format!(
                            "{name} {} GB is not below this Mac's {} GB of RAM",
                            fmt_num(v),
                            fmt_gb(p)
                        ));
                    }
                }
            }
        }
        for (name, v) in [
            ("prefill_step_size", self.prefill_step_size),
            ("max_context_tokens", self.max_context_tokens),
            ("max_concurrent_requests", self.max_concurrent_requests),
        ] {
            if v == Some(0) {
                problems.push(format!("{name} must be at least 1"));
            }
        }
        for (name, v, (lo, hi)) in [
            (
                "auto_utilization_floor",
                self.auto_utilization_floor,
                AUTO_FLOOR_RANGE,
            ),
            (
                "auto_utilization_ceiling",
                self.auto_utilization_ceiling,
                AUTO_CEILING_RANGE,
            ),
            (
                "auto_headroom_fraction",
                self.auto_headroom_fraction,
                AUTO_HEADROOM_RANGE,
            ),
            (
                "system_reserve_fraction",
                self.system_reserve_fraction,
                SYSTEM_RESERVE_RANGE,
            ),
        ] {
            if let Some(v) = v {
                if v < lo || v > hi {
                    problems.push(format!(
                        "{name} {} is outside {}..{}",
                        fmt_num(v),
                        fmt_num(lo),
                        fmt_num(hi)
                    ));
                }
            }
        }
        if self.auto_utilization_floor.is_some() || self.auto_utilization_ceiling.is_some() {
            let floor = self.auto_utilization_floor.unwrap_or(ENGINE_AUTO_FLOOR);
            let ceiling = self.auto_utilization_ceiling.unwrap_or(ENGINE_AUTO_CEILING);
            if floor > ceiling {
                problems.push(format!(
                    "auto_utilization_floor {} is above auto_utilization_ceiling {}",
                    fmt_num(floor),
                    fmt_num(ceiling)
                ));
            }
        }
        if let (Some(cap), Some(p)) = (self.metal_limit_gb, physical_gb) {
            let floor = self.memory_floor_gb.unwrap_or(0.0);
            if cap > 0.0 && cap <= p && cap + floor > p {
                problems.push(format!(
                    "metal_limit_gb {} GB + memory_floor_gb {} GB is more than this Mac's {} GB of RAM",
                    fmt_num(cap),
                    fmt_num(floor),
                    fmt_gb(p)
                ));
            }
        }
        if let (Some(cap), Some(tokens), Some(per_token)) = (
            self.metal_limit_gb,
            self.max_context_tokens,
            kv_bytes_per_token,
        ) {
            let kv_gb = tokens as f64 * per_token as f64 / GB;
            if kv_gb > cap {
                problems.push(format!(
                    "max_context_tokens {tokens}: its KV cache alone is {} GB, more than the {} GB metal_limit_gb",
                    fmt_gb(kv_gb),
                    fmt_num(cap)
                ));
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems.join("; "))
        }
    }
}

/// This Mac's (physical, available) bytes, read while no engine runs (hw.memsize and the live
/// page counters `host_statistics64` copies, `memory::measure`).
pub fn machine_memory() -> anyhow::Result<(u64, u64)> {
    let reading = crate::memory::measure()?;
    Ok((reading.total_bytes, reading.available_bytes))
}

/// The engine's `/v1/status` `memory_budget` block (fork INTERFACE.md): what it planned under and
/// what it holds now. Every figure is `None` when the engine did not report it, never a guessed 0.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EngineMemoryBudget {
    pub physical_bytes: Option<u64>,
    pub device_working_set_bytes: Option<u64>,
    pub wired_limit_mb: Option<u64>,
    pub mode: Option<String>,
    pub source: Option<String>,
    pub resolved_utilization: Option<f64>,
    pub limit_bytes: Option<u64>,
    pub binding: Option<String>,
    pub weights_bytes: Option<u64>,
    pub plan_context_tokens: Option<u64>,
    pub kv_bytes_per_token: Option<u64>,
    pub plan_need_bytes: Option<u64>,
    pub prefix_budget_bytes: Option<u64>,
    pub memory_floor_bytes: Option<u64>,
    pub others_bytes: Option<u64>,
    pub free_bytes: Option<u64>,
    pub effective_ceiling_bytes: Option<u64>,
    pub active_bytes: Option<u64>,
    pub peak_bytes: Option<u64>,
    pub max_context_at_ceiling: Option<u64>,
    pub plan_limit_bytes: Option<u64>,
    pub step_binding: Option<String>,
    pub used_bytes: Option<u64>,
    pub cache_bytes: Option<u64>,
    pub plan_sequence_need_bytes: Option<u64>,
    /// What the floor counted (lz.20 `memory_budget.machine`): `None` on an engine without it.
    pub machine: Option<EngineMachineMemory>,
    pub tunables: std::collections::BTreeMap<String, serde_json::Value>,
}

/// `memory_budget.machine`: the free-memory floor's input. Used = wired + anonymous - purgeable +
/// compressor; file-backed pages (the file cache, incl. the engine's own memory-mapped files) are
/// left out because macOS frees them on demand. Of the others, `others_engine_non_metal_bytes` is
/// the engine's own memory outside Metal (it really holds it, so it stays counted).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EngineMachineMemory {
    pub wired_bytes: Option<u64>,
    pub anonymous_bytes: Option<u64>,
    pub compressor_bytes: Option<u64>,
    pub purgeable_excluded_bytes: Option<u64>,
    pub file_backed_excluded_bytes: Option<u64>,
    pub free_bytes: Option<u64>,
    pub engine_metal_bytes: Option<u64>,
    pub engine_resident_bytes: Option<u64>,
    pub others_engine_non_metal_bytes: Option<u64>,
    pub others_rest_bytes: Option<u64>,
}

/// The `memory_budget` block of a `/v1/status` body: `Ok(None)` when the engine has none (an
/// engine before lz.20), `Err` when it has one goose cannot read.
pub fn parse_memory_budget(body: &str) -> anyhow::Result<Option<EngineMemoryBudget>> {
    let parsed: serde_json::Value = serde_json::from_str(body)?;
    match parsed.get("memory_budget") {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(block) => Ok(Some(serde_json::from_value(block.clone()).map_err(
            |e| anyhow::anyhow!("/v1/status memory_budget is unreadable: {e}"),
        )?)),
    }
}

/// A float the way an engine flag reads it: no trailing `.0` on whole numbers.
fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

fn fmt_gb(v: f64) -> String {
    format!("{v:.1}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn the_status_memory_budget_parses_and_its_absence_is_none() {
        let body = serde_json::json!({
            "num_running": 0,
            "memory_budget": {
                "physical_bytes": 103079215104u64,
                "device_working_set_bytes": 77309411328u64,
                "wired_limit_mb": 0,
                "mode": "auto",
                "source": "auto-context",
                "binding": "engine",
                "limit_bytes": 70000000000u64,
                "kv_bytes_per_token": 65536,
                "active_bytes": null,
                "machine": {"file_backed_excluded_bytes": 19500000000u64,
                            "others_engine_non_metal_bytes": 12000000000u64},
                "tunables": {"metal_limit_gb": null, "memory_floor_gb": 8}
            }
        })
        .to_string();
        let b = parse_memory_budget(&body).unwrap().unwrap();
        assert_eq!(b.kv_bytes_per_token, Some(65536));
        assert_eq!(b.binding.as_deref(), Some("engine"));
        assert_eq!(b.active_bytes, None);
        assert_eq!(b.tunables["memory_floor_gb"], 8);
        let m = b.machine.as_ref().unwrap();
        assert_eq!(m.file_backed_excluded_bytes, Some(19_500_000_000));
        assert_eq!(m.others_engine_non_metal_bytes, Some(12_000_000_000));
        assert_eq!(m.wired_bytes, None);
        assert_eq!(parse_memory_budget(r#"{"num_running":0}"#).unwrap(), None);
        assert!(parse_memory_budget(r#"{"memory_budget":{"limit_bytes":"x"}}"#).is_err());
    }

    #[test]
    fn the_default_floor_is_five_percent_of_physical_gb() {
        assert_eq!(default_memory_floor_gb(128 * GIB), 7.0);
        assert_eq!(default_memory_floor_gb(96 * GIB), 5.0);
        assert_eq!(default_memory_floor_gb(64 * GIB), 3.0);
    }

    #[test]
    fn an_unset_floor_follows_the_machine_and_a_set_one_wins() {
        let t = MemoryTunables::default();
        assert_eq!(t.effective_floor_gb(Some(128 * GIB)), Some(7.0));
        assert_eq!(t.effective_floor_gb(None), None);
        let t = MemoryTunables {
            memory_floor_gb: Some(4.0),
            ..Default::default()
        };
        assert_eq!(t.effective_floor_gb(Some(128 * GIB)), Some(4.0));
    }

    fn flags(t: &MemoryTunables) -> Vec<String> {
        t.engine_flags(t.max_context_tokens, Some(96 * GIB), true)
    }

    fn value_of(argv: &[String], flag: &str) -> Option<String> {
        argv.iter()
            .position(|a| a == flag)
            .map(|i| argv[i + 1].clone())
    }

    #[test]
    fn each_tunable_reaches_its_flag() {
        let cases: Vec<(MemoryTunables, &str, &str)> = vec![
            (
                MemoryTunables {
                    metal_limit_gb: Some(112.0),
                    ..Default::default()
                },
                "--metal-limit-gb",
                "112",
            ),
            (
                MemoryTunables {
                    memory_floor_gb: Some(4.5),
                    ..Default::default()
                },
                "--memory-floor-gb",
                "4.5",
            ),
            (
                MemoryTunables {
                    prefix_cache_gb: Some(10.0),
                    ..Default::default()
                },
                "--cache-memory-mb",
                "9537",
            ),
            (
                MemoryTunables {
                    prefill_step_size: Some(512),
                    ..Default::default()
                },
                "--prefill-step-size",
                "512",
            ),
            (
                MemoryTunables {
                    max_context_tokens: Some(131072),
                    ..Default::default()
                },
                "--max-model-len",
                "131072",
            ),
            (
                MemoryTunables {
                    guard_margin_gb: Some(2.0),
                    ..Default::default()
                },
                "--memory-guard-margin-gb",
                "2",
            ),
            (
                MemoryTunables {
                    auto_utilization_floor: Some(0.85),
                    ..Default::default()
                },
                "--auto-utilization-floor",
                "0.85",
            ),
            (
                MemoryTunables {
                    auto_utilization_ceiling: Some(0.95),
                    ..Default::default()
                },
                "--auto-utilization-ceiling",
                "0.95",
            ),
            (
                MemoryTunables {
                    auto_headroom_fraction: Some(0.1),
                    ..Default::default()
                },
                "--auto-headroom-fraction",
                "0.1",
            ),
            (
                MemoryTunables {
                    system_reserve_fraction: Some(0.05),
                    ..Default::default()
                },
                "--system-reserve-fraction",
                "0.05",
            ),
        ];
        for (t, flag, want) in cases {
            let argv = flags(&t);
            assert_eq!(value_of(&argv, flag).as_deref(), Some(want), "{argv:?}");
        }
    }

    #[test]
    fn unset_tunables_send_only_the_machine_floor() {
        let argv = flags(&MemoryTunables::default());
        assert_eq!(argv, vec!["--memory-floor-gb".to_string(), "5".to_string()]);
    }

    #[test]
    fn an_older_engine_gets_only_the_flags_it_knows() {
        let t = MemoryTunables {
            metal_limit_gb: Some(80.0),
            prefix_cache_gb: Some(1.048576),
            prefill_step_size: Some(1024),
            max_context_tokens: Some(4096),
            ..Default::default()
        };
        let argv = t.engine_flags(t.max_context_tokens, Some(96 * GIB), false);
        assert_eq!(
            argv,
            vec!["--cache-memory-mb", "1000", "--prefill-step-size", "1024"]
        );
        assert_eq!(
            t.needs_new_engine(),
            vec!["metal_limit_gb", "max_context_tokens"]
        );
    }

    #[test]
    fn impossible_combinations_are_refused_by_name() {
        let physical = Some(128 * GIB);
        let cap_floor = MemoryTunables {
            metal_limit_gb: Some(130.0),
            memory_floor_gb: Some(11.0),
            ..Default::default()
        };
        let e = cap_floor.validate(physical, None).unwrap_err();
        assert!(
            e.contains("metal_limit_gb 130 GB + memory_floor_gb 11 GB"),
            "{e}"
        );

        let fractions = MemoryTunables {
            auto_utilization_floor: Some(0.98),
            ..Default::default()
        };
        let e = fractions.validate(physical, None).unwrap_err();
        assert!(
            e.contains("auto_utilization_floor 0.98 is above auto_utilization_ceiling 0.97"),
            "{e}"
        );

        let kv = MemoryTunables {
            metal_limit_gb: Some(40.0),
            max_context_tokens: Some(1_000_000),
            ..Default::default()
        };
        assert!(
            kv.validate(physical, None).is_ok(),
            "unknown KV/token: the engine judges"
        );
        let e = kv.validate(physical, Some(65_536)).unwrap_err();
        assert!(e.contains("max_context_tokens 1000000"), "{e}");

        let ranges = MemoryTunables {
            metal_limit_gb: Some(0.0),
            system_reserve_fraction: Some(0.6),
            max_concurrent_requests: Some(0),
            memory_floor_gb: Some(-1.0),
            ..Default::default()
        };
        let e = ranges.validate(physical, None).unwrap_err();
        for part in [
            "metal_limit_gb must be above 0",
            "system_reserve_fraction 0.6 is outside 0..0.5",
            "max_concurrent_requests must be at least 1",
            "memory_floor_gb cannot be negative",
        ] {
            assert!(e.contains(part), "{part} in {e}");
        }

        let nan = MemoryTunables {
            metal_limit_gb: Some(f64::NAN),
            ..Default::default()
        };
        assert!(nan.validate(physical, None).is_err());

        let fine = MemoryTunables {
            metal_limit_gb: Some(112.0),
            memory_floor_gb: Some(4.0),
            max_context_tokens: Some(262_144),
            ..Default::default()
        };
        assert_eq!(fine.validate(physical, Some(65_536)), Ok(()));
    }
}
