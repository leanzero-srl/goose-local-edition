//! Every heading in SWARM-AGENDA.md is stamped by hand at a tick. On 2026-08-29 all 33 labels of
//! that session drifted ahead of the commit that wrote them — from +1h05m at the start to +3h05m by
//! the end, five of them past the wall clock — because they were written from a remembered clock
//! instead of `date`. The agenda is ordered by those labels and nothing else, so a label that runs
//! ahead of reality silently reorders the record. This refuses the next one.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

const AGENDA: &str = "SWARM-AGENDA.md";

/// Labels are written in the repo owner's local time (EEST, +03:00). Reading them at the largest
/// plausible offset yields the earliest UTC instant, so a genuine label can never fail on a
/// timezone technicality — only a stamp that is provably ahead of its own commit does.
const LABEL_UTC_OFFSET_SECS: i64 = 3 * 3600;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

/// Unix seconds at which each 1-based line was last committed. An uncommitted line blames to the
/// current time, which is exactly the reference a not-yet-committed stamp should be held to.
fn commit_times(root: &Path) -> Option<HashMap<usize, i64>> {
    let out = Command::new("git")
        .current_dir(root)
        .args(["blame", "--line-porcelain", "-w", "--", AGENDA])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;

    let mut times = HashMap::new();
    let mut line_no: Option<usize> = None;
    for line in text.lines() {
        let mut fields = line.split(' ');
        let head = fields.next().unwrap_or_default();
        // A `summary` or `author` header can end in a bare number, so only a real commit header
        // — 40 hex digits — is allowed to move the cursor.
        if head.len() == 40 && head.bytes().all(|c| c.is_ascii_hexdigit()) {
            line_no = fields.nth(1).and_then(|n| n.parse::<usize>().ok());
        } else if let Some(rest) = line.strip_prefix("author-time ") {
            if let (Some(n), Ok(t)) = (line_no, rest.trim().parse::<i64>()) {
                times.insert(n, t);
            }
        }
    }
    (!times.is_empty()).then_some(times)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn digits(bytes: &[u8], at: usize, len: usize) -> Option<i64> {
    let slice = bytes.get(at..at + len)?;
    slice
        .iter()
        .all(u8::is_ascii_digit)
        .then(|| std::str::from_utf8(slice).ok()?.parse().ok())
        .flatten()
}

/// Every `YYYY-MM-DD HH:MM` in the line, as (label text, UTC seconds).
fn labels(line: &str) -> Vec<(String, i64)> {
    let b = line.as_bytes();
    let mut found = Vec::new();
    for i in 0..b.len() {
        if i + 16 > b.len() || b[i + 4] != b'-' || b[i + 7] != b'-' || b[i + 10] != b' ' {
            continue;
        }
        if b[i + 13] != b':' || (i > 0 && (b[i - 1].is_ascii_digit() || b[i - 1] == b'-')) {
            continue;
        }
        let (Some(y), Some(mo), Some(d)) =
            (digits(b, i, 4), digits(b, i + 5, 2), digits(b, i + 8, 2))
        else {
            continue;
        };
        let (Some(h), Some(mi)) = (digits(b, i + 11, 2), digits(b, i + 14, 2)) else {
            continue;
        };
        if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 {
            continue;
        }
        let utc = days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 - LABEL_UTC_OFFSET_SECS;
        found.push((format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}"), utc));
    }
    found
}

/// Labels standing later than the moment their line was last written. `references` is empty when
/// git cannot answer, and `fallback` — the wall clock — is then the reference every line is held to.
fn labels_ahead(text: &str, references: &HashMap<usize, i64>, fallback: i64) -> Vec<String> {
    let mut ahead = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let line_no = idx + 1;
        let reference = references.get(&line_no).copied().unwrap_or(fallback);
        for (label, utc) in labels(line) {
            if utc > reference {
                ahead.push(format!(
                    "  {}:{} stamped {} — {} minutes after the commit that carries it",
                    AGENDA,
                    line_no,
                    label,
                    (utc - reference) / 60
                ));
            }
        }
    }
    ahead
}

fn utc_of(label: &str) -> i64 {
    labels(label).first().expect("a parsable label").1
}

#[test]
fn no_agenda_label_is_later_than_the_commit_that_wrote_it() {
    let root = repo_root();
    let agenda = root.join(AGENDA);
    if !agenda.exists() {
        return;
    }
    let text = std::fs::read_to_string(&agenda).expect("read agenda");

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_secs() as i64;
    let references = commit_times(&root).unwrap_or_default();
    let ahead = labels_ahead(&text, &references, now);

    assert!(
        ahead.is_empty(),
        "SWARM-AGENDA.md labels run ahead of their own history:\n{}\n\n\
         Run `date` and stamp what it says. If you are quoting someone else's mis-stamp as evidence, \
         write the bare time (`stamped 12:10`) with no date in front of it — this gate cannot tell a \
         quoted label from a real one. See `## HOW TO STAMP AN ENTRY` at the end of the agenda.",
        ahead.join("\n")
    );
}

/// The positive control. A gate that has only ever reported zero is indistinguishable from a gate
/// that cannot see, and the drift it exists to catch was invisible for a whole session.
#[test]
fn a_label_ahead_of_its_commit_is_caught() {
    let drifted = "## THE PLAN DOCUMENT IS STALE — confirmed 2026-08-29 12:30";
    let refs = HashMap::from([(1usize, utc_of("2026-08-29 09:25"))]);

    let ahead = labels_ahead(drifted, &refs, i64::MAX);
    assert_eq!(ahead.len(), 1, "the 3h05m drift went unseen: {ahead:?}");
    assert!(ahead[0].contains("2026-08-29 12:30") && ahead[0].contains("185 minutes"));

    let on_time = "## RUN 6 LIVE — 2026-08-29 09:00 EEST";
    let refs = HashMap::from([(1usize, utc_of("2026-08-29 09:00") + 39)]);
    assert!(labels_ahead(on_time, &refs, i64::MAX).is_empty());
}

#[test]
fn without_git_the_wall_clock_is_the_reference() {
    let future = "stamped 2026-08-29 12:10 EEST";
    let now = utc_of("2026-08-29 10:46");
    assert_eq!(labels_ahead(future, &HashMap::new(), now).len(), 1);
    assert!(labels_ahead("stamped 2026-08-29 09:11 EEST", &HashMap::new(), now).is_empty());
}

/// Quoting a mis-stamp as evidence must stay possible, so the bare time carries no date beside it.
#[test]
fn only_a_date_adjacent_time_is_a_label() {
    assert!(
        labels("the .agents changelog was stamped 12:10 on a file whose mtime was 09:14")
            .is_empty()
    );
    assert!(labels("the heartbeat froze at 22:15:03Z, one minute into that tick").is_empty());
    assert!(labels("ts 2026-08-29T06:50:39.181Z").is_empty());
    assert_eq!(labels("authored `2026-08-29 09:25:22 +0300`").len(), 1);
}

#[test]
fn every_field_of_a_label_is_read() {
    // 2026-08-29 00:26 EEST is 2026-08-28 21:26 UTC — the day rolls back across the offset.
    assert_eq!(utc_of("2026-08-29 00:26"), 1_787_952_360);
    assert_eq!(utc_of("2026-08-29 00:27") - utc_of("2026-08-29 00:26"), 60);
    assert_eq!(
        utc_of("2026-08-30 00:26") - utc_of("2026-08-29 00:26"),
        86_400
    );
    assert!(labels("2026-13-29 00:26").is_empty());
    assert!(labels("2026-08-29 24:00").is_empty());
}
