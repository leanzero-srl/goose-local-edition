//! The ledger MINI writers — the task leg, the gate round and the repair shard row — moved
//! verbatim from swarm.rs under the incremental-split law (development_gates::
//! swarm_rs_line_count_only_decreases), paying for THE SPLIT's dispatch/completion wiring in the
//! root (2c S2/S3). `write_task_ledger` gained its `extra` row keys in the same commit: a shard's
//! `shard_note` / `handoffs` ride its task row so the merger's dossier and the reader find them.

use std::path::{Path, PathBuf};

use super::{spec_sets, verify_owned_files, verify_tree_imports, write_forming_atomic};

/// What one task attempt's ledger row is written from (the task leg's arguments, named).
pub(super) struct TaskLedgerWrite<'a> {
    pub(super) task_id: &'a str,
    pub(super) status: &'a str,
    pub(super) salvaged: bool,
    pub(super) owned_files: &'a [String],
    pub(super) attempt: u32,
    pub(super) calls_mirror_dir: Option<PathBuf>,
    /// Extra keys merged into the row — a shard's `shard_note` / `handoffs` (S3); `None` keeps
    /// every other row byte-identical.
    pub(super) extra: Option<serde_json::Value>,
}

pub(super) fn write_task_ledger(root: &Path, w: TaskLedgerWrite<'_>) -> Option<std::path::PathBuf> {
    let TaskLedgerWrite {
        task_id,
        status,
        salvaged,
        owned_files,
        attempt,
        calls_mirror_dir,
        extra,
    } = w;
    let mut row = super::build_task_ledger_row(
        root,
        task_id,
        status,
        salvaged,
        owned_files,
        attempt,
        calls_mirror_dir,
    );
    if let (Some(obj), Some(serde_json::Value::Object(more))) = (row.as_object_mut(), extra) {
        for (k, v) in more {
            obj.insert(k, v);
        }
    }
    write_ledger_mini(
        root,
        &format!("{}.json", super::activity_digest_key(task_id)),
        &row,
    )
}

/// One gate round's verdict, persisted where the NEXT dispatch can read it. `verified` is the
/// source's own affirmation shape — `established` (bool) for the in-run smoke gate, the
/// affirmative check count for a spec-contract replay — kept as-is rather than flattened,
/// because collapsing them is how "inconclusive" and "verified" became the same value once
/// before. Carries no timestamp: `goose swarm gate` on the same tree twice must be a no-op.
pub(super) fn write_gate_ledger(
    root: &Path,
    round: u64,
    source: &str,
    findings: &[String],
    inconclusive: &[String],
    verified: serde_json::Value,
) -> Option<std::path::PathBuf> {
    let row = serde_json::json!({
        "kind": "gate",
        "round": round,
        "source": source,
        "findings": findings,
        "inconclusive": inconclusive,
        "verified": verified,
    });
    write_ledger_mini(root, &format!("gate-r{round}.json"), &row)
}

/// What one repair shard reported, verdict lines parsed and PAIRED with the findings they judge
/// — a bare "FINDING 2: NOT FIXED" is only actionable next round alongside what finding 2 was.
pub(super) struct RepairLedgerRow<'a> {
    pub(super) round: usize,
    pub(super) shard: &'a str,
    pub(super) owned_files: &'a [String],
    /// The run's file list — a HANDOFF is only ever a path that exists in it.
    pub(super) all_files: &'a [String],
    pub(super) description: &'a str,
    pub(super) output: &'a str,
    pub(super) promoted: bool,
    pub(super) baseline: usize,
    pub(super) agent_ok: bool,
    /// S5d (iv): did the shard's shadow diverge from the tree at all? A FIXED verdict on an
    /// unedited shadow is a claim without an edit — rendered so by `render_repair_history`.
    pub(super) edited: bool,
    /// S5d (ii): finding numbers whose NOT REAL quoted no replayed request+response — not
    /// accepted; the finding stays open and the next shard is told so.
    pub(super) unreplayed: &'a [u32],
}

pub(super) fn write_repair_ledger(
    root: &Path,
    row: RepairLedgerRow<'_>,
) -> Option<std::path::PathBuf> {
    let findings = super::parse_numbered_findings(row.description);
    // r6c: the brief tells a shard to HAND OFF a fix it cannot land by name in its final
    // message; the app.js lane did ("HANDOFF — Files touched: `app/drafts.py` only") and the
    // verdict line's 300-char tail was all that survived — the handoff reached nobody. Persisted
    // here; the next wave's attribution consumes it (attribution::handoffs_from_rollup).
    let handoffs: Vec<serde_json::Value> =
        super::parse_handoffs(row.output, row.all_files, row.owned_files)
            .into_iter()
            .map(|h| serde_json::json!({"path": h.path, "symbol": h.symbol, "note": h.note}))
            .collect();
    let verdicts: Vec<serde_json::Value> = super::parse_finding_verdicts(row.output)
        .into_iter()
        .map(|(n, verdict, detail)| {
            serde_json::json!({
                "n": n,
                "finding": findings.get((n as usize).saturating_sub(1)),
                "verdict": verdict,
                "detail": detail,
                "unreplayed": row.unreplayed.contains(&n),
            })
        })
        .collect();
    let mini = serde_json::json!({
        "kind": "repair",
        "round": row.round,
        "shard": row.shard,
        "owned_files": row.owned_files,
        "findings_assigned": findings,
        "verdicts": verdicts,
        "handoffs": handoffs,
        "promoted": row.promoted,
        "baseline": row.baseline,
        "agent_ok": row.agent_ok,
        "edited": row.edited,
    });
    write_ledger_mini(
        root,
        &format!(
            "repair-r{}-{}.json",
            row.round,
            super::activity_digest_key(row.shard)
        ),
        &mini,
    )
}

// ─────────────────────────────── THE PER-RUN LEDGER (§II.2) ───────────────────────────────
//
// capture → LEDGER → message. Models are stateless; the harness is the state. Everything a run
// measures about a task — what it wrote, what it ran, what failed, what the gate found, what a
// repair round already tried — is captured today (digests, `<task>.calls.jsonl`,
// delivery_defects events, gate verdicts) and was thrown away before the next dispatch could
// read it: on r2, `delivery_defects` named the sink's whole problem 0.24 ms before the sink was
// dispatched without it, and the sink then re-derived everything with 3 whole-suite runs and 17
// discovery calls. These writers persist those facts as `.swarm/ledger/<key>.json` minis plus a
// `.swarm/ledger.json` roll-up rebuilt WHOLE from the minis on every write — the
// `.swarm/prereview/` file mechanics, the injection channel this engine has measured to work.
//
// THE LEDGER INFORMS, NEVER GATES. Every write is best-effort (a failed write must never
// disturb a run), nothing here stops, caps, retries or refuses model work, and no time value is
// an input to what is written or how it renders — timestamps are provenance only.

pub(super) const LEDGER_DIR: &str = ".swarm/ledger";

/// Write one ledger mini and rebuild the roll-up from ALL minis. Every writer funnels through
/// here so the roll-up can never drift from its parts. Returns the mini's path, None on any
/// failure — the caller emits `ledger_written` only for a write that actually happened.
pub(super) fn write_ledger_mini(
    root: &Path,
    file_name: &str,
    row: &serde_json::Value,
) -> Option<std::path::PathBuf> {
    write_ledger_mini_checked(root, file_name, row).ok()
}

/// The same funnel with the failure NAMED (VA-030 D10-5, gate 1): the research fan's four writers
/// emit `research_mini_write_failed` from this error instead of discarding an Option — a fact mini
/// that failed to write was counted in `research_planned.facts` and rendered from memory while
/// resume, cover and the snowball never saw it, with no event.
pub(super) fn write_ledger_mini_checked(
    root: &Path,
    file_name: &str,
    row: &serde_json::Value,
) -> Result<std::path::PathBuf, String> {
    let dir = root.join(LEDGER_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let path = dir.join(file_name);
    let bytes = serde_json::to_string_pretty(row).map_err(|e| format!("serialize: {e}"))?;
    // Finding 9: an unchanged row is a byte-AND-mtime no-op, so a gate replay over an archived
    // tree leaves its ledger looking exactly as archived ("freshest 0s ago" was a replay
    // artifact, not run activity). The roll-up write below makes the same comparison.
    if std::fs::read_to_string(&path).is_ok_and(|old| old == bytes) {
        rebuild_ledger_rollup(root);
        return Ok(path);
    }
    std::fs::write(&path, &bytes).map_err(|e| format!("write {}: {e}", path.display()))?;
    rebuild_ledger_rollup(root);
    Ok(path)
}

/// Rebuild `.swarm/ledger.json` WHOLE from the minis. Rewriting the roll-up in full on every
/// write is what makes the writers order-independent and idempotent — the prereview mechanics,
/// not an append log that could double-count. `open_defects` is re-derived from the tree NOW
/// (verify_tree_imports + each task's owned-file stat), so a defect fixed since its task
/// completed vanishes instead of haunting every later prompt.
pub(super) fn rebuild_ledger_rollup(root: &Path) -> Option<serde_json::Value> {
    let dir = root.join(LEDGER_DIR);
    let mut tasks: std::collections::BTreeMap<String, serde_json::Value> = Default::default();
    let mut gates: Vec<serde_json::Value> = Vec::new();
    let mut repairs: Vec<serde_json::Value> = Vec::new();
    let mut research: Vec<serde_json::Value> = Vec::new();
    // GEN-6a #3 (fallback rule): a mini that fails to read or parse used to `continue` silently,
    // so a dependent read a TRUNCATED roll-up as the whole history. The dropped rows are named
    // in the roll-up itself; render_ledger_block states them and the writers emit
    // `ledger_row_unreadable`. Sorted for the roll-up's idempotence (read_dir order is not).
    let mut rows_dropped: Vec<serde_json::Value> = Vec::new();
    for e in std::fs::read_dir(&dir).ok()?.flatten() {
        if e.path().extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let fname = e.file_name().to_string_lossy().into_owned();
        let v = match std::fs::read_to_string(e.path()) {
            Ok(t) => match serde_json::from_str::<serde_json::Value>(&t) {
                Ok(v) => v,
                Err(err) => {
                    rows_dropped.push(serde_json::json!({"file": fname, "error": err.to_string()}));
                    continue;
                }
            },
            Err(err) => {
                rows_dropped.push(serde_json::json!({"file": fname, "error": err.to_string()}));
                continue;
            }
        };
        match v.get("kind").and_then(|k| k.as_str()) {
            Some("task") => {
                if let Some(id) = v.get("task_id").and_then(|t| t.as_str()) {
                    tasks.insert(id.to_string(), v);
                }
            }
            Some("gate") => gates.push(v),
            Some("repair") => repairs.push(v),
            // RESEARCH FAN v2: without this arm a research mini would fall into `_ => {}` and
            // be silently invisible to every rollup reader — captured vs invisible is this line.
            Some("research") => research.push(v),
            _ => {}
        }
    }
    gates.sort_by_key(|g| g.get("round").and_then(|r| r.as_u64()).unwrap_or(0));
    repairs.sort_by_key(|r| {
        (
            r.get("round").and_then(|x| x.as_u64()).unwrap_or(0),
            r.get("shard")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
        )
    });
    research.sort_by_key(|r| {
        (
            r.get("slice")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
            r.get("q_index").and_then(|x| x.as_u64()).unwrap_or(0),
        )
    });
    let tests_run_total: u64 = tasks
        .values()
        .filter_map(|t| t.pointer("/commands/test/count").and_then(|c| c.as_u64()))
        .sum();
    let last_full_suite = tasks
        .values()
        .filter_map(|t| t.get("last_full_suite").filter(|v| !v.is_null()))
        .max_by_key(|s| {
            s.get("ts")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string()
        })
        .cloned();
    let mut open_defects: Vec<String> = verify_tree_imports(root);
    for t in tasks.values() {
        let entries: Vec<(String, u64)> = t
            .get("owned_files")
            .and_then(|o| o.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|f| {
                        Some((
                            f.get("path").and_then(|p| p.as_str())?.to_string(),
                            f.get("bytes").and_then(|b| b.as_u64()).unwrap_or(0),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        // Re-verify a row's files ONLY when they changed since its write. verify_owned_files
        // spawns py_compile per .py file; running the full detector set for every row on every
        // rebuild would put a growing, blocking cost on every task completion. The row already
        // stores each file's bytes-on-disk at write time, so an unchanged file keeps the row's
        // stored verdict and a changed one (any fix changes size in practice; a same-size edit
        // keeps a stale advisory line at worst) is re-measured — which is what makes a FIXED
        // defect vanish from the roll-up.
        let changed = entries.iter().any(|(p, b)| {
            std::fs::metadata(root.join(p))
                .map(|m| m.len())
                .unwrap_or(0)
                != *b
        });
        let defects: Vec<String> = if changed {
            let paths: Vec<String> = entries.into_iter().map(|(p, _)| p).collect();
            verify_owned_files(root, &paths)
        } else {
            t.get("delivery_defects")
                .and_then(|d| d.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default()
        };
        for d in defects {
            if !open_defects.contains(&d) {
                open_defects.push(d);
            }
        }
    }
    rows_dropped.sort_by_key(|r| {
        r.get("file")
            .and_then(|f| f.as_str())
            .unwrap_or("")
            .to_string()
    });
    let mut rollup = serde_json::json!({
        "tasks": tasks,
        "gate": gates,
        "repair": { "rounds": repairs },
        "tests_run_total": tests_run_total,
        "last_full_suite": last_full_suite,
        "open_defects": open_defects,
    });
    // Absent when clean, so an intact ledger's roll-up bytes are unchanged.
    if !rows_dropped.is_empty() {
        rollup["rows_dropped"] = serde_json::Value::from(rows_dropped);
    }
    // Absent when no research ran, for the same byte-stability reason.
    if !research.is_empty() {
        rollup["research"] = serde_json::Value::from(research);
    }
    // SPEC-ENUMERATED FILE SETS (r5 item 3): the excess is re-derived from the tree NOW, the
    // same rule as open_defects, so a removed/merged extra vanishes. Absent when clean (and
    // when the run froze no enumeration — the sidecar simply does not exist).
    let spec_set_exceeded = spec_sets::exceeded_facts(root);
    if !spec_set_exceeded.is_empty() {
        rollup["spec_set_exceeded"] = serde_json::Value::from(spec_set_exceeded);
    }
    let out_path = root.join(".swarm").join("ledger.json");
    let bytes = serde_json::to_string_pretty(&rollup).ok()?;
    // Finding 9: identical roll-up bytes keep the archived file's mtime (see write_ledger_mini).
    if !std::fs::read_to_string(&out_path).is_ok_and(|old| old == bytes) {
        // tmp+rename (the forming capture's own atomic writer): the roll-up is read on a poll
        // by every dispatch's render and by the desktop, so a torn read must be impossible.
        write_forming_atomic(&out_path, &bytes).ok()?;
    }
    Some(rollup)
}
