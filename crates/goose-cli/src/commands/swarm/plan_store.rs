//! The plan's durable forms — the sidecar writers (OUT) and the resume reader (IN) — in one file
//! so the load-bearing rule is visible on one screen: **the sidecars have NO engine reader.**
//!
//! OUT — `.swarm/plan.json` is written at the `plan_synthesized` seam: the full parsed plan the
//! moment it exists, complete briefs included. `.swarm/plan-loaded.json` is the post-REVIEW/
//! patched/repaired form the DAG actually loaded, written only when its bytes differ from the
//! synthesized stage, so diffing the two files shows exactly what review and the repairs changed.
//! WHY: r6c carried 133k chars of briefs that were persisted NOWHERE between `plan_synthesized`
//! (which records only counts) and `plan_loaded` — calls.jsonl truncated the synthesis
//! final_output mid-word ("depend…"), so the vigil could not audit a single brief until BUILD.
//! These files are PURE SIDECARS for the operator/vigil: nothing in the engine reads them back,
//! and nothing may ever — resume (below, the whole IN half) replays the run's own
//! `run-swarm-*.jsonl` `plan_loaded` events, never these files. Losing a sidecar loses
//! auditability, never work, which is why a failed write is a loud `plan_persist_failed` event
//! and a continued run — never a stop, never silence.
//!
//! IN — `ResumeState` / `resume_state_from_dir` / `resume_state_from_log`, moved here verbatim
//! from swarm.rs (the incremental-split law's payment for the sidecar wiring).

use goose_swarm::EventSink;
use std::path::Path;

/// tmp+rename write of one plan sidecar under `<working_dir>/.swarm/`. Same-directory rename so
/// the swap is atomic in-dir (the forming-sidecar pattern in supervision.rs): a reader mid-poll
/// sees the old file or the new one, never a torn write. A failed write emits the named
/// `plan_persist_failed { path, error }` event — gate 1: the absence is stated, never silent —
/// and returns, because the sidecar is not load-bearing and must never cost the run.
pub(super) fn persist_plan_sidecar(
    working_dir: &Path,
    name: &str,
    plan_json: &str,
    sink: &dyn EventSink,
) {
    let dir = working_dir.join(".swarm");
    let path = dir.join(name);
    let write = || -> std::io::Result<()> {
        std::fs::create_dir_all(&dir)?;
        let mut tmp_os = path.as_os_str().to_os_string();
        tmp_os.push(".tmp");
        let tmp = std::path::PathBuf::from(tmp_os);
        std::fs::write(&tmp, plan_json)?;
        std::fs::rename(&tmp, &path)
    };
    if let Err(e) = write() {
        sink.write_value(serde_json::json!({
            "event": "plan_persist_failed",
            "path": path.display().to_string(),
            "error": e.to_string(),
        }));
        eprintln!(
            "  ! plan sidecar write failed ({e}) — {} will be missing; the run continues",
            path.display()
        );
    }
}

/// The second sidecar stage, at the `plan_loaded` seam. Skipped only when byte-identical to
/// `.swarm/plan.json`: equal bytes mean REVIEW and the repairs changed nothing, and a second
/// copy would say nothing. Any difference writes — and so does an absent or unreadable
/// `plan.json` (a RESUMED run never ran synthesis this time, so there is no synthesized stage
/// to compare against; the final form the DAG loaded is still worth having). The unreadable arm
/// is deliberately the WRITING arm, not a quiet skip: a broken plan.json must never suppress
/// the one copy the vigil can still get.
pub(super) fn persist_plan_loaded_sidecar(
    working_dir: &Path,
    plan_json: &str,
    sink: &dyn EventSink,
) {
    if std::fs::read_to_string(working_dir.join(".swarm").join("plan.json"))
        .is_ok_and(|synthesized| synthesized == plan_json)
    {
        return;
    }
    persist_plan_sidecar(working_dir, "plan-loaded.json", plan_json, sink);
}

/// What a previous run of this directory got through, so a new one need not redo it.
///
/// WHY THIS EXISTS: Mihai powered off a machine mid-run twice in one day and lost ~2.5h each time. There is
/// no way to say "stop, I need my hardware" without destroying the run. His scope, verbatim: "don't think of
/// making it TRUE, it just needs to resume from SOME point, whatever point it is."
///
/// That makes it small, because BOTH halves are already durable in the run's own jsonl:
///   plan_loaded.tasks     — the whole DAG, ids + deps + files. So a resume skips research AND planning,
///                           which measured 119 of 152 minutes on the baseline: 78% of the run.
///   task_completed.task_id — every task that finished.
///
/// THE RULE THAT MAKES IT SAFE: only a task with a task_completed event is skipped. Anything ambiguous —
/// in flight when the power went, half-written, never dispatched — simply RE-RUNS and overwrites its own
/// files. So the failure mode is "we redo a little work", never "we silently skipped something". That
/// direction is the whole reason this needs no corruption detection.
#[derive(Debug, Clone)]
pub(super) struct ResumeState {
    /// The plan_loaded event's task array, verbatim — the same shape Dag::from_planner_json already parses.
    pub(super) plan_json: String,
    /// Tasks the previous run finished. REPORTED, not skipped — see the RESUME wiring in swarm.rs for why
    /// that is deliberate and why it is the safe half of this feature.
    pub(super) completed: std::collections::HashSet<String>,
}

/// Read the newest run log in `<dir>/.swarm` and recover what it finished.
///
/// Returns None when there is nothing to resume FROM (no log, no plan_loaded) or nothing to resume INTO
/// (the run already emitted run_finished — a finished run is not a candidate, resuming it would rebuild an
/// app that is already there).
/// STATUS 2026-08-22: RESUME WORKS. It never did before — this rebuilt the plan under `tasks`
/// while `Dag::from_planner_json` (its only consumer) requires `subtasks`, so every resume exited
/// with "the resumed plan will not parse: missing field `subtasks`" after paying a full scout
/// phase. Both keys are emitted now and `a_resumed_plan_parses_into_a_dag` pins it: if that test
/// is green, a recovered plan parses. Do not "re-discover" the old breakage from the ledger.
pub(super) fn resume_state_from_dir(dir: &std::path::Path) -> Option<ResumeState> {
    let mut logs: Vec<_> = std::fs::read_dir(dir.join(".swarm"))
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("run-swarm-") && n.ends_with(".jsonl"))
        })
        .collect();
    logs.sort();
    // NEWEST FIRST, and skip the ones with nothing in them.
    //
    // This took `logs.last()` — and by the time it runs, the CURRENT run has already created its own
    // log in this same directory, so `last()` is that empty file and resume returned None every single
    // time. The bench path escaped it only because run_build.py redirects the live log out of `.swarm`
    // entirely; from a desktop or a plain CLI run, resume has never once worked.
    //
    // A log with no `plan_loaded` in it yields None from the pure half, so walking backwards until one
    // parses is both the fix and the general rule: resume from the most recent run that got far enough
    // to have something to resume.
    for path in logs.iter().rev() {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        if let Some(state) = resume_state_from_log(&text) {
            return Some(state);
        }
    }
    None
}

/// The pure half: given a run log's text, what can be resumed? Separate so it is testable against the REAL
/// logs on disk without a filesystem fixture.
fn resume_state_from_log(text: &str) -> Option<ResumeState> {
    let mut plan_json = None;
    let mut completed = std::collections::HashSet::new();
    for line in text.lines() {
        let Ok(e) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        match e.get("event").and_then(|x| x.as_str()) {
            // A run that FINISHED has nothing to resume — its app exists.
            Some("run_finished") => return None,
            // LAST plan_loaded wins: a run that re-planned mid-flight ends on the plan it actually built.
            Some("plan_loaded") => {
                if let Some(t) = e.get("tasks") {
                    // BOTH KEYS, because the two readers disagreed and resume could never work:
                    // `Dag::from_planner_json` (the consumer) requires `subtasks` — the planner's
                    // own field name — while this rebuilt the plan under `tasks`, the name the
                    // plan_loaded EVENT uses. MEASURED: every resume died instantly with "the
                    // resumed plan will not parse: missing field `subtasks`", after paying the
                    // full scout phase, and the harness then scored the unbuilt tree. The banner
                    // that counts tasks reads `tasks`, so both names are emitted rather than
                    // renaming one and breaking the other.
                    plan_json = Some(serde_json::json!({ "subtasks": t, "tasks": t }).to_string());
                }
            }
            Some("task_completed") => {
                if let Some(id) = e.get("task_id").and_then(|x| x.as_str()) {
                    completed.insert(id.to_string());
                }
            }
            _ => {}
        }
    }
    Some(ResumeState {
        plan_json: plan_json?,
        completed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use goose_swarm::{Dag, SwarmEvent};
    use std::sync::Mutex;

    #[derive(Default)]
    struct ValueSink(Mutex<Vec<serde_json::Value>>);
    impl EventSink for ValueSink {
        fn emit(&self, _event: &SwarmEvent) {}
        fn write_value(&self, value: serde_json::Value) {
            self.0.lock().unwrap().push(value);
        }
    }

    /// The r6c gap this module closes: 133k chars of briefs existed only inside a truncated
    /// calls.jsonl row between `plan_synthesized` and BUILD. The sidecar must carry the plan
    /// BYTE-IDENTICAL — briefs intact, nothing summarized — and leave no tmp behind.
    #[test]
    fn the_synthesized_plan_sidecar_lands_with_briefs_intact() {
        let dir = tempfile::tempdir().unwrap();
        let sink = ValueSink::default();
        let brief = format!(
            "Implement app/ledger_core.py: class Ledger with post(entry) and balance(account). {}",
            "The full module specification continues at length. ".repeat(200)
        );
        let plan = serde_json::json!({"subtasks": [
            {"id": "ledger", "files": ["app/ledger_core.py"], "depends_on": [], "description": brief},
        ]})
        .to_string();
        persist_plan_sidecar(dir.path(), "plan.json", &plan, &sink);
        let on_disk = std::fs::read_to_string(dir.path().join(".swarm/plan.json")).unwrap();
        assert_eq!(
            on_disk, plan,
            "the sidecar must be the plan, byte-identical"
        );
        assert!(on_disk.contains("class Ledger with post(entry)"));
        assert!(
            !dir.path().join(".swarm/plan.json.tmp").exists(),
            "the tmp must be renamed away"
        );
        assert!(
            sink.0.lock().unwrap().is_empty(),
            "a clean write emits nothing"
        );
    }

    /// Gate 1's shape for this writer: the failure is a NAMED event carrying the path and the
    /// error, and the function RETURNS — the run continues, because the sidecar is not
    /// load-bearing (resume reads the run log, never this file).
    #[test]
    fn a_failed_persist_emits_plan_persist_failed_and_the_run_continues() {
        let dir = tempfile::tempdir().unwrap();
        // A regular FILE where the working dir should be: create_dir_all(".swarm") must fail.
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, "not a directory").unwrap();
        let sink = ValueSink::default();
        persist_plan_sidecar(&blocker, "plan.json", "{\"subtasks\":[]}", &sink);
        let events = sink.0.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["event"], "plan_persist_failed");
        assert!(
            events[0]["path"].as_str().unwrap().ends_with("plan.json"),
            "the event names the path that is missing: {}",
            events[0]
        );
        assert!(
            !events[0]["error"].as_str().unwrap().is_empty(),
            "the event carries the real io error"
        );
    }

    /// plan-loaded.json exists exactly when it says something: skipped on identical bytes
    /// (review changed nothing), written on a difference, and written when plan.json is absent
    /// (the resume path never ran synthesis this run).
    #[test]
    fn plan_loaded_sidecar_writes_only_when_it_differs_from_the_synthesized_stage() {
        let dir = tempfile::tempdir().unwrap();
        let sink = ValueSink::default();
        let synthesized = r#"{"subtasks":[{"id":"a","files":["a.py"],"depends_on":[]}]}"#;
        persist_plan_sidecar(dir.path(), "plan.json", synthesized, &sink);

        persist_plan_loaded_sidecar(dir.path(), synthesized, &sink);
        assert!(
            !dir.path().join(".swarm/plan-loaded.json").exists(),
            "identical bytes mean review changed nothing — no second copy"
        );

        let repaired = r#"{"subtasks":[{"id":"a","files":["a.py","b.py"],"depends_on":[]}]}"#;
        persist_plan_loaded_sidecar(dir.path(), repaired, &sink);
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".swarm/plan-loaded.json")).unwrap(),
            repaired
        );

        let resumed_dir = tempfile::tempdir().unwrap();
        persist_plan_loaded_sidecar(resumed_dir.path(), repaired, &sink);
        assert_eq!(
            std::fs::read_to_string(resumed_dir.path().join(".swarm/plan-loaded.json")).unwrap(),
            repaired,
            "with no synthesized stage on disk the loaded form must still land"
        );
    }

    #[test]
    fn a_resumed_plan_parses_into_a_dag() {
        // The bug this pins: resume rebuilt the plan under "tasks" while Dag::from_planner_json
        // requires "subtasks", so EVERY resume exited with "missing field `subtasks`" after
        // paying the whole scout phase.
        let dir = tempfile::tempdir().unwrap();
        let sw = dir.path().join(".swarm");
        std::fs::create_dir_all(&sw).unwrap();
        let plan_loaded = serde_json::json!({
            "event": "plan_loaded",
            "tasks": [
                {"id": "core", "description": "build core", "files": ["app/core.py"],
                 "depends_on": [], "difficulty": "easy"},
                {"id": "api", "description": "build api", "files": ["app/api.py"],
                 "depends_on": ["core"], "difficulty": "hard"}
            ]
        });
        let log = format!(
            "{}\n{}\n",
            plan_loaded,
            serde_json::json!({"event": "task_completed", "task_id": "core", "status": "done"})
        );
        std::fs::write(sw.join("run-swarm-00-resumed.jsonl"), log).unwrap();
        let r = resume_state_from_dir(dir.path()).expect("resume state must be recovered");
        assert!(r.completed.contains("core"));
        Dag::from_planner_json(&r.plan_json)
            .expect("a resumed plan MUST parse into a Dag — this is the whole point of resume");
    }

    /// Resume, against the shapes real runs actually produce.
    ///
    /// The bar Mihai set is deliberately low — "it just needs to resume from SOME point, whatever point it
    /// is" — so the only thing that MUST hold is the conservative rule: a task is skipped ONLY if it has a
    /// task_completed event. Everything else re-runs and overwrites its own files. Getting that backwards
    /// would silently skip unfinished work, which is exactly the false-green class this loop exists to hunt.
    #[test]
    fn resume_skips_only_what_provably_finished() {
        let log = [
            r#"{"event":"run_started","prompt":"build it"}"#,
            r#"{"event":"plan_loaded","plan_confidence":52,"tasks":[{"id":"db","deps":[]},{"id":"api","deps":["db"]},{"id":"ui","deps":[]}]}"#,
            r#"{"event":"task_completed","task_id":"db","elapsed_ms":1000}"#,
            r#"not json at all"#,
            r#"{"event":"task_completed","task_id":"ui","elapsed_ms":2000}"#,
        ]
        .join("\n");
        let r = resume_state_from_log(&log).expect("a crashed run with a plan is resumable");
        assert_eq!(r.completed.len(), 2);
        assert!(r.completed.contains("db") && r.completed.contains("ui"));
        // `api` was mid-flight when the power went. It has no task_completed, so it MUST re-run.
        assert!(
            !r.completed.contains("api"),
            "a task without task_completed must never be skipped"
        );
        // The plan survives verbatim in the shape Dag::from_planner_json parses.
        let v: serde_json::Value = serde_json::from_str(&r.plan_json).unwrap();
        assert_eq!(v["tasks"].as_array().unwrap().len(), 3);

        // A FINISHED run is not a resume candidate — its app already exists.
        let finished = format!("{log}\n{}", r#"{"event":"run_finished","report":{}}"#);
        assert!(
            resume_state_from_log(&finished).is_none(),
            "a finished run must not be resumed"
        );

        // A run that died BEFORE planning has nothing to resume from — research/plan must run again.
        let early = r#"{"event":"run_started"}
{"event":"research_completed","findings":3}"#;
        assert!(resume_state_from_log(early).is_none());

        // A re-planned run ends on the plan it actually BUILT: the last plan_loaded wins.
        let replanned = [
            r#"{"event":"plan_loaded","tasks":[{"id":"old","deps":[]}]}"#,
            r#"{"event":"plan_loaded","tasks":[{"id":"new-a","deps":[]},{"id":"new-b","deps":[]}]}"#,
        ]
        .join("\n");
        let r2 = resume_state_from_log(&replanned).unwrap();
        let v2: serde_json::Value = serde_json::from_str(&r2.plan_json).unwrap();
        assert_eq!(v2["tasks"].as_array().unwrap().len(), 2);
        assert_eq!(v2["tasks"][0]["id"], "new-a");

        assert!(resume_state_from_log("").is_none());
    }

    #[test]
    /// THE BUG THAT MADE RESUME DEAD EVERYWHERE BUT THE BENCH.
    ///
    /// `resume_state_from_dir` took `logs.last()`, and by the time it runs the CURRENT run has already
    /// created its own (empty) log in the same directory — so `last()` was always that empty file and
    /// resume returned None from a desktop or CLI run every time. The bench escaped it only because
    /// run_build.py redirects the live log elsewhere. This writes exactly that arrangement: a prior run
    /// with a plan, and a newer empty log beside it.
    fn resume_ignores_the_current_runs_own_empty_log() {
        let dir = tempfile::tempdir().expect("tempdir");
        let swarm = dir.path().join(".swarm");
        std::fs::create_dir_all(&swarm).expect("mkdir");
        std::fs::write(
            swarm.join("run-swarm-20260101-000000000.jsonl"),
            r#"{"event":"plan_loaded","tasks":[{"id":"core","depends_on":[]}]}
{"event":"task_completed","task_id":"core"}
"#,
        )
        .expect("prior log");
        // Sorts AFTER the prior log, exactly as a freshly-created live log does.
        std::fs::write(swarm.join("run-swarm-20260102-000000000.jsonl"), "").expect("empty log");
        let state = resume_state_from_dir(dir.path()).expect("the prior run is still resumable");
        assert!(
            state.plan_json.contains("\"id\":\"core\""),
            "the prior plan must come back, not an empty one"
        );
        assert!(state.completed.contains("core"));
    }

    /// The same reader, against the REAL logs this machine has produced — a fixture can agree with a bug.
    #[test]
    fn resume_reads_the_real_run_logs() {
        let home = std::env::var("HOME").unwrap_or_default();
        let finished = std::path::PathBuf::from(&home).join("goose-builds/loop-ab-baseline/.swarm");
        if !finished.is_dir() {
            return; // not this machine — the unit test above still pins the logic
        }
        // loop-ab-baseline COMPLETED (run_finished present) => must NOT be resumable.
        let dir = std::path::PathBuf::from(&home).join("goose-builds/loop-ab-baseline");
        assert!(
            resume_state_from_dir(&dir).is_none(),
            "a run with run_finished must never be offered as resumable"
        );
    }
}
