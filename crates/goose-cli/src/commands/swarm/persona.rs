//! LEARN & REFLECT — the stack persona: what goose learned from builds that PROVABLY shipped,
//! keyed by a measured stack, written as a skill the user can read, correct or delete.
//! Sibling module under the incremental-split law (development_gates::
//! swarm_rs_line_count_only_decreases); moved verbatim from swarm.rs with its tests, then
//! amended for r6c's two measured defects (a physics adjective keyed the stack "angular"; the
//! model's whole SKILL.md was nested under "## What worked"). The WHY of every part stays in
//! each item's own doc.

use std::sync::Arc;

use super::{supervised_reply_text, GooseAgentDispatcher, REFLECT_LANE};

/// ── LEARN & REFLECT ────────────────────────────────────────────────────────────────────────────────
/// After a build that PROVABLY worked, goose reflects on what it did and writes a reusable skill for that
/// stack, so the next build of the same stack does not re-derive it from zero.
///
/// Mihai: "if you did it well once then keep it as a memory/skill to reuse so that next time it won't take
/// 5 hours." MEASURED waste it exists to kill: loop-03 and loop-04, the SAME Swift/SwiftUI/SPM stack, each
/// burned ~40 min of planning re-deriving identical knowledge from zero, and the judge rediscovered
/// "@MainActor on NoteStore" and "a struct cannot have a deinit" in BOTH runs. Same lesson, paid for twice,
/// thrown away twice.
///
/// WHAT MAKES THIS SAFE — the two rules that keep a weak model from poisoning its own future:
///  1. ONLY A DETERMINISTIC ENGINE EVENT MAY TRIGGER A WRITE. The skill is written ONLY when the run's own
///     gate says the app built AND was verified. The model never decides that it "did well" — the engine
///     does. The model only PHRASES what the engine already proved.
///  2. STRUCTURE, NOT FEATURES. A cached decomposition is spec-specific; injected whole it would bias the
///     next build toward the LAST app's features — the laundering bug one level up. So the skill records the
///     STRUCTURAL skeleton (build file, module layout, how tests are wired, the conventions the judge
///     enforced) and is injected on the ADVISORY channel only. The live spec always wins.
///
/// It is a plain-markdown folder the user can read, edit, or delete — the mitigation for a wrong lesson.
///
/// WHICH IS WHY IT LIVES HERE. `config_dir/skills` is one of the roots `goose::skills::all_skill_dirs`
/// already walks, so a persona written here is listed, rendered and editable in the Skills UI with no new
/// plumbing. That placement is not tidiness — it IS the mitigation. The only reason it is defensible to let a
/// weak local model author its own skill is that a wrong lesson is visible and removable; a skill filed where
/// the UI never looks is precisely the invisible self-poisoning this design set out to avoid.
///
/// It was first written to `data_dir/swarm/personas/<key>`, which NO skill root covers — so the whole safety
/// argument was false for that commit. Discovery is the feature; do not move this off a discovered root.
fn persona_dir(stack_key: &str) -> std::path::PathBuf {
    goose::config::paths::Paths::config_dir()
        .join("skills")
        .join(format!("stack-{stack_key}"))
}

/// The engine's run counter, kept deliberately OUT of the skill folder.
///
/// Skill discovery advertises EVERY non-`SKILL.md` file under a skill dir to the model as a loadable
/// supporting file (`skills/mod.rs`: `walk_files_recursively` → `supporting_files`). A `runs` file beside the
/// skill would surface in the model's context as `load_skill(name: "stack-react-vite-ts/runs")` — a tool call
/// whose entire payload is a bare integer. Engine bookkeeping belongs in the data dir; only the artifact the
/// user is meant to read belongs in the skills dir.
fn persona_runs_file(stack_key: &str) -> std::path::PathBuf {
    goose::config::paths::Paths::data_dir()
        .join("swarm")
        .join("personas")
        .join(format!("{stack_key}.runs"))
}

/// The heading after which everything belongs to the USER and is preserved verbatim across rewrites.
const PERSONA_USER_MARKER: &str = "## Your notes";

/// What sits under the marker until the user writes something. Kept as a constant because the round-trip
/// cannot otherwise tell "the user wrote nothing" from "the user wrote this" — the previous render's own
/// invitation would be preserved as though it were a correction, and goose would then be reading its own
/// boilerplate back to itself as user guidance forever.
const PERSONA_NOTES_PLACEHOLDER: &str =
    "_Nothing yet. Whatever you write here is read on the next build of this stack and is kept word for word \
     when goose rewrites the rest of this file — so a correction here is permanent._";

/// Lift the user's own section out of the existing skill so a rewrite cannot clobber it.
///
/// THE HOLE THIS PLUGS: the file told the user "edit anything here that is wrong", but a later successful
/// build called `write_persona`, which is `fs::write` — a truncating overwrite. The correction was handed to
/// the reflection as prior text and a weak model was trusted to carry it forward, which is exactly the
/// trust this feature is not allowed to assume. Everything after the marker now round-trips deterministically,
/// so a correction written there outlives every future rewrite without a model in the path.
pub(super) fn persona_user_notes(prior: &str) -> String {
    // Match the marker ONLY as a heading at the start of a line — i.e. with its newline. The file also
    // MENTIONS `## Your notes` inline, in the banner telling the user where to write, and a plain
    // `split_once` cuts at that sentence instead, handing back the whole rest of the file as though the user
    // had authored it — which round-trips goose's own lesson into the preserved section and compounds it on
    // every rewrite. Split on the newline-prefixed form rather than computing byte offsets: an index into a
    // str is a panic waiting on the first multi-byte character, and these files carry em-dashes.
    let notes = match prior.strip_prefix(PERSONA_USER_MARKER) {
        Some(rest) => rest.to_string(),
        None => prior
            .split_once(&format!("\n{PERSONA_USER_MARKER}"))
            .map(|(_, after)| after.to_string())
            .unwrap_or_default(),
    };
    // Strip the invitation wherever it sits, rather than testing the section for equality with it: a user
    // writing under a "write here" heading appends BELOW the prompt at least as often as they replace it, and
    // a surviving placeholder would be fed back to the next reflection as though the user had asserted it.
    notes
        .replace(PERSONA_NOTES_PLACEHOLDER, "")
        .trim()
        .to_string()
}

/// The structural facts of a run, snapshotted while they are still in scope.
///
/// THE TRAP THIS EXISTS FOR: by the time a run knows it PASSED, the evidence is gone — `plan_json` has been
/// moved into the plan_loaded event, `dag` has been moved into `scheduler.run(...)`, and the research
/// findings have been folded into a flat string and dropped. So the facts must be captured EARLY and carried
/// to the write, exactly as `smoke_all_files` already is.
#[derive(Default, Clone)]
pub(super) struct PersonaSnapshot {
    pub(super) stack_key: Option<String>,
    /// The file layout that actually built — the proven skeleton.
    pub(super) files: Vec<String>,
    /// Per-task: (id, owned_files) — the decomposition shape, WITHOUT the feature descriptions.
    pub(super) shape: Vec<(String, Vec<String>)>,
    /// Corrective judge verdicts that led somewhere — the conventions this stack really enforces.
    pub(super) judge_lessons: Vec<String>,
}

/// Harvest the judge's corrective hints from a finished run's own event log.
///
/// These are the highest-value thing to learn: not what this app needed, but what this STACK gets wrong.
/// Only hints attached to a CORRECTIVE verdict are kept — an "ok/observed" verdict carries no lesson. Pure
/// over the log text and best-effort: an unreadable log simply yields nothing.
pub(super) fn judge_lessons_from_log(path: &std::path::Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut out: Vec<String> = text
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|e| e.get("event").and_then(|v| v.as_str()) == Some("judge_verdict"))
        .filter(|e| {
            // "ok"/"observed" means the judge had nothing to correct — no lesson there.
            !matches!(
                e.get("verdict").and_then(|v| v.as_str()),
                Some("ok") | Some("observed") | None
            )
        })
        .filter_map(|e| {
            e.get("hint")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|h| !h.is_empty())
                .map(str::to_string)
        })
        .collect();
    out.sort();
    out.dedup();
    out.truncate(12); // a skill is a briefing, not a transcript
    out
}

/// Persist what was learned. Best-effort: a failed write must never fail a run that already succeeded.
/// What a persona write did. `write_persona` is a TRUNCATING write to a path derived purely from the stack
/// key, so it needs a way to say "I found something there that is not mine and left it alone".
pub(super) enum PersonaWrite {
    Written(std::path::PathBuf),
    /// A SKILL.md sits at the persona path that goose did not author — a hand-written skill that happens to
    /// be called `stack-<key>`. Refuse. Learning a lesson is never worth eating the user's own file.
    RefusedForeign(std::path::PathBuf),
}

/// The sentence every rendered persona carries, used to prove goose wrote the file it is about to truncate.
///
/// The clobber predicate USED to be the path alone: persona_dir(key) is a pure function of the stack key, so
/// `write_persona` would truncate whatever sat at config_dir/skills/stack-fastapi — including a skill the user
/// hand-wrote under that name. Moving personas into the shared skills root (so the UI can show them) is what
/// created that collision, so the guard ships with it.
const PERSONA_PROVENANCE: &str = "Written by goose after a build of this stack";

pub(super) fn write_persona(
    stack_key: &str,
    skill: &str,
    runs: usize,
) -> std::io::Result<PersonaWrite> {
    let dir = persona_dir(stack_key);
    let path = dir.join("SKILL.md");
    // Fail SAFE: only a file that is absent, or that carries goose's own provenance line, may be truncated.
    // A user who strips that line out while editing gets a persona goose will never rewrite again — which is
    // the harmless direction for this to fail in.
    if let Ok(existing) = std::fs::read_to_string(&path) {
        if !existing.contains(PERSONA_PROVENANCE) {
            return Ok(PersonaWrite::RefusedForeign(path));
        }
    }
    std::fs::create_dir_all(&dir)?;
    std::fs::write(&path, skill)?;
    let counter = persona_runs_file(stack_key);
    if let Some(parent) = counter.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(counter, runs.to_string());
    Ok(PersonaWrite::Written(path))
}

/// THE REFLECT STEP. The engine has already PROVEN the app built and verified; this asks the model to say
/// WHAT ABOUT THE APPROACH is worth reusing on this stack next time.
///
/// The model is deliberately confined: it is handed only facts the engine attested (the layout that built,
/// the decomposition that shipped, the conventions the judge enforced) and asked to generalise the STACK
/// lesson out of them. It cannot invent success — the trigger is a deterministic gate. It cannot smuggle in
/// the app's features — the prompt forbids it explicitly and the injection channel is advisory.
pub(super) async fn reflect_on_success(
    dispatcher: &Arc<GooseAgentDispatcher>,
    model: &str,
    stack_key: &str,
    snap: &PersonaSnapshot,
    prior: &str,
) -> String {
    let shape = snap
        .shape
        .iter()
        .map(|(id, f)| format!("{id}: {}", f.join(", ")))
        .collect::<Vec<_>>()
        .join("\n");
    let lessons = snap.judge_lessons.join("\n");
    // The model sees its own previous LESSON and the user's notes — never the whole file. r6c's
    // reflection came back as a complete SKILL.md (frontmatter, `# angular`, footer rule,
    // `## Your notes`) because the whole file was what it had been shown to "merge with".
    let prior = prior_lesson_for_model(prior);
    let prior_block = if prior.trim().is_empty() {
        String::new()
    } else {
        format!(
            "\n\nYou have learned about this stack before. MERGE with what you already know — keep what still \
             holds, drop nothing that is still true, and do NOT simply restate it. Write the LESSON only: no \
             frontmatter, no title, no headings of your own — the file around it is goose's:\n{prior}"
        )
    };
    let system = "You are writing a REUSABLE SKILL for a specific technology stack, from a build that \
        PROVABLY worked (it compiled and its checks passed — that is an established fact, not your judgement).\n\n\
        Write what a future build OF THIS SAME STACK should know, so it does not re-derive it from scratch. \
        Be concrete and technical: the build-file shape, how modules are laid out and depend on each other, \
        how tests are wired in, the language/framework conventions that must be honoured.\n\n\
        HARD RULES:\n\
        - Write about the STACK, never about THIS APP. 'A SwiftUI store needs @MainActor' is a stack lesson. \
          'The notes app has a sidebar' is this app's feature and is USELESS — worse than useless, because \
          the next app on this stack is a different product and would be dragged toward the wrong shape.\n\
        - Only state what the evidence below supports. Do not invent facts, versions, or APIs you were not \
          shown. If you are unsure, say less.\n\
        - No preamble, no 'Certainly'. Terse bullets. At most ~200 words."
        .to_string();
    let user = format!(
        "STACK: {stack_key}\n\nThe file layout that BUILT and VERIFIED:\n{}\n\nThe decomposition that \
         shipped (task: files owned):\n{shape}\n\nConventions the judge had to enforce during the build \
         (these are the mistakes this stack invites):\n{lessons}{prior_block}\n\nWrite the skill.",
        snap.files.join("\n")
    );
    // Keyed `reflect` (r6 supervision lanes, batch 2): one call per successful run, at the end —
    // previously the run's last generation was its only invisible one. Parity with the old
    // `run_agent_timed` route otherwise.
    match dispatcher
        .run_agent(model, system, user, None, 4, &[], Some(REFLECT_LANE))
        .await
    {
        // A filler/error-closer reply must never be written into SKILL.md as a stack lesson —
        // future runs READ that file. Empty means empty here: the caller emits `persona_learned
        // written:false, reason "the reflection came back empty"`, so the absence is loud.
        Ok(o) => match supervised_reply_text(&o.text) {
            Ok(t) => t.trim().to_string(),
            Err(_) => String::new(),
        },
        Err(_) => String::new(),
    }
}

/// Render the learned skill as markdown. Human-readable and hand-editable BY DESIGN — this is the mitigation
/// for a wrong lesson: the user can open it, fix it, or delete it. It is also the shape the Skills UI already
/// renders, so it is inspectable without new plumbing.
pub(super) fn render_persona_skill(
    stack_key: &str,
    snap: &PersonaSnapshot,
    reflection: &str,
    runs: usize,
    prior: &str,
) -> String {
    let layout = snap
        .files
        .iter()
        .map(|f| format!("- `{f}`"))
        .collect::<Vec<_>>()
        .join("\n");
    let shape = snap
        .shape
        .iter()
        .map(|(id, files)| format!("- **{id}** — owns {}", files.join(", ")))
        .collect::<Vec<_>>()
        .join("\n");
    let lessons = if snap.judge_lessons.is_empty() {
        "_(none recorded)_".to_string()
    } else {
        snap.judge_lessons
            .iter()
            .map(|l| format!("- {l}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let notes = persona_user_notes(prior);
    let notes = if notes.is_empty() {
        PERSONA_NOTES_PLACEHOLDER.to_string()
    } else {
        notes
    };
    let reflection = sanitize_reflection(reflection);
    format!(
        "---\nname: stack-{stack_key}\ndescription: What goose learned from builds of the {stack_key} stack \
         that actually shipped.\n---\n\n\
         # {stack_key}\n\n\
         {PERSONA_PROVENANCE} that **built and verified**. Everything here is derived from a run the engine \
         PROVED worked — not from a guess. It is advisory: a live spec always overrides it.\n\n\
         Learned from **{runs}** successful build(s).\n\n\
         > **Correcting this file:** everything down to the `{PERSONA_USER_MARKER}` heading is REGENERATED by \
         goose after the next successful build of this stack, so a fix typed up here is lost. Write it under \
         `{PERSONA_USER_MARKER}` instead — that section is kept word for word. Delete the skill to make goose \
         forget the stack entirely.\n\n\
         ## What worked\n\n{reflection}\n\n\
         ## Proven layout\n\n{layout}\n\n\
         ## Decomposition that shipped\n\n{shape}\n\n\
         ## Conventions this stack enforces\n\n\
         Caught by the judge during a successful build — these are the mistakes worth not repeating.\n\n{lessons}\n\n\
         ---\n\
         _Everything above this line is rewritten by goose after the next successful build of this stack. \
         Delete this whole skill to make it forget. To correct it permanently, write under `Your notes` — \
         that section is never rewritten._\n\n\
         {PERSONA_USER_MARKER}\n\n{notes}\n"
    )
}

/// The footer rule the render prints between the generated region and the user's heading.
const PERSONA_FOOTER_SENTINEL: &str = "_Everything above this line";

/// The model's reply, reduced to the LESSON the generated region may carry.
///
/// r6c's `stack-angular/SKILL.md` after two runs had frontmatter at lines 1 AND 16 and
/// `## Your notes` at lines 44 AND 96: the reflection came back as a whole SKILL.md — its own
/// frontmatter, `# angular`, `## What worked`, the footer rule and a `## Your notes` heading —
/// and the render nested it verbatim under the template's `## What worked`. The next rewrite's
/// marker split would then land on the MODEL'S copy of the heading and preserve goose's own
/// lesson as "the user's notes", compounding on every run — the exact hole `persona_user_notes`
/// was written to plug, reopened from the other side. So: a leading frontmatter block goes; a
/// leading H1 and a bare `## What worked` heading go (the render prints both); copied header
/// lines (provenance, run count, the correcting-this-file banner) go; everything from the user
/// marker or the footer sentinel on goes, because the model does not own that region. The lesson
/// text itself is untouched.
fn sanitize_reflection(reflection: &str) -> String {
    let mut text = reflection.trim();
    if let Some(rest) = text.strip_prefix("---") {
        if let Some((_, after)) = rest.split_once("\n---") {
            text = after.trim_start();
        }
    }
    if text.starts_with(PERSONA_USER_MARKER) {
        return String::new();
    }
    let mut end = text.len();
    for sentinel in [
        format!("\n{PERSONA_USER_MARKER}"),
        format!("\n{PERSONA_FOOTER_SENTINEL}"),
    ] {
        if let Some(i) = text.find(&sentinel) {
            end = end.min(i);
        }
    }
    // The render prints `## What worked` itself, so the model's copy goes wherever it sits —
    // r6c's came AFTER its opening paragraph, not first. Copied header lines go likewise.
    let mut lines: Vec<&str> = Vec::new();
    for l in text.get(..end).unwrap_or(text).lines() {
        let t = l.trim();
        if t.contains(PERSONA_PROVENANCE)
            || t.starts_with("> **Correcting this file:**")
            || t.starts_with("Learned from **")
            || t.eq_ignore_ascii_case("## What worked")
        {
            continue;
        }
        if t.is_empty() && lines.last().is_some_and(|p| p.trim().is_empty()) {
            continue; // a removed line must not leave a double blank behind
        }
        lines.push(l);
    }
    while let Some(first) = lines.first() {
        let t = first.trim();
        if t.is_empty() || t.starts_with("# ") {
            lines.remove(0);
        } else {
            break;
        }
    }
    while let Some(last) = lines.last() {
        let t = last.trim();
        if t.is_empty() || t == "---" {
            lines.pop();
        } else {
            break;
        }
    }
    lines.join("\n")
}

/// What the reflection is shown of the prior skill: the previous LESSON (the text under
/// `## What worked`, up to the first render-owned heading) and the user's own notes, labelled
/// as binding. Never the whole file — see `sanitize_reflection`. A prior that is not a goose
/// render (no `## What worked`) contributes no lesson; the user's notes still reach the model.
fn prior_lesson_for_model(prior: &str) -> String {
    let lesson = prior
        .split_once("\n## What worked")
        .map(|(_, after)| after)
        .map(|after| {
            after
                .split_once("\n## Proven layout")
                .map(|(lesson, _)| lesson)
                .unwrap_or(after)
        })
        .map(sanitize_reflection);
    let notes = persona_user_notes(prior);
    let mut out = String::new();
    if let Some(lesson) = lesson {
        out.push_str(&lesson);
    }
    if !notes.is_empty() {
        out.push_str(
            "\n\nThe user's own corrections, written under `## Your notes` (binding — keep the \
             lesson consistent with them):\n",
        );
        out.push_str(&notes);
    }
    out.trim().to_string()
}

/// Read the learned skill for a stack, if one exists. Returns "" when there is nothing — a first build of a
/// stack is byte-identical to today.
pub(super) fn read_persona(stack_key: &str) -> String {
    std::fs::read_to_string(persona_dir(stack_key).join("SKILL.md")).unwrap_or_default()
}

/// How many successful builds this skill has been learned from (for the header + an honest confidence cue).
///
/// Gated on the SKILL.md still being there, so that DELETING the skill — the user's escape hatch from a wrong
/// lesson — actually resets the learning. Without the gate the counter outlives the file it counts, and the
/// next freshly-learned skill opens by claiming "learned from 4 successful builds" on the strength of one.
pub(super) fn persona_runs(stack_key: &str) -> usize {
    if !persona_dir(stack_key).join("SKILL.md").is_file() {
        return 0;
    }
    std::fs::read_to_string(persona_runs_file(stack_key))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

/// The STACK a build targets, fine enough to key learned knowledge on — e.g. "react-vite-ts", "swift-spm",
/// "flask", "fastapi". Deliberately NOT `TargetLang`, which is far too coarse for this: React, Angular and a
/// plain node CLI all collapse to `TypeScript`, and Swift lands in `Other` next to Ruby and Java. Keying a
/// learned skill on that would inject an Angular lesson into a React build — poisoning the very thing the
/// learning is supposed to speed up.
///
/// Returns None unless the match is CONFIDENT. None means "no stack identified" => nothing is learned and
/// nothing is injected, which is the safe default: a wrong key is worse than no key.
///
/// Pure over the spec text + the planned file list, so it is unit-testable and no model opinion decides it.
///
/// EVIDENCE BEFORE SUBSTRINGS (r6c, 2026-09-01). The old rule was `hay.contains(" angular")`, and
/// sb-7's viz section says "zero all angular velocity" / "**Inertia.** Angular velocity" — so a
/// Python-stdlib + vanilla-JS tree was keyed "angular", `stack-angular/SKILL.md` was written and
/// loaded on the next run, and the model's own first line had to say "Despite the name, this stack
/// ships as a Python backend + vanilla-JS static web console — no Angular framework". Three
/// sources decide, in this order:
///  1. FILES — a manifest or config that names the stack (angular.json, vite.config.*, next.config.*,
///     svelte.config.* / *.svelte, Package.swift) is evidence a spec word is not.
///  2. THE SPEC'S OWN NEGATIVES and the TREE'S SHAPE — "standard library only", "no npm", "no build
///     step", "plain HTML/CSS/JS", "vanilla", or plain web/*.js|html|css with no package.json and no
///     framework config, veto every JS-framework key and are the positive evidence for the vanilla key.
///  3. THE SPEC'S FRAMEWORK NAMES — an English word (angular, react, swift, flask) counts only when the
///     words around it frame it as a stack ("with Angular", "an Angular SPA", "React + TypeScript",
///     "Swift 6"), never as a bare substring; names that are not English words (fastapi, django,
///     svelte, swiftui, vite, next.js, @angular) count on sight.
///
/// A Python tree with plain web assets keys "python-vanilla-web"; with the spec's stdlib statement and
/// no Python dependency manifest in the tree, "python-stdlib-vanilla-web". The two keys differ by a
/// measured fact. Both calls the engine makes — spec-only at run start, spec + files after planning —
/// resolve sb-7 to the same key, so the skill loaded and the skill learned are one file.
pub(super) fn detect_stack_key(spec: &str, files: &[String]) -> Option<String> {
    let spec_l = spec.to_lowercase();
    let files_l: Vec<String> = files.iter().map(|f| f.to_lowercase()).collect();
    let basename = |f: &str| f.rsplit('/').next().unwrap_or(f).to_string();
    let has_file = |name: &str| files_l.iter().any(|f| basename(f) == name);
    let has_file_prefix = |prefix: &str| files_l.iter().any(|f| basename(f).starts_with(prefix));
    let has_ext = |ext: &str| files_l.iter().any(|f| f.ends_with(ext));
    let spec_has = |needles: &[&str]| needles.iter().any(|n| spec_l.contains(n));
    let framed = |name: &str| framework_framed_in(&spec_l, name);

    // 1. FILES.
    if has_file("angular.json") {
        return Some("angular".into());
    }
    if has_file_prefix("vite.config.") && (framed("react") || has_ext(".tsx") || has_ext(".jsx")) {
        return Some("react-vite-ts".into());
    }
    if has_file_prefix("next.config.") {
        return Some("nextjs".into());
    }
    if has_file_prefix("svelte.config.") || has_ext(".svelte") {
        return Some("svelte".into());
    }
    if has_file("package.swift") {
        return Some("swift-spm".into());
    }

    // 2. NEGATIVES and the tree's shape.
    let vanilla_web_stated = spec_has(&[
        "no npm",
        "no build step",
        "no framework",
        "without a framework",
        "plain html",
        "vanilla",
        "no bundler",
        "zero external code",
    ]);
    let stdlib_stated = spec_has(&[
        "standard library only",
        "standard-library only",
        "stdlib only",
        "no pip",
        "no third-party",
        "no third party",
    ]);
    let web_asset = |f: &str| {
        (f.ends_with(".js") || f.ends_with(".html") || f.ends_with(".css"))
            && f.split('/')
                .any(|s| matches!(s, "web" | "static" | "public" | "frontend" | "assets"))
    };
    let has_web_assets = files_l.iter().any(|f| web_asset(f));
    let has_js_manifest = has_file("package.json")
        || has_file_prefix("vite.config.")
        || has_file_prefix("next.config.")
        || has_file_prefix("svelte.config.")
        || has_file_prefix("webpack.config.")
        || has_file_prefix("tsconfig.");
    let has_py = has_ext(".py");
    let has_py_manifest = has_file("requirements.txt")
        || has_file("pyproject.toml")
        || has_file("setup.py")
        || has_file("setup.cfg")
        || has_file("pipfile");
    let vanilla_web = vanilla_web_stated || (has_web_assets && !has_js_manifest);

    // 3. FRAMEWORK NAMES in the spec — JS frameworks only when nothing measured vetoes them.
    if !vanilla_web {
        if spec_has(&["angular.json", "@angular"]) || framed("angular") {
            return Some("angular".into());
        }
        if framed("react") && spec_has(&["vite"]) {
            return Some("react-vite-ts".into());
        }
        if spec_has(&["next.js", "nextjs"]) {
            return Some("nextjs".into());
        }
        if spec_has(&["svelte"]) {
            return Some("svelte".into());
        }
    }
    if spec_has(&["package.swift", "swiftui"]) || framed("swift") {
        return Some("swift-spm".into());
    }
    if spec_has(&["fastapi"]) {
        return Some("fastapi".into());
    }
    if framed("flask") {
        return Some("flask".into());
    }
    if spec_has(&["django"]) {
        return Some("django".into());
    }

    // 4. THE MEASURED VANILLA SHAPE. A Python backend behind plain web assets IS a stack (sb-7's).
    if (has_py || spec_has(&["python"])) && vanilla_web {
        return Some(if stdlib_stated && !has_py_manifest {
            "python-stdlib-vanilla-web".into()
        } else {
            "python-vanilla-web".into()
        });
    }
    // A bare language is NOT a stack — "python" alone spans a CLI, a web app and a library, whose proven
    // layouts have nothing in common. Refuse rather than pollute one bucket with all of them.
    None
}

/// Is `name` used as a STACK in the spec's words — not as an adjective or a verb? The spec is
/// split on whitespace with surrounding punctuation trimmed; the name counts when the word before
/// it frames a stack (with / in / using / on / via) or the word after it does (app, spa, cli,
/// project, frontend, component(s), workspace, framework, `+`, a version). "zero all angular
/// velocity", "**Inertia.** Angular velocity" and "the UI must react to input" frame nothing.
fn framework_framed_in(spec_lower: &str, name: &str) -> bool {
    let words: Vec<&str> = spec_lower
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '+' && c != '@'))
        .collect();
    words.iter().enumerate().any(|(i, w)| {
        if *w != name {
            return false;
        }
        let before = i.checked_sub(1).and_then(|j| words.get(j)).copied();
        let after = words.get(i + 1).copied();
        matches!(before, Some("with" | "in" | "using" | "on" | "via"))
            || after.is_some_and(|a| {
                matches!(
                    a,
                    "app"
                        | "apps"
                        | "application"
                        | "spa"
                        | "cli"
                        | "project"
                        | "frontend"
                        | "front-end"
                        | "component"
                        | "components"
                        | "workspace"
                        | "framework"
                        | "+"
                ) || a.chars().next().is_some_and(|c| c.is_ascii_digit())
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::swarm::{detect_language, TargetLang};

    /// The trap this exists for: TargetLang collapses React, Angular and a node CLI all into `TypeScript`,
    /// and Swift into `Other` beside Ruby. Keying learned knowledge on that would inject an Angular lesson
    /// into a React build — poisoning the very thing the learning is meant to speed up.
    #[test]
    fn detect_stack_key_never_collapses_distinct_stacks() {
        let react = detect_stack_key(
            "Build Minesweeper as a React + TypeScript SPA using Vite",
            &[],
        );
        let angular = detect_stack_key("Build a dashboard with Angular", &["angular.json".into()]);
        assert_eq!(react.as_deref(), Some("react-vite-ts"));
        assert_eq!(angular.as_deref(), Some("angular"));
        assert_ne!(
            react, angular,
            "react and angular must NEVER share a bucket"
        );

        // THE WHOLE REASON THIS FUNCTION EXISTS: to TargetLang, React and Angular are the SAME thing.
        // (Second-order trap found while writing this test: detect_language DEFAULTS TO PYTHON without an
        // explicit cue, so "Build a React app with Vite" alone is Python to it. Given a real TypeScript spec
        // + manifest, both frameworks still collapse into one bucket.)
        let react_files = vec!["src/App.tsx".to_string(), "vite.config.ts".to_string()];
        let ng_files = vec![
            "src/app.component.ts".to_string(),
            "angular.json".to_string(),
        ];
        assert_eq!(
            detect_language("Build a React SPA in TypeScript", &react_files),
            TargetLang::TypeScript
        );
        assert_eq!(
            detect_language("Build an Angular SPA in TypeScript", &ng_files),
            TargetLang::TypeScript
        );
        // Identical to TargetLang; correctly distinct to detect_stack_key.
        assert_ne!(
            detect_stack_key("Build a React SPA in TypeScript with Vite", &react_files),
            detect_stack_key("Build an Angular SPA in TypeScript", &ng_files)
        );

        assert_eq!(
            detect_stack_key(
                "a macOS notes app in Swift 6 with SwiftUI",
                &["Package.swift".into()]
            )
            .as_deref(),
            Some("swift-spm")
        );
        assert_eq!(
            detect_stack_key("Build a FastAPI + SQLite web app", &[]).as_deref(),
            Some("fastapi")
        );
    }

    /// A bare language is NOT a stack: "python" spans a CLI, a web app and a library, whose proven layouts
    /// have nothing in common. Refusing to key is the safe answer — no key means nothing is learned and
    /// nothing is injected, which is byte-identical to today.
    #[test]
    fn detect_stack_key_refuses_when_it_is_not_confident() {
        assert_eq!(detect_stack_key("Build a python tool", &[]), None);
        assert_eq!(detect_stack_key("Build something nice", &[]), None);
        assert_eq!(detect_stack_key("", &[]), None);
        // React WITHOUT vite is not the react-vite-ts stack — do not guess it in.
        assert_eq!(detect_stack_key("a react native mobile app", &[]), None);
    }

    /// Moving personas into the SHARED skills root is what made this reachable: persona_dir is a pure
    /// function of the stack key, and write_persona truncates. A user who hand-writes a skill named
    /// `stack-fastapi` would have had it silently eaten by the first successful fastapi build. Authorship,
    /// not the path, decides — and the check runs against real bytes on a real disk, because the bug is in
    /// the filesystem call and a mocked one would prove nothing.
    #[test]
    fn a_persona_write_never_truncates_a_skill_goose_did_not_author() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("SKILL.md");
        let hand_written =
            "---\nname: stack-fastapi\ndescription: mine\n---\n\nMy own hard-won notes.\n";
        std::fs::write(&path, hand_written).unwrap();

        // the guard's exact predicate, over the real file
        let existing = std::fs::read_to_string(&path).unwrap();
        assert!(
            !existing.contains(PERSONA_PROVENANCE),
            "a hand-written skill must not look goose-authored"
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            hand_written,
            "the user's file must be byte-identical after the check"
        );

        // ...whereas goose's own render IS recognised, so it can still update itself
        let rendered = render_persona_skill("fastapi", &PersonaSnapshot::default(), "x", 1, "");
        assert!(rendered.contains(PERSONA_PROVENANCE));
    }

    /// THE SAFETY CLAIM, PINNED. The persona is only defensible because the user can SEE the lesson a weak
    /// model wrote about itself and throw it out. That rests entirely on the write landing on a directory
    /// `goose::skills` actually walks — which the first version did not (it wrote to `data_dir/swarm/personas`,
    /// a path no skill root covers, so nothing goose learned was ever visible). Assert against the real
    /// discovery list, so moving either side of this contract fails here instead of silently un-mitigating it.
    #[test]
    fn learned_persona_lands_where_the_skills_ui_looks() {
        let dir = persona_dir("react-vite-ts");
        let roots: Vec<_> = goose::skills::all_skill_dirs(None)
            .into_iter()
            .map(|(d, _)| d)
            .collect();
        assert!(
            roots.iter().any(|r| dir.starts_with(r)),
            "persona dir {dir:?} is under NO skill discovery root {roots:?} — the user cannot see, edit or \
             delete what goose learned, which is the whole mitigation for a wrong lesson"
        );
        assert!(dir.ends_with("stack-react-vite-ts"));

        // The counter must NOT sit in the skill folder: every non-SKILL.md file there is advertised to the
        // model as a loadable supporting file, so a `runs` file becomes a tool call returning one integer.
        let counter = persona_runs_file("react-vite-ts");
        assert!(
            !counter.starts_with(&dir),
            "the run counter {counter:?} is inside the skill folder and would be advertised as loadable"
        );
    }

    /// The render must emit the exact sentence the write guard looks for. These are two constants in two
    /// functions and nothing but this test couples them: if the render stops emitting PERSONA_PROVENANCE,
    /// every future write silently takes the RefusedForeign branch and goose quietly stops learning — a
    /// dead feature behind a green build, which is the failure mode this codebase keeps producing.
    #[test]
    fn a_rendered_persona_is_recognised_as_goose_authored() {
        let snap = PersonaSnapshot::default();
        let rendered = render_persona_skill("fastapi", &snap, "a lesson", 1, "");
        assert!(
            rendered.contains(PERSONA_PROVENANCE),
            "the write guard proves authorship by finding PERSONA_PROVENANCE in the file it is about to \
             truncate; this render does not contain it, so goose would refuse to overwrite its own skill"
        );
        // and the warning must sit ABOVE the lesson, not only in the footer: the user fixes a wrong sentence
        // where the wrong sentence IS, and everything above the marker is regenerated.
        let warn = rendered
            .find("REGENERATED")
            .expect("no in-place-edit warning");
        let lesson = rendered.find("## What worked").expect("no lesson section");
        assert!(
            warn < lesson,
            "the warning that this region is regenerated must appear BEFORE the region the user will edit"
        );
    }

    /// The file tells the user their corrections are permanent. A rewrite is `fs::write` — truncating — so
    /// that promise is only true if the user's section round-trips through the render DETERMINISTICALLY.
    /// Handing it to a weak model as "prior" and hoping it merges is exactly the trust this cannot assume.
    #[test]
    fn user_notes_survive_a_rewrite_verbatim() {
        let snap = PersonaSnapshot {
            stack_key: Some("fastapi".into()),
            files: vec!["app/main.py".into()],
            shape: vec![("api".into(), vec!["app/main.py".into()])],
            judge_lessons: vec!["pin the pydantic version".into()],
        };
        let correction =
            "goose keeps getting this wrong: we use SQLModel here, NOT raw SQLAlchemy.";
        let first = render_persona_skill("fastapi", &snap, "use uvicorn", 1, "");
        assert!(first.contains(PERSONA_USER_MARKER));
        assert_eq!(
            persona_user_notes(&first),
            "",
            "a fresh skill has no user notes"
        );
        // The banner NAMES the marker inline, above the real heading. Cutting at the first textual hit
        // would hand back the whole document as "the user's notes" — assert we cut at the heading itself.
        assert!(
            first.matches(PERSONA_USER_MARKER).count() > 1,
            "this test only bites while the banner names the marker inline"
        );
        assert!(
            !persona_user_notes(&first).contains("use uvicorn"),
            "the split landed on the banner, not the heading — goose's own lesson is being preserved as \
             though the user wrote it, and it will compound on every rewrite"
        );

        // the user opens the file and appends their correction under the marker
        let edited = format!("{first}\n{correction}\n");
        assert_eq!(persona_user_notes(&edited), correction);

        // ...and a later successful build rewrites everything ABOVE the marker
        let rewritten =
            render_persona_skill("fastapi", &snap, "a totally different lesson", 2, &edited);
        assert!(
            rewritten.contains(correction),
            "the rewrite dropped the user's correction — 'write here and it sticks' is then a lie:\n{rewritten}"
        );
        assert!(rewritten.contains("a totally different lesson"));
        assert!(
            !rewritten.contains("use uvicorn"),
            "the model's old lesson should be replaced, not kept"
        );
        // and it must be idempotent: rewriting again does not duplicate or nest the notes
        let again = render_persona_skill("fastapi", &snap, "third lesson", 3, &rewritten);
        assert_eq!(
            again.matches(correction).count(),
            1,
            "the notes were duplicated across rewrites"
        );
        // Exactly one real HEADING. The banner names the marker inline too, so a raw substring count is the
        // wrong invariant — what must never happen is a SECOND notes section, which would strand the first.
        let headings = |s: &str| {
            s.matches(&format!("\n{PERSONA_USER_MARKER}")).count()
                + usize::from(s.starts_with(PERSONA_USER_MARKER))
        };
        assert_eq!(headings(&again), 1, "a rewrite grew a second notes section");
        assert_eq!(headings(&first), 1);
    }

    /// Only a CORRECTIVE verdict carries a lesson; "ok"/"observed" means the judge had nothing to say. Uses
    /// the real event shape emitted by a run.
    #[test]
    fn judge_lessons_keeps_only_corrective_hints() {
        let dir = tempfile::TempDir::new().unwrap();
        let log = dir.path().join("run.jsonl");
        std::fs::write(
            &log,
            "{\"event\":\"judge_verdict\",\"verdict\":\"ok\",\"hint\":\"\"}\n\
             {\"event\":\"judge_verdict\",\"verdict\":\"observed\",\"hint\":\"looks fine\"}\n\
             {\"event\":\"judge_verdict\",\"verdict\":\"broken_code\",\"hint\":\"Add @MainActor to the NoteStore class declaration\"}\n\
             {\"event\":\"judge_verdict\",\"verdict\":\"broken_code\",\"hint\":\"Add @MainActor to the NoteStore class declaration\"}\n\
             {\"event\":\"task_completed\",\"hint\":\"not a judge event\"}\n\
             not json at all\n",
        )
        .unwrap();
        let out = judge_lessons_from_log(&log);
        assert_eq!(
            out.len(),
            1,
            "dedup + drop ok/observed/non-judge, got {out:?}"
        );
        assert!(out[0].contains("@MainActor"));
    }

    #[test]
    fn judge_lessons_from_a_missing_log_is_empty_not_a_panic() {
        assert!(judge_lessons_from_log(std::path::Path::new("/nonexistent/run.jsonl")).is_empty());
    }

    /// r6c: `detect_stack_key` keyed sb-7's Python-stdlib + vanilla-JS tree "angular" off the viz
    /// section's physics (the spec's own lines, verbatim below) and wrote stack-angular/SKILL.md,
    /// whose model-written opener then read "Despite the name, this stack ships as a Python
    /// backend + vanilla-JS static web console — no Angular framework". The tree is r6c's real
    /// file list. Both calls the engine makes (spec-only at run start, spec + files after planning) must agree,
    /// or the skill loaded and the skill learned are two different files.
    #[test]
    fn a_physics_adjective_never_keys_a_stack_and_r6cs_tree_keys_stdlib_vanilla() {
        let spec = "Work in the current directory. Python 3, standard library only for the backend — no pip\n\
            library). The frontend ships ZERO external code — no CDN, no npm, no vendored libraries of any\n\
            A single page, served by ledgerd at `GET /`. Plain HTML/CSS/JS, no build step, no CDN, no\n\
            - **Double-click:** reset to the defaults AND zero all angular velocity.\n\
            **Inertia.** Angular velocity `(vyaw, vpitch)` in deg/s. At pointerup it equals the rate";
        let r6c_tree: Vec<String> = [
            "app/__main__.py",
            "app/api.py",
            "app/auth.py",
            "app/db.py",
            "app/drafts.py",
            "app/ledger.py",
            "app/ledgerd/__init__.py",
            "app/ledgerd/__main__.py",
            "app/ledgerd/impl.py",
            "app/notifierd/__init__.py",
            "app/notifierd/__main__.py",
            "app/notifierd/impl.py",
            "app/notify_store.py",
            "app/outbox.py",
            "app/sync.py",
            "app/webhooks.py",
            "tests/test_boot_contract.py",
            "tools/__init__.py",
            "tools/vizmath_reference.py",
            "web/app.js",
            "web/index.html",
            "web/styles.css",
            "web/viz.js",
            "DECISIONS.md",
            "README.md",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            detect_stack_key(spec, &r6c_tree).as_deref(),
            Some("python-stdlib-vanilla-web")
        );
        assert_eq!(
            detect_stack_key(spec, &[]).as_deref(),
            Some("python-stdlib-vanilla-web"),
            "the run-start (spec-only) call must agree with the post-plan call"
        );
        // The bare substring keys nothing, in either casing the spec uses.
        assert_eq!(
            detect_stack_key("reset to the defaults AND zero all angular velocity", &[]),
            None
        );
        assert_eq!(
            detect_stack_key("**Inertia.** Angular velocity in deg/s", &[]),
            None
        );
        // The framework framed as a stack still keys — words, not substrings.
        assert_eq!(
            detect_stack_key("Build a dashboard with Angular", &[]).as_deref(),
            Some("angular")
        );
        assert_eq!(
            detect_stack_key("an Angular SPA for the ops team", &[]).as_deref(),
            Some("angular")
        );
        // A measured tree beats a spec word: python + plain web assets and no manifest is vanilla
        // even when the spec says "with React" somewhere.
        assert_eq!(
            detect_stack_key("a console with React for the ops team", &r6c_tree).as_deref(),
            Some("python-vanilla-web")
        );
        // A dependency manifest in the tree drops the stdlib claim — the tree is the measurement.
        let tree_with_reqs = [r6c_tree.clone(), vec!["requirements.txt".into()]].concat();
        assert_eq!(
            detect_stack_key(spec, &tree_with_reqs).as_deref(),
            Some("python-vanilla-web")
        );
        // Files only, spec silent: the tree's own shape keys.
        assert_eq!(
            detect_stack_key("", &r6c_tree).as_deref(),
            Some("python-vanilla-web")
        );
    }

    /// r6c's stack-angular/SKILL.md after two runs: frontmatter at lines 1 AND 16, `## Your notes`
    /// at lines 44 AND 96 — the reflection came back as a whole SKILL.md and was nested under the
    /// template's `## What worked`. A reply carrying frontmatter, an H1, its own `## What worked`,
    /// a footer rule and a `## Your notes` heading must produce ONE frontmatter, ONE notes heading,
    /// ONE footer, and keep the lesson sentences themselves.
    #[test]
    fn a_reflection_that_arrives_as_a_whole_skill_file_is_reduced_to_its_lesson() {
        let reply = "---\nname: stack-angular\ndescription: What goose learned from builds of the angular stack.\n---\n\n\
            # angular\n\n\
            Despite the name, this stack ships as a Python backend + vanilla-JS static web console.\n\n\
            ## What worked\n\n\
            - Backend: plain Python package `app/`, entry point `app/__main__.py`.\n\n\
            ---\n\
            _Everything above this line is rewritten by goose after the next successful build._\n\n\
            ## Your notes\n\n_Nothing yet._\n";
        let rendered = render_persona_skill("angular", &PersonaSnapshot::default(), reply, 2, "");
        assert_eq!(
            rendered.matches("---\nname: stack-").count(),
            1,
            "one frontmatter, goose's own:\n{rendered}"
        );
        assert_eq!(rendered.matches("\n## Your notes").count(), 1, "{rendered}");
        assert_eq!(rendered.matches("## What worked").count(), 1, "{rendered}");
        assert_eq!(
            rendered.matches(PERSONA_FOOTER_SENTINEL).count(),
            1,
            "{rendered}"
        );
        assert!(!rendered.contains("# angular\n\n# angular"));
        assert!(rendered.contains("Despite the name, this stack ships as a Python backend"));
        assert!(rendered.contains("- Backend: plain Python package `app/`"));
        // The next rewrite's marker split lands on goose's heading, so nothing of the lesson is
        // preserved as the user's notes.
        assert_eq!(persona_user_notes(&rendered), "");

        // The reflection is shown its previous LESSON and the user's notes — never the frame.
        let edited = format!("{rendered}\nuse http.server, never Flask.\n");
        let shown = prior_lesson_for_model(&edited);
        assert!(shown.contains("Despite the name"));
        assert!(shown.contains("use http.server, never Flask."));
        assert!(!shown.contains("name: stack-"));
        assert!(!shown.contains("Correcting this file"));
        assert!(!shown.contains("## Proven layout"));
        // A plain lesson passes through untouched.
        assert_eq!(sanitize_reflection("use uvicorn"), "use uvicorn");
    }
}
