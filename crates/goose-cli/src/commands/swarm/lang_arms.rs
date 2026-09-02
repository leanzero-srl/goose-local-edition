//! VA-060 (gate 10, "a language assumption is a hard-coded bit"): the run's language for the
//! Python-first arms — rule (c)'s module→package rewrite (`plan_repairs.rs`), the scoped AST
//! review and the pytest-tail parse (`transcripts.rs`). Each arm branches on `TargetLang`: Python
//! keeps today's path byte-identical; every other language gets ONE loud
//! `lang_unsupported{arm, lang, skipped}` per arm per run and skips — never a silent skip, never
//! a Python check run over a Node/Rust/Go tree (a pytest tail parsed off `npm test` output was
//! reading garbage as a test result). Sibling module under the incremental-split law.

use std::collections::BTreeSet;
use std::sync::{Mutex, OnceLock};

use super::{EventSink, TargetLang};

fn event_row(arm: &str, lang: &str, skipped: &str) -> serde_json::Value {
    serde_json::json!({
        "event": "lang_unsupported",
        "arm": arm,
        "lang": lang,
        "skipped": skipped,
    })
}

/// The named absence for a Python-only arm on a `lang` run (`lang` rides as its variant name:
/// `TypeScript`, `Rust`, `Go`, `Other`).
pub(super) fn lang_unsupported_event(
    arm: &str,
    lang: TargetLang,
    skipped: &str,
) -> serde_json::Value {
    event_row(arm, &format!("{lang:?}"), skipped)
}

/// The run's detected language plus the arms that already said they do not apply to it — ONE
/// field on `GooseAgentDispatcher`, set once by `run_linear_plan` from the tree at start (the same
/// pure derivation the plan door uses), read by every lane.
#[derive(Default)]
pub(super) struct LangArms {
    lang: OnceLock<TargetLang>,
    said: Mutex<BTreeSet<String>>,
}

impl LangArms {
    pub(super) fn set(&self, lang: TargetLang) {
        let _ = self.lang.set(lang);
    }

    pub(super) fn get(&self) -> Option<TargetLang> {
        self.lang.get().copied()
    }

    /// True iff the Python-only `arm` applies to this run. Otherwise the arm is said ONCE per run
    /// (`lang_unsupported{arm, lang, skipped}`; `lang: "undetected"` when no run set it) and the
    /// caller skips. A poisoned `said` lock says the arm again rather than never — louder, not quieter.
    pub(super) fn python_only(&self, arm: &str, skipped: &str, events: &dyn EventSink) -> bool {
        let lang = self.get();
        if lang == Some(TargetLang::Python) {
            return true;
        }
        let first = match self.said.lock() {
            Ok(mut said) => said.insert(arm.to_string()),
            Err(_) => true,
        };
        if first {
            let name = lang.map_or_else(|| "undetected".to_string(), |l| format!("{l:?}"));
            events.write_value(event_row(arm, &name, skipped));
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goose_swarm::SwarmEvent;

    #[derive(Default)]
    struct ValueSink(Mutex<Vec<serde_json::Value>>);
    impl EventSink for ValueSink {
        fn emit(&self, _event: &SwarmEvent) {}
        fn write_value(&self, value: serde_json::Value) {
            self.0.lock().unwrap().push(value);
        }
    }

    /// Python: the arm applies, nothing is said (sb-7 byte-identical). TypeScript: the arm is
    /// refused and said exactly once per arm per run, by name, with what was skipped.
    #[test]
    fn a_python_only_arm_is_said_once_per_arm_off_python_and_never_on_python() {
        let sink = ValueSink::default();
        let py = LangArms::default();
        py.set(TargetLang::Python);
        assert!(py.python_only("pytest_tail", "pytest summary parse", &sink));
        assert!(py.python_only("ast_review", "AST review", &sink));
        assert!(sink.0.lock().unwrap().is_empty(), "Python says nothing");

        let ts = LangArms::default();
        ts.set(TargetLang::TypeScript);
        assert!(!ts.python_only("pytest_tail", "pytest summary parse", &sink));
        assert!(!ts.python_only("pytest_tail", "pytest summary parse", &sink));
        assert!(!ts.python_only("ast_review", "AST review", &sink));
        let rows = sink.0.lock().unwrap();
        assert_eq!(rows.len(), 2, "one row per arm, not per call: {rows:?}");
        assert_eq!(rows[0]["event"], "lang_unsupported");
        assert_eq!(rows[0]["arm"], "pytest_tail");
        assert_eq!(rows[0]["lang"], "TypeScript");
        assert_eq!(rows[0]["skipped"], "pytest summary parse");
        assert_eq!(rows[1]["arm"], "ast_review");
    }

    /// A lane that runs before any run set the language is not silently Python: the arm is
    /// refused and the row names the absence.
    #[test]
    fn an_unset_language_is_said_as_undetected_not_defaulted_to_python() {
        let sink = ValueSink::default();
        let arms = LangArms::default();
        assert!(!arms.python_only("pytest_tail", "pytest summary parse", &sink));
        let rows = sink.0.lock().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["lang"], "undetected");
        assert_eq!(
            lang_unsupported_event("x", TargetLang::Go, "y")["lang"],
            "Go"
        );
    }
}
