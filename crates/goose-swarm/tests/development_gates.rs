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
/// Tightened to 43,208: the resume cluster (ResumeState, resume_state_from_dir/_from_log, four
/// tests) moved to commands/swarm/plan_store.rs, paying for the plan-sidecar wiring
/// (`.swarm/plan.json` at the plan_synthesized seam, `.swarm/plan-loaded.json` at plan_loaded —
/// r6c's 133k-char briefs were persisted nowhere the vigil could read during REVIEW).
/// Tightened to 43,085: `review_dedupe_key` and its six tests moved to
/// commands/swarm/review_merge.rs with the per-lane patch union (`union_lane_patches`, extracted
/// from `review_plan_fanned`), paying for the r6c prose-rewrite wiring in
/// `repair_module_package_collisions` and the `plan_patched.lanes` provenance in the same commit.
/// Tightened to 42,764: the walking-skeleton cluster (SKELETON_ID, skeleton_invocation_files,
/// skeleton_description, prepend_skeleton_task, three tests) moved to commands/swarm/skeleton.rs,
/// paying for `refresh_skeleton_description`'s wiring in the repair chain (r6c: the skeleton's
/// PLANNED MODULES block baked pre-repair paths and the live lane re-derived ownership from the
/// engine-authored contradiction).
/// Tightened to 42,747: `is_agent_loop_filler` and its test moved to commands/swarm/supervision.rs
/// beside its one non-root caller (`supervised_reply_text`), paying for the wrong-channel wiring
/// at the nudge-delivery seam (r6c: web-console poured 70,600 chars of its owned files' CSS/HTML
/// into CHAT TEXT with 0 owned files on disk, and the chars-based `advancing` hold shielded it
/// from the restream rung — `ladder::wrong_channel_stall`).
/// Tightened to 42,741: `ledger_task_states` moved to commands/swarm/imports.rs beside the
/// attribution vocabulary it feeds (`task_state_label`), paying for the lane-defect-view wiring
/// at the judge seam (r6c seq 1954: the disk-measured defect list ordered web-console to fix
/// web-viz's web/viz.js "at that exact path" — `judge_context::lane_defect_view` now reshapes a
/// sibling-owned dangling ref into a do-not-write line naming the owner and its measured state).
/// Raised to 42,746 (brief-authorized, r6e smalls): +5 lines of event/prompt fidelity in place —
/// `wrong_channel` on `judge_restream_held`, OK-verdict ESTABLISHED, ETA own-line placement.
/// Tightened to 42,692 (r6e lane-view extension): `emit_delivery_defects` routes through
/// `lane_defect_view` (+15 wired), paid for by moving `owned_files_from_run_log` + its test to
/// judge_context.rs (-69) per the split law.
/// Tightened to 42,640 (r6c aux live-load routing): `reconcile_pool_with_fleet` moved to
/// commands/swarm/fleet_order.rs (-111, its planner pick now calling the named
/// `planner_grade`/`planner_rank` there), paying for the mid-run aux router wiring in the same
/// commit — the `InflightGuard` door count in `run_agent_in_inner`, `aux_model_for_call`, the
/// omni-look and replanner reroutes, and the `aux_routed` event.
/// Tightened to 42,635 (r6c steer-delivery write-progress arms): the inline drift-streak rule
/// and its test moved to commands/swarm/ladder.rs as `drift_streak_step` (with `write_progress`
/// beside `wrong_channel_stall`), paying for the owned-bytes baselines and the write-progress
/// wiring at the omni seam in the same commit.
/// Tightened to 42,551 (r6c judge-input fidelity): `tail_shingle_set`/`tails_recur` and their
/// shifting-loop test moved to commands/swarm/ladder.rs, paying for the escalation clause's
/// write-progress facts (`ladder::escalation_moved` — the judge is told "read-only — no owned
/// bytes written" instead of a raw action count) and the aux read path's poison recovery.
/// Tightened to 42,548 (r6c promised-delivery): the steer's SUPERVISOR NOTE format moved to
/// commands/swarm/ladder.rs as `steer_note`, paying for the `delivery_promise_due` wiring at
/// the omni seam (a drift hold's promise on a zero-action files-owing lane now delivers by
/// seeded restream instead of deferring behind think-advance forever).
/// Tightened to 42,202 (r6c durable-clamp): the judge-reply parse cluster
/// (`parse_judge_reply`/`parse_judge_eta_mins`/`omni_judge_says_looping`) and its four tests
/// moved to commands/swarm/supervision.rs beside `supervised_reply_text` (the reply door that
/// must run before the parse), paying for the verdict-site durable-transcript clamp
/// (`ladder::durable_clamped_produced` — a look's produced claim the durable think.log does
/// not back reads as zero, so a stream dead across a whole look cycle can no longer hold a
/// drift verdict on "fresh content" that was backlog draining through the meter).
/// Tightened to 41,872 (r6c repair brief): the THREE colliding repair-prompt blocks — the repair
/// owner body, the current-content note and the fix directive's ownership bullet — moved to
/// commands/swarm/briefs.rs as `repair_owner_body`/`current_content_block`/`fix_directive`, beside
/// the asset-owner and write-granularity classifiers that now serve both the dispatch and the
/// `rules_delivered` labels. The move paid for the repair arms of `stopping_rules` and
/// `rules_sections` (a repair shard no longer reads "report DONE immediately", "STOP WHEN GREEN"
/// and "you may edit ANY file" against its own order).
/// Tightened to 40,765 (r6e header-named shape column): the endpoint-table parser
/// (`SpecSurface`/`spec_surface_rows`/`heading_service_name`/`spec_advertised_surface`/
/// `spec_post_endpoints`) and its two table tests moved to commands/swarm/spec_surface.rs,
/// paying for the header-named expected-shape column (r6c's briefs read §5's ROLE column as the
/// response shape: "POST /api/drafts -> EXPECT maker or checker").
/// Tightened to 40,716 (r6e decision contract): `OpenSlice`/`OpenOutput`/`open_schema` moved to
/// commands/swarm/opener.rs beside the new parse-time decision gate (`OpenOutputRaw::qualify`),
/// paying for the opener prompt's open-decision contract (question + two options + citation) and
/// the qualified parse at the OPEN call site.
/// Tightened to 40,691 (r6e research-lane judge clause): `parse_json_lenient` /
/// `extract_first_json_object` moved to commands/swarm/lenient_json.rs, paying for the judge
/// contract's research-lane clause (NEXT names where to look or "emit what you have", never the
/// answer — r6d's judge-research-ledger-core-q2 dictated the mini's content at look 1 and its
/// opposite at look 4).
/// Tightened to 39,696 (the fan cut, C1): `briefs_from_slices` and its four tests moved to
/// commands/swarm/research.rs beside the rows it partitions (-312 net), paying for the opener's
/// question contract at the OPEN call (request file + SOURCES before the opener runs, the
/// kind/cite/fact paragraph) and the fan's cited-fact arm — r6d's 13 spec lookups, 201 lane-min.
/// Held at 39,696 (the fan cut, C2): `files_from_objective` moved to research.rs beside its one
/// caller (-42), paying for the decision routing call and the covered-mini check at the lane.
/// Tightened to 39,588 (the fan cut, C3): `fanout_over_fleet` moved to commands/swarm/
/// fleet_order.rs (-116) and the terminal-fold test to research.rs beside its fold, paying for
/// the one-lane-per-slice fan (batches, per-question rows, strays named) and E7's second drain
/// site inside the judge-probe select! (the loop-top drain was unreachable during a look).
/// Tightened to 39,583: the dead `max_research_questions` lever retired (its default fn, the
/// CLI printer/menu/editor arms and the live levers echo deleted; the field survives as
/// `Option` for the config round-trip and is echoed under `retired_levers`).
/// Tightened to 39,482 (VA-023 D0): `decomposition_of` and its two standalone tests moved to
/// commands/swarm/plan_shape.rs, paying for the skeleton verdict rows flattened onto the sink in
/// `finalize_plan_before_dag` (first-class `skeleton_dep_kept`/`skeleton_dep_relaxed` events) and
/// the audit test's assertion flip (`web` owns `web/app.js` only and no longer waits on the skeleton).
/// Tightened to 38,328 (VA-014, gate 9): the LLM REVIEW round deleted — `review_once`, the
/// dispatcher's `review_plan`/`review_plan_fanned`/`review_plan_part`, `review_user_message`,
/// `review_must_fix_block`, `review_patch_schema`, `cut_request_into_sections`/`_portions`,
/// `slugify_slice_id`, the `review_fan` seam parameter and eleven tests; commands/swarm/review_merge.rs
/// (the per-lane patch union) deleted with it. Measured: zero effective patches in three runs.
/// Tightened to 38,043 (VA-014 D1, second half): the M5 completion-time pre-review deleted —
/// `PreReviewer::pre_review`, `read_prereview_findings` and its sink injection, the `.swarm/prereview`
/// channel, `prereview_dim`, the GOOSE_SWARM_PREREVIEW gate (the trait's other idle jobs attach
/// unconditionally behind their own gates); scheduler.rs lost `pick_prereview_request`, the M5 loop
/// arm and `pre_reviewed`; event.rs lost `PreReview`/`PreReviewFailed`. Zero `pre_review` events in
/// r5/r6c/r6d — off in every measured run.
/// Tightened to 37,822 (VA-015, gate 9): the dynamic replanner deleted — `impl Replanner for
/// GooseAgentDispatcher` (the replan prompt, orientation and lane), `replan_schema`, the
/// `dynamic_replan`/`max_replans` defaults, CLI flag, printer, menu and editor arms and the levers echo
/// (the fields survive as `Option` under `retired_levers`); goose-swarm lost replan.rs, the scheduler's
/// summon/splice/repair path, `Replanned` and their tests.
/// Tightened to 37,819 (VA-030 / D10): the worker-channel copy of the research-settled decisions
/// (`research_settled_worker_block` + `settled_decisions`) deleted — the brief carries them once, per
/// slice and whole; `spec_documented_keys` gained its prose-shape consumer and the research prompt the
/// plan-wide `consumed_spec_sections` inputs.
/// Tightened to 37,655 (VA-016, gate 9): LEARN & REFLECT deleted — the persona read half
/// (`persona_loaded`), the structural snapshot and the post-verdict write half (`reflect_on_success`,
/// `persona_learned`), the `reflect` lane and the levers echo; commands/swarm/persona.rs (1,224 lines)
/// deleted with it. `lessons: 0` on both runs that wrote a skill; `persona` survives as a retired field.
/// Tightened to 37,649 (VA-013/VA-019 D4): the ASK-floor heuristics (`model_active_params_b`,
/// `ask_floor_weak_bump`, their test) moved to commands/swarm/ask_floor.rs, paying for the judge's
/// evidence-only summon on build lanes (`ladder::judge_summon_trigger`, the forming-stall sampler) and
/// the `node` / `secs` / `forming_bytes` / `trigger` fields on the look events.
/// Tightened to 37,561 (VA-009 D5): `repair_sink_files` and its test moved to
/// commands/swarm/plan_repairs.rs beside the new rule (e) `repair_brief_file_mentions`, paying for the
/// rule's wiring in `repair_plan_flags` / `PlanRepairs.mentions` / the finalize seam's
/// `brief_names_unowned_file` fan-out.
/// Tightened to 37,549 (VA-008 adjunct D6): `snapshot_tree_files` moved to commands/swarm/tree.rs,
/// paying for the worker prompt's REQUEST_FILE line (the full request on disk, named once, absolute;
/// `request_file_absent_at_dispatch` when it is not there).
/// Tightened to 37,514 (VA-027 D7): the two inline rsync argument lists (best-tree snapshot and
/// restore) collapsed into `tree::rsync_app_tree` over ONE exclusion list, paying for the write-once
/// `.swarm/prefix-tree` snapshot at the INTEGRATE -> REPAIR handover (`prefix_tree_snapshot{ok, files}`).
/// Tightened to 37,503 (VA-030 D10-5/6/7): `content_hash` and its test moved to commands/swarm/tree.rs,
/// paying for `write_ledger_mini_checked` (the research fan's four writers emit
/// `research_mini_write_failed` through `persist_research_row`) and the cover site's sink argument.
/// Tightened to 37,501 (VA-034 D10-8): the opener's QUESTIONS rule moved out of the system prompt's head
/// into `opener::opener_questions_rule`, rendered right after the SOURCES block; the schema requires a
/// cite on every kind.
#[test]
fn swarm_rs_line_count_only_decreases() {
    // Tightened to 37,117 (VA-030 D11a): the ledger block renderer moved to
    // `commands/swarm/ledger_block.rs`, paying for the shard decisions channel's wiring.
    // Tightened to 37,068 (D11b): the replanner's bonus-task green rule and report split deleted.
    const SWARM_RS_LINE_BASELINE: usize = 37_068;
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
/// events; to 127 on 2026-09-01 when the retired `split_inherit_spec` echo row took its env-read
/// `unwrap_or_default()` with it. The survivors are the honest-empty class: json field reads and format-string absences
/// where empty genuinely means empty. The count may only DECREASE. If you legitimately need a new
/// one, prove the empty MEANS empty in a comment at the call site (honest-empty exemplar:
/// scheduler.rs hashes "ABSENT" distinctly instead of hashing nothing) and adjust the baseline in
/// the SAME commit — the diff then shows the proof next to the licence.
const UNWRAP_OR_DEFAULT_BASELINE: usize = 127;

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
/// and the pinned sink shipped owning README.md. Every DAG entry walks through the same repairs.
/// VA-015 (2026-09-01, gate 9) DELETED the dynamic replanner — the door r4 came through no longer
/// exists — so this test now refuses its RETURN (no `Replanner` attach, no `.replan(` call, no
/// `repair_replan_specs` in scheduler.rs) and enumerates the one splice site left (apply_split's
/// partition door, whose validation IS its ownership repair). The sink-file strip must stay in the
/// plan-repair chain, and both agentic docs must carry the gate so a compaction cannot lose it.
#[test]
fn every_dag_entry_walks_through_the_same_repairs() {
    let sched = read("crates/goose-swarm/src/scheduler.rs");
    for gone in [
        "with_replanner",
        ".replan(",
        "repair_replan_specs",
        "Replanner",
    ] {
        assert!(
            !sched.contains(gone),
            "scheduler.rs contains `{gone}` — the dynamic replanner was deleted (VA-015: r6c's \
             replan-r0 ran 208 unsupervised minutes for two bonus tasks nothing imported; r5's held \
             two READY tasks 19 minutes). A mid-run task-adding path returns only with the \
             measurement gate 9 demands, and then it walks the same ownership repairs as every \
             other door and joins the enumerated list below."
        );
    }
    // GATE-AUDITOR STRENGTHENING (2026-08-30): the old form windowed ONE known door and was a
    // lock, not a tripwire — a splice call inserted after the guarded window would have passed
    // unguarded (the r4 lesson applied to the gate's own test). Every splice call site is
    // ENUMERATED (a call-site ratchet, same species as the unwrap_or_default ratchet) and each
    // must show its own repair discipline inside its own function body. A new door fails the
    // build until it names its repair and joins the list.
    let sites: Vec<usize> = sched
        .match_indices(".splice_specs(")
        .map(|(i, _)| i)
        .collect();
    assert_eq!(
        sites.len(),
        1,
        "scheduler.rs has {} `.splice_specs(` call sites; the known-door list has 1 (apply_split's \
         partition door). A NEW door must carry its ownership repair and be added here with its \
         guard assert — never spliced past the repairs (gate 6, the r4 class).",
        sites.len()
    );
    // The one door — apply_split's partition validation IS its ownership repair (an exact partition
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
         splice — they are this door's ownership repair"
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

/// GATE 9 — THE VALUE GATE (Mihai 2026-09-01, three hours into r6d's 4-hour research fan under four
/// ticks that said `continue`: "Why would a phase that takes 4 hours and doesn't bring value continue?
/// This is the question." / "we don't want steps that consume time and not a lot of value. Get that
/// straight"). A step exists only while its measured delivery is consumed downstream; the vigil grades
/// the CURRENT phase every tick and files ACTIONS into the queue surgeons are dispatched from. This is
/// the tripwire: it pins the practice in the docs and instruments that arm every session. The reader
/// is the gate.
#[test]
fn the_value_gate_is_carried() {
    for (doc, needle) in [
        ("AGENTS.md", "THE VALUE GATE"),
        (".claude/rules/development-gates.md", "THE VALUE GATE"),
        (".claude/agents/tick-surgeon.md", "PHASE VALUE"),
        (".claude/agents/tick-surgeon.md", "note.sh action"),
        (
            "VIGIL-ACTIONS.md",
            "| id | filed | surface | status | action |",
        ),
        ("CLAUDE.md", "VIGIL-ACTIONS.md"),
    ] {
        assert!(
            read(doc).contains(needle),
            "{doc} lost the VALUE gate ({needle})"
        );
    }
    let home = std::env::var("HOME").expect("HOME set");
    let loop_state = std::path::Path::new(&home).join("goose-builds/loop-state");
    let note = std::fs::read_to_string(loop_state.join("note.sh")).expect("note.sh readable");
    assert!(
        note.contains("VIGIL-ACTIONS.md") && note.contains("\"action\""),
        "note.sh lost the `action` kind that feeds the surgeons' queue"
    );
    let tick = std::fs::read_to_string(loop_state.join("tick.py")).expect("tick.py readable");
    assert!(
        tick.contains("PHASE VALUE research") && tick.contains("PHASE VALUE build"),
        "tick.py lost the PHASE VALUE cost rows"
    );
}
