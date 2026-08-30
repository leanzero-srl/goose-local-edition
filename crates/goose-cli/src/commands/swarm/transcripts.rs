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

use std::path::Path;

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
}
