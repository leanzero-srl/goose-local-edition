//! The spec's own BOOT INVOCATIONS — the `python -m X` parsers every boot-facing surface reads
//! (the walking skeleton, the sink's brief, the plan repairs, the gate's argv). Moved verbatim
//! from swarm.rs under the incremental-split law (development_gates::
//! swarm_rs_line_count_only_decreases), paying for VA-142's wiring in the root: the ownership
//! seam's `spec` argument to rule (e) and the skeleton's `skeleton_flags_absent` fan-out.
//! `spec_boot_flags` is the one addition — the FLAGS of an invocation, so the skeleton brief
//! renders `--db-dir P --port N …` instead of the sentence "exactly the flags the spec
//! documents" (r6j's skeleton spent its first three calls grepping the spec for them).

/// The `python3 -m PKG` entry package the spec literally advertises, if any — skipping tool
/// modules (`python3 -m pytest` in a testing note is not the app entry). Pure/testable.
pub(super) fn spec_python_entry(spec: &str) -> Option<String> {
    let re = regex::Regex::new(r"python3?\s+-m\s+([A-Za-z_][\w.]*)").ok()?;
    let names: Vec<String> = re.captures_iter(spec).map(|c| c[1].to_string()).collect();
    names.into_iter().find(|p| {
        !matches!(
            p.as_str(),
            "pytest" | "pip" | "venv" | "unittest" | "http.server" | "compileall" | "build"
        )
    })
}

/// EVERY `python -m X` package the spec advertises (tool modules excluded, deduped,
/// spec order). F910 defect 2's parser: the gate must boot each of these, not just the
/// first — the sb-7 fleet run shipped ledgerd/notifierd packages with no __main__.py and
/// nothing in-run could see it. Pure/testable.
pub(super) fn spec_python_invocations(spec: &str) -> Vec<String> {
    let Ok(re) = regex::Regex::new(r"python3?\s+-m\s+([A-Za-z_][\w.]*)") else {
        return Vec::new();
    };
    let mut seen = std::collections::HashSet::new();
    re.captures_iter(spec)
        .map(|c| c[1].to_string())
        .filter(|p| {
            !matches!(
                p.as_str(),
                "pytest" | "pip" | "venv" | "unittest" | "http.server" | "compileall" | "build"
            )
        })
        .filter(|p| seen.insert(p.clone()))
        .collect()
}

/// The spec's own boot invocation for `pkg`, verbatim with its placeholders — the SHAPE of the
/// argv `run_spec_contract` will spawn. `spec_run_argv_v2` fills the same backtick span, but
/// calling it at dispatch would bind real ephemeral ports and create scratch dirs just to print
/// a prompt string, so the span is quoted as the spec wrote it instead.
pub(super) fn spec_boot_line(spec: &str, pkg: &str) -> Option<String> {
    let needle = format!("-m {pkg}");
    let span = spec.split('`').find(|s| s.contains(&needle))?;
    Some(span.trim().to_string())
}

/// The FLAGS the spec documents for ONE invocation — the text after `-m {inv}` in its own
/// backtick span (`--db-dir P --port M` for `python -m app.notifierd --db-dir P --port M`).
/// Bounded on the invocation: `-m app` never reads `-m app.ledgerd`'s span, which
/// `spec_boot_line`'s substring search would (sb-7 lists `app` first, so that caller is
/// right by order alone). `None` when the spec carries no backtick boot span for the
/// invocation — the caller SAYS so (`skeleton_flags_absent`); `Some("")` when the span exists
/// and carries no flags, which is a boot with none, not an absence.
pub(super) fn spec_boot_flags(spec: &str, inv: &str) -> Option<String> {
    let needle = format!("-m {inv}");
    let bounded_at = |s: &str| {
        s.match_indices(&needle).map(|(at, _)| at).find(|at| {
            s[at + needle.len()..]
                .chars()
                .next()
                .is_none_or(char::is_whitespace)
        })
    };
    spec.split('`')
        .find_map(|s| bounded_at(s).map(|at| s[at + needle.len()..].trim().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_python_invocations_lists_every_advertised_entry_once() {
        let spec = "`python -m app --db-dir P` boots both. `python -m app.ledgerd --port N` alone. `python -m app.notifierd --port M` alone. Test with `python -m pytest`. Again: `python -m app --db-dir P`.";
        assert_eq!(
            spec_python_invocations(spec),
            vec!["app", "app.ledgerd", "app.notifierd"],
            "deduped, spec order, tool modules excluded"
        );
    }

    /// VA-142 (b): the flags are read per invocation from the real sb-7 spec — the composer's
    /// five, each service's own — and the `app` prefix never reads a sub-package's span.
    #[test]
    fn spec_boot_flags_reads_each_invocations_own_span() {
        let spec = include_str!("../../../../../evals/swarm-bench/spec-build-sb7.md");
        assert_eq!(
            spec_boot_flags(spec, "app").as_deref(),
            Some("--db-dir P --ledger-port N --notifier-port M --vendor URL --tokens-file T")
        );
        assert_eq!(
            spec_boot_flags(spec, "app.ledgerd").as_deref(),
            Some("--db-dir P --port N --notifier http://127.0.0.1:M --vendor URL --tokens-file T")
        );
        assert_eq!(
            spec_boot_flags(spec, "app.notifierd").as_deref(),
            Some("--db-dir P --port M")
        );
        // A sub-package listed FIRST is not read as the package's span.
        let reordered = "`python -m app.ledgerd --port N` alone; `python -m app --db-dir P` both.";
        assert_eq!(
            spec_boot_flags(reordered, "app").as_deref(),
            Some("--db-dir P")
        );
        // A bare invocation is a boot with no flags; an unlisted one is an absence.
        assert_eq!(
            spec_boot_flags("run `python3 -m quorum` then curl", "quorum").as_deref(),
            Some("")
        );
        assert_eq!(spec_boot_flags(reordered, "app.notifierd"), None);
    }
}
