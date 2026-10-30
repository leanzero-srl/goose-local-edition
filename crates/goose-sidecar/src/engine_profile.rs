//! How the single engine goose mounts runs a long prefill — the two facts the single-engine cost
//! model (`crate::fit::SingleEngineCost`) needs from the ENGINE rather than the model: the chunk a
//! prefill step takes at the context's limit, and the memory ceiling it plans that step under.
//!
//! They differ by engine, and the difference is the whole context: on Gabee's 64 GB M3 Max the 27B
//! Q8 at Rapid-MLX lz.10's 2,048-token chunk under the Metal working set holds ~57k tokens; the
//! memory-guard engine halves a step down to 256 tokens under its own 0.90 cap and holds ~152k.
//! Before a mount the profile is the PINNED engine's (`profile_of_launcher` over the launcher goose
//! will spawn). Ported from t12/local-bench-fidelity (d44f8a81b) for the guard pin: the release
//! uses it to decide the prefix-cache flags (`engine::prefix_cache_flags`); the cost model that
//! reads the chunk and the ceiling stays on that branch.

use crate::engine::ENGINE_LAUNCHER;

/// The chunk Rapid-MLX runs every prefill step at without a memory guard: `SchedulerConfig`
/// `prefill_step_size` (goose passes no `--prefill-step-size`). lz.10's adaptive prefill shrinks it
/// only past 70% of its Metal cap measured BEFORE the step, which did not stop Gabee's OOMs.
pub const UNGUARDED_PREFILL_CHUNK_TOKENS: u64 = 2048; // measured: rapid_mlx/scheduler.py:280 at v0.14.3-lz.10
/// The smallest chunk the memory guard halves a prefill step down to before it refuses: the
/// scheduler's `adaptive_prefill_min_chunk_size` (memory_guard.py floor_tokens). `/v1/status`
/// `memory_guard.floor_chunk_tokens` replaces it once the engine runs.
pub const GUARD_FLOOR_CHUNK_TOKENS: u64 = 256; // measured: rapid_mlx/scheduler.py:536, lz/memory-guard 605548ef6

// Rapid-MLX's auto Metal cap (memory_budget.py `plan_metal_limit`, AUTO mode — goose passes no
// --gpu-memory-utilization), the ceiling the memory guard plans every prefill step and its
// max_model_len under: the measured weights plus max(8% of them, 512 MiB) of headroom, as a share
// of Metal's recommended working set, clamped to [0.90, 0.97]. `/v1/status`
// `memory_guard.ceiling_bytes` replaces it once the engine runs.
const ENGINE_CAP_HEADROOM_SHARE: f64 = 0.08; // ratio: memory_budget.py _AUTO_HEADROOM_FRACTION, of the weights
const ENGINE_CAP_HEADROOM_MIN_BYTES: u64 = 512 * 1024 * 1024; // ratio: memory_budget.py _AUTO_HEADROOM_MIN_BYTES
const ENGINE_CAP_MIN_SHARE: f64 = 0.90; // ratio: memory_budget.py AUTO_UTILIZATION_FLOOR, of the working set
const ENGINE_CAP_MAX_SHARE: f64 = 0.97; // ratio: memory_budget.py AUTO_UTILIZATION_CEILING, of the working set

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineMemoryProfile {
    /// No memory guard: every step runs `prefill_chunk_tokens` and nothing stops it before the
    /// Metal working set (`mx.set_memory_limit` is a GC threshold, not a refusal).
    Unguarded { prefill_chunk_tokens: u64 },
    /// The memory guard: a step halves down to `floor_chunk_tokens` before the request is refused,
    /// under the engine's own Metal cap.
    Guarded { floor_chunk_tokens: u64 },
}

impl EngineMemoryProfile {
    /// The chunk a step takes at the largest context the engine holds.
    pub fn chunk_tokens(self) -> u64 {
        match self {
            EngineMemoryProfile::Unguarded {
                prefill_chunk_tokens,
            } => prefill_chunk_tokens,
            EngineMemoryProfile::Guarded { floor_chunk_tokens } => floor_chunk_tokens,
        }
    }

    /// The ceiling a step is planned under on a Mac whose Metal working set is `ceiling_bytes`.
    pub fn ceiling_bytes(self, weights_bytes: u64, ceiling_bytes: u64) -> u64 {
        match self {
            EngineMemoryProfile::Unguarded { .. } => ceiling_bytes,
            EngineMemoryProfile::Guarded { .. } => engine_metal_cap(weights_bytes, ceiling_bytes),
        }
    }
}

/// What an engine with no declared profile is charged: the unguarded engine, the larger chunk
/// under the whole working set — never more context than a declared engine would get.
pub const UNDECLARED: EngineMemoryProfile = EngineMemoryProfile::Unguarded {
    prefill_chunk_tokens: UNGUARDED_PREFILL_CHUNK_TOKENS,
};

/// Each Rapid-MLX tag goose pins, and how it prefills. A pin bump adds its row here — the test
/// `the_pinned_engine_declares_its_memory_profile` refuses a pin without one. The memory-guard
/// engine's row is `Guarded { floor_chunk_tokens: GUARD_FLOOR_CHUNK_TOKENS }`.
const PINNED_PROFILES: &[(&str, EngineMemoryProfile)] = &[
    (
        "v0.14.3-lz.10",
        EngineMemoryProfile::Unguarded {
            prefill_chunk_tokens: UNGUARDED_PREFILL_CHUNK_TOKENS,
        },
    ),
    (
        "v0.14.3-lz.11",
        EngineMemoryProfile::Unguarded {
            prefill_chunk_tokens: UNGUARDED_PREFILL_CHUNK_TOKENS,
        },
    ),
    (
        "v0.14.3-lz.12",
        EngineMemoryProfile::Guarded {
            floor_chunk_tokens: GUARD_FLOOR_CHUNK_TOKENS,
        },
    ),
    (
        "v0.14.3-lz.13",
        EngineMemoryProfile::Guarded {
            floor_chunk_tokens: GUARD_FLOOR_CHUNK_TOKENS,
        },
    ),
    (
        "v0.14.3-lz.14",
        EngineMemoryProfile::Guarded {
            floor_chunk_tokens: GUARD_FLOOR_CHUNK_TOKENS,
        },
    ),
    (
        "v0.14.3-lz.14-ple.1",
        EngineMemoryProfile::Guarded {
            floor_chunk_tokens: GUARD_FLOOR_CHUNK_TOKENS,
        },
    ),
    (
        "v0.14.3-lz.16-ple.1",
        EngineMemoryProfile::Guarded {
            floor_chunk_tokens: GUARD_FLOOR_CHUNK_TOKENS,
        },
    ),
    (
        "v0.14.3-lz.16-ple.2",
        EngineMemoryProfile::Guarded {
            floor_chunk_tokens: GUARD_FLOOR_CHUNK_TOKENS,
        },
    ),
    (
        "v0.14.3-lz.16-ple.3",
        EngineMemoryProfile::Guarded {
            floor_chunk_tokens: GUARD_FLOOR_CHUNK_TOKENS,
        },
    ),
    (
        "v0.14.3-lz.16-ple.4",
        EngineMemoryProfile::Guarded {
            floor_chunk_tokens: GUARD_FLOOR_CHUNK_TOKENS,
        },
    ),
    (
        "v0.14.3-lz.16-ple.5",
        EngineMemoryProfile::Guarded {
            floor_chunk_tokens: GUARD_FLOOR_CHUNK_TOKENS,
        },
    ),
    (
        "v0.14.3-lz.16-ple.6",
        EngineMemoryProfile::Guarded {
            floor_chunk_tokens: GUARD_FLOOR_CHUNK_TOKENS,
        },
    ),
    (
        "v0.14.3-lz.17",
        EngineMemoryProfile::Guarded {
            floor_chunk_tokens: GUARD_FLOOR_CHUNK_TOKENS,
        },
    ),
    (
        "v0.14.3-lz.18",
        EngineMemoryProfile::Guarded {
            floor_chunk_tokens: GUARD_FLOOR_CHUNK_TOKENS,
        },
    ),
];

/// The profile of the engine `spawn_command` launches; `Err` names why it is not known (an
/// owner's own launcher, a tag without a row), and the caller charges [`UNDECLARED`] and says so.
pub fn profile_of_launcher(spawn_command: &[String]) -> Result<EngineMemoryProfile, String> {
    let tag = spawn_command
        .iter()
        .find_map(|arg| arg.rsplit_once("Rapid-MLX@").map(|(_, tag)| tag.trim()))
        .ok_or_else(|| {
            format!(
                "the engine launcher `{}` names no Rapid-MLX tag, so goose does not know how it \
                 prefills",
                spawn_command.join(" ")
            )
        })?;
    PINNED_PROFILES
        .iter()
        .find(|(pinned, _)| *pinned == tag)
        .map(|(_, profile)| *profile)
        .ok_or_else(|| format!("Rapid-MLX {tag} declares no prefill memory profile in goose"))
}

/// The pinned launcher's profile.
pub fn pinned_profile() -> Result<EngineMemoryProfile, String> {
    let launcher: Vec<String> = ENGINE_LAUNCHER.iter().map(|s| s.to_string()).collect();
    profile_of_launcher(&launcher)
}

/// The ceiling the guarded engine plans under on a Mac whose Metal working set is
/// `ceiling_bytes`, with `weights_bytes` loaded (Gabee's 64 GB M3 Max and the 27B: 0.90 × 55.66 GB
/// = 50.10 GB).
pub fn engine_metal_cap(weights_bytes: u64, ceiling_bytes: u64) -> u64 {
    let headroom = ((weights_bytes as f64 * ENGINE_CAP_HEADROOM_SHARE) as u64)
        .max(ENGINE_CAP_HEADROOM_MIN_BYTES);
    let share = ((weights_bytes + headroom) as f64 / ceiling_bytes.max(1) as f64)
        .clamp(ENGINE_CAP_MIN_SHARE, ENGINE_CAP_MAX_SHARE);
    (ceiling_bytes as f64 * share) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launcher(tag: &str) -> Vec<String> {
        vec![
            "uvx".into(),
            "--from".into(),
            format!("rapid-mlx[mtp] @ git+https://github.com/leanzero-srl/Rapid-MLX@{tag}"),
            "rapid-mlx".into(),
        ]
    }

    /// A pin bump without its row would size every context with the undeclared profile: refuse it.
    #[test]
    fn the_pinned_engine_declares_its_memory_profile() {
        let pinned = pinned_profile();
        assert!(
            pinned.is_ok(),
            "{pinned:?} — add the pinned tag to PINNED_PROFILES (a memory-guard engine is \
             `Guarded {{ floor_chunk_tokens: GUARD_FLOOR_CHUNK_TOKENS }}`)"
        );
    }

    /// lz.12, lz.13, lz.14, lz.14-ple.1, lz.16-ple.1, lz.16-ple.2, lz.16-ple.3, lz.16-ple.4, lz.16-ple.5, lz.16-ple.6, lz.17 and lz.18 are the memory-guard line: the pinned launcher must be read as guarded, or goose would
    /// pass it the prefix-cache share the guard takes as an operator cap.
    #[test]
    fn the_shipped_pin_is_the_guarded_engine() {
        for tag in [
            "v0.14.3-lz.12",
            "v0.14.3-lz.13",
            "v0.14.3-lz.14",
            "v0.14.3-lz.14-ple.1",
            "v0.14.3-lz.16-ple.1",
            "v0.14.3-lz.16-ple.2",
            "v0.14.3-lz.16-ple.3",
            "v0.14.3-lz.16-ple.4",
            "v0.14.3-lz.16-ple.5",
            "v0.14.3-lz.16-ple.6",
            "v0.14.3-lz.17",
            "v0.14.3-lz.18",
        ] {
            assert_eq!(
                profile_of_launcher(&launcher(tag)),
                Ok(EngineMemoryProfile::Guarded {
                    floor_chunk_tokens: GUARD_FLOOR_CHUNK_TOKENS
                }),
                "{tag}"
            );
        }
        assert_eq!(
            profile_of_launcher(&launcher("v0.14.3-lz.11")),
            Ok(UNDECLARED)
        );
    }

    #[test]
    fn lz10_prefills_at_its_configured_chunk_and_an_unknown_engine_is_named() {
        assert_eq!(
            profile_of_launcher(&launcher("v0.14.3-lz.10")),
            Ok(EngineMemoryProfile::Unguarded {
                prefill_chunk_tokens: 2048
            })
        );
        let unknown = profile_of_launcher(&launcher("v9.9.9-lz.99")).unwrap_err();
        assert!(unknown.contains("v9.9.9-lz.99"), "{unknown}");
        assert!(profile_of_launcher(&["python3".into(), "serve.py".into()]).is_err());
    }

    /// The four ratios are copies of the engine's (memory_budget.py at lz.10 and lz/memory-guard);
    /// once the engine runs, `/v1/status` `memory_guard.ceiling_bytes` is read instead. This pins the
    /// copy: a weights share inside [0.90, 0.97] follows the weights, outside it clamps.
    #[test]
    fn the_engine_cap_follows_its_auto_rule() {
        let ceiling = 55_662_788_608;
        assert_eq!(
            engine_metal_cap(31_885_090_345, ceiling),
            (ceiling as f64 * 0.90) as u64
        );
        let big = 49_000_000_000u64;
        let expected = (big as f64 * 1.08) / ceiling as f64;
        assert_eq!(
            engine_metal_cap(big, ceiling),
            (ceiling as f64 * expected) as u64
        );
        assert_eq!(
            engine_metal_cap(54_000_000_000, ceiling),
            (ceiling as f64 * 0.97) as u64
        );
        assert_eq!(
            engine_metal_cap(1, 1_000_000_000),
            900_000_000,
            "512 MiB floor then clamp"
        );
    }
}
