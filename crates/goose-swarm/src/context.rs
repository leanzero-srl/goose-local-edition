//! Shared context corroboration: an append-only store of completed task outputs, with a slice
//! extractor that gives each task the outputs of its dependencies when it is dispatched.

use crate::dag::TaskId;
use std::collections::HashMap;

#[derive(Default, Debug)]
pub struct SharedContext {
    summaries: HashMap<TaskId, String>,
    order: Vec<TaskId>,
}

impl SharedContext {
    pub fn new() -> Self {
        Self::default()
    }

    /// Merge a completed task's output into the overall context.
    pub fn merge(&mut self, task_id: &str, output: String) {
        if !self.summaries.contains_key(task_id) {
            self.order.push(task_id.to_string());
        }
        self.summaries.insert(task_id.to_string(), output);
    }

    /// The relevant slice for a task about to run: the outputs of its dependencies, in dep order.
    /// CAPPED per-dependency and overall — a high-fan-in task (e.g. integrate-verify, which depends on
    /// every other task) would otherwise prefill an enormous context and stream-stall on a local model.
    /// The actual files are on disk; the summaries are just orientation.
    pub fn slice_for(&self, deps: &[TaskId]) -> String {
        const PER_DEP: usize = 600;
        const TOTAL: usize = 5000;
        let mut parts = Vec::new();
        let mut total = 0usize;
        for d in deps {
            if let Some(s) = self.summaries.get(d) {
                let body: String = s.chars().take(PER_DEP).collect();
                let body = if s.chars().count() > PER_DEP {
                    format!("{body}… [truncated — read the file on disk]")
                } else {
                    body
                };
                let part = format!("### Output of dependency `{d}`\n{body}");
                total += part.len();
                parts.push(part);
                if total >= TOTAL {
                    parts.push(
                        "… [further dependency outputs omitted — read the files on disk]"
                            .to_string(),
                    );
                    break;
                }
            }
        }
        parts.join("\n\n")
    }

    pub fn completed(&self) -> &[TaskId] {
        &self.order
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "completed_order": self.order,
            "summaries": self.summaries,
        })
    }
}
