//! The judge-context cluster: what a task DELIVERED, measured off the tree.
//!
//! First sibling module under the incremental-split law (development_gates::
//! swarm_rs_line_count_only_decreases): swarm.rs is a module ROOT and may only shrink, so the
//! delivery-measurement functions the omni judge's look prompt is built from live here. Moved
//! verbatim from swarm.rs (N-7, ef23d728e lineage) — behavior unchanged; the WHY of every part
//! stays in each function's own doc.

use std::path::Path;

use super::shape_excerpt;

/// Declared here rather than in swarm.rs: `verify_owned_files` below is its only consumer, and
/// the split law prices every wiring line in the root. `#[path]` resolves beside this file, so
/// web_refs.rs stays a flat commands/swarm/ sibling like the rest.
#[path = "web_refs.rs"]
mod web_refs;

/// An empty `__init__.py` or `py.typed` is a CORRECT, INTENTIONAL file, not a missing deliverable.
///
/// ONE rule in ONE place, because it was five hand-written copies and the fifth had already diverged.
/// The hallucinated-completion guard, the watchdog salvage's acceptance test and the two missing-deliverable
/// gates all exempt BOTH names; `verify_owned_files` exempted `__init__.py` alone, so an owned empty
/// `py.typed` cleared every guard that decides whether a task delivered and was then reported by the
/// verifier as "exists but is EMPTY" — a false positive on a finding class that is fed straight back to
/// the worker as a DELIVERY DEFECT steer, so a wrong one costs a real turn arguing with the model about
/// a file that was correct all along.
/// The comment at the missing-deliverable gate records that flagging exactly this case already burned a
/// whole fix round re-creating a file that was never wrong.
///
/// A BASENAME test, not a suffix test: `ends_with("__init__.py")` also matches `pkg__init__.py`.
///
/// NOT the same rule as `scheduler.rs`'s `looks_like_manifest_file`, which decides whether a salvaged
/// deliverable is too trivial to count. Merging them would put two different questions on one predicate.
pub(super) fn is_intentional_empty_marker(path: &str) -> bool {
    let base = path.rsplit('/').next().unwrap_or(path);
    base == "__init__.py" || base == "py.typed"
}

/// N-7: which signature extractor fits ONE file, by extension. `shape_excerpt` is called per owned
/// file in the judge's delivery block, where no plan-level TargetLang is in scope; `Other` falls
/// back to the raw body at the call site, so a wrong guess degrades to today's behaviour.
fn sig_lang_for_path(path: &str) -> goose_swarm::SigLang {
    match std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
    {
        Some("py") => goose_swarm::SigLang::Python,
        Some("rs") => goose_swarm::SigLang::Rust,
        Some("go") => goose_swarm::SigLang::Go,
        Some("ts" | "tsx" | "js" | "jsx" | "mjs") => goose_swarm::SigLang::TypeScript,
        _ => goose_swarm::SigLang::Other,
    }
}

/// N-7: WHAT WAS DELIVERED, as the omni judge's prompt sees it — extracted into one pure function so
/// the block is testable without a stream. Four measurements, no verdict logic:
///
///   1. the census — every owned path's existence + byte count (a task with nothing on disk SAYS so);
///   2. the parse facts — `verify_owned_files`' deterministic findings (py_compile, skeletons), passed
///      in because the caller computes them once and uses them twice (prompt + defect steer);
///   3. a BUDGETED shape excerpt of what the files actually hold — `shape_excerpt` (P1-6): signatures
///      plus the key-literal/route/returned-dict lines, the densest per-char answer to "is this the
///      right file", replacing the raw 1,200-char head of only the FIRST file;
///   4. the window's unowned writes — paths that appeared/changed since this attempt started that NO
///      planned task owns, from the same snapshot machinery as the attempt-end fs_delta (II-1).
///
/// 4 is the r2 camera-system evidence: the judge said "ok" while the deliverable sat at the tree root,
/// because existence alone said only "missing" and nothing connected the missing owned path to the
/// same-named file appearing where nobody owns one. A basename match between an unowned write and an
/// owned path is called out as a WRONG-PATH fact the judge can put straight into NEXT.
///
/// Excerpts are bounded and placed LAST on purpose: this prompt's verdict quality rests on the
/// reasoning tail and the recurrence evidence, and file bytes must never crowd those out of a 27B's
/// window.
pub(super) fn judge_delivery_block(
    working_dir: &Path,
    owned: &[String],
    defects: &[String],
    unowned_writes: &[String],
) -> String {
    if owned.is_empty() {
        return String::new();
    }
    let listed: Vec<String> = owned
        .iter()
        .map(|f| {
            let state = match std::fs::metadata(working_dir.join(f)) {
                Ok(m) if m.len() > 0 => format!("EXISTS, {} bytes", m.len()),
                Ok(_) => "EXISTS BUT EMPTY".to_string(),
                Err(_) => "DOES NOT EXIST".to_string(),
            };
            format!("  {f} — {state}")
        })
        .collect();
    let defect_block = if defects.is_empty() {
        String::new()
    } else {
        format!(
            "\n\nDETERMINISTIC CHECKS OF WHAT IT HAS WRITTEN SO FAR — these are \
             FACTS, not opinions, read off the files a moment ago:\n{}\n\
             If one of these is still true when the call ends, the task has not \
             delivered. Put the FIRST of them into NEXT, naming the exact path.",
            defects
                .iter()
                .map(|d| format!("  {d}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    };
    // The window's unowned writes. A stat of the tree, never a self-report — the window is shared
    // with sibling calls, so plain listings carry that caveat; a basename match against an owned
    // path is the one shape specific enough to state as this task's own misplaced deliverable.
    let misplaced: Vec<String> = unowned_writes
        .iter()
        .filter_map(|p| {
            let base = std::path::Path::new(p).file_name()?.to_str()?;
            owned
                .iter()
                .find(|o| {
                    *o != p
                        && std::path::Path::new(o).file_name().and_then(|n| n.to_str())
                            == Some(base)
                })
                .map(|o| {
                    format!(
                        "  `{p}` has the SAME NAME as owned `{o}` — the deliverable is at the \
                         WRONG PATH. Put \"move it to `{o}`\" into NEXT."
                    )
                })
        })
        .collect();
    let unowned_block = if unowned_writes.is_empty() {
        String::new()
    } else {
        format!(
            "\n\nFILES WRITTEN IN THIS CALL'S WINDOW THAT NO PLANNED TASK OWNS (a stat of the \
             tree since this attempt started, not a self-report; sibling calls share the window, \
             so attribute plain entries with care):\n{}{}",
            unowned_writes
                .iter()
                .map(|p| format!("  {p}"))
                .collect::<Vec<_>>()
                .join("\n"),
            if misplaced.is_empty() {
                String::new()
            } else {
                format!("\n{}", misplaced.join("\n"))
            }
        )
    };
    const OWNED_EXCERPT_TOTAL: usize = 2_400;
    const OWNED_EXCERPT_PER_FILE: usize = 1_200;
    let mut excerpt_budget = OWNED_EXCERPT_TOTAL;
    let mut excerpt_block = String::new();
    for f in owned {
        if excerpt_budget == 0 {
            break;
        }
        let Ok(body) = std::fs::read_to_string(working_dir.join(f)) else {
            continue;
        };
        if body.trim().is_empty() {
            continue;
        }
        let shaped = shape_excerpt(&body, sig_lang_for_path(f));
        let (source, kind): (&str, &str) = if shaped.trim().is_empty() {
            (body.as_str(), "raw head")
        } else {
            (shaped.as_str(), "signatures + shape lines")
        };
        let cap = excerpt_budget.min(OWNED_EXCERPT_PER_FILE);
        let head: String = source.chars().take(cap).collect();
        let cut = if head.chars().count() < source.chars().count() {
            "\n  … (cut — an excerpt, not the whole file; never treat the cut as unfinished work)"
        } else {
            ""
        };
        excerpt_budget = excerpt_budget.saturating_sub(head.chars().count());
        excerpt_block.push_str(&format!(
            "\n\nWhat `{f}` holds right now ({kind}):\n{head}{cut}"
        ));
    }
    format!(
        "\n\nTHIS TASK OWNS THESE FILES, and nothing else in the build will write \
         them:\n{}\n\
         ITS DELIVERABLE IS THE FILE, NOT THE REASONING. Characters of thinking are \
         not progress here; bytes on disk are. If it owns a file that does not exist \
         yet and it has taken no action, it is composing the file in its head and \
         waiting for it to be perfect — the single most expensive failure this run \
         can have. Tell it to write a first minimal version of that exact path NOW \
         and extend it afterwards.{defect_block}{unowned_block}{excerpt_block}",
        listed.join("\n")
    )
}

/// A DETERMINISTIC CHECK OF WHAT A TASK ACTUALLY DELIVERED.
///
/// The omni-judge asks a 27B to INFER from a reasoning tail whether work is going well: 211 calls on run 4,
/// median 49s, **46% of the whole fleet**, to produce 38 nudges. Meanwhile `python3 -m py_compile app/x.py`
/// answers "does this parse" definitively, in milliseconds, on no node at all.
///
/// This is the free tier of that idea. It reads FILES, never reasoning, and every finding it returns is a
/// fact rather than an opinion — which is also what makes it safe to act on automatically.
///
/// Returns one line per defect, empty when the task delivered what it owned.
pub(super) fn verify_owned_files(working_dir: &Path, owned: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for rel in owned {
        let path = working_dir.join(rel);
        match std::fs::metadata(&path) {
            Err(_) => {
                // Tense-neutral ON PURPOSE (II-9): this exact string reaches two audiences — the
                // completion-time `delivery_defects` event, where "finished without writing" was
                // true, and the judge's mid-run defect steer, where the same words told a call
                // that was STILL RUNNING that it had finished. A steer may never claim "finished"
                // to a live call, so the fact is stated without a tense claim.
                out.push(format!(
                    "{rel} does not exist — this task owns it and nothing has written it"
                ));
                continue;
            }
            // AN EMPTY `__init__.py` IS CORRECT PYTHON, not a defect — it is how a package is marked, and
            // flagging it would make the verifier cry wolf on every well-formed tree. CAUGHT BY REPLAY
            // before this ever reached a run: the first sweep reported four findings across the three
            // SCORED local runs and every one was an empty `__init__.py`.
            Ok(m) if m.len() == 0 => {
                if !is_intentional_empty_marker(rel) {
                    out.push(format!("{rel} exists but is EMPTY"));
                }
                continue;
            }
            Ok(_) => {}
        }
        // A python file that does not parse is a fact, and every task downstream of it is already broken.
        if rel.ends_with(".py") {
            let ok = std::process::Command::new("python3")
                .arg("-m")
                .arg("py_compile")
                .arg(&path)
                .output();
            if let Ok(o) = ok {
                if !o.status.success() {
                    let err = String::from_utf8_lossy(&o.stderr);
                    let first = err
                        .lines()
                        .rev()
                        .find(|l| !l.trim().is_empty())
                        .unwrap_or("syntax error");
                    out.push(format!("{rel} DOES NOT PARSE: {}", first.trim()));
                    // One defect per file: a file that does not parse must not ALSO be reported as a
                    // skeleton, whose message claims it "exists and parses". Two findings for one broken
                    // file is noise, and the wrong one of the two is a lie.
                    continue;
                }
            }
        }
        // A FILE THAT EXISTS AND IS ONLY STUBS IS NOT A DELIVERABLE, and it is the defect most likely to
        // be mistaken for success: it exists, it is non-empty, it parses, and it does nothing. The engine
        // already refuses to salvage one of these as "done" at the watchdog path (`skeleton_only`,
        // judge.rs:66) — the same test belongs here, where it is cheap and early rather than at the end.
        if rel.ends_with(".py") {
            if let Ok(body) = std::fs::read_to_string(&path) {
                if goose_swarm::judge::skeleton_only(&body) {
                    out.push(format!(
                        "{rel} is a SKELETON — it exists and parses, but every body is a stub"
                    ));
                    continue;
                }
            }
        }
        // A browser JS file that references an identifier defined nowhere in reach dies at boot
        // with a ReferenceError that `node --check` structurally cannot see — r5 shipped exactly
        // that (viz.js defined onBrushChange, registered onBrushChangeTracked) behind a green
        // syntax check and `delivery_defects: []`, and four of five graded mechanisms were dead
        // on arrival. The scan is MILD and false-positive-averse by construction; see web_refs.rs.
        if rel.ends_with(".js") || rel.ends_with(".mjs") {
            out.extend(web_refs::browser_js_undefined_refs(working_dir, rel));
        }
        // An HTML file that points at a file nobody wrote renders blank, and nothing else notices until
        // someone opens it — which historically has been nobody until the score comes back.
        if rel.ends_with(".html") {
            if let Ok(body) = std::fs::read_to_string(&path) {
                for cap in body.split(['"', '\'']) {
                    let c = cap.trim();
                    if (c.ends_with(".js") || c.ends_with(".css"))
                        && !c.starts_with("http")
                        && !c.is_empty()
                    {
                        let target = path.parent().map(|p| p.join(c)).unwrap_or_default();
                        if !target.exists() {
                            out.push(format!("{rel} references `{c}`, which does not exist"));
                        }
                    }
                }
            }
        }
    }
    out
}
