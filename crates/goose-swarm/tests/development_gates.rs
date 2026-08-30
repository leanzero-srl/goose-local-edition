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
    // The invariant is POSITIONAL in the run loop: between receiving the replanner's answer and
    // handing anything to `splice_specs`, the batch passes the repair. A `find()` on the whole
    // file would be satisfied by the fn DEFINITION (which precedes everything) — the same weak
    // pattern this morning's skill-gate controls caught — so the assertion reads only the window
    // between the two anchors.
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
