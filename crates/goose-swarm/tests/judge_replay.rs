//! Replay archived `judge_observed` rows through the REAL `deterministic_verdict`.
//!
//! WHY THIS EXISTS: every judge change was being validated by launching a ~100-minute swarm run and
//! reading the resulting log. That is the wrong loop for a PURE FUNCTION. `deterministic_verdict` takes
//! a `JudgeInput` and returns a verdict with no I/O, no model and no fleet — so every archived
//! observation can be re-judged in milliseconds, and a change's effect on the whole corpus is knowable
//! before a single node is booked.
//!
//! It reads the run logs the bench already writes (`evals/swarm-bench/runs/nodeloop/*/run.jsonl`) and
//! is SKIPPED, not failed, when the archive is absent — so a clean checkout and CI stay green.
//!
//! The archive records the raw inputs deliberately (`judge_observed` carries tool_calls,
//! thinking_chars, any_owned_written, secs_since_last_write, owns_files) rather than a re-derived
//! "would_trip" flag, exactly so the predicate can be re-run offline against them.

use goose_swarm::{deterministic_verdict, JudgeConfig, JudgeInput, Verdict};
use std::path::{Path, PathBuf};

/// One archived observation, plus the task id so the replay can reconstruct the file-ownership terms.
struct Row {
    task_id: String,
    elapsed_secs: u64,
    tool_calls: Option<u32>,
    thinking_chars: Option<u64>,
    any_owned_written: bool,
    secs_since_last_write: Option<u64>,
    owns_files: bool,
}

fn archive_dir() -> Option<PathBuf> {
    let d = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .parent()?
        .join("evals/swarm-bench/runs/nodeloop");
    d.is_dir().then_some(d)
}

fn field_u64(line: &str, key: &str) -> Option<u64> {
    let at = line.find(&format!("\"{key}\":"))? + key.len() + 3;
    let rest = line[at..].trim_start();
    let end = rest.find(|c: char| !c.is_ascii_digit())?;
    rest[..end].parse().ok()
}

fn field_bool(line: &str, key: &str) -> Option<bool> {
    let at = line.find(&format!("\"{key}\":"))? + key.len() + 3;
    let rest = line[at..].trim_start();
    if rest.starts_with("true") {
        Some(true)
    } else if rest.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

fn field_str(line: &str, key: &str) -> Option<String> {
    let at = line.find(&format!("\"{key}\":\""))? + key.len() + 4;
    let rest = &line[at..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn load_rows() -> Vec<Row> {
    let Some(dir) = archive_dir() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return out;
    };
    for e in entries.flatten() {
        let log = e.path().join("run.jsonl");
        let Ok(text) = std::fs::read_to_string(&log) else {
            continue;
        };
        for line in text.lines() {
            if !line.contains("\"judge_observed\"") {
                continue;
            }
            let Some(task_id) = field_str(line, "task_id") else {
                continue;
            };
            out.push(Row {
                task_id,
                elapsed_secs: field_u64(line, "elapsed_secs").unwrap_or(0),
                tool_calls: field_u64(line, "tool_calls").map(|v| v as u32),
                thinking_chars: field_u64(line, "thinking_chars"),
                any_owned_written: field_bool(line, "any_owned_written").unwrap_or(false),
                secs_since_last_write: field_u64(line, "secs_since_last_write"),
                owns_files: field_bool(line, "owns_files").unwrap_or(false),
            });
        }
    }
    out
}

/// Rebuild the `JudgeInput` the engine would have built. `owns_files` is the only ownership signal the
/// archive keeps, so a `.py` deliverable stands in for the owned set — which is what every task in the
/// corpus except `web` (an `index.html` owner) actually had.
fn to_input(
    r: &Row,
    prev_calls: Option<u32>,
    prev_think: Option<u64>,
    prev_at: Option<u64>,
    written_ok: bool,
) -> JudgeInput {
    let owned: Vec<String> = if r.owns_files {
        vec![format!("{}.py", r.task_id)]
    } else {
        Vec::new()
    };
    let file_contents = if written_ok {
        owned
            .iter()
            .map(|f| (f.clone(), "x = 1\n".to_string()))
            .collect()
    } else {
        Vec::new()
    };
    JudgeInput {
        task_id: r.task_id.clone(),
        description: String::new(),
        owned_files: owned,
        file_contents,
        compile_errors: Vec::new(),
        elapsed_secs: r.elapsed_secs,
        any_owned_written: r.any_owned_written,
        secs_since_last_write: r.secs_since_last_write,
        worker_tool_calls: r.tool_calls,
        worker_thinking_chars: r.thinking_chars,
        prev_thinking_chars: prev_think,
        prev_tool_calls: prev_calls,
        prev_observed_secs: prev_at,
        split_count: 0,
    }
}

/// A worker that is ACTIVELY MAKING TOOL CALLS must not be killed for missing the first-write deadline.
///
/// This is the F201 defect as a test: the deadline was a bare `elapsed >= 420` with no evidence term,
/// while measured time-to-first-owned-write is p90 475s for implementers and p90 831s (max 1099s) for
/// test-authors — a constant sitting BELOW the p90 of both populations it judged.
#[test]
fn a_worker_still_taking_actions_survives_the_first_write_deadline() {
    let cfg = JudgeConfig::default();
    let base = Row {
        task_id: "test-meridian".into(),
        elapsed_secs: 500,
        tool_calls: Some(9),
        thinking_chars: Some(4_000),
        any_owned_written: false,
        secs_since_last_write: None,
        owns_files: true,
    };
    // Tool calls climbing 4 -> 9 between observations: this worker is doing things.
    let producing = to_input(
        &base,
        Some(4),
        Some(3_000),
        Some(base.elapsed_secs - 60),
        false,
    );
    assert!(
        deterministic_verdict(&producing, &cfg).is_none(),
        "a worker whose tool-call count is still climbing was killed at {}s; the deadline needs an \
         evidence term, not just a stopwatch",
        base.elapsed_secs
    );
    // Same instant, but the counters are FLAT — nothing is happening. This one must still be caught.
    let stalled = to_input(
        &base,
        Some(9),
        Some(4_000),
        Some(base.elapsed_secs - 60),
        false,
    );
    let v = deterministic_verdict(&stalled, &cfg).expect("a stalled worker is still caught");
    assert_eq!(v.verdict, Verdict::OverReading);
    assert!(v.deterministic);
}

/// A ZERO-tool-call worker is stuck before its first byte — it did not "over-read", it read nothing.
#[test]
fn a_worker_that_ran_no_command_is_labelled_no_first_write() {
    let cfg = JudgeConfig::default();
    let r = Row {
        task_id: "test-api".into(),
        elapsed_secs: 467,
        tool_calls: Some(0),
        thinking_chars: Some(10_743),
        any_owned_written: false,
        secs_since_last_write: None,
        owns_files: true,
    };
    let v = deterministic_verdict(
        &to_input(&r, Some(0), Some(9_000), Some(r.elapsed_secs - 60), false),
        &cfg,
    )
    .expect("the deadline still fires on a silent worker");
    assert_eq!(
        v.verdict,
        Verdict::NoFirstWrite,
        "a worker with tool_calls == 0 must not be recorded as `over_reading` — that label sent three \
         separate analyses down a false causal chain"
    );
}

/// A COMPLETE deliverable must be ACCEPTED, not killed for spinning.
///
/// F165: `test-meridian` was recorded a TERMINAL FAILURE with its file on disk carrying 8 passing test
/// functions that the crunched app still runs. Without an accept verdict the judge's only lever is
/// kill, and the third kill is terminal.
#[test]
fn a_finished_deliverable_is_accepted_rather_than_failed() {
    let cfg = JudgeConfig::default();
    let r = Row {
        task_id: "test-meridian".into(),
        elapsed_secs: 1_656,
        tool_calls: Some(6),
        thinking_chars: Some(7_674),
        any_owned_written: true,
        secs_since_last_write: Some(600),
        owns_files: true,
    };
    let v = deterministic_verdict(
        &to_input(&r, Some(6), Some(7_674), Some(r.elapsed_secs - 60), true),
        &cfg,
    )
    .expect("an idle worker with a finished file still gets a verdict");
    assert_eq!(
        v.verdict,
        Verdict::Accept,
        "a complete, compiling deliverable must finish, not fail"
    );
    assert!(
        !v.verdict.is_problem(),
        "Accept must never reach the intervention path"
    );
}

/// THE CORPUS REPLAY. Re-judges every archived observation and reports what the current predicate does
/// to it. Prints a summary and asserts only the invariant that must hold for any archive: the engine
/// never labels a zero-tool-call worker `over_reading`.
#[test]
fn replay_the_whole_archive() {
    let rows = load_rows();
    if rows.is_empty() {
        eprintln!("no archive under evals/swarm-bench/runs/nodeloop — replay skipped");
        return;
    }
    let cfg = JudgeConfig::default();
    let mut counts: std::collections::BTreeMap<&'static str, usize> = Default::default();
    let mut mislabelled = 0usize;
    let (mut prev_calls, mut prev_think, mut prev_at) = (None, None, None);
    let mut last_task = String::new();
    for r in &rows {
        if r.task_id != last_task {
            prev_calls = None;
            prev_think = None;
            prev_at = None;
            last_task = r.task_id.clone();
        }
        let input = to_input(r, prev_calls, prev_think, prev_at, r.any_owned_written);
        match deterministic_verdict(&input, &cfg) {
            Some(o) => {
                *counts.entry(o.verdict.as_str()).or_default() += 1;
                if o.verdict == Verdict::OverReading && r.tool_calls == Some(0) {
                    mislabelled += 1;
                }
            }
            None => *counts.entry("(no verdict)").or_default() += 1,
        }
        prev_calls = r.tool_calls;
        prev_think = r.thinking_chars;
        prev_at = Some(r.elapsed_secs);
    }
    eprintln!("replayed {} archived observations: {counts:?}", rows.len());
    assert_eq!(
        mislabelled, 0,
        "{mislabelled} observations were labelled `over_reading` with tool_calls == 0"
    );
}

/// WHAT THE SHIPPED CHANGES ACTUALLY DO TO THE ARCHIVE.
///
/// The corpus was produced by the OLD engine, so every `judge_verdict` in it is that engine's answer.
/// Re-judging the same `judge_observed` rows with the CURRENT predicate and diffing the two gives the
/// predicted effect of the change set — in milliseconds, on real data, before any run confirms it.
///
/// Registered as a PREDICTION, not a result: the run in flight is what confirms or refutes it.
///
/// ⚠ APPROXIMATION, stated because it bounds the claim: the archive keeps `owns_files` but not the
/// owned PATHS, so `to_input` synthesises one `.py` deliverable per task. That is right for every task
/// in the corpus except `web` (an `index.html` owner, which `is_code_deliverable` correctly exempts),
/// so this OVERSTATES the population the deadline can touch by exactly that one task.
#[test]
fn quantify_the_change_against_the_recorded_verdicts() {
    let rows = load_rows();
    if rows.is_empty() {
        eprintln!("no archive — quantification skipped");
        return;
    }
    let cfg = JudgeConfig::default();
    // The 11 archived deadline trips, by their measured (tool_calls, elapsed) signature.
    let mut trips_now_survive = 0usize;
    let mut trips_still_fire = 0usize;
    let mut accepts = 0usize;
    let (mut prev_calls, mut prev_think, mut prev_at) = (None, None, None);
    let mut last = String::new();
    for r in &rows {
        if r.task_id != last {
            prev_calls = None;
            prev_think = None;
            last = r.task_id.clone();
        }
        let input = to_input(r, prev_calls, prev_think, prev_at, r.any_owned_written);
        let now = deterministic_verdict(&input, &cfg);
        // An archived row that WOULD have tripped the old bare stopwatch: owns code, nothing written,
        // past 420s. That predicate had no evidence term, so this is exactly the old branch.
        let old_would_trip = r.owns_files && !r.any_owned_written && r.elapsed_secs >= 420;
        if old_would_trip {
            match &now {
                None => trips_now_survive += 1,
                Some(_) => trips_still_fire += 1,
            }
        }
        if now.as_ref().is_some_and(|o| o.verdict == Verdict::Accept) {
            accepts += 1;
        }
        prev_calls = r.tool_calls;
        prev_think = r.thinking_chars;
    }
    eprintln!(
        "PREDICTED EFFECT on {} archived observations:\n  \
         deadline trips that NOW SURVIVE (worker was still acting): {}\n  \
         deadline trips that STILL FIRE (worker genuinely stalled): {}\n  \
         observations that NOW yield Accept instead of a kill:      {}",
        rows.len(),
        trips_now_survive,
        trips_still_fire,
        accepts
    );
    assert!(
        trips_still_fire > 0,
        "the deadline must still catch a genuinely stalled worker — a change that disarms it entirely \
         would trade one failure mode for a worse one"
    );
}

/// A tool-call increase seen across a 21-MINUTE gap is not evidence that the worker is producing NOW.
///
/// THE DEFECT, measured on `swarm-3node-r0`. The judge runs only when a device is idle, and that run
/// suppressed 66 of 72 opportunities as `no_idle_device`. Consecutive observations of the same attempt
/// therefore had a median gap of 60s but a MAX of 1,267s. `test-meridian` was observed at 360s with 0
/// tool calls and again at 1,627s with 8 — a genuine increase, but spread over 21 minutes during which
/// `secs_since_last_write` had reached 705s. `is_still_producing` returned true on that pair, which
/// BLOCKED the Accept branch and DOUBLED the stall deadline for a worker that had finished its files
/// twelve minutes earlier and was holding one of six fleet slots at 27 minutes.
///
/// The fix is not a new threshold: the increase must be observed inside the SAME window the predicate is
/// overriding (`min_age_secs.max(420)`), so a predicate can never veto a staleness rule using evidence
/// coarser than that rule.
#[test]
fn an_action_increase_across_a_stale_gap_does_not_count_as_producing() {
    let cfg = JudgeConfig::default();
    let r = Row {
        task_id: "test-meridian".into(),
        elapsed_secs: 1_627,
        tool_calls: Some(8),
        thinking_chars: Some(2_007),
        any_owned_written: true,
        secs_since_last_write: Some(705),
        owns_files: true,
    };
    // The real pair from the run: previous look at 360s with 0 calls, this look at 1627s with 8.
    let stale = to_input(&r, Some(0), Some(1_048), Some(360), true);
    let v = deterministic_verdict(&stale, &cfg)
        .expect("a worker idle for 705s must not be protected by a 21-minute-old action count");
    assert!(v.deterministic);

    // CONTROL, the other direction: the SAME counts observed 60s apart are real production and must
    // still suppress the verdict. Without this the test would pass for a predicate that never fires.
    let fresh = to_input(&r, Some(0), Some(1_048), Some(1_627 - 60), true);
    assert!(
        deterministic_verdict(&fresh, &cfg).is_none(),
        "a worker that made 8 tool calls in the last 60s is producing and must not be killed"
    );
}
