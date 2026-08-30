//! The pytest-tail parser: what a test run actually said, read by code instead of trusted from a
//! tool's exit status. Sibling module under the incremental-split law
//! (development_gates::swarm_rs_line_count_only_decreases). Moved verbatim from swarm.rs —
//! behavior unchanged — paying for the r5 defect-routing wiring (`cross_task` on the
//! delivery_defects event) landing in the same commit.

use serde::Serialize;

/// What a pytest tail actually says, parsed by code instead of trusted from a tool's exit status.
///
/// WHY (r2): the live sink's digest recorded `ok: true` on a run whose tail read "32 failed, 46
/// passed" — the suite ran behind `| tail`, the pipeline exited 0, and nothing downstream ever
/// read the counts. Every consumer of "did the tests pass" was reading the WRONG bit. This is the
/// measurement, attached to the captured call row; it changes no behaviour and gates nothing.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub(super) struct PytestSummary {
    passed: u32,
    failed: u32,
    errors: u32,
    /// A collection/import failure — the suite never ran, whatever the counts say. r2's testgen
    /// root files (imports of task ids as modules) produced exactly this shape.
    collect_error: bool,
    /// Names from the short-summary `FAILED path::test` / `ERROR path` lines, capped so a huge
    /// suite cannot bloat a capture row.
    failures: Vec<String>,
}

/// Pure parse of a pytest output tail into counts + failure names; `None` when the text carries no
/// pytest signature at all. Tolerant of the real shapes r2 produced: the `=== N failed, M passed,
/// K warnings in Xs ===` bar, a bare "15 passed, 2 failed", "no tests ran", and the
/// collection-error banner. The LAST count-bearing line wins — a tail can hold several runs.
pub(super) fn parse_pytest_summary(tail: &str) -> Option<PytestSummary> {
    let mut counts: Option<(u32, u32, u32)> = None;
    let mut failures: Vec<String> = Vec::new();
    const MAX_FAILURE_NAMES: usize = 30;
    for line in tail.lines() {
        let trimmed = line.trim();
        // Short-summary attribution lines. Requiring a `.py`/`::` token keeps prose like
        // "ERROR 500" out — every real pytest line names a file or a nodeid.
        for prefix in ["FAILED ", "ERROR "] {
            if let Some(rest) = trimmed.strip_prefix(prefix) {
                if let Some(name) = rest.split_whitespace().next() {
                    if (name.contains(".py") || name.contains("::"))
                        && failures.len() < MAX_FAILURE_NAMES
                        && !failures.iter().any(|f| f == name)
                    {
                        failures.push(name.to_string());
                    }
                }
            }
        }
        if trimmed.contains("no tests ran") {
            counts = Some((0, 0, 0));
            continue;
        }
        // Count tokens: a digit word immediately followed by passed/failed/error(s). Windows over
        // alphanumeric tokens dodge the prose false-positives ("HTTP Error 500", "errors (`400`)").
        let toks: Vec<&str> = trimmed
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|t| !t.is_empty())
            .collect();
        let (mut lp, mut lf, mut le) = (None, None, None);
        for w in toks.windows(2) {
            if let Ok(n) = w[0].parse::<u32>() {
                match w[1] {
                    "passed" => lp = Some(n),
                    "failed" => lf = Some(n),
                    "error" | "errors" => le = Some(n),
                    _ => {}
                }
            }
        }
        if lp.is_some() || lf.is_some() || le.is_some() {
            // ALL THREE reset from this line: "1 failed, 9 warnings" means 0 passed on THIS run,
            // not whatever an earlier bar in the same tail said.
            counts = Some((lp.unwrap_or(0), lf.unwrap_or(0), le.unwrap_or(0)));
        }
    }
    let collect_error = tail.contains("error during collection")
        || tail.contains("errors during collection")
        || tail.contains("ImportError while importing test module");
    if counts.is_none() && !collect_error && failures.is_empty() {
        return None;
    }
    let (passed, failed, errors) = counts.unwrap_or((0, 0, 0));
    Some(PytestSummary {
        passed,
        failed,
        errors,
        collect_error,
        failures,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// II-1's parser, on r2's REAL tails (pulled from the archived run's activity digests). The
    /// load-bearing case is the first: a failing suite behind `| tail` recorded `ok: true`, and
    /// the counts are the only honest bit.
    #[test]
    fn parse_pytest_summary_reads_r2s_real_output_shapes() {
        // ledger-core-tests attempt 0 (ok:true in the live digest — the false green):
        let s = parse_pytest_summary(
            "=================== 7 failed, 19 passed, 13 warnings in 0.29s ===================",
        )
        .unwrap();
        assert_eq!((s.passed, s.failed, s.errors), (19, 7, 0));
        assert!(!s.collect_error);

        // integrate-verify's clean run:
        let s = parse_pytest_summary(
            "============================== 43 passed in 1.40s ==============================",
        )
        .unwrap();
        assert_eq!((s.passed, s.failed, s.errors), (43, 0, 0));

        // A failed-only bar with warnings — passed must read 0, not inherit an earlier line:
        let s = parse_pytest_summary(
            "===== 43 passed in 1.36s =====\n======================== 1 failed, 9 warnings in 0.07s =========================",
        )
        .unwrap();
        assert_eq!((s.passed, s.failed), (0, 1));

        // The bare non-bar form slice-camera-system produced:
        let s = parse_pytest_summary("15 passed, 2 failed").unwrap();
        assert_eq!((s.passed, s.failed), (15, 2));

        // Short-summary names, verbatim from r2:
        let s = parse_pytest_summary(
            "FAILED tests/test_ledger_core.py::TestUpsertIdempotency::test_reversal_idempotent\n\
             FAILED tests/test_ledger_concurrency.py::TestStress::test_high_volume_concurrent_writes\n\
             ========================= 2 failed, 24 passed in 1.11s =========================",
        )
        .unwrap();
        assert_eq!(s.failures.len(), 2);
        assert!(s.failures[0].contains("test_reversal_idempotent"));

        // The testgen-poison collect error (task ids imported as modules):
        let s = parse_pytest_summary(
            "==================================== ERRORS ====================================\n\
             ImportError while importing test module 'test_interfaces.py'.\n\
             E   ModuleNotFoundError: No module named 'app.approval_workflow'\n\
             ERROR test_interfaces.py\n\
             !!!!!!!!!!!!!!!!!!! Interrupted: 1 error during collection !!!!!!!!!!!!!!!!!!!!\n\
             =========================== 1 error in 0.12s ===========================",
        )
        .unwrap();
        assert!(s.collect_error);
        assert_eq!(s.errors, 1);
        assert_eq!(s.failures, vec!["test_interfaces.py".to_string()]);

        // Non-pytest output must yield None — prose 'error' words included (real r2 strings):
        assert!(parse_pytest_summary(
            "urllib.error.HTTPError: HTTP Error 500: Internal Server Error"
        )
        .is_none());
        assert!(parse_pytest_summary("            consecutive_errors = 0").is_none());
        assert!(parse_pytest_summary("ledger-core: all checks passed").is_none());
        assert!(parse_pytest_summary("").is_none());

        // "no tests ran" is a real (empty) run, not a parse miss:
        let s = parse_pytest_summary("no tests ran in 0.01s").unwrap();
        assert_eq!((s.passed, s.failed), (0, 0));
    }
}
