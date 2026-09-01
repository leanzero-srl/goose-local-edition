//! The ledger block — §II.2 MESSAGE FORMATION's read-before-act renderer over
//! `.swarm/ledger.json`, moved verbatim from swarm.rs under the incremental-split law
//! (development_gates::swarm_rs_line_count_only_decreases) to pay for the VA-030 D11 wiring in
//! the root. Two changes rode the move: the plan-time DECISIONS render WHOLE and outside the
//! measured-state budget, and every budget drop is RECORDED (`LedgerBlock::dropped`) so the sink's
//! dispatch can name what its reader never saw (`ledger_block_section_dropped`).

use std::path::Path;

use super::decisions::DECISION_SLICE;
use super::orientation::head_to_sentence_end;
use super::research::RESEARCH_ANSWERED;
use super::supervision::tail_chars;

/// The rendered block plus what the budget removed: `(section, chars)` per dropped section, in
/// drop order; `measured_state_truncated` carries the chars the line-boundary cut removed.
#[derive(Default)]
pub(super) struct LedgerBlock {
    pub(super) text: String,
    pub(super) dropped: Vec<(&'static str, usize)>,
}

/// The block as a string — the tests' form; production reads `render_ledger_block_measured`.
#[cfg(test)]
pub(super) fn render_ledger_block(
    root: &Path,
    task_id: &str,
    deps: &[String],
    all_files: &[String],
    budget: usize,
    collect_only: Option<&str>,
) -> String {
    render_ledger_block_measured(root, task_id, deps, all_files, budget, collect_only).text
}

/// The roll-up as the renderers consume it. None (ledger absent/unreadable) must render as an
/// EMPTY block downstream — the dispatch then proceeds byte-identical, never blocked.
pub(super) fn read_ledger_rollup(root: &Path) -> Option<serde_json::Value> {
    serde_json::from_str(&std::fs::read_to_string(root.join(".swarm").join("ledger.json")).ok()?)
        .ok()
}

/// F196-style truncation for the ledger block: cut on a LINE boundary and say so. A block cut
/// mid-line reads as a complete fact that is wrong; the never-drop content is rendered at the
/// top precisely so a tail cut can only eat the droppable end.
pub(super) fn truncate_block_at_line(s: &str, budget: usize) -> String {
    if s.chars().count() <= budget {
        return s.to_string();
    }
    let head: String = s.chars().take(budget.saturating_sub(60)).collect();
    let whole = head.rsplit_once('\n').map(|(h, _)| h).unwrap_or(&head);
    format!("{whole}\n… LEDGER TRUNCATED — the full state is in .swarm/ledger.json.\n")
}

/// §II.2 MESSAGE FORMATION — the read-before-act block, one renderer for every consumer (today
/// the sink's brief; a REPAIR shard reads `render_repair_history`). Pure over `.swarm/ledger.json`
/// plus an optional engine-run collect-only tail the CALLER produced (keeping the render itself
/// free of subprocesses). Absent/unreadable/empty ledger renders "" and the dispatch proceeds
/// byte-identical — the injection IS the whole mechanism, nothing is ever blocked on it.
///
/// Content order is the drop order in reverse: open defects, gate findings and NOT FIXED
/// verdicts first (never dropped), then the test table and the §II.3 don't-repeat facts, then
/// per-class failure tails, and only then the droppables — ok-only command classes and each
/// lane's final_text, removed in that order when over budget, with a line-boundary truncation
/// as the last resort. No time value orders or gates anything here. Every drop is RECORDED in
/// `LedgerBlock::dropped` (VA-030 D11) so the caller can name it. The plan-time DECISIONS render
/// WHOLE and OUTSIDE the budget — see `decisions_section` below.
pub(super) fn render_ledger_block_measured(
    root: &Path,
    task_id: &str,
    deps: &[String],
    all_files: &[String],
    budget: usize,
    collect_only: Option<&str>,
) -> LedgerBlock {
    let Some(rollup) = read_ledger_rollup(root) else {
        return LedgerBlock::default();
    };
    let empty_map = serde_json::Map::new();
    let tasks = rollup
        .get("tasks")
        .and_then(|t| t.as_object())
        .unwrap_or(&empty_map);
    if tasks.is_empty()
        && rollup
            .get("gate")
            .and_then(|g| g.as_array())
            .is_none_or(|v| v.is_empty())
        && rollup
            .pointer("/repair/rounds")
            .and_then(|r| r.as_array())
            .is_none_or(|v| v.is_empty())
        // A plan-time-only ledger (research rows, nothing built yet) still INFORMS.
        && rollup
            .get("research")
            .and_then(|r| r.as_array())
            .is_none_or(|v| v.is_empty())
    {
        return LedgerBlock::default();
    }
    // Dependencies' rows carry the facts THIS task builds on — they render before strangers'.
    let ordered_tasks: Vec<(&String, &serde_json::Value)> = {
        let mut v: Vec<_> = tasks.iter().collect();
        v.sort_by_key(|(id, _)| (!deps.contains(*id), (*id).clone()));
        v
    };

    let mut never = String::new();
    never.push_str(
        "MEASURED STATE OF THIS TREE — read before acting. Every line is a stat, a command \
         record, or a gate probe from this run's own ledger; trust it over re-deriving.\n",
    );
    // GEN-6a #3: an incomplete table must SAY it is incomplete — a truncated roll-up read as
    // whole is how a dependent builds on history that is not there.
    if let Some(dropped) = rollup.get("rows_dropped").and_then(|d| d.as_array()) {
        if !dropped.is_empty() {
            never.push_str(&format!(
                "WARNING — {} ledger row(s) were UNREADABLE and are missing from this table \
                 (the history below is INCOMPLETE): {}\n",
                dropped.len(),
                dropped
                    .iter()
                    .map(|r| {
                        format!(
                            "{} ({})",
                            r.get("file").and_then(|f| f.as_str()).unwrap_or("?"),
                            r.get("error").and_then(|e| e.as_str()).unwrap_or("?")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
    }
    let open_defects: Vec<&str> = rollup
        .get("open_defects")
        .and_then(|d| d.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
        .unwrap_or_default();
    if !open_defects.is_empty() {
        never
            .push_str("OPEN DEFECTS (re-stat'd at the last ledger write; a fixed one vanishes):\n");
        for d in &open_defects {
            never.push_str(&format!("  - {d}\n"));
        }
    }
    // r5 item 3: a measured fact, deliberately NOT under "OPEN DEFECTS" — an extra file may be a
    // documented plan decision (r5's brush.js was), so the line hands the excess to the reader
    // with the doc as the decider, never as an instruction to delete.
    if let Some(exceeded) = rollup.get("spec_set_exceeded").and_then(|d| d.as_array()) {
        for f in exceeded {
            let listify = |key: &str| -> String {
                match f.get(key).and_then(|v| v.as_array()) {
                    Some(a) => a
                        .iter()
                        .filter_map(|x| x.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    // Empty means empty: exceeded_facts builds every fact with these arrays; the
                    // one arm without them (the unparseable-sidecar fact) carries its own `error`
                    // field, so nothing-to-list here IS the truth, not a swallowed failure.
                    None => String::new(),
                }
            };
            never.push_str(&format!(
                "SPEC-ENUMERATED FILE SET EXCEEDED — the spec freezes {}/ to [{}]; the tree also \
                 holds: [{}]. An extra file may be a documented decision: verify it is named in \
                 the delivered docs and stays inside any budget the spec counts over the frozen \
                 set; never delete on this line alone.\n",
                f.get("area").and_then(|a| a.as_str()).unwrap_or("?"),
                listify("frozen"),
                listify("extra"),
            ));
        }
    }
    if let Some(gates) = rollup.get("gate").and_then(|g| g.as_array()) {
        if let Some(g) = gates.last() {
            let findings: Vec<&str> = g
                .get("findings")
                .and_then(|f| f.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
                .unwrap_or_default();
            if !findings.is_empty() {
                never.push_str(&format!(
                    "GATE (round {}) found, against the RUNNING app:\n",
                    g.get("round").and_then(|r| r.as_u64()).unwrap_or(0)
                ));
                for f in &findings {
                    never.push_str(&format!("  - {f}\n"));
                }
            }
        }
    }
    if let Some(rounds) = rollup.pointer("/repair/rounds").and_then(|r| r.as_array()) {
        let mut lines = String::new();
        for r in rounds {
            let round = r.get("round").and_then(|x| x.as_u64()).unwrap_or(0);
            let shard = r.get("shard").and_then(|s| s.as_str()).unwrap_or("?");
            for v in r
                .get("verdicts")
                .and_then(|v| v.as_array())
                .unwrap_or(&Vec::new())
            {
                let verdict = v.get("verdict").and_then(|s| s.as_str()).unwrap_or("");
                // FIXED and NOT REAL are history; NOT FIXED is a live instruction — an
                // approach that already failed, which the next attempt must not repeat.
                if verdict != "NOT FIXED" {
                    continue;
                }
                lines.push_str(&format!(
                    "  - round {round}, {shard}: FINDING {} NOT FIXED — {}{}\n",
                    v.get("n").and_then(|n| n.as_u64()).unwrap_or(0),
                    v.get("detail").and_then(|d| d.as_str()).unwrap_or(""),
                    v.get("finding")
                        .and_then(|f| f.as_str())
                        .map(|f| format!(" (finding: {f})"))
                        .unwrap_or_default(),
                ));
            }
        }
        if !lines.is_empty() {
            never.push_str(
                "ALREADY TRIED AND NOT FIXED (do not retry the same approach — try a different one):\n",
            );
            never.push_str(&lines);
        }
    }
    let mut outside: std::collections::BTreeSet<String> = Default::default();
    for (id, t) in &ordered_tasks {
        for p in t
            .pointer("/fs_delta/outside_manifest")
            .and_then(|x| x.as_array())
            .unwrap_or(&Vec::new())
        {
            if let Some(s) = p.as_str() {
                outside.insert(format!("{s} (written during `{id}`)"));
            }
        }
    }
    if !outside.is_empty() {
        never.push_str("FILES OUTSIDE THE PLAN — they exist on disk but NO task owns them:\n");
        for o in &outside {
            never.push_str(&format!("  - {o}\n"));
        }
    }

    let mut mid = String::new();
    {
        // The test table: every test file the ledger knows, its owning lane, how often that
        // lane ran pytest, and the lane's last recorded outcome — the state the r2 sink paid
        // 3 whole-suite re-runs to learn.
        let mut table = String::new();
        for (id, t) in &ordered_tasks {
            let owned: Vec<&str> = t
                .get("owned_files")
                .and_then(|o| o.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|f| f.get("path").and_then(|p| p.as_str()))
                        .collect()
                })
                .unwrap_or_default();
            let test_files: Vec<&str> = owned
                .iter()
                .copied()
                .filter(|f| {
                    let base = f.rsplit('/').next().unwrap_or(f);
                    base.starts_with("test_") || base.ends_with("_test.py")
                })
                .collect();
            if test_files.is_empty() {
                continue;
            }
            let runs = t
                .pointer("/commands/test/count")
                .and_then(|c| c.as_u64())
                .unwrap_or(0);
            let outcome = t
                .pointer("/last_pytest_filewide/summary/raw")
                .and_then(|r| r.as_str())
                .or_else(|| {
                    t.pointer("/last_pytest/summary/raw")
                        .and_then(|r| r.as_str())
                })
                .map(|r| format!("last: {r}"))
                .unwrap_or_else(|| "never run by its lane".to_string());
            table.push_str(&format!(
                "  - {} — lane `{id}`: pytest ran {runs}x, {outcome}\n",
                test_files.join(" + "),
            ));
        }
        if !table.is_empty() {
            mid.push_str("TEST TABLE — this IS the suite's state:\n");
            mid.push_str(&table);
        }
        if let Some(c) = collect_only {
            mid.push_str(&format!(
                "  `pytest --collect-only` has ALREADY been run; it FAILS at import:\n    {}\n",
                c.trim().replace('\n', "\n    ")
            ));
        }
        // §II.3 — the don't-repeat facts as INPUT, never a block: no tool is blocked, no retry
        // refused; the model is handed the table instead of paying ~79 s a turn to re-derive it.
        let total_runs: u64 = tasks
            .values()
            .filter_map(|t| t.pointer("/commands/test/count").and_then(|c| c.as_u64()))
            .sum();
        let lanes = tasks
            .values()
            .filter(|t| {
                t.pointer("/commands/test/count")
                    .and_then(|c| c.as_u64())
                    .unwrap_or(0)
                    > 0
            })
            .count();
        if total_runs > 0 {
            let last = rollup
                .pointer("/last_full_suite/summary/raw")
                .and_then(|r| r.as_str())
                .map(|r| format!(" Last full run: {r} — the failing names are in the table above."))
                .unwrap_or_default();
            mid.push_str(&format!(
                "DO NOT RE-DERIVE: pytest already ran {total_runs} time(s) across {lanes} lane(s) \
                 before this dispatch.{last} Do not re-run the whole suite to learn its state; run \
                 a single test only after an edit that targets its failure.\n"
            ));
        }
        // The last failure per command class, dependencies first — the error text the next
        // action should start from instead of reproducing it.
        let mut fails = String::new();
        for (id, t) in &ordered_tasks {
            for (class, c) in t
                .get("commands")
                .and_then(|c| c.as_object())
                .unwrap_or(&serde_json::Map::new())
            {
                let tail = c
                    .get("last_failure_tail")
                    .and_then(|x| x.as_str())
                    .unwrap_or("");
                if !tail.is_empty() {
                    fails.push_str(&format!("  - `{id}` {class}: {tail}\n"));
                }
            }
        }
        if !fails.is_empty() {
            mid.push_str("LAST FAILURE per lane and command class (start from this error text):\n");
            mid.push_str(&fails);
        }
    }

    // Droppables, in drop order: the plan-time research answers go first (they are the
    // judge_established class this slot was reserved for — settled decisions, not measured
    // state), then ok-only command classes, then lane self-reports. Dropped whole on overflow —
    // never a defect, a gate finding, or a NOT FIXED verdict.
    let research_section = {
        let mut s = String::new();
        for r in rollup
            .get("research")
            .and_then(|r| r.as_array())
            .unwrap_or(&Vec::new())
        {
            if r.get("slice").and_then(|x| x.as_str()) == Some(DECISION_SLICE) {
                // A decision renders WHOLE in `decisions_section` — never as a 400-char head.
                continue;
            }
            if r.get("status").and_then(|x| x.as_str()) != Some(RESEARCH_ANSWERED) {
                // An unanswered question is not re-stated here: its absence already rode
                // `research_unanswered` and the owning brief carries the raw question.
                continue;
            }
            let q = r.get("question").and_then(|x| x.as_str()).unwrap_or("?");
            let a = r.get("answer").and_then(|x| x.as_str()).unwrap_or("");
            s.push_str(&format!(
                "  - Q: {q}\n    A: {}\n",
                head_to_sentence_end(a, 400).replace('\n', " ")
            ));
        }
        if s.is_empty() {
            s
        } else {
            format!(
                "SETTLED AT PLAN TIME — research answers (rank below the measured state above; \
                 the spec and USER DECISIONS outrank these):\n{s}"
            )
        }
    };
    let ok_classes = {
        let mut s = String::new();
        for (id, t) in &ordered_tasks {
            let classes: Vec<String> = t
                .get("commands")
                .and_then(|c| c.as_object())
                .unwrap_or(&serde_json::Map::new())
                .iter()
                .filter(|(_, c)| {
                    c.get("last_failure_tail")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .is_empty()
                })
                .map(|(class, c)| {
                    format!(
                        "{class} {}x",
                        c.get("count").and_then(|x| x.as_u64()).unwrap_or(0)
                    )
                })
                .collect();
            if !classes.is_empty() {
                s.push_str(&format!("  - `{id}` ran clean: {}\n", classes.join(", ")));
            }
        }
        if s.is_empty() {
            s
        } else {
            format!("COMMANDS THAT RAN CLEAN (no need to repeat them):\n{s}")
        }
    };
    let final_texts = {
        let mut s = String::new();
        for (id, t) in &ordered_tasks {
            if *id == task_id {
                continue;
            }
            let text = t
                .get("final_text")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim();
            if !text.is_empty() {
                s.push_str(&format!("  - `{id}` said: {}\n", tail_chars(text, 200)));
            }
        }
        if s.is_empty() {
            s
        } else {
            format!("WHAT EACH LANE SAID IT DELIVERED (self-reports — rank below the stats above):\n{s}")
        }
    };

    let _ = all_files; // manifest facts arrive via fs_delta's outside_manifest, computed at write

    // VA-030 D11: the plan-time DECISIONS — the conventions the sink certifies the tree against
    // — render WHOLE and OUTSIDE the measured-state budget. A decision cut to its first 400 chars
    // is the convention the sink cannot check, and r6c's three ran 2,562/3,066/3,243 chars: the
    // 7,000 budget could not hold them beside the measured state, so they were the first section
    // dropped, silently. They are the plan's binding text, not measured state, so the budget
    // (which orders measured state) does not count them.
    let decisions_section = {
        let mut s = String::new();
        for r in rollup
            .get("research")
            .and_then(|r| r.as_array())
            .unwrap_or(&Vec::new())
        {
            if r.get("slice").and_then(|x| x.as_str()) != Some(DECISION_SLICE)
                || r.get("status").and_then(|x| x.as_str()) != Some(RESEARCH_ANSWERED)
            {
                continue;
            }
            let q = r.get("question").and_then(|x| x.as_str()).unwrap_or("?");
            let a = r.get("answer").and_then(|x| x.as_str()).unwrap_or("");
            s.push_str(&format!("  - {q}\n"));
            for line in a.trim().lines() {
                s.push_str(&format!("    {line}\n"));
            }
        }
        if s.is_empty() {
            s
        } else {
            format!(
                "DECISIONS SETTLED AT PLAN TIME — every decision WHOLE, binding for consistency \
                 (the spec and USER DECISIONS outrank these; verify the tree honours each):\n{s}"
            )
        }
    };

    // §II.2 drop order: the research answers first (settled plan-time facts, the least durable
    // against the measured tree), then ok-only command classes, then final_text; a defect, gate
    // finding or NOT FIXED verdict is never dropped. Every drop is recorded with the section's
    // size — the caller emits `ledger_block_section_dropped{section, chars}` per entry, so a
    // section the reader never saw no longer vanishes without a trace.
    let mut dropped: Vec<(&'static str, usize)> = Vec::new();
    let mut droppable: Vec<(&'static str, &str)> = vec![
        ("research_answers", &research_section),
        ("ok_command_classes", &ok_classes),
        ("lane_self_reports", &final_texts),
    ];
    let mut body = format!("{never}{mid}{research_section}{ok_classes}{final_texts}");
    while body.chars().count() > budget && !droppable.is_empty() {
        let (section, text) = droppable.remove(0);
        if !text.is_empty() {
            dropped.push((section, text.chars().count()));
        }
        body = format!(
            "{never}{mid}{}",
            droppable.iter().map(|(_, t)| *t).collect::<String>()
        );
    }
    if body.chars().count() > budget {
        dropped.push((
            "measured_state_truncated",
            body.chars().count().saturating_sub(budget),
        ));
        body = truncate_block_at_line(&body, budget);
    }
    LedgerBlock {
        text: format!("{body}{decisions_section}"),
        dropped,
    }
}

/// II-4, the repair shard's splice (budget one dep-file, 3,500 chars): this round's gate row,
/// the PRIOR rounds' verdicts touching this shard's findings or files, and the owning tasks'
/// ledger rows — pure over the roll-up so a round-1 shard is testable against a round-0 fixture.
/// Round N+1 shards measurably re-tried what round N tried (prompt comment above
/// smoke_fix_description); the fresh per-round Scheduler discards its SharedContext, so this
/// on-disk history is the only channel that survives between rounds. Overflow drop order: the
/// owning-task rows first, then the gate row — NEVER a prior NOT FIXED verdict, which is the one
/// line whose loss re-schedules a failed approach; a line-boundary cut is the last resort.
pub(super) fn render_repair_history(
    rollup: Option<&serde_json::Value>,
    shard_files: &[String],
    findings: &[String],
    round: usize,
) -> String {
    const BUDGET: usize = 3_500;
    let Some(rollup) = rollup else {
        return String::new();
    };
    let mut verdicts = String::new();
    if let Some(rounds) = rollup.pointer("/repair/rounds").and_then(|r| r.as_array()) {
        for r in rounds {
            let r_round = r.get("round").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
            if r_round >= round {
                continue;
            }
            let shard = r.get("shard").and_then(|x| x.as_str()).unwrap_or("?");
            let owns_overlap = r
                .get("owned_files")
                .and_then(|o| o.as_array())
                .is_some_and(|a| {
                    a.iter()
                        .filter_map(|f| f.as_str())
                        .any(|f| shard_files.iter().any(|sf| sf == f))
                });
            for v in r
                .get("verdicts")
                .and_then(|v| v.as_array())
                .unwrap_or(&Vec::new())
            {
                let finding = v.get("finding").and_then(|f| f.as_str()).unwrap_or("");
                let matches_finding = findings.iter().any(|f| f == finding);
                if !owns_overlap && !matches_finding {
                    continue;
                }
                // S5d: a FIXED on a shard whose shadow never changed, and a NOT REAL that quoted
                // no replay, are named as what they are — the next shard must not read either
                // as a closed finding (r6c r1 read a zero-edit FIXED as a regression).
                let verdict = v.get("verdict").and_then(|x| x.as_str()).unwrap_or("?");
                let edited = r.get("edited").and_then(|e| e.as_bool());
                let unreplayed = v
                    .get("unreplayed")
                    .and_then(|u| u.as_bool())
                    .unwrap_or(false);
                let qualifier = match (verdict, edited, unreplayed) {
                    ("FIXED", Some(false), _) => " — CLAIMED FIXED WITHOUT AN EDIT (its shadow was byte-identical to the tree; nothing landed)",
                    ("NOT REAL", _, true) => " — NOT ACCEPTED (no replayed request+response quoted; the finding stays open)",
                    _ => "",
                };
                verdicts.push_str(&format!(
                    "  - round {r_round}, {shard}: FINDING {} {}{qualifier} — {}\n",
                    v.get("n").and_then(|n| n.as_u64()).unwrap_or(0),
                    verdict,
                    v.get("detail").and_then(|d| d.as_str()).unwrap_or(""),
                ));
            }
        }
    }
    let verdicts_block = if verdicts.is_empty() {
        String::new()
    } else {
        format!(
            "WHAT PRIOR ROUNDS ALREADY TRIED on these findings/files (a NOT FIXED approach must \
             not be retried as-is — try something different in kind):\n{verdicts}"
        )
    };
    let gate_block = rollup
        .get("gate")
        .and_then(|g| g.as_array())
        .and_then(|gates| {
            gates
                .iter()
                .find(|g| g.get("round").and_then(|r| r.as_u64()) == Some(round as u64))
                .or(gates.last())
        })
        .map(|g| {
            let n = g
                .get("findings")
                .and_then(|f| f.as_array())
                .map_or(0, |a| a.len());
            let inc = g
                .get("inconclusive")
                .and_then(|f| f.as_array())
                .map_or(0, |a| a.len());
            format!(
                "GATE round {}: {n} finding(s) against the RUNNING app, {inc} check(s) \
                 inconclusive — your numbered findings above are drawn from this measurement.\n",
                g.get("round").and_then(|r| r.as_u64()).unwrap_or(0)
            )
        })
        .unwrap_or_default();
    let mut owners = String::new();
    if let Some(tasks) = rollup.get("tasks").and_then(|t| t.as_object()) {
        for (id, t) in tasks {
            let owns_overlap = t
                .get("owned_files")
                .and_then(|o| o.as_array())
                .is_some_and(|a| {
                    a.iter()
                        .filter_map(|f| f.get("path").and_then(|p| p.as_str()))
                        .any(|f| shard_files.iter().any(|sf| sf == f))
                });
            if !owns_overlap {
                continue;
            }
            let fail = t
                .get("commands")
                .and_then(|c| c.as_object())
                .and_then(|c| {
                    c.iter()
                        .filter_map(|(_, v)| v.get("last_failure_tail").and_then(|x| x.as_str()))
                        .find(|tl| !tl.is_empty())
                })
                .unwrap_or("");
            owners.push_str(&format!(
                "  - `{id}` built these files ({} attempt(s), {}){}\n",
                t.get("attempts").and_then(|a| a.as_u64()).unwrap_or(1),
                t.get("status").and_then(|x| x.as_str()).unwrap_or("?"),
                if fail.is_empty() {
                    String::new()
                } else {
                    format!("; its last failure: {}", tail_chars(fail, 200))
                },
            ));
        }
    }
    let owners_block = if owners.is_empty() {
        String::new()
    } else {
        format!("WHO BUILT THE FILES you are repairing:\n{owners}")
    };
    let full = format!("{gate_block}{verdicts_block}{owners_block}");
    if full.chars().count() <= BUDGET {
        return full;
    }
    let without_owners = format!("{gate_block}{verdicts_block}");
    if without_owners.chars().count() <= BUDGET {
        return without_owners;
    }
    if verdicts_block.chars().count() <= BUDGET {
        return verdicts_block;
    }
    truncate_block_at_line(&verdicts_block, BUDGET)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ledger_with(dir: &Path, rows: serde_json::Value) {
        std::fs::create_dir_all(dir.join(".swarm")).unwrap();
        std::fs::write(
            dir.join(".swarm").join("ledger.json"),
            serde_json::json!({ "tasks": {}, "research": rows }).to_string(),
        )
        .unwrap();
    }

    /// r6c's shape: three decision answers of 2,562/3,066/3,243 chars beside 21 slice answers.
    /// Every decision reaches the reader WHOLE, past the 7,000 budget; a slice answer keeps its
    /// 400-char head; nothing is dropped when the measured state fits.
    #[test]
    fn decisions_render_whole_and_outside_the_budget() {
        let dir = tempfile::tempdir().unwrap();
        let d1 = format!(
            "Verdict: stay brushed.\n\n{}\n\n3. `web/app.js` behavior contract: {}",
            "x".repeat(1_200),
            "y".repeat(1_300)
        );
        let slice_answer = format!(
            "The sort keys are ts, id. {}. Then the cursor: {}",
            "z".repeat(500),
            "v".repeat(300)
        );
        ledger_with(
            dir.path(),
            serde_json::json!([
                { "slice": "__open_decisions__", "q_index": 0, "status": "answered",
                  "question": "D1 — does the brush survive a streamed mutation?", "answer": d1 },
                { "slice": "ledgerd-api", "q_index": 1, "status": "answered",
                  "question": "sort keys?", "answer": slice_answer },
            ]),
        );
        let block =
            render_ledger_block_measured(dir.path(), "integrate-verify", &[], &[], 7_000, None);
        assert!(block
            .text
            .contains("DECISIONS SETTLED AT PLAN TIME — every decision WHOLE"));
        assert!(
            block.text.contains(&"y".repeat(1_300)),
            "the decision's last paragraph must reach the reader"
        );
        assert!(block.text.contains("A: The sort keys are ts, id."));
        assert!(
            block.text.contains(&"z".repeat(500)) && !block.text.contains(&"v".repeat(300)),
            "a slice answer keeps its head to the sentence end past 400 chars, not the whole"
        );
        assert_eq!(
            block.text.matches("does the brush survive").count(),
            1,
            "a decision is never repeated as a research answer"
        );
        assert!(block.dropped.is_empty(), "{:?}", block.dropped);
    }

    /// Over budget, the research answers go first and the drop is NAMED with its size; the
    /// decisions still arrive whole because the budget never counted them.
    #[test]
    fn a_dropped_section_is_named_with_its_size() {
        let dir = tempfile::tempdir().unwrap();
        ledger_with(
            dir.path(),
            serde_json::json!([
                { "slice": "__open_decisions__", "q_index": 0, "status": "answered",
                  "question": "D2 — cursor state?", "answer": "Verdict: the file. ".repeat(60) },
                { "slice": "web-viz", "q_index": 0, "status": "answered",
                  "question": "viz keys?", "answer": "w".repeat(600) },
            ]),
        );
        let block =
            render_ledger_block_measured(dir.path(), "integrate-verify", &[], &[], 300, None);
        assert_eq!(block.dropped.len(), 1, "{:?}", block.dropped);
        assert_eq!(block.dropped[0].0, "research_answers");
        assert!(block.dropped[0].1 > 400, "{:?}", block.dropped);
        assert!(!block.text.contains("viz keys?"));
        assert!(block.text.contains("D2 — cursor state?"));
        assert!(block.text.contains("Verdict: the file. ".repeat(60).trim()));
    }
}
