//! Path-token prose rewriting for the plan repairs. Sibling module under the incremental-split
//! law (development_gates::swarm_rs_line_count_only_decreases): when a repair renames a path in a
//! task's `files[]`, this renames the same token in that task's DESCRIPTION, so the words the
//! model reads agree with the metadata the scheduler reads.
//!
//! WHY (r6c, 2026-08-31, run swarm-3node-r0 seq 1385/1386): `repair_module_package_collisions`
//! rewrote ledgerd-core's `app/ledgerd.py` to `app/ledgerd/impl.py` in files[] — loud and correct
//! — but the description still opened "Own app/__main__.py ... app/ledgerd.py (ledgerd
//! entrypoint: ...)". The live skeleton lane read the PROSE and tripped on the contradiction:
//! "ledgerd-core lists app/ledgerd.py (not a package!) ... impl.py — owned by whom? Not me". The
//! description is what the MODEL reads (specificity gate); a repair that fixes the metadata but
//! not the words ships a contradiction to every reader. MILD: text repair, never a refusal.

/// Replace standalone occurrences of the path token `old` with `new`; returns the rewritten text
/// and how many occurrences changed. Boundary-aware so a short path cannot mangle a longer
/// sibling token: a hit is skipped when it is glued into a longer token — preceded by
/// [A-Za-z0-9_./-] (`myapp/ledgerd.py` and `src/app/ledgerd.py` are DIFFERENT paths from
/// `app/ledgerd.py`) or followed by [A-Za-z0-9_] (the `app/ledgerd.pyc` class). Punctuation,
/// backticks, quotes and whitespace on either side are prose, and the token is replaced.
// string_slice: every index is a char boundary by construction — `i` and `i + old.len()` come
// from `match_indices`, and `last` is always 0 or a previous match end. The adjacent-byte glue
// checks are byte-safe: every glue character is ASCII, so a multi-byte char's continuation byte
// (>= 0x80) correctly reads as not-glued.
#[allow(clippy::string_slice)]
pub(super) fn rewrite_path_token(text: &str, old: &str, new: &str) -> (String, usize) {
    if old.is_empty() || old == new {
        return (text.to_string(), 0);
    }
    let bytes = text.as_bytes();
    let glued_before = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'/' | b'-');
    let glued_after = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut out = String::with_capacity(text.len());
    let mut last = 0usize;
    let mut count = 0usize;
    for (i, _) in text.match_indices(old) {
        if i < last {
            continue;
        }
        let standalone_before = i == 0 || !glued_before(bytes[i - 1]);
        let end = i + old.len();
        let standalone_after = end >= bytes.len() || !glued_after(bytes[end]);
        if standalone_before && standalone_after {
            out.push_str(&text[last..i]);
            out.push_str(new);
            last = end;
            count += 1;
        }
    }
    out.push_str(&text[last..]);
    (out, count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_standalone_path_token_is_rewritten_wherever_prose_holds_it() {
        let (out, n) = rewrite_path_token(
            "Own app/ledgerd.py (entrypoint), see `app/ledgerd.py`, ends with app/ledgerd.py.",
            "app/ledgerd.py",
            "app/ledgerd/impl.py",
        );
        assert_eq!(n, 3);
        assert_eq!(
            out,
            "Own app/ledgerd/impl.py (entrypoint), see `app/ledgerd/impl.py`, ends with \
             app/ledgerd/impl.py."
        );
    }

    #[test]
    fn a_glued_longer_token_is_never_mangled() {
        for prose in [
            "keep myapp/ledgerd.py as-is",
            "keep src/app/ledgerd.py as-is",
            "keep app/ledgerd.pyc as-is",
            "keep app/ledgerd.py_backup as-is",
            "keep pre-app/ledgerd.py as-is",
        ] {
            let (out, n) = rewrite_path_token(prose, "app/ledgerd.py", "app/ledgerd/impl.py");
            assert_eq!(n, 0, "{prose}");
            assert_eq!(out, prose);
        }
    }

    /// The r6c shape verbatim: files[] rewritten to the package form while the description still
    /// named the shadowed module. After the repair the prose carries the rewritten path, the
    /// longer sibling token is untouched, and the action reports the prose rewrite count.
    #[test]
    fn a_repaired_tasks_description_carries_no_stale_path_token() {
        let mut plan = serde_json::json!({"subtasks": [
            {"id": "ledgerd-core",
             "files": ["app/ledgerd.py", "app/db.py"],
             "depends_on": [],
             "description": "Own app/ledgerd.py (ledgerd entrypoint: bind and listen within 10s \
                             even while the vendor is down), app/db.py (schema). Leave \
                             app/ledgerd_util.py to its own task."},
            {"id": "skeleton",
             "files": ["app/ledgerd/__main__.py", "app/ledgerd/__init__.py"],
             "depends_on": [],
             "description": "walking skeleton"},
        ]});
        let r = super::super::repair_plan_flags(&mut plan, "", super::super::TargetLang::Python);
        let files: Vec<&str> = plan["subtasks"][0]["files"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|f| f.as_str())
            .collect();
        assert!(files.contains(&"app/ledgerd/impl.py"), "{files:?}");
        let desc = plan["subtasks"][0]["description"].as_str().unwrap();
        assert!(
            !desc.contains("app/ledgerd.py"),
            "the stale module path survived in the prose the model reads: {desc}"
        );
        assert!(
            desc.contains("app/ledgerd/impl.py (ledgerd entrypoint"),
            "{desc}"
        );
        assert!(
            desc.contains("app/ledgerd_util.py"),
            "boundary: the longer sibling token must stay untouched: {desc}"
        );
        assert!(
            r.actions.iter().any(|a| a.contains("prose_rewrites: 1")),
            "the action reports the prose rewrite count: {:?}",
            r.actions
        );
    }
}
