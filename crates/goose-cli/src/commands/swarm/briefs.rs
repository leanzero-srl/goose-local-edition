//! Worker-brief text fragments measured against the tree at dispatch time. Sibling module under
//! the incremental-split law (development_gates::swarm_rs_line_count_only_decreases).

use std::path::Path;

/// The multi-file note a worker owning >1 file reads. Two honest framings, branched on a
/// MEASURED predicate, never a guess:
///
/// - AUTHORING (any owned file missing): multi-file tasks fail by writing the first owned file,
///   forgetting the rest, then claiming done — the completion guard retries but the worker
///   repeats it, so the note demands every path exist and be non-empty.
/// - REPAIR where every owned file already EXISTS NON-EMPTY (the 6585f0845 winner+runner-up shard shape:
///   route table + handler body): "you MUST write EVERY one" was a lie-shaped pressure to
///   rewrite two live files whose defect lives in ONE of them. The softened note states the
///   measured fact (all files exist) and asks for a targeted edit wherever the defect actually
///   lives — either side can land, per the promote's owned-files surface.
///
/// Empty for a single-file task, exactly as before.
pub(super) fn multi_file_note(owned_files: &[String], repairing: bool, root: &Path) -> String {
    if owned_files.len() <= 1 {
        return String::new();
    }
    let n = owned_files.len();
    // Non-empty, not merely present: an existing-but-EMPTY owned file still needs its one
    // write, and "already exists — targeted fix" would suppress exactly that. An unreadable
    // stat counts as missing, which keeps the DEMANDING arm — the honest degradation.
    let exists_non_empty = |f: &String| {
        std::fs::metadata(root.join(f))
            .map(|m| m.is_file() && m.len() > 0)
            .unwrap_or(false)
    };
    if repairing && owned_files.iter().all(exists_non_empty) {
        return format!(
            "\nYOU OWN {n} FILES — every one already exists on disk. Your job is a targeted \
             fix, not a rewrite: the defect may live in EITHER file (route table vs handler \
             body — whichever side you fix can land), so read them, edit the one(s) that \
             actually carry the defect, and leave the rest as they are. Do NOT rewrite a file \
             just to have written it."
        );
    }
    format!(
        "\nYOU OWN {n} FILES — you MUST write EVERY one. The classic multi-file failure is \
         writing the first and forgetting the rest, then claiming done: this task is NOT \
         complete until ALL {n} paths above exist and are non-empty. Write them one after \
         another and verify each is on disk before you finish."
    )
}

/// Non-entry MULTI-FILE modules are the other over-read failure class (verified UNIQ13 plan-shopping, which owns
/// plan.py + shopping.py and needs 4 sibling modules: across 3 attempts it ran ls/tree/find/cat exploring the
/// layout + reading deps but NEVER wrote an owned file, so the no-write over-read timeout killed each attempt and
/// cascade-failed the run — 2nd instance after the UNIQ9 tests-writer). The entry gets skeleton_note; give non-entry
/// multi-file owners the same MECHANICAL fix: write a COMPILING STUB of each owned file FIRST (which flips
/// any_owned_written true and exempts the over-read timeout), then read deps + fill. Scoped to multi-file only —
/// single-file skeleton-first was a same-spec-A/B WASH. Empty when an owned file is the entry (skeleton_note covers
/// it). Gated on GOOSE_SWARM_SKELETON_FIRST (passed in as `enabled`). Pure + unit-tested.
///
/// DISARMED for a REPAIR shard (the 0dc8c297f tracer's addendum): a repairing multi-file shard
/// — exactly the two-file winner+runner-up shape 6585f0845 creates (route table + handler
/// body) — owns LIVE files, and "your FIRST actions must be a `write` for EACH owned file
/// emitting a COMPILING STUB… with a `pass` body" orders it to gut both before fixing one.
/// Same `repairing` predicate `multi_file_note` branches on, never re-derived.
pub(super) fn multifile_stub_note(
    owned_files: &[String],
    enabled: bool,
    repairing: bool,
) -> String {
    if !enabled
        || repairing
        || owned_files.len() <= 1
        || owned_files.iter().any(|f| is_entry_file(f.as_str()))
    {
        return String::new();
    }
    "\nSTUB-FIRST (you own MULTIPLE non-entry files): do NOT run ls/tree/find or read every dependency before \
     producing — a weak worker that explores first burns its budget and is KILLED for over-reading before it \
     writes anything (a whole task lost). Your FIRST actions must be a `write` for EACH owned file emitting a \
     COMPILING STUB: the imports it needs plus every public function/class with its real signature and a `pass` \
     body. Once the files EXIST you are exempt from the over-read timeout — THEN read only the specific dependency \
     APIs you need (injected below under 'API of …') and fill each body with a focused `edit`. Never finish with a \
     `pass`/stub body still in place."
        .to_string()
}

/// THE one copy of the entry-file rule: skeleton_first_note, multifile_stub_note and swarm.rs's
/// cli-contract arming all branch on it. It was two hand-rolled closures (one here, one in
/// swarm.rs) that could drift apart silently — the DeliveredFile::present class.
pub(super) fn is_entry_file(f: &str) -> bool {
    f.ends_with("cli.py")
        || f.ends_with("__main__.py")
        || f.ends_with("main.rs")
        || f.ends_with("index.ts")
        || f.ends_with("cli.ts")
        || f.ends_with("main.go")
}

/// The SKELETON-FIRST order for an entry-file AUTHOR (GOOSE_SWARM_SKELETON_FIRST, passed in as
/// `enabled`; `check` is the language's entry-run example). Text unchanged from its swarm.rs
/// inline origin. Pure + unit-tested.
///
/// DISARMED for a REPAIR shard — the same defect class as `multifile_stub_note`'s disarm above,
/// on the arm that stayed uncovered: a complete-fix shard owning an entry file received "your
/// FIRST `write` emits the COMPILING SKELETON … each with a placeholder body" directly beside
/// the repair body's "your FIRST tool call is `read` … never re-emit a LARGE file from memory".
/// Unlike the stub note's shape this one FIRED twice in the motivating run (both dispatches of
/// one entry-file repair shard). Its file already exists; the skeleton order is first-authoring
/// scaffolding only. Same `repairing` predicate as the notes above, never re-derived.
pub(super) fn skeleton_first_note(
    owned_files: &[String],
    enabled: bool,
    repairing: bool,
    check: &str,
) -> String {
    if !enabled || repairing || !owned_files.iter().any(|f| is_entry_file(f.as_str())) {
        return String::new();
    }
    format!(
        "\nSKELETON-FIRST (OVERRIDES the 'write the whole file in ONE write' rule below, \
         for your ENTRY/wiring file ONLY): your entry file wires many commands, so do NOT \
         plan the entire file then dump it in one write — that front-loads thinking, burns \
         turns, and hides a bad import until the very end. Instead: (1) your FIRST `write` \
         emits the COMPILING SKELETON — every import plus every command/subcommand the spec \
         advertises REGISTERED, each with a placeholder body (`pass` / `todo!()` / \
         `throw new Error('todo')`); (2) run `{check}` ONCE and confirm it imports and \
         lists EVERY command; (3) THEN fill each handler body with a focused `edit`. You \
         MUST finish with EVERY body fully implemented — a skeleton with placeholder bodies \
         left in is NOT done and will fail verification. Write any NON-entry owned file \
         complete in one write as usual."
    )
}

/// GOOSE_SWARM_CLI_CONTRACT (default ON): whether to inject the CLI-STRUCTURE contract into the entry worker.
pub(super) fn cli_contract_enabled() -> bool {
    std::env::var("GOOSE_SWARM_CLI_CONTRACT")
        .map(|v| {
            !matches!(
                v.trim().to_lowercase().as_str(),
                "0" | "off" | "false" | "no"
            )
        })
        .unwrap_or(true)
}

/// The entry file DEFINES the app's command-line interface, and it is the module the weak worker most often
/// drifts on the SHAPE of — verified twice: UNIQ9 built `checkin NAME DATE` (positional) instead of the spec's
/// `checkin NAME --date DATE`; UNIQ10 built flat `group-add` + per-command positional db + cents display instead
/// of the spec's nested `group add` + a GLOBAL `--db` before the subcommand + dollars. In both the ENGINE was
/// correct but the interface violated the spec, so spec-drift review failed the entry (and blocked its
/// dependents). This note freezes the interface CONTRACT for the entry worker: preserve the spec's exact command
/// tree, option placement, units and value syntax. Pure + unit-tested.
pub(super) fn cli_contract_note(has_entry_file: bool, enabled: bool) -> String {
    if !enabled || !has_entry_file {
        return String::new();
    }
    "\nCLI STRUCTURE CONTRACT (your entry file IS the command-line interface — match the spec's SHAPE exactly; \
     spec-drift review verifies this and FAILS a working-but-wrong-shaped CLI):\n\
     - NESTED subcommands stay NESTED: if the spec writes `group add NAME` / `member add GROUP NAME`, implement \
       a `group` command WITH an `add` subcommand — NOT a flat hyphenated `group-add`.\n\
     - GLOBAL options stay GLOBAL: if the spec shows an option BEFORE the subcommand (e.g. `--db PATH init`), \
       parse it at the top level so it works before ANY subcommand — NOT as a per-command positional argument.\n\
     - Match each argument's POSITIONAL-vs-FLAG form EXACTLY as the spec writes it: a BARE word after the \
       subcommand (e.g. `product add SKU`, `warehouse add NAME`, `stock level SKU`) is a POSITIONAL argument — keep \
       it positional, do NOT convert it into a `--sku`/`--name` flag; conversely a `--flag VALUE` stays a flag, not \
       a positional. Converting the spec's positionals into flags (or vice-versa) is a spec-drift FAILURE even when \
       the logic is correct.\n\
     - Use the spec's EXACT option and command names — do NOT rename or 'improve' them: `--from`/`--to` must stay \
       `--from`/`--to` (not `--source`/`--dest`), `--reorder` must stay `--reorder` (not `--reorder-level`). Match \
       value UNITS (dollars with 2 decimals vs raw cents) and share/pair SYNTAX (`name=value`, not `name:value`). A \
       CLI that computes correctly but does not accept the spec's exact invocations is a spec-drift FAILURE — do not \
       silently re-shape the interface for convenience.\n\
     - Subcommand NAMES passed to add_parser() are STRINGS, not Python identifiers: use the spec's EXACT subcommand \
       name even when it is a Python reserved word — write `add_parser(\"import\")`, `add_parser(\"class\")`, \
       `add_parser(\"del\")`, NOT `\"import_\"`/`\"import2\"`/`\"import_cmd\"`. Trailing-underscore keyword-avoidance \
       is for Python VARIABLE/function names ONLY (`import_parser = subparsers.add_parser(\"import\")` is correct); \
       the CLI-facing subcommand string must stay verbatim so `prog import --file` works. Renaming the subcommand \
       `import` to `import_` makes the spec's `import` invocation fail = spec-drift.\n"
        .to_string()
}

/// THE WRITE-GRANULARITY BULLET of the universal TOOLS block, which REVERSES between the two
/// kinds and used to ship only its authoring half. Authoring: one complete `write` per file,
/// because a chain of small edits costs a round-trip each on a local model. Repair: the opposite
/// — the order is the smallest edit that removes an OBSERVED defect, and a whole-file re-emit
/// from memory is how a repair round regresses code no finding named (the same reason
/// `skeleton_first_note` and `multifile_stub_note` are disarmed for this kind). The authoring
/// text is byte-identical to its swarm.rs inline origin, including the supervisor-note exception.
pub(super) fn write_granularity_rule(repairing: bool) -> &'static str {
    if repairing {
        "- CHANGE ONLY WHAT THE FINDING NAMES: prefer `edit` on the exact lines, and do not \
         re-emit a live file from memory — a rewrite silently drops the parts of it no finding \
         mentioned. A full `write` is right only for a small file you have read in full here.\n"
    } else {
        "- Write each file COMPLETE in ONE `write` and move on. Do NOT write a rough draft then refine \
         it with a chain of small `edit`s — plan the whole file first, then write it once. Every extra \
         round-trip costs ~30-60s on a local model and is the main reason tasks run slow. EXCEPTION: a \
         SUPERVISOR NOTE asking for a FIRST MINIMAL VERSION of a file OVERRIDES this rule — a minimal \
         version IS a complete file (it parses/loads clean and exports the named API; stub bodies are \
         fine), which you then EXTEND with further complete writes. When such a note arrives, the bytes \
         on disk are the deliverable; composing more of the file in your reasoning instead of writing \
         it is the failure mode the note is correcting.\n"
    }
}

/// ASSET OWNER (F873 waste mine): a styles.css/index.html owner used to get the implementer
/// rules VERBATIM — "verify by RUNNING", "run python3 -m pytest" — so css workers ran the full
/// Python suite (measured: 11 css-owner tasks, 57 shell calls, ~2,500 node-s above floor). No
/// Python suite applies to a static asset. ONE classifier: the dispatch branches its owner body
/// on it AND the `rules_delivered` event labels the delivered arm with it, which is why it is a
/// named function rather than a closure inside one of the two.
pub(super) fn is_asset_owner(owned_files: &[String]) -> bool {
    !owned_files.is_empty()
        && owned_files.iter().all(|f| {
            f.ends_with(".html")
                || f.ends_with(".htm")
                || f.ends_with(".css")
                || f.ends_with(".js")
                || f.ends_with(".mjs")
                || f.ends_with(".md")
                || f.ends_with(".txt")
        })
}

/// THE REPAIR ORDER — the whole body a `fix::`/`complete-fix::` shard reads under its owned
/// paths. It replaces the first-authoring script for that kind, and it is now the ONLY place the
/// repair rules are stated, because the three that stated them SEPARATELY contradicted each other
/// in one prompt (measured on r6c, quoted from the lanes' own think.logs):
///
/// - this body demanded an edit ("by your THIRD tool call you MUST have made an `edit`… a turn
///   that ends with ZERO file modifications FAILS"),
/// - `current_content_block` below opened with "FIRST run the program/tests to check whether it
///   already satisfies the spec; if it does, report DONE immediately",
/// - `fix_directive` said "you own no files by default: you may edit ANY file the fix requires"
///   directly beside "YOU OWN — write EXACTLY these ABSOLUTE paths, and write NOTHING outside
///   them".
///
/// The lanes read the collision and resolved it AGAINST repairing: complete-fix::web/app.js —
/// "Forced to make an edit (zero-edit turn = automatic FAIL)" and then "No file edits were needed
/// (per the explicit escape clause 'if it already satisfies the spec, immediately report DONE')";
/// complete-fix::app/ledgerd/__init__.py quoted the escape clause SIX times and closed 0 edits;
/// complete-fix::app/webhooks.py re-probed a finding with 8 request variants and returned NOT REAL
/// in round 0, then was re-dispatched the same finding and did it AGAIN in round 1 (4,809s).
/// So: ONE order, ONE precedence, no fourth block explaining which of the three wins.
///
/// `speculative` is the dispatch's own shadow flag, because the promotion sentence must be TRUE:
/// a shadowed shard's edits outside its owned paths are dropped at promote (r6c's app.js shard
/// verified a fix in `app/drafts.py` and lost it exactly that way), while a non-shadow repair
/// shares the live tree with other writers. Pure + unit-tested.
pub(super) fn repair_owner_body(owned_files: &[String], speculative: bool) -> String {
    let paths = owned_files
        .iter()
        .map(|f| format!("`{f}`"))
        .collect::<Vec<_>>()
        .join(", ");
    let landing = if speculative {
        format!(
            "WHERE YOUR EDITS LAND: you are working in a private copy of the tree and only {paths} \
             {} copied back out of it, so an edit to any other file is DISCARDED even when it is \
             right.",
            if owned_files.len() == 1 { "is" } else { "are" }
        )
    } else {
        format!(
            "WHERE YOUR EDITS LAND: {paths} {} yours; every other file in this tree belongs to \
             another worker that may be writing it right now, so an edit there can be overwritten \
             without warning.",
            if owned_files.len() == 1 { "is" } else { "are" }
        )
    };
    format!(
        "YOU ARE REPAIRING AN EXISTING FILE AGAINST A DEFECT THAT WAS ALREADY OBSERVED. Every \
         numbered finding in your task was measured against the RUNNING app by the engine check \
         named beside it — it is a report of something that happened, not a hypothesis for you to \
         re-derive. The current content of {paths} is inlined below, so your first move is to read \
         the part the finding names (never `cat` a file that is already inlined), then make the \
         SMALLEST edit that removes that defect, then run the ONE check that reproduces the \
         finding's own symptom and watch it stop: if it names a URL, request that URL; if it names \
         a command, run that command; if it names an element or a field, look for it in the real \
         response. Reproducing the symptom ONCE before you edit is fine — an investigation is not: \
         a round spent re-measuring code instead of changing it closes nothing.\n\
         KEEP THE EDIT SMALL: use `edit`, keep everything the finding does not name, and never \
         re-emit a LARGE file from memory (under ~60 lines a full corrected `write` is fine and \
         better than paralysis). READ whatever the finding points at — including the TEST that \
         failed, which is the definition of the expected behaviour. Fix the side that is WRONG \
         against the spec, not the side that is easier to edit. An imperfect edit costs nothing: \
         only a strictly better tree is kept, so there is no reason to withhold one.\n\
         {landing} If the real defect lives in a file you do not own, do NOT spend the round \
         editing it — HAND IT OFF in your final message: the exact path, the exact function or \
         symbol, and the change it needs. For that finding, that handoff IS the deliverable, and \
         the next round acts on it.\n\
         IF A FINDING IS NOT REAL, say so in ONE sentence naming the output that rules it out, \
         and move on to the next finding. Do not spend the round proving that working code \
         works.\n\n"
    )
}

/// The FIX directive (GOOSE_SWARM_READ_ON_FIX) that opens a repair worker's system prompt, ahead
/// of everything else it reads. Moved here VERBATIM except its ownership bullet, which was the
/// third of the three colliding blocks: "you own no files by default: you may edit ANY file the
/// fix requires" is TRUE for the owns-nothing sink and FALSE for a per-file repair shard, which
/// reads it ~1,400 characters before "write NOTHING outside them" and then reasons about the
/// conflict instead of the defect (r6c, complete-fix::app/ledgerd/__init__.py: "I can only write
/// to app/ledgerd/__init__.py! Hmm, 'write NOTHING outside them.'"). Branched on the dispatch's
/// own owned_files, so the sink's text stays byte-identical.
pub(super) fn fix_directive(owned_files: &[String]) -> String {
    let ownership = if owned_files.is_empty() {
        "- You own no files by default: you may edit ANY file the fix requires.\n".to_string()
    } else {
        format!(
            "- EDIT ONLY THE FILE(S) YOU OWN, listed below — reading is unrestricted, writing is \
             not. A fix that belongs in a file you do not own is HANDED OFF by name (path, symbol, \
             the change it needs) in your final message; edited there, it is lost.\n\
             - Your owned path(s): {}.\n",
            owned_files
                .iter()
                .map(|f| format!("`{f}`"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    format!(
        "\nYOU ARE FIXING A PROVEN DEFECT, NOT WRITING NEW CODE. The rules about not reading are \
         SUSPENDED for this task, because the failure below was already reproduced by running the \
         app and may span SEVERAL files:\n\
         - READ every file named in the error, and the file that DEFINES any symbol the error \
         mentions. A signature mismatch lives in TWO files — the caller and the callee — and you \
         cannot fix it from one.\n\
         - Before editing, confirm the REAL signature/behaviour by reading the definition. Do NOT \
         guess it and do NOT trust an injected excerpt over the actual source here.\n\
         - Fix the side that is WRONG relative to the project's own spec, not whichever is easier \
         to edit.\n\
         {ownership}\
         - Do not stop at the first green sub-test; re-run the failing command itself and confirm \
         THAT passes.\n"
    )
}

/// The CURRENT content of every owned file that already exists on disk, inlined so the worker
/// never re-`cat`s it. Moved out of swarm.rs's dispatch assembly VERBATIM for the amendment case;
/// the REPAIR case is new and is the second of the three colliding blocks described on
/// `repair_owner_body`.
///
/// The amendment text ("FIRST run the program/tests to check whether it already satisfies the
/// spec; if it does, report DONE immediately") is correct for a RE-DISPATCHED build task, whose
/// hazard is redoing finished work. It is wrong for a repair shard, which exists BECAUSE a defect
/// was observed with evidence, and where "report DONE" is an exit that closes nothing — measured
/// as the first instruction three r6c shards quoted back while making zero edits. So a repairing
/// dispatch reads a repair order and a re-dispatched author reads the byte-identical old text.
pub(super) fn current_content_block(
    root: &Path,
    owned_files: &[String],
    repairing: bool,
) -> String {
    let mut block = String::new();
    for f in owned_files {
        let Ok(content) = std::fs::read_to_string(root.join(f)) else {
            continue;
        };
        if content.trim().is_empty() {
            continue;
        }
        let capped: String = content.chars().take(12000).collect();
        let note = if content.chars().count() > 12000 {
            " [truncated — head only; cat the rest only if needed]"
        } else {
            ""
        };
        if repairing {
            block.push_str(&format!(
                "## CURRENT content of {f}{note} — the file you are REPAIRING, inlined so you \
                 never `cat` it. The defect named in your task was OBSERVED against this app \
                 while it was running; go to the part the finding points at, make the smallest \
                 edit that removes it, and prove it with the finding's own check. Do NOT rewrite \
                 the file from scratch — that re-does finished work and regresses code no finding \
                 named:\n```\n{capped}\n```\n\n"
            ));
        } else {
            block.push_str(&format!(
                "## CURRENT content of {f}{note} — this file ALREADY EXISTS (you were re-dispatched, \
                 or it is an amendment). Do NOT rewrite it from scratch — that re-does finished work \
                 and risks another timeout. FIRST run the program/tests to check whether it already \
                 satisfies the spec; if it does, report DONE immediately. Otherwise edit ONLY the real \
                 defect, from here. Do NOT `cat` it again:\n```\n{capped}\n```\n\n"
            ));
        }
    }
    block
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The predicate is measured on disk: a repair shard whose owned files ALL exist gets the
    /// targeted-edit framing; a missing file (or a non-repair task) keeps the write-every-one
    /// demand; a single file gets nothing.
    #[test]
    fn the_multi_file_note_softens_only_for_a_repair_shard_whose_files_all_exist() {
        let dir = std::env::temp_dir().join(format!("briefs-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("app")).unwrap();
        let owned = vec!["app/a.py".to_string(), "app/b.py".to_string()];
        std::fs::write(dir.join("app/a.py"), "x = 1\n").unwrap();
        std::fs::write(dir.join("app/b.py"), "y = 2\n").unwrap();
        let soft = multi_file_note(&owned, true, &dir);
        assert!(soft.contains("targeted"), "softened framing: {soft}");
        assert!(!soft.contains("MUST write EVERY one"));
        // Same files, not a repair shard: the authoring demand stands.
        assert!(multi_file_note(&owned, false, &dir).contains("MUST write EVERY one"));
        // Repair shard but one file EMPTY: the demand stands — "already exists" must not
        // suppress the one write an empty file needs (the 0dc8c297f tracer's addendum).
        std::fs::write(dir.join("app/b.py"), "").unwrap();
        assert!(multi_file_note(&owned, true, &dir).contains("MUST write EVERY one"));
        // Repair shard but one file missing: the demand stands (the missing file must appear).
        std::fs::remove_file(dir.join("app/b.py")).unwrap();
        assert!(multi_file_note(&owned, true, &dir).contains("MUST write EVERY one"));
        assert_eq!(multi_file_note(&owned[..1], true, &dir), "");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The skeleton order fired on a repairing entry-file shard at BOTH of its dispatches in the
    /// motivating run, ordering "your FIRST `write` emits the COMPILING SKELETON" directly beside
    /// the repair body's "your FIRST tool call is `read`". A repairing shard's file exists; the
    /// order is authoring scaffolding and must never reach it. An authoring entry task keeps it.
    #[test]
    fn the_skeleton_order_never_reaches_a_repairing_shard() {
        let owned = vec!["app/__main__.py".to_string()];
        assert_eq!(
            skeleton_first_note(&owned, true, true, "python3 -m app --help"),
            ""
        );
        let note = skeleton_first_note(&owned, true, false, "python3 -m app --help");
        assert!(note.contains("COMPILING SKELETON"));
        assert!(note.contains("python3 -m app --help"));
        // Non-entry owner and lever-off both stay silent, exactly as at the inline origin.
        assert_eq!(
            skeleton_first_note(&["app/util.py".into()], true, false, "x"),
            ""
        );
        assert_eq!(skeleton_first_note(&owned, false, false, "x"), "");
    }

    #[test]
    fn cli_contract_note_fires_only_for_entry_when_enabled() {
        // Entry file + enabled -> a non-empty CLI-structure contract mentioning nested/global/units.
        let note = cli_contract_note(true, true);
        assert!(note.contains("CLI STRUCTURE CONTRACT"));
        assert!(note.contains("NESTED") && note.contains("GLOBAL"));
        // POSITIONAL-vs-flag + no-rename rules (UNIQ16 drifted positionals to flags + renamed --from/--to).
        assert!(note.contains("POSITIONAL") && note.contains("do NOT rename"));
        // Keyword-subcommand-name rule (UNIQ26 registered `import_` for the spec's `import` -> `store import` failed).
        assert!(note.contains("reserved word") && note.contains("add_parser(\"import\")"));
        // Disabled, or no entry file among the owned set -> empty (no-op, byte-identical default-off path).
        assert!(cli_contract_note(true, false).is_empty());
        assert!(cli_contract_note(false, true).is_empty());
    }

    #[test]
    fn multifile_stub_note_fires_only_for_multifile_non_entry() {
        // Multi-file non-entry module (the plan-shopping case) -> stub-first note; entry,
        // single-file, disabled, and REPAIRING -> empty.
        let note = multifile_stub_note(
            &["recipes/plan.py".into(), "recipes/shopping.py".into()],
            true,
            false,
        );
        assert!(note.contains("STUB-FIRST") && note.contains("COMPILING STUB"));
        // A REPAIR shard's owned files are LIVE: no stub order may reach it — the r5 round-2
        // two-file shape (route table + handler body) must not be told to gut both.
        assert!(
            multifile_stub_note(
                &["app/ledgerd/__init__.py".into(), "app/httpapi.py".into()],
                true,
                true,
            )
            .is_empty(),
            "a repairing multi-file shard reads no stub-first order"
        );
        // A file set that includes the entry is covered by skeleton_note -> empty here.
        assert!(
            multifile_stub_note(&["pkg/cli.py".into(), "pkg/util.py".into()], true, false)
                .is_empty()
        );
        assert!(
            multifile_stub_note(&["pkg/__main__.py".into(), "pkg/x.py".into()], true, false)
                .is_empty()
        );
        // Single-file -> empty (skeleton-first was a wash on simple single-file tasks).
        assert!(multifile_stub_note(&["pkg/only.py".into()], true, false).is_empty());
        // Disabled -> empty.
        assert!(multifile_stub_note(&["a.py".into(), "b.py".into()], false, false).is_empty());
    }

    /// THE DONE-ESCAPE IS THE FIRST THING A REPAIR SHARD USED TO READ. r6c, verbatim from
    /// complete-fix::app/ledgerd/__init__.py's think.log: "Oh. That's explicitly written in my
    /// task header! 'FIRST run the program/tests to check whether it already satisfies the spec;
    /// if it does, report DONE immediately.'" — that shard closed the round with zero edits, as
    /// did complete-fix::web/app.js ("per the explicit escape clause"). The amendment arm keeps
    /// the text byte-for-byte: for a RE-DISPATCHED author the hazard really is redoing work.
    #[test]
    fn a_repair_shard_reads_a_repair_order_and_an_amendment_reads_the_old_text() {
        let dir = std::env::temp_dir().join(format!(
            "goose-briefs-current-content-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(dir.join("app")).unwrap();
        std::fs::write(dir.join("app/webhooks.py"), "def handle():\n    return 1\n").unwrap();
        let owned = vec!["app/webhooks.py".to_string()];

        let repair = current_content_block(&dir, &owned, true);
        assert!(
            repair.contains("def handle()"),
            "the file's real content must still be inlined"
        );
        assert!(
            !repair.contains("report DONE immediately")
                && !repair.contains("already satisfies the spec"),
            "a repair shard must not be handed a done-escape: {repair}"
        );
        assert!(
            repair.contains("REPAIRING") && repair.contains("OBSERVED"),
            "the repair arm states the defect was observed"
        );

        let amend = current_content_block(&dir, &owned, false);
        assert!(
            amend.contains(
                "FIRST run the program/tests to check whether it already satisfies the spec; if \
                 it does, report DONE immediately."
            ),
            "the amendment text is correct for its case and must stay byte-identical"
        );

        // A file that is absent or empty contributes nothing, either way.
        assert!(current_content_block(&dir, &["app/nope.py".to_string()], true).is_empty());
        std::fs::write(dir.join("app/blank.py"), "   \n").unwrap();
        assert!(current_content_block(&dir, &["app/blank.py".to_string()], true).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// ONE ORDER, ONE PRECEDENCE. The three r6c collisions must all be gone from the body a
    /// repair shard reads: no turn-count clock, no zero-edit threat that a lane answers with a
    /// cosmetic edit, and exactly one statement of where an edit may land — plus the handoff that
    /// replaces the edit when the defect lives elsewhere (r6c's web/app.js shard verified a fix in
    /// app/drafts.py it did not own, and the promote dropped it).
    #[test]
    fn the_repair_order_states_one_precedence_and_offers_a_handoff() {
        let body = repair_owner_body(&["web/viz.js".into(), "web/index.html".into()], true);
        assert!(
            body.contains("OBSERVED"),
            "the defect is a report, not a guess"
        );
        for banned in [
            "report DONE",
            "ACT ON A CLOCK",
            "THIRD tool call",
            "ZERO file modifications FAILS",
        ] {
            assert!(
                !body.contains(banned),
                "the repair order still carries `{banned}`: {body}"
            );
        }
        assert!(
            body.contains("HAND IT OFF") && body.contains("exact function or symbol"),
            "a defect outside ownership is handed off by name, not edited and lost"
        );
        assert!(
            body.contains("`web/viz.js`, `web/index.html`"),
            "the ownership sentence names THIS shard's real paths"
        );
        // The promotion sentence must be true of the dispatch that reads it.
        assert!(body.contains("private copy of the tree"));
        assert!(
            repair_owner_body(&["app/api.py".into()], false).contains("belongs to another worker")
        );
        // Singular/plural agreement — a prompt that reads as machine filler is read as filler.
        assert!(repair_owner_body(&["app/api.py".into()], true).contains("`app/api.py` is copied"));
    }

    /// The universal TOOLS bullet that reverses by kind: a repair shard was reading "plan the
    /// whole file first, then write it once" in the same prompt as "make the SMALLEST edit".
    #[test]
    fn the_write_granularity_rule_reverses_for_a_repair_shard() {
        let authoring = write_granularity_rule(false);
        assert!(
            authoring.contains("Write each file COMPLETE in ONE `write`")
                && authoring.contains("SUPERVISOR NOTE"),
            "the authoring half stays byte-identical, exception included"
        );
        let repair = write_granularity_rule(true);
        assert!(
            !repair.contains("COMPLETE in ONE `write`") && !repair.contains("plan the whole file"),
            "a repair shard must not be told to re-emit its live file: {repair}"
        );
        assert!(repair.contains("CHANGE ONLY WHAT THE FINDING NAMES"));
    }

    /// The sink owns nothing and MAY edit anywhere; a per-file shard may not, and used to read
    /// both rules in one prompt. Same predicate the dispatch already branches ownership on.
    #[test]
    fn fix_directive_never_tells_an_owning_shard_it_owns_nothing() {
        let sink = fix_directive(&[]);
        assert!(
            sink.contains("You own no files by default: you may edit ANY file the fix requires."),
            "the owns-nothing sink's directive stays byte-identical"
        );
        let shard = fix_directive(&["app/webhooks.py".into()]);
        assert!(
            !shard.contains("you own no files by default"),
            "an owning shard must not be told it owns nothing: {shard}"
        );
        assert!(
            shard.contains("EDIT ONLY THE FILE(S) YOU OWN")
                && shard.contains("`app/webhooks.py`")
                && shard.contains("HANDED OFF"),
            "the shard reads its real paths and the handoff rule: {shard}"
        );
        // The shared half — the reason the read prohibitions lift — is identical in both.
        for d in [&sink, &shard] {
            assert!(d.contains("The rules about not reading are SUSPENDED for this task"));
            assert!(d.contains("re-run the failing command itself and confirm THAT passes"));
        }
    }
}
