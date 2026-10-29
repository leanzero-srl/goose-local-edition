//! Per-model n-gram table offload ("Keep n-gram table on disk"): Qwen3.8-Flash-Next (`qwen4_exp`)
//! carries a 32 GB per-layer-embedding (PLE) n-gram table in layer 1
//! (`…ple.ple_embedding.ngram_embedding.shard_{0..127}`). Decode gathers ~16 rows of ~100 B per
//! token from it — a lookup, not a stream — so the engine can leave it in the checkpoint on disk
//! and read the rows it needs (`rapid-mlx serve --qwen4-ple-offload`, fork tag
//! v0.14.3-lz.14-ple.1, `rapid_mlx/models/qwen4_offload.py`).
//!
//! Measured on an M5 Max 128 GB (2026-10-26, MTP k=2, prefix cache off): MLX active memory
//! 104.25 GB resident → 72.25 GB with the table on disk; decode tok/s code/prose/json/zh/long/copy
//! 65.7/52.3/75.7/54.0/51.7/114 resident → 64.3/51.2/63.2/55.9/50.7/119; a 7.8k-token prefill
//! 7.18 s → 6.95 s; greedy output byte-identical where the resident reference itself is stable.
//!
//! The profile's choice is `Option<bool>`: `None` = auto, which turns the offload ON for a
//! `qwen4_exp` checkpoint exactly when the model would not otherwise fit this Mac (below), and
//! leaves every other model alone. A checkpoint without the table never gets the flag, whatever the
//! profile says (the lever is not offered for it).

use std::path::Path;

use crate::fit;

/// The engine flag (serve only, store_true).
pub const ENGINE_FLAG: &str = "--qwen4-ple-offload";
/// The substring every tensor of the n-gram table carries in its safetensors name.
pub const TABLE_MARK: &str = "ple_embedding.ngram_embedding";
/// The architecture whose auto default is ON (the only one the engine implements the offload for).
pub const AUTO_MODEL_TYPE: &str = "qwen4_exp";

/// Share of a Mac's RAM the auto rule assumes is available to the engine at mount time.
/// WHY 0.70 and not the whole RAM: the mount gate's budget is LIVE available memory minus the 9.3%
/// margin (`fit::budget_bytes`), and a working Mac keeps a share of its RAM busy. The owner's 128 GB
/// MacBook was recorded at 93.0 GB available (72.7%, planner test
/// `the_recorded_flash_refusal_is_the_mount_gates_verdict_and_names_the_split`): budget 81.1 GB,
/// Flash-Next fully resident short. Judged with the whole RAM available it would "fit" at 107.5 GB
/// and auto would never offload on the one Mac the lever exists for. The rule must read STATIC
/// machine facts (RAM, Metal ceiling), never the live reading: the argv is part of the engine's
/// identity, and a live input would restart the engine whenever memory moved.
pub const TYPICAL_AVAILABLE_RATIO: f64 = 0.70;

/// The static facts the auto rule reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Machine {
    pub total_bytes: u64,
    pub ceiling_bytes: u64,
}

impl Machine {
    /// This Mac: `hw.memsize` and Metal's recommended working set. `None` when either is unknown.
    pub fn local() -> Option<Machine> {
        let total_bytes = crate::memory::measure().ok()?.total_bytes;
        let ceiling_bytes = crate::engine::local_gpu_ceiling().ok()?;
        Some(Machine {
            total_bytes,
            ceiling_bytes,
        })
    }

    /// The fit rule's budget on a Mac with `TYPICAL_AVAILABLE_RATIO` of its RAM available.
    pub fn typical_budget_bytes(self) -> u64 {
        fit::budget_bytes(
            (self.total_bytes as f64 * TYPICAL_AVAILABLE_RATIO) as u64,
            self.total_bytes,
            self.ceiling_bytes,
        )
    }
}

/// Whether a tensor name belongs to the n-gram table.
pub fn is_table_tensor(name: &str) -> bool {
    name.contains(TABLE_MARK)
}

/// Whether the checkpoint carries the n-gram table: `model.safetensors.index.json`'s weight map
/// names one of its tensors. A single-file checkpoint without an index has no table (the table
/// alone is 32 GB, so every checkpoint holding it is sharded).
pub fn has_table(model_dir: &Path) -> bool {
    let Ok(bytes) = std::fs::read(model_dir.join("model.safetensors.index.json")) else {
        return false;
    };
    let Ok(index) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return false;
    };
    index
        .get("weight_map")
        .and_then(serde_json::Value::as_object)
        .is_some_and(|map| map.keys().any(|k| is_table_tensor(k)))
}

/// The profile's choice resolved for one Mac. `full_need_bytes` is the single engine's need with
/// the table resident (weights + KV at the smallest useful context, `fit::Need::total_bytes`).
pub fn resolve(
    choice: Option<bool>,
    has_table: bool,
    model_type: &str,
    full_need_bytes: u64,
    machine: Option<Machine>,
) -> bool {
    if !has_table {
        return false;
    }
    match choice {
        Some(on) => on,
        None => {
            model_type == AUTO_MODEL_TYPE
                && machine.is_some_and(|m| full_need_bytes > m.typical_budget_bytes())
        }
    }
}

/// The note a fit verdict carries when the table stays on disk.
pub fn fit_note(table_bytes: u64, held_bytes: u64) -> String {
    format!(
        "fits with the n-gram table on disk (~{} held, {} read from the SSD as needed)",
        fit::gb(held_bytes),
        fit::gb(table_bytes)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1 << 30;
    /// The owner's MacBook: M4 Max, 128 GB, Metal recommendedMaxWorkingSetSize.
    const MACBOOK: Machine = Machine {
        total_bytes: 128 * GIB,
        ceiling_bytes: 115_448_725_504,
    };
    const STUDIO_256: Machine = Machine {
        total_bytes: 256 * GIB,
        ceiling_bytes: 206_158_430_208,
    };
    /// Flash-Next on disk (98 GiB) + a small KV: what the gate charges with the table resident.
    const FLASH_FULL: u64 = 98 * GIB + GIB / 2;

    #[test]
    fn auto_offloads_flash_next_on_a_128_gb_mac_and_not_on_a_256_gb_one() {
        assert!(MACBOOK.typical_budget_bytes() < FLASH_FULL);
        assert!(resolve(None, true, "qwen4_exp", FLASH_FULL, Some(MACBOOK)));
        assert!(!resolve(
            None,
            true,
            "qwen4_exp",
            FLASH_FULL,
            Some(STUDIO_256)
        ));
    }

    #[test]
    fn the_owners_choice_wins_but_never_without_the_table() {
        assert!(!resolve(
            Some(false),
            true,
            "qwen4_exp",
            FLASH_FULL,
            Some(MACBOOK)
        ));
        assert!(resolve(Some(true), true, "qwen4_exp", 1, Some(STUDIO_256)));
        assert!(!resolve(
            Some(true),
            false,
            "qwen4_exp",
            FLASH_FULL,
            Some(MACBOOK)
        ));
        assert!(!resolve(
            None,
            false,
            "qwen4_exp",
            FLASH_FULL,
            Some(MACBOOK)
        ));
    }

    #[test]
    fn auto_is_off_for_other_architectures_and_on_an_unknown_machine() {
        assert!(!resolve(
            None,
            true,
            "qwen3_5_moe",
            FLASH_FULL,
            Some(MACBOOK)
        ));
        assert!(!resolve(None, true, "qwen4_exp", FLASH_FULL, None));
    }

    #[test]
    fn the_table_is_found_through_the_index_weight_map() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!has_table(dir.path()), "no index, no table");
        std::fs::write(
            dir.path().join("model.safetensors.index.json"),
            r#"{"weight_map":{"model.language_model.layers.0.mlp.gate.weight":"model-00001.safetensors"}}"#,
        )
        .unwrap();
        assert!(!has_table(dir.path()));
        std::fs::write(
            dir.path().join("model.safetensors.index.json"),
            r#"{"weight_map":{"model.language_model.layers.1.ple.ple_embedding.ngram_embedding.shard_0.weight":"model-00002.safetensors"}}"#,
        )
        .unwrap();
        assert!(has_table(dir.path()));
    }
}
