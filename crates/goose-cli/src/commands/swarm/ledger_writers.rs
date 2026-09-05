//! The ledger MINI writers — the task leg, the gate round and the repair shard row — moved
//! verbatim from swarm.rs under the incremental-split law (development_gates::
//! swarm_rs_line_count_only_decreases), paying for THE SPLIT's dispatch/completion wiring in the
//! root (2c S2/S3). `write_task_ledger` gained its `extra` row keys in the same commit: a shard's
//! `shard_note` / `handoffs` ride its task row so the merger's dossier and the reader find them.

use std::path::{Path, PathBuf};

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

pub(super) fn write_task_ledger(
    root: &Path,
    w: TaskLedgerWrite<'_>,
) -> Result<PathBuf, super::LedgerWriteError> {
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
    super::write_ledger_mini_checked(
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
) -> Result<PathBuf, super::LedgerWriteError> {
    let row = serde_json::json!({
        "kind": "gate",
        "round": round,
        "source": source,
        "findings": findings,
        "inconclusive": inconclusive,
        "verified": verified,
    });
    super::write_ledger_mini_checked(root, &format!("gate-r{round}.json"), &row)
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
) -> Result<PathBuf, super::LedgerWriteError> {
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
    super::write_ledger_mini_checked(
        root,
        &format!(
            "repair-r{}-{}.json",
            row.round,
            super::activity_digest_key(row.shard)
        ),
        &mini,
    )
}
