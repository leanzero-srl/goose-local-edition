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
    pub fn slice_for(&self, deps: &[TaskId]) -> String {
        let mut parts = Vec::new();
        for d in deps {
            if let Some(s) = self.summaries.get(d) {
                parts.push(format!("### Output of dependency `{d}`\n{s}"));
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
