//! The durable-transcript cluster: the append-only `<task>.log`/`<task>.think.log`/
//! `<task>.calls.jsonl` writers and the attempt-boundary marker. Sixth sibling module under the
//! incremental-split law (development_gates::swarm_rs_line_count_only_decreases). Moved verbatim
//! from swarm.rs — behavior unchanged, each item keeps its own WHY — paying for the r6 instrument
//! batch landing in the same commits (timestamped steer/note blocks, the measured defect-steer
//! closing).
//!
//! THE MIRROR DIMENSION (r5, run swarm-20260830-083847650, REPAIR round 0). A repair shard runs
//! in a speculative SHADOW tree ($TMPDIR/.tmpXXXXXX) that is deleted when the wave ends, and its
//! `activity_file` is rooted there. The digest and forming sidecar already mirrored into the real
//! tree (`fix_shard_mirror_dir`), but the durable appends wrote to the shadow only — r5's two
//! complete-fix lanes (`app/__main__.py`, `app/sync.py`) left digests in the real tree and their
//! whole `.log`/`.think.log`/`.calls.jsonl` inside `.tmpDTOxbF`/`.tmpmoxv5g`, rescued only by an
//! operator script. So every appender here takes `mirror: Option<&Path>` and lands the SAME fresh
//! bytes in both trees. State (the thinking buffer, each watermark) is consumed exactly once, by
//! the PRIMARY write; a retrying appender feeds the mirror only bytes the primary accepted in the
//! same call, so no target can receive a byte twice. Mirror failures degrade loudly like primary
//! ones — the caller notes them under a distinguishable kind (`think.log.mirror` class).

use std::path::{Path, PathBuf};

/// The write-failure kinds the appenders return: `(kind, error)` pairs the caller feeds to
/// `note_transcript_write_failure`, which dedupes per (activity key, kind) and emits ONE
/// `transcript_write_failed` event — the GEN-6a #8 loud-degrade contract, mirror included.
pub(super) type AppendErrs = Vec<(&'static str, String)>;

fn append_bytes(path: &Path, bytes: &[u8]) -> Option<String> {
    use std::io::Write;
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        Ok(mut f) => f.write_all(bytes).err().map(|e| e.to_string()),
        Err(e) => Some(e.to_string()),
    }
}

/// Append buffered THINKING to `<activity>.think.log` (primary AND mirror) and clear the buffer.
///
/// The digest carries a 2,400-character rolling window of the reasoning channel, which is why the
/// panel's THINKING pane clears and refills instead of accumulating. This is the reasoning
/// channel's only durable record. Best-effort: a transcript that cannot be written must never
/// disturb a run.
///
/// GEN-6a #8: the durable transcripts were best-effort-SILENT — a failed open/write left the
/// `.think.log` frozen with no trace, and the operator read a stale log as "the worker stopped
/// thinking". The appenders RETURN their write errors so the caller (run_agent_in, which has the
/// events sink) can emit `transcript_write_failed` once per activity key. The write still
/// degrades — a transcript failure must never stop a worker — but it degrades loudly.
///
/// State rule: the buffer clears iff the PRIMARY write succeeded, and the mirror is written with
/// the same bytes in that same call — so a primary failure retries the bytes to BOTH targets next
/// flush, and a mirror failure loses only the mirror's copy (noted), never double-appends.
pub(super) fn append_thinking_transcript(
    activity_path: &Path,
    mirror: Option<&Path>,
    buf: &mut String,
) -> AppendErrs {
    if buf.is_empty() {
        return Vec::new();
    }
    let mut errs = Vec::new();
    match append_bytes(&activity_path.with_extension("think.log"), buf.as_bytes()) {
        None => {
            if let Some(m) = mirror {
                if let Some(e) = append_bytes(&m.with_extension("think.log"), buf.as_bytes()) {
                    errs.push(("think.log.mirror", e));
                }
            }
            buf.clear();
        }
        Some(e) => errs.push(("think.log", e)),
    }
    errs
}

/// Append the reasoning produced SINCE THE LAST CALL to `<activity>.log` (primary AND mirror),
/// and return the new watermark index.
///
/// WHY THIS EXISTS. The digest's `full_reasoning` is a 24,000-char TAIL clip, so a long call's
/// narration starts partway through — Mihai, twice, on a node whose panel began at item 25 of a
/// 39-item list: *"the generations stop displaying past a certain number of characters"*. The clip
/// is not gratuitous: the digest is REWRITTEN on a hot 400ms timer, so it cannot simply grow, and
/// raising the number just moves the cliff while making every rewrite more expensive.
///
/// An append-only sibling has neither problem. Each write costs only the NEW text, the file is the
/// whole narration with nothing elided, and the digest keeps its bounded tail for the judge and
/// the live panel. Best-effort throughout: a transcript that fails to write must never disturb a
/// run. The watermark advances exactly once regardless of either target's outcome (the historical
/// contract: a failed write loses those bytes loudly rather than retrying), and the fresh bytes
/// are computed once and offered to both targets independently — the mirror is the copy that
/// survives the shadow's deletion, so it must not be gated on the shadow's writability.
pub(super) fn append_reasoning_transcript(
    activity_path: &Path,
    mirror: Option<&Path>,
    texts: &[String],
    already: usize,
) -> (usize, AppendErrs) {
    if texts.len() <= already {
        return (already, Vec::new());
    }
    let fresh = texts[already..].join("");
    if fresh.is_empty() {
        return (texts.len(), Vec::new());
    }
    let mut errs = Vec::new();
    if let Some(e) = append_bytes(&activity_path.with_extension("log"), fresh.as_bytes()) {
        errs.push(("log", e));
    }
    if let Some(m) = mirror {
        if let Some(e) = append_bytes(&m.with_extension("log"), fresh.as_bytes()) {
            errs.push(("log.mirror", e));
        }
    }
    (texts.len(), errs)
}

/// II-1: append the NEW completed call records to the append-only `<task>.calls.jsonl`
/// (primary AND mirror).
///
/// The digest is REWRITTEN in place and RESEEDED at every re-dispatch — that seed is what erased
/// ledger-core-tests' 12-minute attempt 0 and the sink's 46-call attempt 0 on r2. This sibling is
/// append-only across attempts (the `.log`/`.think.log` rule applied to calls), so an attempt's
/// work survives its own death. `already` is the caller's watermark into `call_records`; rows are
/// never rewritten and never duplicated. Best-effort like the transcripts: a failed write must
/// never disturb a run. BOTH targets degrade loudly (GEN-6a class: `calls.jsonl` for the
/// primary, `calls.jsonl.mirror` for the copy that survives the shadow) — a silent primary gap
/// froze the only durable call record with no trace, the same evidence-hiding shape the mirror
/// kind already closed. The watermark advances iff the primary accepted every row, and the
/// mirror is fed only in that same call — no target can receive a row twice.
pub(super) fn append_calls_jsonl(
    activity_path: &Path,
    mirror: Option<&Path>,
    attempt: u32,
    call_records: &[(String, String, Option<bool>, String)],
    already: &mut usize,
) -> AppendErrs {
    if call_records.len() <= *already {
        return Vec::new();
    }
    let ts = chrono::Utc::now().to_rfc3339();
    let mut rows = String::new();
    for (name, summary, ok, result) in &call_records[*already..] {
        let mut row = serde_json::json!({
            "ts": ts,
            "attempt": attempt,
            "name": name,
            "summary": summary,
            "ok": ok,
            "result_tail": super::tail_chars(result, 2000),
        });
        if let Some(py) = super::parse_pytest_summary(result) {
            row["pytest"] = serde_json::to_value(py).unwrap_or(serde_json::Value::Null);
        }
        rows.push_str(&format!("{row}\n"));
    }
    if let Some(e) = append_bytes(
        &activity_path.with_extension("calls.jsonl"),
        rows.as_bytes(),
    ) {
        return vec![("calls.jsonl", e)];
    }
    *already = call_records.len();
    let mut errs = Vec::new();
    if let Some(m) = mirror {
        if let Some(e) = append_bytes(&m.with_extension("calls.jsonl"), rows.as_bytes()) {
            errs.push(("calls.jsonl.mirror", e));
        }
    }
    errs
}

/// One pre-serialized row (the terminal flush's `attempt_end` snapshot) appended to
/// `<task>.calls.jsonl` on both targets. Same contracts as `append_calls_jsonl`: loud on both
/// targets, mirror fed only what the primary accepted.
pub(super) fn append_calls_row(
    activity_path: &Path,
    mirror: Option<&Path>,
    row: &str,
) -> AppendErrs {
    let line = format!("{row}\n");
    let mut errs = Vec::new();
    if let Some(e) = append_bytes(
        &activity_path.with_extension("calls.jsonl"),
        line.as_bytes(),
    ) {
        errs.push(("calls.jsonl", e));
        return errs;
    }
    if let Some(m) = mirror {
        if let Some(e) = append_bytes(&m.with_extension("calls.jsonl"), line.as_bytes()) {
            errs.push(("calls.jsonl.mirror", e));
        }
    }
    errs
}

/// II-8's READ half of the mirror dimension: the previous-attempt capture, primary first, mirror
/// second. A re-dispatched FIX SHARD runs in a FRESH shadow (`copy_tree_excluding` skips
/// `.swarm`), so the root-relative `<key>.calls.jsonl` is missing/empty even though every
/// attempt-0 row was mirrored into the real tree at write time (fce592811) — r5's round-1
/// re-shard of app/sync.py would have opened blind on 16 mirrored rows. Reading the mirror when
/// the primary is empty is honest recovery of rows that EXIST, not a substitution; when both are
/// missing/empty this returns None and the caller's absent-behavior stands unchanged (the
/// fallback gate: never invent content for a true absence). A normal task passes `mirror: None`
/// (`fix_shard_mirror_dir` is the one predicate) and behaves byte-identically to before.
pub(super) fn read_calls_capture(primary: &Path, mirror: Option<PathBuf>) -> Option<String> {
    let read = |p: &Path| {
        std::fs::read_to_string(p)
            .ok()
            .filter(|t| !t.trim().is_empty())
    };
    read(primary).or_else(|| mirror.as_deref().and_then(read))
}

/// The per-task ledger row, from what the run already captured: the append-only
/// `<key>.calls.jsonl` (per-call rows + `attempt_end` snapshots with fs_delta), the live digest's
/// `last_text`, and a fresh `verify_owned_files` stat — a re-run, not a copy, so a defect fixed
/// since completion vanishes instead of being re-litigated.
///
/// II-8's second root-relative-calls-read (the 0dc8c297f RESIDUAL): a FIX SHARD's fresh shadow
/// skips `.swarm`, so reading `<key>.calls.jsonl` from `root` alone under-counts PRIOR attempts
/// whose rows were mirrored into the real tree. `calls_mirror_dir` is the ONE existing predicate
/// (`fix_shard_mirror_dir`, never a re-derived prefix test), consulted through
/// `read_calls_capture` only when the root file is missing/empty — honest recovery of rows that
/// EXIST; both empty keeps the empty-row behavior byte-identical. Every normal caller passes
/// `None` and behaves exactly as before.
pub(super) fn build_task_ledger_row(
    root: &Path,
    task_id: &str,
    status: &str,
    salvaged: bool,
    owned_files: &[String],
    attempt: u32,
    calls_mirror_dir: Option<PathBuf>,
) -> serde_json::Value {
    let key = super::activity_digest_key(task_id);
    let activity = root
        .join(".swarm")
        .join("activity")
        .join(format!("{key}.json"));
    #[derive(Default)]
    struct Class {
        count: u64,
        last_ok: Option<bool>,
        last_failure_tail: String,
    }
    let mut classes: std::collections::BTreeMap<&'static str, Class> =
        std::collections::BTreeMap::new();
    let mut attempts_seen: u32 = attempt;
    let mut last_full_suite: Option<serde_json::Value> = None;
    let mut last_pytest: Option<serde_json::Value> = None;
    let mut last_pytest_filewide: Option<serde_json::Value> = None;
    let mut fs_appeared: std::collections::BTreeSet<String> = Default::default();
    let mut fs_changed: std::collections::BTreeSet<String> = Default::default();
    let mut fs_outside: std::collections::BTreeSet<String> = Default::default();
    if let Some(text) = read_calls_capture(
        &activity.with_extension("calls.jsonl"),
        calls_mirror_dir.map(|d| d.join(format!("{key}.calls.jsonl"))),
    ) {
        for line in text.lines() {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            if let Some(a) = v.get("attempt").and_then(|a| a.as_u64()) {
                attempts_seen = attempts_seen.max(a as u32);
            }
            if v.get("kind").and_then(|k| k.as_str()) == Some("attempt_end") {
                if let Some(d) = v.get("fs_delta") {
                    let take = |key: &str, into: &mut std::collections::BTreeSet<String>| {
                        for p in d.get(key).and_then(|x| x.as_array()).unwrap_or(&Vec::new()) {
                            if let Some(s) = p.as_str() {
                                into.insert(s.to_string());
                            }
                        }
                    };
                    take("appeared", &mut fs_appeared);
                    take("changed", &mut fs_changed);
                    take("outside_manifest", &mut fs_outside);
                }
                continue;
            }
            if v.get("name").and_then(|n| n.as_str()) != Some("shell") {
                continue;
            }
            let cmd = v.get("summary").and_then(|s| s.as_str()).unwrap_or("");
            let class = super::classify_command(cmd);
            let ok = v.get("ok").and_then(|o| o.as_bool());
            let entry = classes.entry(class).or_default();
            entry.count += 1;
            entry.last_ok = ok;
            let py = v.get("pytest");
            let failed_pytest = py
                .and_then(|p| p.get("failed"))
                .and_then(|f| f.as_u64())
                .unwrap_or(0)
                > 0;
            if ok == Some(false) || failed_pytest {
                entry.last_failure_tail = super::tail_chars(
                    &format!(
                        "{cmd}: {}",
                        v.get("result_tail").and_then(|r| r.as_str()).unwrap_or("")
                    ),
                    300,
                );
            }
            if let Some(py) = py {
                last_pytest = Some(serde_json::json!({ "cmd": cmd, "summary": py }));
                // A `::node` re-run answers one test; the lane's file-wide outcome is what the
                // test table reports, or a targeted 1-failed would shadow a 7-failed suite state.
                if !cmd.contains("::") {
                    last_pytest_filewide = Some(serde_json::json!({ "cmd": cmd, "summary": py }));
                }
                if super::pytest_runs_whole_suite(cmd) {
                    last_full_suite = Some(serde_json::json!({
                        "cmd": cmd,
                        "summary": py,
                        "task_id": task_id,
                        "ts": v.get("ts").cloned().unwrap_or(serde_json::Value::Null),
                    }));
                }
            }
        }
    }
    let commands: serde_json::Map<String, serde_json::Value> = classes
        .into_iter()
        .map(|(class, c)| {
            (
                class.to_string(),
                serde_json::json!({
                    "count": c.count,
                    "ok": c.last_ok,
                    "last_failure_tail": c.last_failure_tail,
                }),
            )
        })
        .collect();
    // This head reaches a model: the ledger block's "WHAT EACH LANE SAID IT DELIVERED" renders
    // `tail_chars(final_text, 200)` into dependents' prompts, so what matters is that the STRING
    // ENDS at a sentence — a hard 400-char cut hands the tail a mid-sentence ending, the r5
    // truncation tax (one cut sentence, four opener re-litigations).
    let final_text: String = super::head_to_sentence_end(
        &std::fs::read_to_string(&activity)
            .ok()
            .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
            .and_then(|d| {
                d.get("last_text")
                    .and_then(|t| t.as_str())
                    .map(String::from)
            })
            .unwrap_or_default(),
        400,
    );
    let owned: Vec<serde_json::Value> = owned_files
        .iter()
        .map(|f| {
            serde_json::json!({
                "path": f,
                "bytes": std::fs::metadata(root.join(f)).map(|m| m.len()).unwrap_or(0),
            })
        })
        .collect();
    serde_json::json!({
        "kind": "task",
        "task_id": task_id,
        "status": status,
        "salvaged": salvaged,
        "attempts": u64::from(attempts_seen) + 1,
        "owned_files": owned,
        "delivery_defects": super::verify_owned_files(root, owned_files),
        "commands": commands,
        "last_full_suite": last_full_suite,
        "last_pytest": last_pytest,
        "last_pytest_filewide": last_pytest_filewide,
        "final_text": final_text,
        "fs_delta": {
            "appeared": fs_appeared,
            "changed": fs_changed,
            "outside_manifest": fs_outside,
        },
        "ts": chrono::Utc::now().to_rfc3339(),
    })
}

/// ONE attempt-marker line, appended to `<task>.log` and `<task>.think.log` when a call is seeded.
/// The transcripts are append-only across attempts (that is their value), so without a boundary the
/// panel cannot tell attempt 0's final error from attempt 1's first words — the UI splits at the
/// LAST marker into a LIVE segment plus superseded ones. Legacy logs without markers read as one
/// live segment, exactly as before. The mirror gets the same boundary so the real tree's splitter
/// works on a fix shard's rescued transcripts.
pub(super) fn attempt_marker_line(attempt: u32, dispatched_at: &str) -> String {
    format!("\n===== swarm attempt {attempt} · dispatched {dispatched_at} =====\n")
}

pub(super) fn append_attempt_marker(
    activity_path: &Path,
    mirror: Option<&Path>,
    attempt: u32,
    dispatched_at: &str,
) {
    let line = attempt_marker_line(attempt, dispatched_at);
    for target in std::iter::once(activity_path).chain(mirror) {
        for ext in ["log", "think.log"] {
            let _ = append_bytes(&target.with_extension(ext), line.as_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::swarm::{seed_worker_digest, SaidProvenance};

    fn read(p: &Path, ext: &str) -> String {
        // unwrap, not a default: in these tests an absent transcript is the defect under test,
        // and it must panic with the path rather than impersonate an empty file.
        std::fs::read_to_string(p.with_extension(ext)).unwrap()
    }

    /// The r5 shape inverted: one append per channel lands IDENTICAL bytes in the primary (the
    /// shard's shadow) and the mirror (the real tree), while the stateful side — the thinking
    /// buffer, the reasoning watermark, the calls watermark — is consumed exactly once, so a
    /// second flush appends nothing anywhere.
    #[test]
    fn a_mirrored_append_lands_identical_bytes_and_consumes_state_once() {
        let dir = tempfile::tempdir().unwrap();
        let shadow = dir.path().join("shadow");
        let real = dir.path().join("real");
        std::fs::create_dir_all(&shadow).unwrap();
        std::fs::create_dir_all(&real).unwrap();
        let p = shadow.join("complete-fix__app~ssync.py.json");
        let m = real.join("complete-fix__app~ssync.py.json");

        append_attempt_marker(&p, Some(&m), 0, "2026-08-30T17:55:12Z");
        let texts = vec!["I will ".to_string(), "patch sync.py".to_string()];
        let (at, errs) = append_reasoning_transcript(&p, Some(&m), &texts, 0);
        assert_eq!(at, 2);
        assert!(errs.is_empty(), "{errs:?}");
        let (at2, errs2) = append_reasoning_transcript(&p, Some(&m), &texts, at);
        assert_eq!((at2, errs2.len()), (2, 0), "watermark consumed once");

        let mut think = String::from("the retry loop drops the queue");
        assert!(append_thinking_transcript(&p, Some(&m), &mut think).is_empty());
        assert!(think.is_empty(), "buffer clears exactly once");
        assert!(append_thinking_transcript(&p, Some(&m), &mut think).is_empty());

        let records = vec![(
            "shell".to_string(),
            "python3 -m pytest".to_string(),
            Some(true),
            "=== 2 passed in 0.11s ===".to_string(),
        )];
        let mut calls_at = 0usize;
        assert!(append_calls_jsonl(&p, Some(&m), 0, &records, &mut calls_at).is_empty());
        assert_eq!(calls_at, 1);
        assert!(append_calls_jsonl(&p, Some(&m), 0, &records, &mut calls_at).is_empty());
        assert!(append_calls_row(&p, Some(&m), "{\"kind\":\"attempt_end\"}").is_empty());

        for ext in ["log", "think.log", "calls.jsonl"] {
            let primary = read(&p, ext);
            assert!(!primary.is_empty(), "{ext} must exist in the shadow");
            assert_eq!(
                primary,
                read(&m, ext),
                "{ext} mirror must be byte-identical"
            );
        }
        assert_eq!(
            read(&p, "calls.jsonl").lines().count(),
            2,
            "one row + attempt_end, no dupes"
        );
        assert!(read(&p, "think.log").contains("the retry loop drops the queue"));
        assert_eq!(read(&p, "think.log").matches("retry loop").count(), 1);
    }

    /// Loud-degrade, mirror side: a mirror whose files cannot be opened (directories squat on the
    /// exact paths) leaves every primary append intact, returns the distinguishable mirror kinds,
    /// and still consumes state once — the thinking buffer clears because the PRIMARY accepted it.
    #[test]
    fn an_unwritable_mirror_keeps_the_primary_append_and_reports() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("complete-fix__x.json");
        let m = dir.path().join("m").join("complete-fix__x.json");
        for ext in ["log", "think.log", "calls.jsonl"] {
            std::fs::create_dir_all(m.with_extension(ext)).unwrap();
        }

        let texts = vec!["fresh reasoning".to_string()];
        let (at, errs) = append_reasoning_transcript(&p, Some(&m), &texts, 0);
        assert_eq!(at, 1);
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].0, "log.mirror");
        assert_eq!(read(&p, "log"), "fresh reasoning");

        let mut think = String::from("still thinking");
        let errs = append_thinking_transcript(&p, Some(&m), &mut think);
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].0, "think.log.mirror");
        assert!(
            think.is_empty(),
            "primary accepted the bytes, so the buffer clears"
        );
        assert_eq!(read(&p, "think.log"), "still thinking");

        let records = vec![(
            "shell".to_string(),
            "ls".to_string(),
            Some(true),
            "ok".to_string(),
        )];
        let mut calls_at = 0usize;
        let errs = append_calls_jsonl(&p, Some(&m), 0, &records, &mut calls_at);
        assert_eq!(calls_at, 1, "primary accepted every row");
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].0, "calls.jsonl.mirror");
        assert_eq!(read(&p, "calls.jsonl").lines().count(), 1);
        let errs = append_calls_row(&p, Some(&m), "{}");
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].0, "calls.jsonl.mirror");
    }

    /// The no-double-consume rule from the other side: a broken PRIMARY keeps the thinking buffer
    /// (it will retry to both targets next flush) and feeds the mirror NOTHING this call, so a
    /// later successful flush cannot double-append to the mirror.
    #[test]
    fn a_failed_primary_keeps_the_thinking_buffer_for_both_targets() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("b").join("complete-fix__y.json");
        std::fs::create_dir_all(p.with_extension("think.log")).unwrap();
        let m = dir.path().join("complete-fix__y.json");

        let mut think = String::from("must survive");
        let errs = append_thinking_transcript(&p, Some(&m), &mut think);
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].0, "think.log");
        assert_eq!(think, "must survive", "buffer retained for the retry");
        assert!(
            !m.with_extension("think.log").exists(),
            "mirror gets only primary-accepted bytes, so the retry cannot duplicate"
        );
    }

    /// GEN-6a (fce592811 handoff): a failed PRIMARY calls.jsonl append is LOUD — it reports the
    /// `calls.jsonl` kind the caller feeds to `note_transcript_write_failure` — and the
    /// watermark stays put so the rows retry next flush instead of vanishing silently. The
    /// mirror gets nothing this call (only primary-accepted bytes), so the retry cannot
    /// double-append.
    #[test]
    fn a_failed_primary_calls_append_reports_and_keeps_the_watermark() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("complete-fix__w.json");
        std::fs::create_dir_all(p.with_extension("calls.jsonl")).unwrap();
        let m = dir.path().join("real").join("complete-fix__w.json");
        std::fs::create_dir_all(m.parent().unwrap()).unwrap();

        let records = vec![(
            "shell".to_string(),
            "ls".to_string(),
            Some(true),
            "ok".to_string(),
        )];
        let mut at = 0usize;
        let errs = append_calls_jsonl(&p, Some(&m), 0, &records, &mut at);
        assert_eq!(at, 0, "watermark must not advance past a failed primary");
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].0, "calls.jsonl");
        assert!(
            !m.with_extension("calls.jsonl").exists(),
            "mirror gets only primary-accepted rows"
        );
        let errs = append_calls_row(&p, Some(&m), "{\"kind\":\"attempt_end\"}");
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].0, "calls.jsonl");
        assert!(!m.with_extension("calls.jsonl").exists());
    }

    /// II-1's isolation fixture: attempt 0's captured calls SURVIVE the digest reseed that a
    /// re-dispatch performs (the seed erased r2's ledger-core-tests attempt 0), and the watermark
    /// never duplicates a row across the multiple write sites. (Moved with `append_calls_jsonl`
    /// under the incremental-split law.)
    #[test]
    fn calls_jsonl_survives_the_redispatch_seed_and_never_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("ledger-core-tests.json");
        let mut records: Vec<(String, String, Option<bool>, String)> = vec![(
            "shell".into(),
            "python3 -m pytest".into(),
            Some(true),
            "=== 7 failed, 19 passed in 0.29s ===".into(),
        )];
        let mut at = 0usize;
        append_calls_jsonl(&p, None, 0, &records, &mut at);
        // The same records again (a second digest flush) must append nothing.
        append_calls_jsonl(&p, None, 0, &records, &mut at);
        // The RE-DISPATCH: the seed overwrites the digest file itself — the erase II-1 outlives.
        let said = SaidProvenance::at_dispatch(1);
        std::fs::write(&p, seed_worker_digest("m", &said, None).to_string()).unwrap();
        records.push((
            "write".into(),
            "write app/x.py".into(),
            Some(true),
            "ok".into(),
        ));
        let mut at1 = 1usize; // fresh attempt's watermark starts past nothing of its own
        append_calls_jsonl(&p, None, 1, &records, &mut at1);
        let lines: Vec<serde_json::Value> =
            std::fs::read_to_string(p.with_extension("calls.jsonl"))
                .unwrap()
                .lines()
                .map(|l| serde_json::from_str(l).unwrap())
                .collect();
        assert_eq!(
            lines.len(),
            2,
            "attempt 0's row survived, attempt 1 appended"
        );
        assert_eq!(lines[0]["attempt"], 0);
        assert_eq!(lines[0]["pytest"]["failed"], 7);
        assert_eq!(lines[1]["attempt"], 1);
    }

    /// The 0dc8c297f RESIDUAL, closed: `build_task_ledger_row` was the second root-relative
    /// calls-read. The r5 round-1 shape — a re-dispatched fix shard in a FRESH shadow whose
    /// `copy_tree_excluding` skipped `.swarm` while the real tree's mirror holds every prior
    /// row — must recover the mirrored history; a normal task (mirror `None`) must not consult
    /// it and reads exactly what its root holds (nothing here).
    #[test]
    fn a_fix_shards_ledger_row_recovers_mirror_rows_the_fresh_shadow_lost() {
        let shadow = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(shadow.path().join(".swarm/activity")).unwrap();
        let mirror = tempfile::tempdir().unwrap();
        let key = crate::commands::swarm::activity_digest_key("complete-fix::app/sync.py");
        // Rows cut from the r5 mirror's real shapes (16 rows, all attempt 0): a boot, an
        // import probe, and the attempt_end whose fs_delta names the one changed file.
        let rows = [
            serde_json::json!({"ts":"2026-08-30T18:01:01Z","attempt":0,"name":"shell",
                "summary":"nohup python3 -m app.ledgerd --db-dir /tmp/ldv-sync --port 8090 &",
                "ok":true,"result_tail":"listening on 8090"}),
            serde_json::json!({"ts":"2026-08-30T18:02:02Z","attempt":0,"name":"shell",
                "summary":"python3 -m pytest --collect-only -q 2>&1 | tail -3","ok":true,
                "result_tail":"401 tests collected"}),
            serde_json::json!({"kind":"attempt_end","ts":"2026-08-30T18:03:00Z","attempt":0,
                "fs_delta":{"appeared":[],"changed":["app/sync.py"],"outside_manifest":[]}}),
        ];
        std::fs::write(
            mirror.path().join(format!("{key}.calls.jsonl")),
            rows.iter().map(|r| format!("{r}\n")).collect::<String>(),
        )
        .unwrap();
        let owned = vec!["app/sync.py".to_string()];
        let row = build_task_ledger_row(
            shadow.path(),
            "complete-fix::app/sync.py",
            "done",
            false,
            &owned,
            1,
            Some(mirror.path().to_path_buf()),
        );
        assert_eq!(row["commands"]["boot"]["count"], 1);
        assert_eq!(row["commands"]["import"]["count"], 1);
        assert_eq!(
            row["attempts"], 2,
            "the mirrored attempt-0 rows count under the round-1 dispatch"
        );
        assert_eq!(
            row["fs_delta"]["changed"],
            serde_json::json!(["app/sync.py"])
        );
        // A NORMAL task never consults the mirror: same missing root file, mirror None — the
        // row is honestly empty, exactly the pre-conversion bytes.
        let bare = build_task_ledger_row(
            shadow.path(),
            "complete-fix::app/sync.py",
            "done",
            false,
            &owned,
            1,
            None,
        );
        assert_eq!(bare["commands"], serde_json::json!({}));
        assert_eq!(bare["fs_delta"]["changed"], serde_json::json!([]));
        // And through the one live shadow-rooted caller: the GEN-3 completion facts.
        let facts = crate::commands::swarm::render_completed_output_from_ledger(
            shadow.path(),
            "complete-fix::app/sync.py",
            &owned,
            1,
            Some(mirror.path().to_path_buf()),
        )
        .expect("mirror-only rows render the wrote-line");
        assert!(facts.contains("wrote: app/sync.py"), "{facts}");
        assert!(
            crate::commands::swarm::render_completed_output_from_ledger(
                shadow.path(),
                "complete-fix::app/sync.py",
                &owned,
                1,
                None,
            )
            .is_none(),
            "no mirror, no facts — the absent behavior is unchanged"
        );
    }

    /// The mirror is SECOND, never first: a primary with rows wins outright (a speculative twin
    /// keeps reading its own shadow; a fix shard whose current attempt already captured calls
    /// reads those, not history) — `read_calls_capture`'s contract, pinned at the row builder.
    #[test]
    fn a_ledger_row_prefers_the_primary_capture_over_the_mirror() {
        let root = tempfile::tempdir().unwrap();
        let act = root.path().join(".swarm/activity");
        std::fs::create_dir_all(&act).unwrap();
        let mirror = tempfile::tempdir().unwrap();
        let key = crate::commands::swarm::activity_digest_key("complete-fix::app/sync.py");
        let primary_row = serde_json::json!({"ts":"2026-08-30T19:00:00Z","attempt":1,
            "name":"shell","summary":"python3 -m pytest tests/ -q","ok":true,
            "result_tail":"3 passed","pytest":{"failed":0,"passed":3,"errors":0,"raw":"3 passed"}});
        std::fs::write(
            act.join(format!("{key}.calls.jsonl")),
            format!("{primary_row}\n"),
        )
        .unwrap();
        let mirror_row = serde_json::json!({"ts":"2026-08-30T18:00:00Z","attempt":0,
            "name":"shell","summary":"python3 -m app --help","ok":true,"result_tail":"usage"});
        std::fs::write(
            mirror.path().join(format!("{key}.calls.jsonl")),
            format!("{mirror_row}\n"),
        )
        .unwrap();
        let row = build_task_ledger_row(
            root.path(),
            "complete-fix::app/sync.py",
            "done",
            false,
            &[],
            1,
            Some(mirror.path().to_path_buf()),
        );
        assert_eq!(row["commands"]["test"]["count"], 1);
        assert_eq!(
            row["commands"].get("boot"),
            None,
            "a non-empty primary is read alone; the mirror's boot row must not leak in"
        );
    }
}
