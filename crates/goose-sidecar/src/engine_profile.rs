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
pub const ENGINE_CAP_HEADROOM_SHARE: f64 = 0.08; // ratio: memory_budget.py _AUTO_HEADROOM_FRACTION, of the weights
pub const ENGINE_CAP_MIN_SHARE: f64 = 0.90; // ratio: memory_budget.py AUTO_UTILIZATION_FLOOR, of the working set
pub const ENGINE_CAP_MAX_SHARE: f64 = 0.97; // ratio: memory_budget.py AUTO_UTILIZATION_CEILING, of the working set

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
    (
        "v0.14.3-lz.19",
        EngineMemoryProfile::Guarded {
            floor_chunk_tokens: GUARD_FLOOR_CHUNK_TOKENS,
        },
    ),
    (
        "v0.14.3-lz.20",
        EngineMemoryProfile::Guarded {
            floor_chunk_tokens: GUARD_FLOOR_CHUNK_TOKENS,
        },
    ),
    (
        "v0.14.3-lz.21",
        EngineMemoryProfile::Guarded {
            floor_chunk_tokens: GUARD_FLOOR_CHUNK_TOKENS,
        },
    ),
    (
        "v0.14.3-lz.22",
        EngineMemoryProfile::Guarded {
            floor_chunk_tokens: GUARD_FLOOR_CHUNK_TOKENS,
        },
    ),
    (
        MEMORY_TUNABLES_BRANCH,
        EngineMemoryProfile::Guarded {
            floor_chunk_tokens: GUARD_FLOOR_CHUNK_TOKENS,
        },
    ),
];

/// The fork branch the memory tunables were built on; a launcher pinned to it reads every lz.20 flag.
pub const MEMORY_TUNABLES_BRANCH: &str = "lz/metal-tunables";
/// The first lz tag of the v0.14.3 line that reads the memory-tunable flags
/// (`--metal-limit-gb`, `--memory-floor-gb`, `--max-model-len`, ... fork INTERFACE.md).
const MEMORY_TUNABLES_FIRST_LZ: u32 = 20; // measured: fork tag v0.14.3-lz.20 introduces the flags

fn launcher_tag(spawn_command: &[String]) -> Option<&str> {
    spawn_command
        .iter()
        .find_map(|arg| arg.rsplit_once("Rapid-MLX@").map(|(_, tag)| tag.trim()))
}

/// Whether the engine `spawn_command` launches reads the memory-tunable flags: a `v0.14.3-lz.N`
/// tag with N >= 20 (any suffix), a later base version, or the tunables branch itself. An owner's
/// own launcher without a tag is not known to, so goose sends none of them to it.
pub fn supports_memory_tunables(spawn_command: &[String]) -> bool {
    let Some(tag) = launcher_tag(spawn_command) else {
        return false;
    };
    if tag == MEMORY_TUNABLES_BRANCH {
        return true;
    }
    let Some(rest) = tag.strip_prefix('v') else {
        return false;
    };
    let (base, lz) = match rest.split_once("-lz.") {
        Some((base, lz)) => (base, Some(lz)),
        None => (rest, None),
    };
    let parts: Vec<u32> = base.split('.').filter_map(|p| p.parse().ok()).collect();
    if parts.len() != 3 {
        return false;
    }
    let base = (parts[0], parts[1], parts[2]);
    if base > (0, 14, 3) {
        return true;
    }
    if base < (0, 14, 3) {
        return false;
    }
    lz.and_then(|lz| {
        lz.split(|c: char| !c.is_ascii_digit())
            .next()
            .and_then(|n| n.parse::<u32>().ok())
    })
    .is_some_and(|n| n >= MEMORY_TUNABLES_FIRST_LZ)
}

/// The profile of the engine `spawn_command` launches; `Err` names why it is not known (an
/// owner's own launcher, a tag without a row), and the caller charges [`UNDECLARED`] and says so.
pub fn profile_of_launcher(spawn_command: &[String]) -> Result<EngineMemoryProfile, String> {
    let tag = launcher_tag(spawn_command).ok_or_else(|| {
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

    /// Every shipped pin before the current one must be in SUPERSEDED_ENGINE_LAUNCHERS, or an install
    /// that persisted it as its default stays on it forever (`migrate_launcher` only moves listed
    /// launchers). The lz.17 -> lz.18 bump forgot lz.17; this keeps the next bump honest.
    #[test]
    fn every_older_pinned_tag_is_a_superseded_launcher() {
        let current: Vec<String> = ENGINE_LAUNCHER.iter().map(|s| s.to_string()).collect();
        for (tag, _) in PINNED_PROFILES {
            if tag.starts_with("v9.9.9") || *tag == MEMORY_TUNABLES_BRANCH {
                continue;
            }
            let l = launcher(tag);
            if l == current {
                continue;
            }
            assert!(
                crate::engine::SUPERSEDED_ENGINE_LAUNCHERS
                    .iter()
                    .any(|old| old.iter().copied().eq(l.iter().map(String::as_str))),
                "{tag} has a profile row but is not in SUPERSEDED_ENGINE_LAUNCHERS"
            );
        }
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

    /// lz.12, lz.13, lz.14, lz.14-ple.1, lz.16-ple.1, lz.16-ple.2, lz.16-ple.3, lz.16-ple.4, lz.16-ple.5, lz.16-ple.6, lz.17, lz.18 and lz.19 are the memory-guard line: the pinned launcher must be read as guarded, or goose would
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
            "v0.14.3-lz.19",
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
    fn only_lz20_and_the_tunables_branch_read_the_memory_flags() {
        for (tag, want) in [
            ("v0.14.3-lz.18", false),
            ("v0.14.3-lz.19", false),
            ("v0.14.3-lz.20", true),
            ("v0.14.3-lz.21", true),
            ("v0.14.3-lz.22", true),
            ("v0.14.3-lz.21-ple.1", true),
            ("v0.14.4", true),
            ("v0.13.1", false),
            ("lz/metal-tunables", true),
            ("lz/other-branch", false),
        ] {
            assert_eq!(supports_memory_tunables(&launcher(tag)), want, "{tag}");
        }
        assert!(!supports_memory_tunables(&[
            "python3".into(),
            "serve.py".into()
        ]));
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
}
