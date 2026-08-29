//! NOW.md is the anti-compaction source of truth: it is read first, at every tick, by a reader who has
//! just lost the thread and has no budget to re-derive anything. Two failure modes have already bitten
//! there, and neither is visible to a human proof-read — a documented command that does not run, and a
//! measurement whose run is not named, so nobody can tell when it expired. Both are mechanical, so both
//! are checked here rather than remembered.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/goose-swarm sits two levels below the repo root")
        .to_path_buf()
}

fn read(rel: &str) -> String {
    let path = repo_root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{} unreadable: {e}", path.display()))
}

/// The recipe docs. Both carried the same bare-`goose` invocation, which is how the trap survived a fix
/// to one of them.
const RECIPE_DOCS: [&str; 2] = ["NOW.md", "DESIGN-REALTIME-UI.md"];

fn strip_markup(token: &str) -> &str {
    token.trim_matches(|c: char| c == '`' || c == '(' || c == '"' || c == '*')
}

/// `goose swarm verify` is the command the "no run is started" rule depends on, and on this machine a
/// bare `goose` is a June build that answers "unrecognized subcommand 'swarm'". An invocation is only
/// runnable if it names a path to a binary, not the bare program name.
#[test]
fn documented_verify_invocations_name_a_binary_path() {
    for doc in RECIPE_DOCS {
        let text = read(doc);
        for (lineno, line) in text.lines().enumerate() {
            let Some(idx) = line.find("swarm verify") else {
                continue;
            };
            let before = line[..idx].trim_end();
            let program = strip_markup(before.split_whitespace().next_back().unwrap_or(""));

            assert!(
                program.ends_with("goose") && program.contains('/'),
                "{doc}:{} invokes `swarm verify` as `{program}`, which is not a path to a binary. A bare \
                 `goose` resolves to the June build with no `swarm` subcommand, so the documented \
                 isolation check errors instead of returning a verdict. Write the repo binary \
                 (./target/release/goose).\n  line: {line}",
                lineno + 1
            );
        }
    }
}

fn has_iso_date(text: &str) -> bool {
    let b = text.as_bytes();
    b.windows(10).any(|w| {
        w[0..4].iter().all(u8::is_ascii_digit)
            && w[4] == b'-'
            && w[5..7].iter().all(u8::is_ascii_digit)
            && w[7] == b'-'
            && w[8..10].iter().all(u8::is_ascii_digit)
    })
}

/// Split NOW.md into its bullets, so a date on an unrelated bullet cannot vouch for this one.
fn bullets(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        if line.starts_with("- ") || line.starts_with("#") || line.starts_with("|") {
            out.push(String::new());
        }
        if let Some(last) = out.last_mut() {
            last.push_str(line);
            last.push('\n');
        }
    }
    out
}

/// The judge measurement went stale invisibly because it was written as "the last full run" — a phrase
/// that never expires and never becomes wrong-looking, while the numbers under it described an engine
/// two fixes ago. A figure that names its run and date announces its own age.
#[test]
fn judge_measurements_name_the_run_they_came_from() {
    let text = read("NOW.md");
    for bullet in bullets(&text) {
        if !bullet.to_lowercase().contains("nudges") {
            continue;
        }
        assert!(
            has_iso_date(&bullet),
            "a NOW.md bullet quotes judge nudge counts without an ISO date naming the run they were \
             measured on, so the next reader cannot tell whether they have expired:\n{bullet}"
        );
    }
}

/// NOW.md cites `swarm.rs` for the claim that steer, not re-stream, is the current delivery. A citation
/// to a line that no longer says what was cited is the same staleness class this file exists to catch.
#[test]
fn the_cited_steer_default_still_exists_in_the_engine() {
    let engine = read("crates/goose-cli/src/commands/swarm.rs");
    assert!(
        engine.contains("let can_steer = pending.is_empty();"),
        "NOW.md cites `let can_steer = pending.is_empty();` as the reason the judge's destructive \
         re-stream is fixed. That line is gone from swarm.rs, so the doc's delivery claim must be \
         re-measured rather than left standing."
    );
}
