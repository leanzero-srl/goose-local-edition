//! The judge-context cluster: what a task DELIVERED, measured off the tree.
//!
//! First sibling module under the incremental-split law (development_gates::
//! swarm_rs_line_count_only_decreases): swarm.rs is a module ROOT and may only shrink, so the
//! delivery-measurement functions the omni judge's look prompt is built from live here. Moved
//! verbatim from swarm.rs (N-7, ef23d728e lineage) — behavior unchanged; the WHY of every part
//! stays in each function's own doc.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use goose_swarm::ShardOf;

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
    shard_pieces: Option<&ShardPiecesView>,
) -> String {
    if owned.is_empty() {
        return String::new();
    }
    // A lane that is no shard has no pieces block — not a fallback, the absence of a section.
    let shard_block = match shard_pieces {
        Some(view) => shard_pieces_block(view),
        None => String::new(),
    };
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
         and extend it afterwards.{defect_block}{unowned_block}{shard_block}{excerpt_block}",
        listed.join("\n")
    )
}

/// VA-066: a shard's PIECES, measured for the judge.
///
/// A shard owns ONE file — `.swarm/shards/<module>/<shard>/README.md`, its handoff — and builds
/// its part as piece files beside it. `tree.rs`'s `SKIP_DIRS` holds `.swarm`, so the attempt's
/// fs_delta never flags those pieces (correct: they are the engine's staging, not the app), but
/// the delivery block above read only the owned README, so the judge never SAW them either: a
/// shard that had written six pieces read as "owns README.md — DOES NOT EXIST" and nothing else,
/// the exact shape the census exists to refute. This is the stat of that folder — names, bytes and
/// the same per-file parser the merger's dossier runs (`shards::parse_piece`; `None` = parses,
/// `Some("unchecked …")` = no parser for the extension, `Some(err)` = a parse error).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ShardPiecesView {
    pub(super) module: String,
    pub(super) shard: String,
    pub(super) folder: String,
    pub(super) pieces: Vec<(String, u64, Option<String>)>,
}

pub(super) async fn shard_pieces_view(working_dir: &Path, shard: &ShardOf) -> ShardPiecesView {
    let dir = working_dir.join(&shard.folder);
    let mut names: Vec<(String, u64)> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| {
            let name = e.file_name().to_str().map(String::from)?;
            (name != "README.md").then(|| (name, e.metadata().map(|m| m.len()).unwrap_or(0)))
        })
        .collect();
    names.sort();
    let mut pieces = Vec::with_capacity(names.len());
    for (name, bytes) in names {
        let verdict = super::shards::parse_piece(&dir.join(&name)).await;
        pieces.push((name, bytes, verdict));
    }
    ShardPiecesView {
        module: shard.module.clone(),
        shard: shard.shard.clone(),
        folder: shard.folder.clone(),
        pieces,
    }
}

fn shard_pieces_block(view: &ShardPiecesView) -> String {
    let listed = if view.pieces.is_empty() {
        "  (no piece files yet — the README alone is not the part; a shard composing its piece in          its head is the same failure as a task composing its file)"
            .to_string()
    } else {
        view.pieces
            .iter()
            .map(|(name, bytes, verdict)| {
                let parse = match verdict {
                    None => "parses".to_string(),
                    Some(e) if e.contains("unchecked") => e.clone(),
                    Some(e) => format!("PARSE ERROR: {e}"),
                };
                format!("  {}/{name} — {bytes} bytes — {parse}", view.folder)
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "\n\nTHIS TASK IS SHARD `{}` OF MODULE `{}`. Its deliverable is the PIECE FILES in `{}/`          (the README above is its handoff, not its part), and the tree stat cannot see that folder.          What is in it right now — a stat, not its claim:\n{listed}",
        view.shard, view.module, view.folder
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
        // goose-swarm `stub::skeleton_only`) — the same test belongs here, where it is cheap and early rather than at the end.
        if rel.ends_with(".py") {
            if let Ok(body) = std::fs::read_to_string(&path) {
                if goose_swarm::skeleton_only(&body) {
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
            for (_, raw) in html_dangling_refs(working_dir, rel) {
                out.push(format!("{rel} references `{raw}`, which does not exist"));
            }
        }
    }
    out
}

/// Every path any task DECLARED IT OWNS on this run, read back out of the tree's own event log.
///
/// `swarm verify` took its ownership list only from a hand-typed `--owns`, so replaying the verifier
/// over a corpus of archived runs could never reach the four detectors that iterate that list. The
/// engine now writes a `task_owns` row per dispatch; this reads them back, de-duplicated and sorted,
/// so a sweep verifies what the run actually built rather than what the operator remembered to type.
///
/// Best-effort by construction: an unreadable log, a run from before the event existed, or a tree with
/// no `.swarm` directory all yield an empty list, and the caller says so rather than printing "clean".
pub(super) fn owned_files_from_run_log(working_dir: &Path) -> Vec<String> {
    let mut out: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let Ok(rd) = std::fs::read_dir(working_dir.join(".swarm")) else {
        return Vec::new();
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(body) = std::fs::read_to_string(&p) else {
            continue;
        };
        for line in body.lines() {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            if v.get("event").and_then(|e| e.as_str()) != Some("task_owns") {
                continue;
            }
            for f in v
                .get("owned_files")
                .and_then(|f| f.as_array())
                .into_iter()
                .flatten()
                .filter_map(|f| f.as_str())
            {
                out.insert(f.to_string());
            }
        }
    }
    out.into_iter().collect()
}

/// Every dangling static reference in an HTML file, as (resolved tree path, raw ref) DATA — the
/// ownership routing in `lane_defect_view` needs the target as a path to match against the plan's
/// ownership map, never re-parsed out of a formatted line (the same rule that split
/// `tree_import_gaps` from `verify_tree_imports`). `verify_owned_files` formats today's line from
/// the raw ref, so the two views can never drift on WHICH refs are dangling.
fn html_dangling_refs(working_dir: &Path, rel: &str) -> Vec<(String, String)> {
    let path = working_dir.join(rel);
    let Ok(body) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for cap in body.split(['"', '\'']) {
        let c = cap.trim();
        if (c.ends_with(".js") || c.ends_with(".css")) && !c.starts_with("http") && !c.is_empty() {
            let target = path.parent().map(|p| p.join(c)).unwrap_or_default();
            if !target.exists() {
                out.push((resolve_ref_rel(rel, c), c.to_string()));
            }
        }
    }
    out
}

/// `viz.js` referenced from `web/index.html` IS `web/viz.js` in the plan's ownership vocabulary.
/// Lexical only (`.` dropped, `..` folded, a leading `/` resolves against the tree root the way a
/// static site would); a ref that escapes the tree keeps its raw shape and simply matches no
/// owner, which routes it to the honest "no task owns it" arm.
fn resolve_ref_rel(html_rel: &str, referenced: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if !referenced.starts_with('/') {
        // A root-level html has parent Some("") whose components are none, so an empty prefix
        // here honestly MEANS "the file sits at the tree root" — never a swallowed failure.
        if let Some(p) = Path::new(html_rel).parent() {
            parts = p
                .components()
                .filter_map(|c| c.as_os_str().to_str())
                .collect();
        }
    }
    for comp in referenced.split('/') {
        match comp {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            c => parts.push(c),
        }
    }
    parts.join("/")
}

/// THE LANE VIEW of `verify_owned_files`: what THIS task's judge and defect steer may act on.
///
/// MEASURED (r6c, run swarm-20260831-072930517 seq 1954): web-console's look-4 defect list opened
/// with "web/index.html references `viz.js`, which does not exist" — but `web/viz.js` is
/// web-viz's file (task_owns seq 1589), dispatched 12:01:05 and still in flight at the 13:15:14
/// steer. The steer's closing order, "Fix the first one before anything else, at that exact
/// path", therefore pointed the lane at a SIBLING's deliverable; an obedient lane writes it and
/// the one-owner-per-file invariant breaks on the SUPERVISOR's own words (r5's promote-discard
/// class, manufactured by the engine). The lane self-saved only because it chose to read viz.js
/// when it lands and align.
///
/// So: a dangling ref whose target another task owns becomes an honest do-not-write line naming
/// the owner and its measured state (`task_state_label`: ledger row, else dispatched=running,
/// else pending — derived from the DAG's ownership map, never hardcoded). A dangling ref the lane
/// itself owns keeps today's line — it is genuinely this lane's gap, and the fix-at-path order is
/// correct for it. A ref NO task owns also keeps today's line: a real gap worth naming. MILD by
/// construction — information reshaped, nothing suppressed, nothing refused.
pub(super) fn lane_defect_view(
    working_dir: &Path,
    lane_task: &str,
    owned: &[String],
    ownership: &HashMap<String, Vec<String>>,
    states: &HashMap<String, String>,
    dispatched: &HashSet<String>,
) -> Vec<String> {
    let mut out = verify_owned_files(working_dir, owned);
    for rel in owned.iter().filter(|r| r.ends_with(".html")) {
        for (resolved, raw) in html_dangling_refs(working_dir, rel) {
            let Some(owner) = ownership
                .iter()
                .find(|(t, files)| t.as_str() != lane_task && files.contains(&resolved))
                .map(|(t, _)| t.clone())
            else {
                continue;
            };
            let today = format!("{rel} references `{raw}`, which does not exist");
            out.retain(|l| *l != today);
            let honest = format!(
                "`{resolved}` is referenced by {rel} but owned by task {owner} (state: {}) — do \
                 not write it; align with it when it lands",
                super::imports::task_state_label(&owner, states, dispatched)
            );
            if !out.contains(&honest) {
                out.push(honest);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "goose-lane-view-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// The r6c seq-1954 shape verbatim: web-console owns web/index.html + web/app.js (missing);
    /// index.html references viz.js (web-viz's file, in flight) and app.js (its own). The
    /// sibling-owned ref must become the do-not-write line naming the real owner, and the
    /// fix-first order must land on a file the lane owns.
    #[test]
    fn a_sibling_owned_dangling_ref_reads_do_not_write_with_the_owner_named() {
        let d = tmp("sibling");
        std::fs::create_dir_all(d.join("web")).unwrap();
        std::fs::write(
            d.join("web/index.html"),
            "<script src=\"viz.js\"></script><script src=\"app.js\"></script>",
        )
        .unwrap();
        let owned = vec!["web/index.html".to_string(), "web/app.js".to_string()];
        let mut ownership: HashMap<String, Vec<String>> = HashMap::new();
        ownership.insert("web-console".into(), owned.clone());
        ownership.insert("web-viz".into(), vec!["web/viz.js".into()]);
        let states: HashMap<String, String> = HashMap::new();
        let dispatched: HashSet<String> = ["web-console", "web-viz"]
            .into_iter()
            .map(String::from)
            .collect();
        let view = lane_defect_view(&d, "web-console", &owned, &ownership, &states, &dispatched);
        let viz: Vec<&String> = view.iter().filter(|l| l.contains("viz.js")).collect();
        assert_eq!(viz.len(), 1, "{view:?}");
        assert!(viz[0].contains("owned by task web-viz"), "{view:?}");
        assert!(viz[0].contains("(state: running)"), "{view:?}");
        assert!(viz[0].contains("do not write it"), "{view:?}");
        // The lane's OWN dangling ref keeps today's fix-at-path line...
        assert!(
            view.iter()
                .any(|l| l == "web/index.html references `app.js`, which does not exist"),
            "{view:?}"
        );
        // ...and the first defect the steer's "Fix the first one" order lands on is the lane's.
        assert!(!view[0].contains("viz.js"), "{view:?}");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A dangling ref NO task owns keeps today's text — a genuine gap worth naming as-is.
    #[test]
    fn an_unowned_dangling_ref_keeps_todays_text() {
        let d = tmp("unowned");
        std::fs::create_dir_all(d.join("web")).unwrap();
        std::fs::write(d.join("web/index.html"), "<script src=\"lib.js\"></script>").unwrap();
        let owned = vec!["web/index.html".to_string()];
        let mut ownership: HashMap<String, Vec<String>> = HashMap::new();
        ownership.insert("web-console".into(), owned.clone());
        let states: HashMap<String, String> = HashMap::new();
        let dispatched: HashSet<String> = ["web-console"].into_iter().map(String::from).collect();
        let view = lane_defect_view(&d, "web-console", &owned, &ownership, &states, &dispatched);
        assert!(
            view.iter()
                .any(|l| l == "web/index.html references `lib.js`, which does not exist"),
            "{view:?}"
        );
        assert!(!view.iter().any(|l| l.contains("do not write")), "{view:?}");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The sibling's state comes from the ledger when a terminal row exists — the honest line
    /// must say what the DAG measured, never a hardcoded "in flight".
    #[test]
    fn the_owners_measured_state_rides_the_do_not_write_line() {
        let d = tmp("state");
        std::fs::create_dir_all(d.join("web")).unwrap();
        std::fs::write(d.join("web/index.html"), "<link href=\"theme.css\">").unwrap();
        let owned = vec!["web/index.html".to_string()];
        let mut ownership: HashMap<String, Vec<String>> = HashMap::new();
        ownership.insert("web-console".into(), owned.clone());
        ownership.insert("web-theme".into(), vec!["web/theme.css".into()]);
        let mut states: HashMap<String, String> = HashMap::new();
        states.insert("web-theme".into(), "failed".into());
        let dispatched: HashSet<String> = ["web-console", "web-theme"]
            .into_iter()
            .map(String::from)
            .collect();
        let view = lane_defect_view(&d, "web-console", &owned, &ownership, &states, &dispatched);
        assert!(
            view.iter()
                .any(|l| l.contains("owned by task web-theme (state: failed)")),
            "{view:?}"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Four of the verifier's five detectors iterate the owned list, so a corpus sweep with no `--owns`
    /// could only ever run the import check — and printed the flat word "clean".
    #[test]
    fn the_owned_files_a_run_declared_can_be_read_back_out_of_its_log() {
        let dir = tmp("runlog");
        std::fs::create_dir_all(dir.join(".swarm")).unwrap();
        std::fs::write(
            dir.join(".swarm/run.jsonl"),
            "{\"event\":\"phase\",\"phase\":\"build\"}\n\
             {\"event\":\"task_owns\",\"task_id\":\"store\",\"owned_files\":[\"app/store.py\"]}\n\
             {\"event\":\"task_owns\",\"task_id\":\"store\",\"owned_files\":[\"app/store.py\"]}\n\
             {\"event\":\"task_owns\",\"task_id\":\"api\",\"owned_files\":[\"app/api.py\"]}\n\
             not json at all\n",
        )
        .unwrap();
        assert_eq!(
            owned_files_from_run_log(&dir),
            vec!["app/api.py".to_string(), "app/store.py".to_string()],
            "de-duplicated, sorted, and unbothered by a truncated line"
        );
        assert!(
            owned_files_from_run_log(&tmp("runlog-empty")).is_empty(),
            "a tree with no .swarm directory yields nothing, and the caller must SAY so"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// N-7: the judge's delivery view is a measurement, and each of its four parts must actually
    /// reach the prompt text. The r2 shape is pinned end to end: an owned file with a syntax error
    /// shows the census line AND the py_compile fact; an owned path with nothing on disk SAYS so;
    /// and a same-named file written where nobody owns one is called out as a WRONG-PATH fact —
    /// the camera-system defect the r2 judge okayed because it read only reasoning.
    #[test]
    fn judge_delivery_block_carries_census_parse_state_and_wrong_path() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("app")).unwrap();
        std::fs::write(dir.path().join("app/store.py"), "def broken(:\n    pass\n").unwrap();
        let owned = vec!["app/store.py".to_string(), "app/api.py".to_string()];
        let defects = verify_owned_files(dir.path(), &owned);
        let block = judge_delivery_block(dir.path(), &owned, &defects, &[], None);
        assert!(
            block.contains("app/store.py — EXISTS,"),
            "census names what is on disk: {block}"
        );
        assert!(
            block.contains("app/api.py — DOES NOT EXIST"),
            "a task with nothing on disk says so: {block}"
        );
        assert!(
            block.contains("app/store.py DOES NOT PARSE"),
            "the py_compile fact reaches the judge's user text: {block}"
        );
        assert!(
            block.contains("What `app/store.py` holds right now"),
            "a budgeted content excerpt is included: {block}"
        );

        // The r2 camera-system shape: the owned path is missing while a file with the SAME NAME
        // appeared at the tree root during this attempt's window.
        let owned = vec!["web/viz_camera.js".to_string()];
        let block = judge_delivery_block(
            dir.path(),
            &owned,
            &[
                "web/viz_camera.js does not exist — this task owns it and nothing has written it"
                    .to_string(),
            ],
            &["viz_camera.js".to_string()],
            None,
        );
        assert!(
            block.contains("NO PLANNED TASK OWNS") && block.contains("viz_camera.js"),
            "the window's unowned writes are in the prompt: {block}"
        );
        assert!(
            block.contains("WRONG PATH") && block.contains("move it to `web/viz_camera.js`"),
            "a basename match is stated as a misplaced deliverable with the exact owned path: {block}"
        );

        // A task owning nothing gets no block at all — planning lanes are covered by the
        // structured-reply block, and an empty census would misread them as undelivered builds.
        assert_eq!(judge_delivery_block(dir.path(), &[], &[], &[], None), "");
    }

    /// N-7: the excerpt honours its budget and says when it cut — a file larger than the per-file
    /// cap must arrive marked as an excerpt, never looking complete (the same honesty rule the
    /// scheduler judge's 1800-char cut learned the hard way).
    #[test]
    fn judge_delivery_excerpt_is_budgeted_and_admits_the_cut() {
        let dir = tempfile::tempdir().unwrap();
        let big = format!("SEED = 1\n{}", "x = 2\n".repeat(2_000));
        std::fs::write(dir.path().join("big.py"), &big).unwrap();
        let owned = vec!["big.py".to_string()];
        let block = judge_delivery_block(dir.path(), &owned, &[], &[], None);
        assert!(
            block.contains("… (cut — an excerpt"),
            "a cut excerpt admits it: {block}"
        );
        let excerpt = block.split("holds right now").nth(1).unwrap();
        assert!(
            excerpt.chars().count() < 2_000,
            "the excerpt respects the per-file budget, got {} chars",
            excerpt.chars().count()
        );
    }

    /// VA-066: a shard lane's delivery view names its PIECES with bytes and the parse verdict, and
    /// says when the folder is empty — the README alone is not the part.
    #[test]
    fn a_shard_lanes_delivery_block_lists_its_pieces_or_says_the_folder_is_empty() {
        let d = tmp("shard-pieces");
        std::fs::create_dir_all(d.join(".swarm/shards/web-viz/render")).unwrap();
        std::fs::write(
            d.join(".swarm/shards/web-viz/render/README.md"),
            "PROVIDES: buildScene\nASSUMES: none\nUNFINISHED: none\nCHECKED_WITH: none\n",
        )
        .unwrap();
        let owned = vec![".swarm/shards/web-viz/render/README.md".to_string()];
        let view = ShardPiecesView {
            module: "web-viz".into(),
            shard: "render".into(),
            folder: ".swarm/shards/web-viz/render".into(),
            pieces: vec![
                ("render.js".into(), 1_204, None),
                (
                    "scene.js".into(),
                    88,
                    Some("SyntaxError: Unexpected token".into()),
                ),
                ("notes.txt".into(), 12, Some("unchecked (txt)".into())),
            ],
        };
        let block = judge_delivery_block(&d, &owned, &[], &[], Some(&view));
        assert!(
            block.contains("THIS TASK IS SHARD `render` OF MODULE `web-viz`"),
            "{block}"
        );
        assert!(
            block.contains(".swarm/shards/web-viz/render/render.js — 1204 bytes — parses"),
            "{block}"
        );
        assert!(
            block.contains("scene.js — 88 bytes — PARSE ERROR: SyntaxError"),
            "{block}"
        );
        assert!(
            block.contains("notes.txt — 12 bytes — unchecked (txt)"),
            "{block}"
        );
        let empty = ShardPiecesView {
            pieces: vec![],
            ..view.clone()
        };
        let block = judge_delivery_block(&d, &owned, &[], &[], Some(&empty));
        assert!(
            block.contains("no piece files yet — the README alone is not the part"),
            "{block}"
        );
        // A build lane that is no shard reads exactly as before.
        let block = judge_delivery_block(&d, &owned, &[], &[], None);
        assert!(!block.contains("THIS TASK IS SHARD"), "{block}");
    }

    /// The view is a stat of the folder: names sorted, bytes measured, README excluded, an
    /// extension no parser covers said as unchecked.
    #[tokio::test]
    async fn the_pieces_view_stats_the_folder_and_skips_the_readme() {
        let d = tmp("shard-view");
        std::fs::create_dir_all(d.join(".swarm/shards/web-viz/pick")).unwrap();
        std::fs::write(
            d.join(".swarm/shards/web-viz/pick/README.md"),
            "PROVIDES: x\n",
        )
        .unwrap();
        std::fs::write(d.join(".swarm/shards/web-viz/pick/zeta.txt"), "12345").unwrap();
        std::fs::write(d.join(".swarm/shards/web-viz/pick/alpha.txt"), "1").unwrap();
        let shard = ShardOf {
            module: "web-viz".into(),
            shard: "pick".into(),
            folder: ".swarm/shards/web-viz/pick".into(),
            ..Default::default()
        };
        let view = shard_pieces_view(&d, &shard).await;
        assert_eq!(view.pieces.len(), 2, "{view:?}");
        assert_eq!(view.pieces[0].0, "alpha.txt");
        assert_eq!(view.pieces[0].1, 1);
        assert_eq!(view.pieces[1].0, "zeta.txt");
        assert_eq!(view.pieces[1].1, 5);
        assert!(
            view.pieces
                .iter()
                .all(|(_, _, v)| v.as_deref().is_some_and(|v| v.contains("unchecked"))),
            "{view:?}"
        );
    }
}
