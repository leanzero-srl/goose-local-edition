//! QUEUED USER NOTES — the `.swarm/inbox` channel a user types into while a build runs, read at
//! every dispatch and bounded by a SHOWN budget (VA-126: `budgets::ShownBudgets::user_notes_chars`,
//! derived from the fleet's probed context window; `USER_NOTES_BUDGET_CHARS` below is the
//! reference value on the 262,144 window). Sibling module under the incremental-split law
//! (development_gates::swarm_rs_line_count_only_decreases): moved verbatim from swarm.rs — the
//! constant, `DeliveredNotes`, `note_epoch_ms`, `read_user_notes` and their seven tests — paying
//! for VA-126's wiring in the dispatcher (the resolved budget set and its consumers).

/// The REFERENCE ceiling on the user-notes block, measured on the 262,144 window (VA-126: the live
/// value is `budgets::ShownBudgets::user_notes_chars`, scaled from the fleet's probed window; every
/// caller passes it in). It rides EVERY dispatch for the rest of the run, so an unbounded inbox
/// silently taxes every worker prompt. Mirrors `dep_budget`, which caps injected dependency APIs.
pub(super) const USER_NOTES_BUDGET_CHARS: usize = 1_500;

/// What the user's queued notes actually contributed to ONE dispatch.
#[derive(Debug, Default, Clone)]
pub(super) struct DeliveredNotes {
    /// The prompt block. Empty = nothing was injected.
    pub(super) block: String,
    /// Inbox filenames actually delivered — the join key between what the user typed and what a worker saw.
    pub(super) ids: Vec<String>,
    /// Notes in scope but cut by the char budget. Reported, never silent.
    pub(super) dropped: usize,
    /// Notes SCOPED OUT as older than this run (filename epoch-ms < run start). A note the user
    /// wrote for THIS run and never saw delivered is indistinguishable from one that vanished —
    /// MEASURED: an operator wrote a note with a SECONDS prefix, it parsed as 1970, was silently
    /// skipped as stale, and nothing anywhere said so (zero user_notes_delivered, no warning).
    /// Carried so the caller can say it out loud.
    pub(super) skipped_stale: Vec<String>,
}

/// The epoch-ms prefix of an inbox filename (the desktop writes `${Date.now()}.json`). `None` when there is
/// no leading digit run — a hand-placed `standing-orders.json` is deliberately never scoped out.
fn note_epoch_ms(file_name: &str) -> Option<i64> {
    let digits: String = file_name
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<i64>().ok()
}

/// QUEUED USER NOTES (GOOSE_SWARM_USER_NOTES, baked ON in the struct default).
///
/// A swarm run is 2+ hours. Today the ONLY user input channel is the one-shot clarify ask at planning time —
/// after that the user watches the whole build with no way to help, even while SEEING it go wrong. Mihai:
/// "I see that the progress is stalled and I want to help and I want to add more background information …
/// allow the user to continuously write messages and for goose to queue them up and from time to time pick
/// them up and use them."
///
/// SHAPE IS FORCED, not chosen: the desktop's write-file IPC has no append mode — it TRUNCATES — so a shared
/// messages.jsonl would destroy every prior note on each send. One file per note is the only shape that works
/// through the channel that already exists. It also makes each write atomic and gives a natural id.
///
/// Notes are never deleted (a crash must not lose one) and never block. Re-reading is idempotent: a note is
/// CONTEXT that rides every SUBSEQUENT DISPATCH OF THIS RUN, exactly as the pillars do — it is not handed to
/// one arbitrary worker. (The config field's doc used to claim "the NEXT dispatched worker"; that was wrong
/// and is now corrected there. One-shot would be the WORSE semantics: with N nodes dispatching concurrently,
/// "the next worker" is a lottery among unrelated tasks, so the user would silently steer one of them while
/// believing they had steered the build.)
///
/// SCOPED TO THIS RUN by `since_ms`. Nothing ever cleared `.swarm/inbox` — run_swarm clears only
/// `.swarm/prereview` — so before this, every note ever written to a project dir was injected into every
/// worker of every FUTURE run there, forever, under a header claiming it was "added while this build was
/// running". Yesterday's "the DB is already seeded" silently steered today's fresh build. A timestamp cutoff
/// fixes that as a pure predicate: no rename, no move, no torn state if the process dies mid-dispatch, and a
/// note is never lost — only out of scope. A filename with no parseable epoch prefix is ALWAYS delivered,
/// which is the deliberate escape hatch for a hand-placed standing note.
pub(super) fn read_user_notes(
    root: &std::path::Path,
    since_ms: i64,
    budget_chars: usize,
) -> DeliveredNotes {
    let dir = root.join(".swarm").join("inbox");
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return DeliveredNotes::default(); // no inbox => no notes => byte-identical prompt
    };
    let mut skipped_stale: Vec<String> = Vec::new();
    let mut notes: Vec<(String, String)> = rd
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            // Out of scope for THIS run: an older run's note left in a reused project dir.
            if note_epoch_ms(&name).is_some_and(|ms| ms < since_ms) {
                skipped_stale.push(name);
                return None;
            }
            let raw = std::fs::read_to_string(e.path()).ok()?;
            let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
            let text = v.get("text")?.as_str()?.trim().to_string();
            if text.is_empty() {
                return None;
            }
            Some((name, text))
        })
        .collect();
    if notes.is_empty() {
        return DeliveredNotes {
            skipped_stale,
            ..DeliveredNotes::default()
        };
    }
    notes.sort(); // filename is epoch-ms-prefixed => chronological

    // BOUND THE PROMPT TAX. This block rides EVERY dispatch for the rest of the run, on a fleet whose own
    // worker prompt warns that a large context is slow and degrades quality on local models. Keep the NEWEST
    // notes (the freshest guidance is the relevant guidance), then restore chronological order.
    let mut kept: Vec<(String, String)> = Vec::new();
    let mut used = 0usize;
    let mut dropped = 0usize;
    for n in notes.iter().rev() {
        let cost = n.1.chars().count() + 3;
        if used + cost > budget_chars && !kept.is_empty() {
            dropped += 1;
            continue;
        }
        used += cost;
        kept.push(n.clone());
    }
    kept.reverse();

    let body = kept
        .iter()
        .map(|(_, t)| format!("- {t}"))
        .collect::<Vec<_>>()
        .join("\n");
    DeliveredNotes {
        block: format!(
            "\n\n## NOTES FROM THE USER (added while this build was running)\n\
             The user is watching this build and added the following as BACKGROUND. Take it into account for the \
             work you are doing now. It does NOT override the spec or any decision already made — where they \
             disagree, the spec wins. If a note does not concern your task, ignore it.\n\n{body}\n"
        ),
        ids: kept.into_iter().map(|(id, _)| id).collect(),
        dropped,
        skipped_stale,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_notes_are_read_in_order_and_never_block_or_vanish() {
        let dir = tempfile::TempDir::new().unwrap();
        let inbox = dir.path().join(".swarm").join("inbox");
        std::fs::create_dir_all(&inbox).unwrap();
        // Filenames are epoch-ms prefixed => sorting them is chronological order.
        std::fs::write(
            inbox.join("1700000002-b.json"),
            r#"{"text":"second: the DB is already seeded"}"#,
        )
        .unwrap();
        std::fs::write(
            inbox.join("1700000001-a.json"),
            r#"{"text":"first: prefer stdlib over new deps"}"#,
        )
        .unwrap();
        // Junk must not break the read — a torn/foreign file is skipped, never fatal.
        std::fs::write(inbox.join("1700000003-c.json"), "{not json").unwrap();
        std::fs::write(inbox.join("ignore.txt"), "not a note").unwrap();
        std::fs::write(inbox.join("1700000004-d.json"), r#"{"text":"   "}"#).unwrap();

        let out = read_user_notes(dir.path(), 0, USER_NOTES_BUDGET_CHARS).block;
        let first = out.find("prefer stdlib").expect("first note present");
        let second = out.find("already seeded").expect("second note present");
        assert!(
            first < second,
            "notes must read in the order they were written"
        );
        assert!(out.contains("NOTES FROM THE USER"));
        // It must never be able to outrank the spec.
        assert!(out.contains("does NOT override the spec"));
        // The notes are still on disk — a crash must never lose one, so reading does not consume.
        assert!(inbox.join("1700000001-a.json").exists());
    }

    #[test]
    fn a_seconds_prefixed_note_is_reported_skipped_not_silently_dropped() {
        // The exact operator error: a note named with SECONDS instead of milliseconds parses as
        // 1970, scopes out, and used to vanish without a word — no delivery, no warning, nothing
        // in the run log to distinguish it from a note that was never written.
        let dir = tempfile::tempdir().unwrap();
        let inbox = dir.path().join(".swarm").join("inbox");
        std::fs::create_dir_all(&inbox).unwrap();
        std::fs::write(
            inbox.join("1787347648-ledger-server-spec.json"), // seconds, not ms
            r#"{"text":"use the brief and contracts as authoritative"}"#,
        )
        .unwrap();
        let since_ms = 1_787_300_000_000i64; // this run started (ms)
        let d = read_user_notes(dir.path(), since_ms, USER_NOTES_BUDGET_CHARS);
        assert!(
            d.ids.is_empty(),
            "a stale-scoped note must not be delivered"
        );
        assert_eq!(
            d.skipped_stale,
            vec!["1787347648-ledger-server-spec.json".to_string()],
            "and it must be REPORTED as skipped"
        );
        // A correctly-stamped note in the same inbox still lands.
        std::fs::write(
            inbox.join(format!("{}-good.json", since_ms + 1000)),
            r#"{"text":"bind before syncing"}"#,
        )
        .unwrap();
        let d2 = read_user_notes(dir.path(), since_ms, USER_NOTES_BUDGET_CHARS);
        assert_eq!(d2.ids.len(), 1);
        assert_eq!(d2.skipped_stale.len(), 1);
    }

    #[test]
    fn no_inbox_means_no_notes_and_a_byte_identical_prompt() {
        let dir = tempfile::TempDir::new().unwrap();
        assert_eq!(
            read_user_notes(dir.path(), 0, USER_NOTES_BUDGET_CHARS).block,
            ""
        );
        // An empty inbox is the same as none.
        std::fs::create_dir_all(dir.path().join(".swarm").join("inbox")).unwrap();
        assert_eq!(
            read_user_notes(dir.path(), 0, USER_NOTES_BUDGET_CHARS).block,
            ""
        );
    }
    /// The desktop prepends a per-turn `<turn-context>` block to the goal. MEASURED (loop-06): it is 171
    /// chars, and the retarget's research question uses `opts.prompt.chars().take(200)` — so the "task" the
    /// researcher was handed was 171 chars of XML and 28 chars of real spec. This fixture is the REAL wrapper

    #[test]
    fn a_note_from_a_previous_run_is_not_delivered_to_this_one() {
        let dir = tempfile::TempDir::new().unwrap();
        let inbox = dir.path().join(".swarm").join("inbox");
        std::fs::create_dir_all(&inbox).unwrap();
        std::fs::write(
            inbox.join("1700000000000.json"),
            r#"{"text":"STALE: the DB is already seeded"}"#,
        )
        .unwrap();
        std::fs::write(
            inbox.join("1800000000000.json"),
            r#"{"text":"FRESH: prefer stdlib over new deps"}"#,
        )
        .unwrap();

        let d = read_user_notes(dir.path(), 1_750_000_000_000, USER_NOTES_BUDGET_CHARS);
        assert!(
            d.block.contains("FRESH"),
            "this run's note must be delivered"
        );
        assert!(
            !d.block.contains("STALE"),
            "a previous run's note must NEVER ride this run: {}",
            d.block
        );
        assert_eq!(d.ids, vec!["1800000000000.json".to_string()]);

        // The note is SCOPED OUT, never destroyed — a crash must not lose one, and an older cutoff sees it.
        assert!(read_user_notes(dir.path(), 0, USER_NOTES_BUDGET_CHARS)
            .block
            .contains("STALE"));
        assert!(inbox.join("1700000000000.json").is_file());
    }

    /// A hand-placed file with no epoch prefix is the deliberate escape hatch for a standing instruction:
    /// it has no run to belong to, so it is always in scope.
    #[test]
    fn a_standing_note_without_a_timestamp_is_always_delivered() {
        let dir = tempfile::TempDir::new().unwrap();
        let inbox = dir.path().join(".swarm").join("inbox");
        std::fs::create_dir_all(&inbox).unwrap();
        std::fs::write(
            inbox.join("standing-orders.json"),
            r#"{"text":"always write tests first"}"#,
        )
        .unwrap();
        let d = read_user_notes(dir.path(), i64::MAX, USER_NOTES_BUDGET_CHARS);
        assert!(d.block.contains("always write tests first"));
    }

    /// The block rides EVERY dispatch for the rest of the run, so an unbounded inbox taxes every prompt on a
    /// fleet whose own worker prompt warns that large context degrades quality. Keep the NEWEST, report the
    /// rest — a silently-trimmed block would make the user think a note landed when it did not.
    #[test]
    fn the_notes_block_is_bounded_and_reports_what_it_dropped() {
        let dir = tempfile::TempDir::new().unwrap();
        let inbox = dir.path().join(".swarm").join("inbox");
        std::fs::create_dir_all(&inbox).unwrap();
        for i in 0..40 {
            std::fs::write(
                inbox.join(format!("18000000000{i:02}.json")),
                serde_json::json!({ "text": format!("note {i} {}", "x".repeat(100)) }).to_string(),
            )
            .unwrap();
        }
        let d = read_user_notes(dir.path(), 0, USER_NOTES_BUDGET_CHARS);
        assert!(
            d.block.chars().count() < 2500,
            "block must stay bounded, got {}",
            d.block.chars().count()
        );
        assert!(d.dropped > 0, "dropped notes must be COUNTED, never silent");
        assert_eq!(
            d.ids.len() + d.dropped,
            40,
            "every in-scope note is either delivered or reported dropped — none may vanish"
        );
        // The NEWEST guidance is what survives.
        assert!(d.block.contains("note 39"));
    }

    #[test]
    fn note_epoch_ms_reads_the_desktops_filename_shape() {
        assert_eq!(note_epoch_ms("1785213270712.json"), Some(1_785_213_270_712));
        assert_eq!(note_epoch_ms("1700000002-b.json"), Some(1_700_000_002));
        assert_eq!(note_epoch_ms("standing-orders.json"), None);
        assert_eq!(note_epoch_ms(".json"), None);
    }
}
