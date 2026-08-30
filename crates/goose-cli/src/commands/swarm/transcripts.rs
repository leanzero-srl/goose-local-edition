//! The durable-transcript cluster: the append-only `<task>.log`/`<task>.think.log` writers and
//! the attempt-boundary marker. Sixth sibling module under the incremental-split law
//! (development_gates::swarm_rs_line_count_only_decreases). Moved verbatim from swarm.rs —
//! behavior unchanged, each item keeps its own WHY — paying for the r6 instrument batch landing
//! in the same commits (timestamped steer/note blocks, the measured defect-steer closing).

use std::path::Path;

/// Append the reasoning produced SINCE THE LAST CALL to `<activity>.log`, and return the new index.
///
/// WHY THIS EXISTS. The digest's `full_reasoning` is a 24,000-char TAIL clip, so a long call's narration
/// starts partway through — Mihai, twice, on a node whose panel began at item 25 of a 39-item list:
/// *"the generations stop displaying past a certain number of characters"*. The clip is not gratuitous:
/// the digest is REWRITTEN on a hot 400ms timer, so it cannot simply grow, and raising the number just
/// moves the cliff while making every rewrite more expensive.
///
/// An append-only sibling has neither problem. Each write costs only the NEW text, the file is the whole
/// narration with nothing elided, and the digest keeps its bounded tail for the judge and the live panel.
/// Best-effort throughout: a transcript that fails to write must never disturb a run.
/// Append buffered THINKING to `<activity>.think.log` and clear the buffer.
///
/// The digest carries a 2,400-character rolling window of the reasoning channel, which is why the panel's
/// THINKING pane clears and refills instead of accumulating. This is the reasoning channel's only durable
/// record. Best-effort: a transcript that cannot be written must never disturb a run.
/// GEN-6a #8: the durable transcripts were best-effort-SILENT — a failed open/write left the
/// `.think.log` frozen with no trace, and the operator read a stale log as "the worker stopped
/// thinking". Both appenders now RETURN the write error so the caller (run_agent_in, which has
/// the events sink) can emit `transcript_write_failed` once per activity key. The write still
/// degrades — a transcript failure must never stop a worker — but it degrades loudly.
pub(super) fn append_thinking_transcript(activity_path: &Path, buf: &mut String) -> Option<String> {
    if buf.is_empty() {
        return None;
    }
    use std::io::Write;
    let log = activity_path.with_extension("think.log");
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
    {
        Ok(mut f) => match f.write_all(buf.as_bytes()) {
            Ok(()) => {
                buf.clear();
                None
            }
            Err(e) => Some(e.to_string()),
        },
        Err(e) => Some(e.to_string()),
    }
}

pub(super) fn append_reasoning_transcript(
    activity_path: &Path,
    texts: &[String],
    already: usize,
) -> (usize, Option<String>) {
    if texts.len() <= already {
        return (already, None);
    }
    use std::io::Write;
    let fresh = texts[already..].join("");
    if fresh.is_empty() {
        return (texts.len(), None);
    }
    let log = activity_path.with_extension("log");
    let err = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
    {
        Ok(mut f) => f.write_all(fresh.as_bytes()).err().map(|e| e.to_string()),
        Err(e) => Some(e.to_string()),
    };
    (texts.len(), err)
}

/// ONE attempt-marker line, appended to `<task>.log` and `<task>.think.log` when a call is seeded.
/// The transcripts are append-only across attempts (that is their value), so without a boundary the
/// panel cannot tell attempt 0's final error from attempt 1's first words — the UI splits at the
/// LAST marker into a LIVE segment plus superseded ones. Legacy logs without markers read as one
/// live segment, exactly as before.
pub(super) fn attempt_marker_line(attempt: u32, dispatched_at: &str) -> String {
    format!("\n===== swarm attempt {attempt} · dispatched {dispatched_at} =====\n")
}

pub(super) fn append_attempt_marker(activity_path: &Path, attempt: u32, dispatched_at: &str) {
    use std::io::Write;
    let line = attempt_marker_line(attempt, dispatched_at);
    for ext in ["log", "think.log"] {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(activity_path.with_extension(ext))
        {
            let _ = f.write_all(line.as_bytes());
        }
    }
}
