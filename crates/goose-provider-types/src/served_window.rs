//! The window an OpenAI-compatible engine actually serves a model at — ONE rule for every reader
//! of a `/v1/models` entry: goose's own MLX engine status, the swarm router's route probes, the
//! remote single over Link, the swarm engine's catalog, and the `openai` provider pointed at an
//! engine.
//!
//! `context_window` is the model's native window as the engine reads it from the checkpoint
//! (262,144 for the 27B). `max_model_len` is the vLLM/SGLang card field, and what it means depends
//! on the engine:
//! - the LeanZero Rapid-MLX fork WITH its memory guard (`/v1/status` `memory_guard.enabled`)
//!   fills it with the largest context ONE sequence holds on this device beside the loaded
//!   weights, at the guard's floor prefill chunk — a DEVICE bound;
//! - Rapid-MLX without the guard (lz.10, upstream #2122 `projected_memory_max_context`) fills it
//!   from its KV admission projection alone: on Gabee's 64 GB M3 Max it listed 262,144 for the
//!   27B Q6 while every prefill from ~76k tokens hit a Metal out-of-memory — ADVISORY, never a
//!   device bound; vLLM's is its configured limit — also an upper bound, not a memory measure.
//!
//! So: a guarded engine's `max_model_len` (else its `max_context_idle`) is the served window. An
//! unguarded one's is only an upper bound: the served window is `min(that bound, the reader's own
//! device estimate)`, and the reader without an estimate keeps the bound — never raised past what
//! the engine itself says. Every unguarded read is said once per model
//! (`served_window_unguarded`), never assumed to fit the device.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

/// What the engine's memory guard says in `/v1/status` (LeanZero Rapid-MLX `memory_guard`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryGuard {
    /// `max_context_idle`: the largest context one sequence holds on this device, idle.
    pub max_context_idle: Option<u64>,
    /// `source: calibrated` — the guard's step-peak model runs on measured peaks, not its prior.
    pub calibrated: bool,
    /// `floor_chunk_tokens`: the smallest chunk the guard halves a prefill step down to.
    pub floor_chunk_tokens: Option<u64>,
    /// `ceiling_bytes`: the Metal cap the guard plans every step under.
    pub ceiling_bytes: Option<u64>,
}

impl MemoryGuard {
    /// `None` unless the body names an ENABLED guard: an engine without one, or with it switched
    /// off, bounds nothing by memory.
    pub fn of_status(status: &Value) -> Option<Self> {
        let guard = status.get("memory_guard")?;
        if guard.get("enabled").and_then(Value::as_bool) != Some(true) {
            return None;
        }
        let positive = |key: &str| guard.get(key).and_then(Value::as_u64).filter(|v| *v > 0);
        let source = ["coefficients", "peak_model"]
            .iter()
            .filter_map(|k| guard.get(*k))
            .chain(std::iter::once(guard))
            .find_map(|block| block.get("source").and_then(Value::as_str));
        Some(MemoryGuard {
            max_context_idle: positive("max_context_idle"),
            calibrated: source == Some("calibrated"),
            floor_chunk_tokens: positive("floor_chunk_tokens"),
            ceiling_bytes: positive("ceiling_bytes"),
        })
    }

    pub fn of_status_body(body: &str) -> Option<Self> {
        serde_json::from_str::<Value>(body)
            .ok()
            .as_ref()
            .and_then(Self::of_status)
    }
}

/// Who bounded a served window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowBound {
    /// The engine's memory guard, from measured step peaks.
    EngineCalibrated,
    /// The engine's memory guard, still on its analytic prior.
    EnginePrior,
    /// The reader's own device estimate (the engine bounds nothing by memory).
    Estimate,
    /// Nothing bounded it by memory: the model's window or the engine's advisory figure.
    Unbounded,
}

/// What one `/v1/models` entry (and, when read, its `/v1/status`) says about the window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ServedWindow {
    /// `context_window`: the model's own window.
    pub native: Option<u64>,
    /// `max_model_len`: `null`, 0 and absent all mean the engine stated none.
    pub max_model_len: Option<u64>,
    /// The enabled memory guard, when `/v1/status` was read and names one.
    pub guard: Option<MemoryGuard>,
}

impl ServedWindow {
    pub fn of_entry(entry: &Value, guard: Option<MemoryGuard>) -> Self {
        let positive = |key: &str| entry.get(key).and_then(Value::as_u64).filter(|v| *v > 0);
        ServedWindow {
            native: positive("context_window"),
            max_model_len: positive("max_model_len"),
            guard,
        }
    }

    /// What the engine says it will not serve past: `min(context_window, max_model_len)`.
    pub fn upper_bound(&self) -> Option<u64> {
        match (self.max_model_len, self.native) {
            (Some(bound), Some(native)) => Some(bound.min(native)),
            (bound, native) => bound.or(native),
        }
    }

    /// The window bounded by THIS device's memory — only a guarded engine's figure.
    pub fn device_bound(&self) -> Option<u64> {
        let guard = self.guard?;
        let bound = self.max_model_len.or(guard.max_context_idle)?;
        Some(self.native.map_or(bound, |native| bound.min(native)))
    }

    /// THE rule: the guard's device bound; else the upper bound the engine states, lowered to the
    /// reader's own device estimate when it has one.
    pub fn served(&self, own_estimate: Option<u64>) -> Option<u64> {
        if let Some(bound) = self.device_bound() {
            return Some(bound);
        }
        match (self.upper_bound(), own_estimate) {
            (Some(upper), Some(estimate)) => Some(upper.min(estimate)),
            (upper, estimate) => upper.or(estimate),
        }
    }

    pub fn bound(&self, own_estimate: Option<u64>) -> WindowBound {
        match (self.guard, self.device_bound()) {
            (Some(guard), Some(_)) if guard.calibrated => WindowBound::EngineCalibrated,
            (Some(_), Some(_)) => WindowBound::EnginePrior,
            _ if own_estimate.is_some() => WindowBound::Estimate,
            _ => WindowBound::Unbounded,
        }
    }

    /// Say once per (`origin`, `model`) that the engine bounds nothing by memory: its
    /// `max_model_len`, if any, is advisory, and the window is the reader's estimate or the
    /// engine's upper bound.
    pub fn warn_once_if_unguarded(&self, origin: &str, model: &str) {
        if self.device_bound().is_some() {
            return;
        }
        static SAID: OnceLock<Mutex<HashSet<(String, String)>>> = OnceLock::new();
        let first = SAID
            .get_or_init(|| Mutex::new(HashSet::new()))
            .lock()
            .map(|mut said| said.insert((origin.to_string(), model.to_string())))
            .unwrap_or(true);
        if first {
            tracing::warn!(
                origin,
                model,
                native_window = ?self.native,
                advisory_max_model_len = ?self.max_model_len,
                "served_window_unguarded: the engine reports no enabled memory guard, so no figure \
                 it lists is bounded by this device's memory; the window is the reader's own \
                 estimate or the engine's upper bound"
            );
        }
    }
}

/// An engine's context overflow says how much fits (`n_ctx` beside `context_length_exceeded`):
/// goose carries it in its own tag on the overflow's message, so a provider that cached a window
/// lowers it to the engine's figure instead of compacting against the same number again.
const OVERFLOW_N_CTX_TAG: &str = " [n_ctx=";

pub fn tag_overflow(message: &str, n_ctx: Option<u64>) -> String {
    match n_ctx.filter(|n| *n > 0) {
        Some(n) => format!("{message}{OVERFLOW_N_CTX_TAG}{n}]"),
        None => message.to_string(),
    }
}

/// The `n_ctx` an overflow carried (`tag_overflow`); `None` when the engine named none.
pub fn overflow_n_ctx(message: &str) -> Option<u64> {
    let (_, tail) = message.rsplit_once(OVERFLOW_N_CTX_TAG)?;
    tail.strip_suffix(']')?.parse().ok()
}

/// The `n_ctx` an OpenAI-shaped error object names (`error.n_ctx`, or top level).
pub fn error_n_ctx(error: &Value) -> Option<u64> {
    error
        .get("n_ctx")
        .or_else(|| error.get("error").and_then(|e| e.get("n_ctx")))
        .and_then(Value::as_u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn guard(calibrated: bool) -> Option<MemoryGuard> {
        MemoryGuard::of_status(&json!({"memory_guard": {
            "enabled": true, "max_context_idle": 150_000, "floor_chunk_tokens": 256,
            "ceiling_bytes": 50_096_509_747_u64,
            "coefficients": {"source": if calibrated { "calibrated" } else { "prior" }}
        }}))
    }

    /// The guarded engine's figure is the window, whatever the reader estimated.
    #[test]
    fn max_model_len_bounds_the_served_window() {
        let entry =
            json!({"id": "AREX-2-Q8-mlx", "context_window": 262_144, "max_model_len": 158_464});
        let w = ServedWindow::of_entry(&entry, guard(true));
        assert_eq!(w.served(None), Some(158_464));
        assert_eq!(
            w.served(Some(120_000)),
            Some(158_464),
            "the engine's own figure wins"
        );
        assert_eq!(w.bound(Some(120_000)), WindowBound::EngineCalibrated);
        assert_eq!(
            ServedWindow::of_entry(&entry, guard(false)).bound(None),
            WindowBound::EnginePrior
        );
        let no_card = ServedWindow::of_entry(&json!({"context_window": 262_144}), guard(true));
        assert_eq!(
            no_card.served(None),
            Some(150_000),
            "the guard's idle window"
        );
        let roomy = ServedWindow::of_entry(
            &json!({"context_window": 32_768, "max_model_len": 400_000}),
            guard(true),
        );
        assert_eq!(
            roomy.served(None),
            Some(32_768),
            "never above the model's own window"
        );
    }

    /// lz.10 on Gabee (live, 2026-10-14): the Q6 listed `max_model_len` 262,144 — upstream's KV-only
    /// projection — with no guard, while prefill OOMed from ~76k. Without the guard it is advisory.
    #[test]
    fn an_unguarded_max_model_len_is_advisory() {
        let lz10 = ServedWindow::of_entry(
            &json!({"id": "AREX-2-Q6-mlx", "context_window": 262_144, "max_model_len": 262_144}),
            MemoryGuard::of_status(&json!({"num_running": 0})),
        );
        assert_eq!(lz10.device_bound(), None);
        assert_eq!(
            lz10.served(Some(72_192)),
            Some(72_192),
            "the reader's estimate"
        );
        assert_eq!(lz10.bound(Some(72_192)), WindowBound::Estimate);
        assert_eq!(
            lz10.served(None),
            Some(262_144),
            "no estimate: the engine's upper bound"
        );
        assert_eq!(lz10.bound(None), WindowBound::Unbounded);
        let vllm = ServedWindow::of_entry(
            &json!({"context_window": 131_072, "max_model_len": 32_768}),
            None,
        );
        assert_eq!(
            vllm.served(Some(90_000)),
            Some(32_768),
            "never above what the engine states"
        );
        let off = MemoryGuard::of_status(&json!({"memory_guard": {"enabled": false}}));
        assert_eq!(off, None);
        for entry in [
            json!({ "context_window": 262_144, "max_model_len": null }),
            json!({ "context_window": 262_144, "max_model_len": 0 }),
        ] {
            assert_eq!(
                ServedWindow::of_entry(&entry, None).served(None),
                Some(262_144)
            );
        }
        assert_eq!(ServedWindow::of_entry(&json!({}), None).served(None), None);
    }

    #[test]
    fn the_guard_block_is_read_in_either_shape() {
        let g = MemoryGuard::of_status_body(
            r#"{"memory_guard":{"enabled":true,"max_context_idle":190000,
                "peak_model":{"source":"prior"}}}"#,
        )
        .unwrap();
        assert!(!g.calibrated);
        assert_eq!(g.max_context_idle, Some(190_000));
        assert_eq!(g.floor_chunk_tokens, None);
        assert_eq!(MemoryGuard::of_status_body("<html>"), None);
    }

    #[test]
    fn an_overflow_carries_the_engines_n_ctx_in_goose_tag() {
        let tagged = tag_overflow("This prompt (120000 tokens) needs 61.0 GB", Some(98_000));
        assert_eq!(overflow_n_ctx(&tagged), Some(98_000));
        assert_eq!(overflow_n_ctx("context length exceeded"), None);
        assert_eq!(tag_overflow("x", None), "x");
        assert_eq!(
            error_n_ctx(&json!({"error": {"code": "context_length_exceeded", "n_ctx": 98000}})),
            Some(98_000)
        );
        assert_eq!(error_n_ctx(&json!({"code": "c", "n_ctx": 5})), Some(5));
    }
}
