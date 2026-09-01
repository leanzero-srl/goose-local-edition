//! The engine's stub predicate — the ONE definition of "this file is still the skeleton the
//! dispatcher pre-created" that the sink's stub check (`judge_context::lane_defect_view`), the
//! deliverable gate and the scheduler's dependency census all share. It lived in `judge.rs`
//! until the idle-model judge was deleted (2c S6, 2026-09-01: 0 `judge_verdict` events in every
//! measured run since the desktop pinned `GOOSE_SWARM_JUDGE=0`); the predicate outlived its
//! first consumer because the others were never about the judge.

/// F884: is this file still the engine's own UNIMPLEMENTED SKELETON? The dispatcher pre-creates
/// owned files as signature stubs (`def f(...) -> T: ...`) so imports resolve during the fan —
/// which means "the owned file exists and is non-empty" is true at t=0 for every task, before its
/// worker has done anything at all. MEASURED (run 10): the meridian worker ran 585s with ZERO tool
/// calls, and the deterministic Accept read the engine's own 274-byte skeleton as "the deliverable
/// is complete". A file counts as skeleton-only when it declares at least one class/def and every
/// body is `...` / `pass` / `raise NotImplementedError` / a docstring — i.e. the worker added no
/// executable statement to what the engine wrote for it.
pub fn skeleton_only(content: &str) -> bool {
    let mut saw_decl = false;
    let mut doc_quote: Option<&'static str> = None;
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(q) = doc_quote {
            if line.contains(q) {
                doc_quote = None;
            }
            continue;
        }
        let triple_double = "\"\"\"";
        let triple_single = "'''";
        if let Some(rest) = line
            .strip_prefix(triple_double)
            .or_else(|| line.strip_prefix(triple_single))
        {
            let q: &'static str = if line.starts_with('"') {
                "\"\"\""
            } else {
                "'''"
            };
            if !rest.contains(q) {
                doc_quote = Some(q);
            }
            continue;
        }
        if line.starts_with("import ") || line.starts_with("from ") || line.starts_with('@') {
            continue;
        }
        let is_decl = line.starts_with("class ")
            || line.starts_with("def ")
            || line.starts_with("async def ");
        if is_decl {
            saw_decl = true;
            // A one-liner carries its body after the LAST colon: `def f(x: int) -> list[dict]: ...`.
            let body = line.rsplit(':').next().unwrap_or("").trim();
            if body.is_empty() || body == "..." || body == "pass" {
                continue;
            }
            return false;
        }
        if line == "..." || line == "pass" || line.starts_with("raise NotImplementedError") {
            continue;
        }
        return false;
    }
    saw_decl
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_engines_own_skeleton_is_not_a_deliverable() {
        let run10_meridian = "class MeridianClient:\n    def __init__(self, base_url: str, api_key: str) -> None: ...\n    def fetch_all_payments(self) -> list[dict]: ...\n    def total_count(self) -> int: ...\n    def create_payment(self, amount_minor: int, currency: str, idempotency_key: str) -> str: ...\n";
        assert!(skeleton_only(run10_meridian));
        // pass-bodies and NotImplementedError count as stubs too.
        assert!(skeleton_only(
            "class A:\n    def f(self):\n        pass\n    def g(self):\n        raise NotImplementedError\n"
        ));
        // Docstrings do not make a stub real.
        assert!(skeleton_only(
            "def f():\n    \"\"\"Fetch everything.\"\"\"\n    ...\n"
        ));
        // ONE real statement anywhere makes it a deliverable.
        assert!(!skeleton_only(
            "class A:\n    def f(self):\n        return 1\n    def g(self): ...\n"
        ));
        // An import-only shim declares nothing — not "skeleton", just thin (other checks own it).
        assert!(!skeleton_only("from .client import MeridianClient\n"));
        // Empty file: nothing declared, not a skeleton.
        assert!(!skeleton_only(""));
    }
}
