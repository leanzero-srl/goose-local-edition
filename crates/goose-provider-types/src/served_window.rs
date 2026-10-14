//! The window an OpenAI-compatible engine actually serves a model at — ONE rule for every reader
//! of a `/v1/models` entry: goose's own MLX engine status, the swarm router's route probe, the
//! remote single over Link, and the `openai` provider pointed at an engine.
//!
//! `context_window` is the model's native window as the engine reads it from the checkpoint
//! (262,144 for the 27B). `max_model_len` is the vLLM/SGLang card field the LeanZero Rapid-MLX
//! fork fills from its memory guard: the largest context ONE sequence can hold on this device
//! beside the loaded weights, at the guard's floor prefill chunk. On Gabee's 64 GB M3 Max the 27B
//! Q8 hit a Metal out-of-memory on every prefill from ~64k tokens while goose compacted at
//! 0.8 × 262,144 ≈ 210k — the native window promised a context the Mac could not hold.
//!
//! The served window is `min(max_model_len, context_window)`. An engine that reports no
//! `max_model_len` keeps today's reading, and the absence is said once per model
//! (`served_window_unreported`), never assumed to be fine.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

/// What one `/v1/models` entry says about the window it serves.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ServedWindow {
    /// `context_window`: the model's own window.
    pub native: Option<u64>,
    /// `max_model_len`: the largest context that fits this device beside the weights. `null`, 0
    /// and absent all mean the engine did not bound it.
    pub max_model_len: Option<u64>,
}

impl ServedWindow {
    pub fn of_entry(entry: &Value) -> Self {
        let positive = |key: &str| entry.get(key).and_then(Value::as_u64).filter(|v| *v > 0);
        ServedWindow {
            native: positive("context_window"),
            max_model_len: positive("max_model_len"),
        }
    }

    /// THE rule: the smaller of the two the engine reported; the native window alone when the
    /// engine bounds nothing.
    pub fn tokens(&self) -> Option<u64> {
        match (self.max_model_len, self.native) {
            (Some(bound), Some(native)) => Some(bound.min(native)),
            (bound, native) => bound.or(native),
        }
    }

    /// The window only when the engine bounded it by memory: a reader with its own fallback for an
    /// engine that says nothing (the `openai` provider's `meta.n_ctx` / LM Studio chain) keeps it.
    pub fn memory_bounded(&self) -> Option<u64> {
        self.max_model_len.and(self.tokens())
    }

    /// Say once per (`origin`, `model`) that the engine named a window it did not bound by memory:
    /// the reader then runs on the native window (or its own fallback), which this device may not
    /// hold.
    pub fn warn_once_if_unbounded(&self, origin: &str, model: &str) {
        if self.max_model_len.is_some() {
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
                "served_window_unreported: the engine's /v1/models entry carries no max_model_len, \
                 so the window is not bounded by this device's memory; the reader keeps the native \
                 window or its own fallback"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Gabee's 27B as the guarded Rapid-MLX lists it: native 262,144, bounded by the device.
    #[test]
    fn max_model_len_bounds_the_served_window() {
        let w = ServedWindow::of_entry(&json!({
            "id": "AREX-2-Q8-mlx", "context_window": 262_144, "max_model_len": 158_464
        }));
        assert_eq!(w.tokens(), Some(158_464));
        assert_eq!(w.memory_bounded(), Some(158_464));
        let roomy = ServedWindow::of_entry(&json!({
            "context_window": 32_768, "max_model_len": 400_000
        }));
        assert_eq!(
            roomy.tokens(),
            Some(32_768),
            "never above the model's own window"
        );
        let bound_only = ServedWindow::of_entry(&json!({ "max_model_len": 90_000 }));
        assert_eq!(bound_only.tokens(), Some(90_000));
    }

    #[test]
    fn an_unbounded_entry_keeps_the_native_window_and_is_named() {
        for entry in [
            json!({ "context_window": 262_144 }),
            json!({ "context_window": 262_144, "max_model_len": null }),
            json!({ "context_window": 262_144, "max_model_len": 0 }),
        ] {
            let w = ServedWindow::of_entry(&entry);
            assert_eq!(w.max_model_len, None, "{entry}");
            assert_eq!(w.tokens(), Some(262_144));
            assert_eq!(w.memory_bounded(), None, "{entry}");
        }
        assert_eq!(ServedWindow::of_entry(&json!({})).tokens(), None);
    }
}
