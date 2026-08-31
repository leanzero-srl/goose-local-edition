//! The gates that refuse — ordered by the owner 2026-08-30 ("let's add gates that stop this madness
//! from ever unfolding"). Each test here is the mechanical half of a rule in AGENTS.md `## GATES` and
//! `.claude/rules/development-gates.md`. The rules exist because a compaction brings the trained urges
//! back — silent fallbacks, template task text, headless benchmark runs — and a rule that lives only
//! in a conversation does not survive one. These are ratchets and doc-gates in the `now_doc_recipe.rs`
//! pattern: they read files as strings and refuse regressions; they touch no engine code.

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

/// The run-path files for the fallback ratchet: the engine loop plus every goose-swarm source file.
/// Enumerated dynamically so a NEW file with a silent-empty fallback cannot enter unscanned.
fn run_path_files() -> Vec<(String, String)> {
    let root = repo_root();
    let mut files = vec![(
        "crates/goose-cli/src/commands/swarm.rs".to_string(),
        read("crates/goose-cli/src/commands/swarm.rs"),
    )];
    // The incremental-split law moves engine code into commands/swarm/<area>.rs siblings; those
    // modules are the SAME run path and must not leave the scanned set by moving.
    let split = root.join("crates/goose-cli/src/commands/swarm");
    let mut split_names: Vec<_> = std::fs::read_dir(&split)
        .unwrap_or_else(|e| panic!("{} unreadable: {e}", split.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".rs"))
        .collect();
    split_names.sort();
    for n in split_names {
        let rel = format!("crates/goose-cli/src/commands/swarm/{n}");
        let text = read(&rel);
        files.push((rel, text));
    }
    let src = root.join("crates/goose-swarm/src");
    let mut names: Vec<_> = std::fs::read_dir(&src)
        .unwrap_or_else(|e| panic!("{} unreadable: {e}", src.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".rs"))
        .collect();
    names.sort();
    for n in names {
        let rel = format!("crates/goose-swarm/src/{n}");
        let text = read(&rel);
        files.push((rel, text));
    }
    files
}

/// THE SPECIFICITY GATE, part (a). "Integrate every module and VERIFY" is the nine-week-banned
/// template that still shipped on 2026-08-30 because its arming predicate was wrong (GEN-1: it fires
/// on an empty ledger instead of an empty spec surface). Lines that REFUSE the phrase — `contains(`
/// guards, `replacen` correctors, comments — are the gate working and are not counted; what is
/// counted is lines that CARRY it as content. Baseline 1 since finding 1 (2026-08-30): the
/// template fns are `#[cfg(test)]`, their opener is assembled by concat so no live line carries
/// the phrase, and both plan-side consumers build from the spec's fact parsers
/// (plan_sink_description). The single surviving content line is the archived-r2 test fixture.
/// Historical note — baseline 2 = the one live emission GEN-1 removes
/// (integrate_verify_spec_inner) plus the r2-plan JSON fixture embedded in a test that documents the
/// r2 shape. When GEN-1 lands the live site goes, the count drops to 1, and this baseline TIGHTENS in
/// the same commit — it may only decrease, never grow.
const INTEGRATE_TEMPLATE_BASELINE: usize = 1;

/// A corrector's needle operand can sit on the line AFTER `.replacen(` (rustfmt breaks the call),
/// so the previous line vouches for it too.
fn carries_banned_template(prev: &str, line: &str) -> bool {
    let trimmed = line.trim_start();
    line.contains("Integrate every module and VERIFY")
        && !line.contains("contains(")
        && !line.contains("replacen")
        && !prev.contains("replacen")
        && !trimmed.starts_with("//")
}

#[test]
fn the_banned_integrate_template_only_shrinks() {
    let engine = read("crates/goose-cli/src/commands/swarm.rs");
    let lines: Vec<&str> = engine.lines().collect();
    let sites: Vec<String> = lines
        .iter()
        .enumerate()
        .filter(|(i, l)| carries_banned_template(if *i == 0 { "" } else { lines[i - 1] }, l))
        .map(|(i, l)| {
            format!(
                "  swarm.rs:{}: {}",
                i + 1,
                l.chars().take(90).collect::<String>()
            )
        })
        .collect();
    assert!(
        sites.len() <= INTEGRATE_TEMPLATE_BASELINE,
        "swarm.rs carries the banned 'Integrate every module and VERIFY' template on {} lines \
         (baseline {INTEGRATE_TEMPLATE_BASELINE}):\n{}\nThis text is banned from reaching a model — \
         the ban is nine weeks old. A dispatched description is assembled from THIS run's facts (spec \
         surface, ledger, fs_delta) or the absence is emitted as a named event; a template is never \
         the answer. See AGENTS.md GATES 2 and .claude/rules/development-gates.md.",
        sites.len(),
        sites.join("\n")
    );
    if sites.len() < INTEGRATE_TEMPLATE_BASELINE {
        eprintln!(
            "integrate-template count is {} < baseline {INTEGRATE_TEMPLATE_BASELINE}: tighten \
             INTEGRATE_TEMPLATE_BASELINE in the same commit so the ratchet holds the gain",
            sites.len()
        );
    }
}

/// THE INCREMENTAL-SPLIT RATCHET (Mihai, 2026-08-30 15:35: "please stop making swarm.rs so
/// fucking big... as we're making changes instead of adding to it let's add it to separate files").
/// swarm.rs is a module ROOT: new functionality goes in `commands/swarm/<area>.rs` siblings, and an
/// edit that must add wiring lines here extracts at least as many in the same commit. Baseline
/// 47,150 measured at the law's birth; it may only DECREASE. This is the refusing form — a charter
/// alone is advice, and advice does not survive a compaction.
/// Tightened to 46,936 at the law's first live test (d93d7ca77: the judge-context cluster moved
/// to commands/swarm/judge_context.rs, -274; judge-look attribution +13 and the forming-sidecar
/// mirror +46 rode the same payment).
/// Tightened to 46,925: the dead ask_max_q lever deleted (WIDTH batch, small 3).
/// Tightened to 46,540: the dead plan_confidence_breakdown cluster deleted (WIDTH batch, small 2).
/// Tightened to 46,406: the research terminal-row cluster moved to commands/swarm/research.rs
/// (-164), paying for the truthful look-1 steer wording (+~30) in the same commit.
/// Tightened to 46,348: the forming-sidecar cluster moved to commands/swarm/supervision.rs
/// (-224), paying for the r6 supervision-lane wiring (keyed judge/replan/review calls, the
/// digest supervision stamp, the judge-lane event fields) in the same commit.
/// Tightened to 46,330: the superseded-fold cluster moved to commands/swarm/supervision.rs
/// (-41), paying for the settled-decisions delivery into the replanner's prompt (r6 addendum).
/// Tightened to 45,990: the cross-task import scan/attribution cluster moved to
/// commands/swarm/imports.rs (-454) and the dead run_agent_timed delegate deleted, paying for
/// the pre-build smalls batch (plan-wide ownership publication, replan orientation + rationale,
/// six supervision-call keys) across its four commits.
/// Tightened to 45,820: two moves banked together — the transcript writers moved to
/// commands/swarm/transcripts.rs (-66, paying for the steer ISO stamps + measured closing), and
/// #F924's RecurrenceMeter cluster moved to commands/swarm/desk.rs (-133 net, paying for the
/// shadow judge desk's mod/use + spawn wiring); a +29 judge-prompt batch rode the same window.
/// Tightened to 45,772: the pytest-tail cluster moved to commands/swarm/pytest_tail.rs (-153
/// with its r2-shapes test), paying for the r5-assessment cluster-A wiring (delivery-defect
/// routing with `cross_task`, the spec_set_exceeded sidecar/roll-up/event) across its two
/// commits; web_refs.rs landed entirely module-side (0 root lines).
/// Tightened to 45,529: the endpoint-attribution cluster moved to commands/swarm/attribution.rs
/// (-255 with its three tests), paying for the r5 REPAIR-round-0 pair (the possessive-apostrophe
/// cut in `clean`, the console-finding `sources/0` attribution suffix) in the same commit.
/// Tightened to 45,453: `attribute_findings` moved to attribution.rs beside its cluster, paying
/// for the fix-1 ownership seam (`resolve_shard_ownership` zip into the complete-fix fan) and
/// the honest three-pair console exemplar in the same commit.
/// Tightened to 45,448: the multi-file note moved to commands/swarm/briefs.rs (softened for a
/// repair shard whose owned files all exist), paying for the II-8 mirror-read wiring
/// (`read_calls_capture` — a redispatched fix shard's fresh shadow skips `.swarm`), the
/// complete-fix promote-contract comment refresh and the `shard_owned_files` dead-code drop.
/// Tightened to 45,265: two moves banked together — `build_task_ledger_row` to
/// commands/swarm/transcripts.rs beside `read_calls_capture` (-157 with its doc), paying for
/// the calls-mirror threading through `write_task_ledger`/`render_completed_output_from_ledger`
/// (the second and last root-relative calls-read, 0dc8c297f's RESIDUAL, now routes through the
/// one mirror predicate); and `multifile_stub_note` to commands/swarm/briefs.rs with its test,
/// paying for its `repairing` disarm (a repair shard's live files must never be stub-written).
/// Tightened to 44,920: the contextual pitfall cluster (DOMAIN_PITFALLS, PITFALL_TRIGGERS,
/// relevant_pitfalls, pitfall_items, two tests) moved to commands/swarm/pitfalls.rs (-345, +2
/// wiring), paying for the two r5-measured lessons taught general there — the
/// referenced-but-never-defined identifier that severs a browser boot function, and the handler
/// whose response omits its route's documented fields.
/// Tightened to 44,901: the SKELETON-FIRST note moved to commands/swarm/briefs.rs beside its
/// sibling stub note and the entry-file rule unified into one `is_entry_file` (-19), paying for
/// the note's `repairing` disarm — the skeleton write-first order reached an entry-file repair
/// shard at both of its dispatches in the motivating run, beside the repair body's read-first
/// rule.
/// Tightened to 44,518: the fleet-ordering cluster (`configured_speed_weight`,
/// `fleet_slot_models`, `live_fleet_slots`, `one_lane_per_host`, `order_fleet_by_speed` and
/// their 8 tests) moved verbatim to commands/swarm/fleet_order.rs (-383), ahead of the r5
/// repair-node-selection fix that resolves speed weights per device instead of matching the
/// substring map against slot model ids.
/// Tightened to 44,511: the r5 fix itself — `measured_rate_for` and its test moved to
/// fleet_order.rs beside the new weight resolution (`config_speed_weights`,
/// `publish_fleet_speed_weights`, `rank_fix_target`), paying for the publish call, the
/// pool_resolved weight echo and the weight-primary re-rank wiring (-7 net).
/// Tightened to 43,982: the finding cluster (`FileGroup`, `normalize_rel_path`,
/// `extract_file_from_finding`, `finding_fingerprint`, `dedupe_findings_exact`,
/// `engine_critical`, `group_findings_by_file` and their 9 tests, -677) moved verbatim to
/// commands/swarm/findings.rs, paying for REPAIR priorities in the same commit: provenance
/// tags at every authoring push site, the severity sort, the wave's severest-first group
/// order, the shard fix-first note and the severity arrays on complete_verify / known_bugs /
/// complete_result.
/// Tightened to 43,933: `elide_middle` and its elision test moved verbatim to
/// commands/swarm/findings.rs beside its primary consumers (`finding_texts` /
/// `inconclusive_reasons`, whose 400-char head-cut defect motivated it), paying for the
/// green-round clear of known_active_bugs(+severities) and handle_repair's
/// PYTHONDONTWRITEBYTECODE guard.
/// Tightened to 43,715: the nudge-ladder cluster (`produced_since_look`, `nudge_arm`,
/// `calls_since_nudge`, `nudge_delivery`, `restream_seed` and their 5 tests, -280) moved to
/// commands/swarm/ladder.rs, paying for the r6a fix in the same commit: `nudge_delivery`'s
/// advancing HOLD (a restream may only take a stream that has stopped) and the restream seam's
/// ladder reset (a fresh attempt starts at nudge 0 — the ignored-steer memory no longer outlives
/// the stream it measured).
/// Tightened to 43,684: the research fan's terminal-row emission (`emit_research_outcome`, one
/// funnel for the two verbatim `research_unanswered` writers) and the panicked-lane row
/// (`fold_research_panic`) moved to commands/swarm/research.rs (-31), paying for the fold of
/// raised research questions into the owning slice's brief (`raised_questions_brief_block`) and
/// the per-question `research_raised_folded` event — r6b's 48 raised questions had reached no
/// builder and no tick.
/// Tightened to 43,444: the spec-orientation cluster (`SpecSection`, `spec_sections`,
/// `orientation_armed`, `spec_orientation`, `head_to_sentence_end`, `unclaimed_sections` and
/// their sb-7 test) moved to commands/swarm/orientation.rs (-257), and the research prompt test
/// moved with the prompt builders it exercises to research.rs, paying for the research fan's
/// grounding wiring in the same commit (r6c: the request file, the snowball block at dispatch,
/// `research_context`, `research_planned`, `phase: research`).
/// Tightened to 43,423: `supervision_reply` and its test absorbed into
/// commands/swarm/supervision.rs's `supervised_reply_text` (the one reply door, which also
/// strips/refuses the agent loop's turn-cap filler — r6a seq 58's fabricated DRIFTING), paying
/// for the omni-judge seam's filler classification wiring in the same commit.
#[test]
fn swarm_rs_line_count_only_decreases() {
    const SWARM_RS_LINE_BASELINE: usize = 43_423;
    let text = read("crates/goose-cli/src/commands/swarm.rs");
    let n = text.lines().count();
    assert!(
        n <= SWARM_RS_LINE_BASELINE,
        "swarm.rs grew to {n} lines (baseline {SWARM_RS_LINE_BASELINE}). New code goes in \
         commands/swarm/<area>.rs modules; if this change needs wiring lines here, extract a \
         cluster of at least equal size in the same commit, then tighten the baseline."
    );
    if n < SWARM_RS_LINE_BASELINE {
        eprintln!(
            "swarm.rs is {n} lines < baseline {SWARM_RS_LINE_BASELINE}: tighten \
             SWARM_RS_LINE_BASELINE in the same commit so the ratchet holds the gain"
        );
    }
}

/// THE SPECIFICITY GATE, part (b). "DO EVERYTHING" is the other named member of the generic-task
/// class the owner banned ("I will ask for the millionth time, the gazillionth time"). It is at zero
/// in the run path and stays there.
#[test]
fn do_everything_never_reaches_a_model() {
    for (rel, text) in run_path_files() {
        assert_eq!(
            text.matches("DO EVERYTHING").count(),
            0,
            "{rel} contains 'DO EVERYTHING' — generic task text of the banned class. Assemble the \
             description from this run's facts instead (AGENTS.md GATES 2)."
        );
    }
}

/// THE FALLBACK GATE. `unwrap_or_default()` in the run path is the signature of a silent
/// substitution: a failed read/parse/call impersonating legitimate emptiness. The GEN-6 sweep
/// (2026-08-30) ranked 10 of these that hide real failures — the worst turned a pillars serialize
/// failure into a green gate. Baseline 130 measured at HEAD bce2901d9 (swarm.rs 104, scheduler.rs 24,
/// dag.rs 1, patch.rs 1); TIGHTENED to 128 by GEN-6a, which converted the evidence-hiding sites
/// (scheduler replan laundering, the pillars serialize/panic pair, distill parse) into named
/// events. The survivors are the honest-empty class: json field reads and format-string absences
/// where empty genuinely means empty. The count may only DECREASE. If you legitimately need a new
/// one, prove the empty MEANS empty in a comment at the call site (honest-empty exemplar:
/// scheduler.rs hashes "ABSENT" distinctly instead of hashing nothing) and adjust the baseline in
/// the SAME commit — the diff then shows the proof next to the licence.
const UNWRAP_OR_DEFAULT_BASELINE: usize = 128;

#[test]
fn run_path_silent_empty_fallbacks_only_shrink() {
    let mut total = 0usize;
    let mut per_file = String::new();
    for (rel, text) in run_path_files() {
        let n = text.matches("unwrap_or_default()").count();
        if n > 0 {
            per_file.push_str(&format!("  {n:4}  {rel}\n"));
        }
        total += n;
    }
    assert!(
        total <= UNWRAP_OR_DEFAULT_BASELINE,
        "the run path carries {total} unwrap_or_default() calls (baseline \
         {UNWRAP_OR_DEFAULT_BASELINE}):\n{per_file}A missing input never silently substitutes \
         content: facts, or a loud NAMED absence-event that tick.py prints. Prove the empty means \
         empty in a comment and lower/adjust the baseline in the same commit, or emit the event and \
         degrade loudly instead. See AGENTS.md GATES 1 and .claude/rules/development-gates.md \
         (the ten evidence-hiders)."
    );
    if total < UNWRAP_OR_DEFAULT_BASELINE {
        eprintln!(
            "run-path unwrap_or_default() count is {total} < baseline {UNWRAP_OR_DEFAULT_BASELINE}: \
             tighten UNWRAP_OR_DEFAULT_BASELINE in the same commit so the ratchet holds the gain"
        );
    }
}

/// The doc half of the gates. AGENTS.md loads every session unconditionally (path-scoped rules do
/// not), so the GATES section living there is what survives a compaction. Its disappearance is a
/// build failure, exactly like now_doc_recipe's.
#[test]
fn agents_md_carries_the_gates_section() {
    let agents = read("AGENTS.md");
    assert!(
        agents.contains("## GATES — the rules that refuse"),
        "AGENTS.md lost its '## GATES — the rules that refuse' section. It is the post-compaction \
         carrier of the fallback/specificity/launch/reaping/no-time-input gates; restore it (the full \
         text is mirrored in .claude/rules/development-gates.md)."
    );
    let rules = read(".claude/rules/development-gates.md");
    for needle in [
        "THE FALLBACK GATE",
        "THE SPECIFICITY GATE",
        "THE BENCHMARK-LAUNCH GATE",
        "THE REAPING GATE",
        "What each gate cost",
    ] {
        assert!(
            rules.contains(needle),
            ".claude/rules/development-gates.md lost its '{needle}' section — the detail and the \
             rebuke table are what make the gates re-derivable after a compaction"
        );
    }
}

/// THE BENCHMARK-LAUNCH GATE's doc anchor. Every headless run of 2026-08-28 was void (no vendor, no
/// fixtures, literal placeholders, no scoring) — twice, the second time via a self-written harness.
/// The campaign skill's §4a is the procedure that prevents it; if the skill loses the rule, this
/// gate's only durable statement is gone. The skill lives outside the repo, so on a machine without
/// it (CI, another checkout) there is no fleet and no way to launch a run either — the test states
/// that and passes rather than failing on an absent file.
#[test]
fn campaign_skill_still_forbids_headless_launches() {
    let home = match std::env::var("HOME") {
        Ok(h) => h,
        Err(_) => {
            eprintln!("no HOME; skipping the campaign-skill doc gate (no skill, no fleet here)");
            return;
        }
    };
    let path = Path::new(&home).join(".claude/skills/goose-swarm-campaign/SKILL.md");
    let Ok(skill) = std::fs::read_to_string(&path) else {
        eprintln!(
            "{} absent; skipping the campaign-skill doc gate (no skill, no fleet on this machine)",
            path.display()
        );
        return;
    };
    assert!(
        skill.contains("START RUNS FROM THE BENCHMARK VIEW. NEVER BY TYPING THE SPEC INTO A CHAT."),
        "the campaign skill lost §4a's never-headless rule — every headless run of 2026-08-28 was \
         void, and this sentence is the durable statement of why. Restore it verbatim."
    );
    assert!(
        skill.contains("bench_dispatch.mjs"),
        "the campaign skill no longer names bench_dispatch.mjs — the ONLY sanctioned way to start a \
         benchmark run (open -n with CDP, then bench_dispatch). Restore the procedure."
    );
}

/// GATE 6 — THE ONE-DOOR GATE (Mihai 2026-08-30, minutes after the r4 kill: "add it to our gates
/// to avoid in the future - make it a practice"). r4's replanner spliced five tasks into the live
/// DAG past every plan repair; one re-created the module/package shadow the repair had just fixed,
/// and the pinned sink shipped owning README.md. Every DAG entry walks through the same repairs:
/// the scheduler's splice site must sanitize through `repair_replan_specs` before `splice_specs`,
/// the sink-file strip must stay in the plan-repair chain, and both agentic docs must carry the
/// gate so a compaction cannot lose it.
#[test]
fn every_dag_entry_walks_through_the_same_repairs() {
    let sched = read("crates/goose-swarm/src/scheduler.rs");
    // GATE-AUDITOR STRENGTHENING (2026-08-30): the old form windowed ONE known door and was a
    // lock, not a tripwire — a third `.splice_specs(` call inserted after the guarded window
    // would have passed unguarded (the r4 lesson applied to the gate's own test). Now every
    // splice call site is ENUMERATED (a call-site ratchet, same species as the
    // unwrap_or_default ratchet) and each must show its own repair discipline inside its own
    // function body. A new door fails the build until it names its repair and joins the list.
    let sites: Vec<usize> = sched
        .match_indices(".splice_specs(")
        .map(|(i, _)| i)
        .collect();
    assert_eq!(
        sites.len(),
        2,
        "scheduler.rs has {} `.splice_specs(` call sites; the known-door list has 2 (apply_split's \
         partition door and the replan door). A NEW door must carry its ownership repair and be \
         added here with its guard assert — never spliced past the repairs (gate 6, the r4 class).",
        sites.len()
    );
    // Door 1 — the replan path: repair_replan_specs stands between the replanner's answer and
    // the splice. Windowed to dodge the fn-definition trap.
    let answer_at = sched
        .find(".replan(ctx).await")
        .expect("the replanner call site exists");
    let splice_at = sched[answer_at..]
        .find(".splice_specs(")
        .map(|i| answer_at + i)
        .expect("the replan splice site exists after the replanner call");
    assert!(
        sched[answer_at..splice_at].contains("repair_replan_specs("),
        "repair_replan_specs must stand between the replanner's answer and splice_specs — \
         a batch that reaches the DAG unrepaired is the r4 shadow-reintroduction class"
    );
    // Door 2 — apply_split's partition validation IS its ownership repair (an exact partition
    // of the parent's already-repaired claim cannot create a second claimant or a new path):
    // its two load-bearing refusals must stand between the fn definition and its splice.
    let split_fn = sched.find("fn apply_split(").expect("apply_split exists");
    let split_splice = sched[split_fn..]
        .find(".splice_specs(")
        .map(|i| split_fn + i)
        .expect("apply_split's splice site exists inside the fn");
    let split_body = &sched[split_fn..split_splice];
    assert!(
        split_body.contains("!orig_files.contains(f)")
            && split_body.contains("union != orig_files"),
        "apply_split's partition refusals (foreign-file and non-exact-cover) must guard its \
         splice — they are this door's equivalent of repair_replan_specs"
    );
    let engine = read("crates/goose-cli/src/commands/swarm.rs");
    assert!(
        engine.contains("repair_sink_files(plan, &mut actions);"),
        "repair_sink_files left the plan-repair chain — the join owning a file is the \
         cascaded-Failed, app-never-binds-a-port class (r4 shipped it owning README.md)"
    );
    for (doc, needle) in [
        ("AGENTS.md", "ONE-DOOR GATE"),
        (".claude/rules/development-gates.md", "THE ONE-DOOR GATE"),
    ] {
        assert!(
            read(doc).contains(needle),
            "{doc} lost the ONE-DOOR gate — the practice Mihai ordered kept"
        );
    }
}

/// GATE 7 — READ THE WORDS (Mihai 2026-08-30, twice in ten minutes: "read the WORDS not the
/// fucking shape... to come up with ACTUAL improvements"). The words decide; shapes corroborate.
/// This asserts the practice cannot be compaction-lost from the docs that arm every session.
#[test]
fn the_read_the_words_gate_is_carried() {
    for (doc, needle) in [
        ("AGENTS.md", "READ-THE-WORDS GATE"),
        (
            ".claude/rules/development-gates.md",
            "THE READ-THE-WORDS GATE",
        ),
        ("AGENTS.md", "THE TRACE GATE"),
        (".claude/rules/development-gates.md", "THE TRACE GATE"),
        (".claude/rules/development-gates.md", "TRACE VERDICT"),
    ] {
        assert!(
            read(doc).contains(needle),
            "{doc} lost the READ-THE-WORDS gate"
        );
    }
    let home = std::env::var("HOME").expect("HOME set");
    let skill = std::fs::read_to_string(
        std::path::Path::new(&home).join(".agents/skills/goose-swarm-campaign/SKILL.md"),
    )
    .expect("campaign skill readable");
    assert!(
        skill.contains("READ THE WORDS FIRST") && skill.contains("tail -c 4000"),
        "the campaign skill lost the words-first checkpoint procedure"
    );
}
