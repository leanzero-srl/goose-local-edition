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
         numbered finding in your task comes from the engine check named beside it, which made a \
         SPECIFIC request against the RUNNING app and observed a SPECIFIC response — both are \
         quoted with the finding. Reproduce THAT request first, exactly as quoted (same method, \
         path, headers and body, or the absence of them), then fix what it shows. The current \
         content of {paths} is inlined below, so go to the part the finding names, make the \
         SMALLEST edit that removes that defect, then run the finding's own check again and watch \
         it stop: if it names a URL, request that URL; if it names a command, run that command; \
         if it names an element or a field, look for it in the real response. Reproducing the \
         quoted request ONCE before you edit is the method — an investigation with requests of \
         your own is not: a round spent re-measuring code instead of changing it closes nothing.\n\
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
        let total = content.chars().count();
        let capped: String = content.chars().take(12000).collect();
        let truncated = total > 12000;
        // r6c, complete-fix::web/app.js attempt 0: "The truncated view only shows up to
        // _validate_create. I need to read the actual file" — while this block said "never `cat`
        // it". Past the cap, the part the finding names may not be here at all, so the text says
        // so and PERMITS the one targeted read the finding requires; a whole-file `cat` stays
        // discouraged in both arms.
        let note = if truncated {
            format!(
                " [TRUNCATED: the first 12,000 of {total} chars are inlined; the rest is on disk]"
            )
        } else {
            String::new()
        };
        if repairing {
            let read_rule = if truncated {
                "If the part the finding names is not in this head, `grep -n` the symbol and \
                 `sed -n 'A,Bp'` exactly that region — that targeted read is expected. Do not \
                 `cat` the whole file."
            } else {
                "It is inlined in full, so you never `cat` it."
            };
            block.push_str(&format!(
                "## CURRENT content of {f}{note} — the file you are REPAIRING. {read_rule} The \
                 defect named in your task was OBSERVED against this app while it was running; go \
                 to the part the finding points at, make the smallest edit that removes it, and \
                 prove it with the finding's own check. Do NOT rewrite the file from scratch — \
                 that re-does finished work and regresses code no finding named:\n```\n{capped}\n```\n\n"
            ));
        } else {
            let read_rule = if truncated {
                "If the defect's part is past this head, `sed -n 'A,Bp'` that region; do not `cat` \
                 the whole file"
            } else {
                "Do NOT `cat` it again"
            };
            block.push_str(&format!(
                "## CURRENT content of {f}{note} — this file ALREADY EXISTS (you were re-dispatched, \
                 or it is an amendment). Do NOT rewrite it from scratch — that re-does finished work \
                 and risks another timeout. FIRST run the program/tests to check whether it already \
                 satisfies the spec; if it does, report DONE immediately. Otherwise edit ONLY the real \
                 defect, from here. {read_rule}:\n```\n{capped}\n```\n\n"
            ));
        }
    }
    block
}

/// WEB-TRIPLET VOCABULARY (F870, the authoring-time half). swarm-3node-r0's css worker
/// designed 36 class rules, its html worker wrote id-only markup, and its js worker invented
/// a third vocabulary for generated rows — three honest workers, zero shared names, an
/// unstyled page. The module contracts freeze Python signatures but nobody froze the ONE
/// thing a web triplet actually shares: its class/id names. This note makes the vocabulary a
/// stated obligation and licenses the ONE read that satisfies it (the sibling web files),
/// overriding the no-exploration rule exactly as SKELETON-FIRST does. Fires for owners of
/// .css/.html always, and for .js/.mjs only under a frontend-shaped path — a Node backend's
/// js has no styling vocabulary to share. Gated on GOOSE_SWARM_WEB_VOCAB (default ON).
///
/// DISARMED for a REPAIR shard — the authoring-time half does not apply once the triplet exists.
/// r6c: complete-fix::web/app.js (36 dispatches) and complete-fix::web/viz.js (21) read this
/// frozen list, which has no `viz-labels` / `viz-label` / `role-token` — the ids their findings
/// were about — beside "do not invent parallel names ... this list IS the agreement". A repair
/// shard's vocabulary is the one on disk, and the finding names the ids it must touch; the
/// finding is the agreement. Same `repairing` predicate the notes above branch on. Moved here
/// from swarm.rs verbatim apart from that arm (the split law prices the wiring line).
pub(super) fn web_vocab_note(owned_files: &[String], enabled: bool, repairing: bool) -> String {
    let frontend_js = |f: &str| {
        (f.ends_with(".js") || f.ends_with(".mjs") || f.ends_with(".ts") || f.ends_with(".tsx"))
            && f.split('/')
                .any(|seg| matches!(seg, "web" | "static" | "public" | "frontend" | "assets"))
    };
    let owns_web = owned_files.iter().any(|f| {
        f.ends_with(".css") || f.ends_with(".html") || f.ends_with(".htm") || frontend_js(f)
    });
    if !enabled || repairing || !owns_web {
        return String::new();
    }
    // A CONCRETE, FROZEN ID LIST — not an instruction to agree. Telling three workers to "share
    // one vocabulary" is a hope, and the measured outcome is a lottery: one run drifted on 2 ids,
    // the next on SEVEN (`summary`, `filter-btn`, `filter-menu`, `table-body`, `pagination`,
    // `loader`, `error-banner` — every one referenced by app.js and defined by no html), which
    // made 7 of that run's 10 findings and left the page inoperable. The contracts phase freezes
    // Python signatures between modules for exactly this reason; the DOM is a cross-file
    // interface too, and it was the only one left unfrozen. These ids are fixed, spelled here,
    // and identical in every web worker's prompt, so agreement is structural rather than
    // negotiated.
    "\nWEB VOCABULARY — A FROZEN CONTRACT, NOT A SUGGESTION (you own a frontend file; this also \
     ADDS one permitted read to the rules below). The page's html, css and js MUST share ONE \
     vocabulary. Use EXACTLY these element ids, spelled exactly like this, for the parts the \
     spec describes — the html DEFINES them, the js looks them up, the css may style them:\n\
     - `app-root` the page container; `app-title` the heading\n\
     - `sync-button` the control that starts a sync; `last-sync` its status/timestamp readout\n\
     - `payments-table` the table; `payments-body` its <tbody> that rows are appended to\n\
     - `summary-total` the count/total readout; `status-filter` the status filter control\n\
     - `pagination` the pager container; `prev-page` and `next-page` its buttons; `page-info` its \
     \"showing X-Y of N\" readout\n\
     - `loading-state`, `empty-state`, `error-state` the three state containers\n\
     An id the js queries that the html never defines is a GUARANTEED null at runtime and a \
     BLOCKING finding — it is the single most common way this page ships broken. Do not invent \
     parallel names, do not rename these, and do not assume a sibling used something else: this \
     list IS the agreement. You may read the sibling web files to match their classes (class \
     names are yours to choose, but css selectors must match the markup that exists, or the page \
     ships unstyled and FAILS verification). State containers must be hidden by default and \
     toggled by the js — never all visible at once."
        .to_string()
}

/// GEN-5: the dispatch-time brief guard's char floor — a MEASURING instrument for the
/// "no one-line spec" checkpoint, which until now had no instrument at all. 240 is the
/// codebase's existing "substantive detail" bar (thin_integrate_verify_spec is >240 chars and
/// passes it), reused rather than invented. This bounds NOTHING: a brief below the floor
/// ships exactly as it is — the guard emits a warning event and may never stop, downgrade or
/// re-route a dispatch (MILD; a gate here would be a cap by another name).
const THIN_BRIEF_MIN_CHARS: usize = 240;

/// What a dispatched description is missing against the named-fact floor: enough chars, at
/// least one of the task's own owned files named (path or basename; skipped for a task owning
/// nothing — a verifier's brief has no file to name), and at least one concrete objective
/// token beyond the task's own title words (a path-, call- or identifier-shaped token — a
/// heuristic, acceptable for a warning that measures and never gates). Empty = floor met.
pub(super) fn thin_brief_missing(
    description: &str,
    owned_files: &[String],
    task_id: &str,
) -> Vec<&'static str> {
    let mut missing = Vec::new();
    if description.chars().count() < THIN_BRIEF_MIN_CHARS {
        missing.push("min_chars");
    }
    if !owned_files.is_empty() {
        let named = owned_files.iter().any(|f| {
            description.contains(f.as_str())
                || std::path::Path::new(f)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|b| description.contains(b))
        });
        if !named {
            missing.push("owned_file");
        }
    }
    let title_words: std::collections::HashSet<&str> = task_id.split(['-', '_']).collect();
    let concrete = description.split_whitespace().any(|raw| {
        let t = raw
            .trim_matches(|c: char| ",.;:!?\"'()".contains(c))
            .trim_matches('`');
        if t.chars().count() < 3 || title_words.contains(t) || t.eq_ignore_ascii_case(task_id) {
            return false;
        }
        t.contains('/')
            || t.contains('(')
            || t.contains('_')
            || t.contains("::")
            || t.split('.').filter(|p| !p.is_empty()).count() >= 2
    });
    if !concrete {
        missing.push("objective_fact");
    }
    missing
}

/// VA-102: the ONE owned file a worker writes FIRST, named in its brief so the first action is a
/// concrete `write` and not a design of every file. r6h (BUILD 05:13→06:00, three lanes, zero live
/// bytes): `ledgerd-core` reasoned 72k chars over "So let me write all 8 files. Plan:" with 0 files;
/// the shard `viz-engine-camera-labels-brush` drafted 46,410 of 95,233 reasoning chars INSIDE code
/// fences — full piece bodies it then had to retype — because its brief asked for every declared
/// name and never said which file comes first. A 27B cannot hold five files and then type them.
///
/// Derived from THIS task's facts, never a literal: the owned file the description names the
/// FEWEST times (a spec section that claims a file names it, so fewer mentions = the smaller part
/// of the task), ties to the plan's order. Intentional-empty markers (`__init__.py`, `py.typed`)
/// are skipped while any other file exists — an empty first write advances nothing. `None` only
/// for a worker that owns nothing (the sink, a read-only verify shard), which never reads the
/// write-first script.
pub(super) fn first_write_target<'a>(
    owned_files: &'a [String],
    description: &str,
) -> Option<&'a str> {
    let real: Vec<(usize, &'a String)> = owned_files
        .iter()
        .enumerate()
        .filter(|(_, f)| !super::judge_context::is_intentional_empty_marker(f))
        .collect();
    let pool: Vec<(usize, &'a String)> = if real.is_empty() {
        owned_files.iter().enumerate().collect()
    } else {
        real
    };
    pool.into_iter()
        .min_by_key(|(i, f)| (mentions(description, f), *i))
        .map(|(_, f)| f.as_str())
}

/// How many times the description names `file` — by basename, because plans and briefs drop the
/// directory (`thin_brief_missing` reads ownership the same way).
fn mentions(description: &str, file: &str) -> usize {
    let name = Path::new(file)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(file);
    description.matches(name).count()
}

/// The file author's WRITE FIRST script — moved here from swarm.rs's owner_body `else` arm (VA-102).
/// It opened "your VERY FIRST action must be to `write` your owned file(s) IN FULL": for an 8-file
/// owner that is an order to finish every file in reasoning before the first write, and r6h's lanes
/// obeyed it (72k–102k reasoning chars, 0 files, one `ls`). The first sentence now names ONE file
/// (`first_write_target`) and forbids drafting a body in reasoning; the reading prohibitions that
/// follow are the original's, unchanged.
pub(super) fn write_first_body(owned_files: &[String], description: &str) -> String {
    // Honest-empty: this is the FILE AUTHOR's script; a worker owning nothing never receives it
    // (its arm is chosen upstream on `owned_files.is_empty()`), so there is no file to name.
    let Some(target) = first_write_target(owned_files, description) else {
        return String::new();
    };
    let n = owned_files.len();
    let then_the_rest = if n > 1 {
        format!(
            " Then the next of your {n} owned files, one `write` each — never all {n} designed in \
             one stretch of reasoning and typed afterwards."
        )
    } else {
        String::new()
    };
    format!(
        "WRITE FIRST. Your FIRST action is ONE `write`: the first version of `{target}`, built from \
         the spec sections above that name it — its imports and every function/class they ask for, \
         bodies included where the spec settles them. Do NOT draft a file's body in your reasoning \
         and then retype it into the tool call: write it to the file, read the tool result back, and \
         only then think about the next piece; anything still open is settled by a later `edit` of \
         that file, not by reasoning longer before the first write.{then_the_rest} Do NOT \
         `ls`/`find`/`tree`/`cat` to 'understand the API', hunt for tests, or 'see the current state \
         of the project': the PROJECT FILE LAYOUT above IS the complete structure (there is nothing \
         on disk to discover), tests are a SEPARATE subtask, and the API of EVERY dependency you \
         import is ALREADY injected below under 'API of …' — read it THERE, NEVER `cat` the module. \
         Cat-ing files whose APIs are already injected only bloats your context until you LOOP — \
         repeating 'let me write the file' over and over without ever emitting the write. Implement \
         from the spec + injected APIs, THEN run `python3 -m pytest` to check — never piped through \
         `head`/`tail` (the pipe hides the real exit code), and `collected 0 items`/`no tests \
         ran`/`file or directory not found` in the output means the check DID NOT RUN, whatever the \
         exit code says. A turn that ends without every owned file written and non-empty FAILS and \
         is retried — exploring/cat-ing instead of writing is the #1 way workers burn their whole \
         budget and produce nothing.\n\n"
    )
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

    #[test]
    fn web_vocab_note_fires_for_frontend_owners_only() {
        // css/html owners always get the vocabulary obligation.
        assert!(web_vocab_note(&["web/styles.css".into()], true, false).contains("ONE vocabulary"));
        assert!(
            web_vocab_note(&["web/index.html".into()], true, false).contains("hidden by default")
        );
        // js only under a frontend-shaped path — a Node backend shares no styling vocabulary.
        assert!(web_vocab_note(&["server/api.js".into()], true, false).is_empty());
        assert!(web_vocab_note(&["web/app.js".into()], true, false).contains("ONE vocabulary"));
        // python-only tasks and the OFF gate stay byte-identical.
        assert!(web_vocab_note(&["pkg/store.py".into()], true, false).is_empty());
        assert!(web_vocab_note(&["web/styles.css".into()], false, false).is_empty());
        // r6c: a REPAIRING web/app.js or web/viz.js shard reads no frozen list — its finding names
        // the ids (viz-labels, role-token) and the list did not carry them.
        assert!(web_vocab_note(&["web/app.js".into()], true, true).is_empty());
        assert!(
            web_vocab_note(&["web/viz.js".into(), "web/index.html".into()], true, true).is_empty()
        );
    }

    /// r6c, complete-fix::web/app.js attempt 0: "The truncated view only shows up to
    /// _validate_create. I need to read the actual file" — the block had said "never `cat` it".
    /// Past the 12,000-char cap the text must say the file is truncated and permit the targeted
    /// read of the named region; under the cap the no-`cat` rule stands in both arms.
    #[test]
    fn a_truncated_inline_names_its_truncation_and_permits_the_targeted_read() {
        let dir =
            std::env::temp_dir().join(format!("goose-briefs-truncated-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("web")).unwrap();
        let mut big = String::new();
        for i in 0..400 {
            big.push_str(&format!("function helper{i}() {{ return {i}; }}\n"));
        }
        big.push_str("function _validate_create(body) { return body; }\n");
        assert!(big.chars().count() > 12000);
        std::fs::write(dir.join("web/app.js"), &big).unwrap();
        std::fs::write(dir.join("web/small.js"), "const x = 1;\n").unwrap();

        let repair = current_content_block(&dir, &["web/app.js".to_string()], true);
        assert!(repair.contains("TRUNCATED"), "{repair:.300}");
        assert!(
            repair.contains("`sed -n 'A,Bp'`") && repair.contains("`grep -n`"),
            "the targeted read the finding requires must be permitted: {repair:.400}"
        );
        assert!(
            !repair.contains("never `cat` it"),
            "a truncated file must not forbid the read the finding needs"
        );
        assert!(
            !repair.contains("_validate_create"),
            "the fixture's named symbol really is past the cap"
        );
        let small = current_content_block(&dir, &["web/small.js".to_string()], true);
        assert!(small.contains("inlined in full, so you never `cat` it"));
        assert!(!small.contains("TRUNCATED"));

        let amend_big = current_content_block(&dir, &["web/app.js".to_string()], false);
        assert!(
            amend_big.contains("`sed -n 'A,Bp'`") && !amend_big.contains("Do NOT `cat` it again")
        );
        let amend_small = current_content_block(&dir, &["web/small.js".to_string()], false);
        assert!(amend_small.contains("Do NOT `cat` it again"));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// B1 (r6c): four of eight findings were a bare bodyless `curl -X POST` answered by a JSON 401
    /// envelope — "a report of something that happened, not a hypothesis" overstated a probe
    /// artifact. The opener now says what is true: the check made a specific request and saw a
    /// specific response, both quoted; reproduce THAT request first. The NOT-REAL exit stays.
    #[test]
    fn the_repair_order_describes_the_probe_honestly() {
        let body = repair_owner_body(&["app/drafts.py".into()], false);
        assert!(!body.contains("not a hypothesis for you to re-derive"));
        assert!(body.contains("made a SPECIFIC request against the RUNNING app"));
        assert!(body.contains("Reproduce THAT request first"));
        assert!(body.contains("IF A FINDING IS NOT REAL, say so in ONE sentence"));
    }

    /// GEN-5: the brief guard is a MEASURING instrument for the "no one-line spec" checkpoint.
    /// It classifies; it may never stop, downgrade or re-route (the dispatcher only ever emits
    /// a `thin_brief` warning event from its result). Pinned here: a substantive brief clears
    /// the floor, the one-line spec misses all three named facts, and a task that owns nothing
    /// is not charged for naming no file.
    #[test]
    fn a_thin_brief_is_measured_never_stopped() {
        let rich = "Implement the ledger core: `app/ledger_core.py` must expose \
                    post_entry(db, amount_minor, currency) and rebuild_balances(db), persisting \
                    through sqlite3.Connection; amounts are integer cents (never floats); \
                    `python3 -m pytest tests/test_ledger_core.py` must pass. The API layer \
                    imports these two functions exactly as named — keep the signatures stable.";
        assert!(
            thin_brief_missing(rich, &["app/ledger_core.py".to_string()], "ledger-core").is_empty(),
            "a substantive brief clears the named-fact floor"
        );
        assert_eq!(
            thin_brief_missing(
                "Build the ledger",
                &["app/ledger_core.py".to_string()],
                "ledger-core"
            ),
            vec!["min_chars", "owned_file", "objective_fact"],
            "the one-line spec misses every named fact, and each miss is named"
        );
        assert!(
            thin_brief_missing(rich, &[], "integrate-verify").is_empty(),
            "a task owning nothing is not charged for naming no file"
        );
        // A basename mention counts as naming the owned file — plans often drop the directory.
        let by_basename = format!("{rich} Write ledger_core.py first.");
        assert!(
            !thin_brief_missing(&by_basename, &["app/ledger_core.py".to_string()], "core")
                .contains(&"owned_file")
        );
    }

    /// VA-102: the first write is ONE file, named — the least-claimed real owned file, the plan's
    /// order on ties; never `__init__.py` while another file exists (an empty first write advances
    /// nothing); `None` only when nothing is owned.
    #[test]
    fn first_write_target_is_the_least_claimed_real_owned_file() {
        let owned: Vec<String> = [
            "app/ledgerd/__init__.py",
            "app/ledgerd/server.py",
            "app/ledgerd/store.py",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let desc = "server.py serves /api/ledger and /api/stream; server.py owns the SSE loop; \
                    store.py holds the rows; server.py parses --port.";
        assert_eq!(
            first_write_target(&owned, desc),
            Some("app/ledgerd/store.py")
        );
        let one = vec!["web/viz.js".to_string()];
        assert_eq!(first_write_target(&one, "anything"), Some("web/viz.js"));
        assert_eq!(first_write_target(&[], "anything"), None);
        let ties: Vec<String> = ["a/x.py", "a/y.py"].iter().map(|s| s.to_string()).collect();
        assert_eq!(
            first_write_target(&ties, "neither named"),
            Some("a/x.py"),
            "ties go to the plan's order"
        );
        let only_marker = vec!["pkg/__init__.py".to_string()];
        assert_eq!(
            first_write_target(&only_marker, ""),
            Some("pkg/__init__.py"),
            "a lone marker is still the one file to write"
        );
    }

    /// r6h `ledgerd-core`: 8 owned files, 72k reasoning chars, 2 calls, 0 files, then "So let me
    /// write all 8 files. Plan:" — the script it read opened with "write your owned file(s) IN
    /// FULL". The script now opens with ONE named write, ahead of every reading prohibition, and
    /// the phrase that ordered all eight at once is gone.
    #[test]
    fn write_first_body_opens_with_one_named_write_and_never_says_in_full() {
        let owned: Vec<String> = [
            "app/ledgerd/__init__.py",
            "app/ledgerd/server.py",
            "app/ledgerd/store.py",
            "app/ledgerd/stream.py",
            "app/ledgerd/routes.py",
            "app/ledgerd/pagination.py",
            "app/ledgerd/errors.py",
            "app/ledgerd/config.py",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let desc = "server.py binds --port; routes.py maps /api/ledger; stream.py is the SSE \
                    loop; pagination.py cursors; config.py reads env; errors.py shapes 4xx; \
                    store.py holds rows; server.py imports routes.py and stream.py.";
        let body = write_first_body(&owned, desc);
        assert!(
            body.starts_with(
                "WRITE FIRST. Your FIRST action is ONE `write`: the first version of \
                 `app/ledgerd/store.py`"
            ),
            "{body}"
        );
        assert!(
            !body.contains("IN FULL"),
            "r6h: 'write your owned file(s) IN FULL' designed 8 files before the first write"
        );
        assert!(body.contains("Do NOT draft a file's body in your reasoning"));
        assert!(body.contains("the next of your 8 owned files, one `write` each"));
        assert!(
            body.find("first version of").unwrap() < body.find("Do NOT `ls`").unwrap(),
            "the named write precedes every reading prohibition"
        );
        assert!(
            write_first_body(&[], "x").is_empty(),
            "owns nothing: no file to name"
        );
        let single = write_first_body(&["web/viz.js".to_string()], "");
        assert!(single.contains("first version of `web/viz.js`"));
        assert!(!single.contains("owned files, one `write` each"));
    }
}
