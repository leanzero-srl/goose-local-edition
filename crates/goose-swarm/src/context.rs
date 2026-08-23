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
    /// A LOOSE backstop cap only — generous enough that a normal task sees its dependencies' full
    /// output (so identical models can agree on unspecified behaviour), while a pathological high-fan-in
    /// task (e.g. integrate-verify on a huge plan) can't prefill an unbounded context. Details live on
    /// disk; workers are told to read the actual files for exact APIs.
    pub fn slice_for(&self, deps: &[TaskId]) -> String {
        const PER_DEP: usize = 4000;
        const TOTAL: usize = 20000;
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

    /// Full dependency outputs for engine-compiled evidence tasks. These tasks are already bounded by
    /// semantic acceptance slices rather than by fleet size; truncating their structured verdict before
    /// the integration sink reads it would erase the very value runtime replanning produced.
    pub fn slice_for_unbounded(&self, deps: &[TaskId]) -> String {
        deps.iter()
            .filter_map(|id| {
                self.summaries
                    .get(id)
                    .map(|summary| format!("### Output of dependency `{id}`\n{summary}"))
            })
            .collect::<Vec<_>>()
            .join("\n\n")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_runtime_evidence_reaches_the_sink_without_legacy_truncation() {
        let mut context = SharedContext::new();
        let evidence = "e".repeat(9000);
        context.merge("replan-review::slice", evidence.clone());
        let deps = vec!["replan-review::slice".to_string()];
        assert!(context.slice_for(&deps).len() < evidence.len());
        let exact = context.slice_for_unbounded(&deps);
        assert!(exact.ends_with(&evidence));
        assert!(!exact.contains("[truncated"));
    }
}
