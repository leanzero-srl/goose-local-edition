//! REPAIR SHARDS THAT FIX IN PARALLEL (VA-022 + VA-020, 2c S5; Mihai 2026-09-01: "Why can't we
//! edit a file in parallel? WHY? What stops us?").
//!
//! What stopped us, measured: findings were grouped BY FILE (`group_findings_by_file`), so r5 put
//! six findings on one shard and worked them serially; rounds were BARRIERS (`fanout_over_fleet`
//! joins every shard — r6c round 1 waited 125.6 minutes on one shard while two nodes idled);
//! promotion was whole-file strictly-better (`shard_beats_baseline` on a preview that OVERWROTE
//! the owned file), so two fixes to one file could only race and the loser's work was discarded;
//! an edit to a non-owned file was dropped silently at promote (r5 `__main__` r1 → httpapi.py;
//! r6c app.js r1 → drafts.py; `verified: null`).
//!
//! What this module does instead:
//! (a) ONE SHARD PER FINDING (`explode_groups`): an attributed finding's shard owns the files its
//!     attribution names (the file group's resolved ownership — `resolve_shard_ownership`, reused);
//!     several findings on one file are several shards that may run at once.
//! (b) EACH SHARD IS A DIFF against its dispatch-time base: the shadow tree is the shard's private
//!     copy (as before); at promotion every owned file is merged THREE-WAY — base (the tree at the
//!     shard's dispatch, snapshotted in `make_shadow`), ours (the shard's file), theirs (the tree
//!     NOW, which siblings may have advanced) — with `git merge-file`. WHY git merge-file: it is a
//!     real hunk-level merger present wherever this engine runs (the harness needs git), it applies
//!     non-overlapping hunks correctly in either arrival order, and it MARKS the overlapping ones,
//!     so the conflict can be quoted verbatim back to the finding's re-dispatch instead of being
//!     resolved by a second, weaker diff engine of our own. A conflicting hunk promotes nothing:
//!     the finding is re-dispatched immediately on the merged base with the conflict quoted.
//!     The tree is RE-GRADED after each promotion (`one_ruler_grade`, per CHECK) so the next shard
//!     is judged against the tree it will actually land on — never the round's opening grade.
//! (c) NO ROUND BARRIER: findings dispatch as nodes free (a slot pool, `JoinSet`); a handoff naming
//!     a non-owned tree file re-shards that finding at once with the file added
//!     (`handoff_reshard{finding, file}`); a finding that was tried against THIS tree and promoted
//!     nothing waits for the tree to change (progress, not a count). The wave ends when no finding
//!     is dispatchable and no shard runs; the outer loop's `complete_fix_converged` (existing) ends
//!     the phase when a wave changed nothing on the tree.
//! (d) REPAIR v2 (VA-087, DESIGN-REPAIR-V2 §1/§2/§6). The finding's CHECK (findings.rs
//!     `FindingCheck`) is the shard's first action and the promoter's ruler:
//!     - the brief OPENS with the gate's own replay command and the localization the evidence
//!       already names (`repro_block`); the shard's durable `<task>.calls.jsonl` says whether the
//!       check was re-run before the first edit — `repro_confirmed` / `edit_before_repro` /
//!       `repro_never_ran`, said, never blocked (r5's `__main__.py` shard: 70 samples, 0 changed,
//!       first_change_secs null — 70 minutes without one byte, nothing said which);
//!     - a preview is PROMOTED ON THE FLIP (`decide_promotion`): the gate re-run on the merged
//!       preview fails the finding's own check FEWER times than the tree now AND fails no check
//!       more — `finding_flipped` / `finding_still_failing{quote}` / `preview_regressed`. The
//!       count-strictly-lower rule (`shard_beats_baseline`) survives ONLY for a finding with no
//!       authoring check, labelled `finding_unverifiable` (r6c: the `web/viz.js` shard sent for
//!       `TypeError: Illegal invocation` was promoted 9→8 for closing a DOM id while the
//!       exception stood in the next verify, verbatim).
//!
//! Sibling module under the incremental-split law; the per-file fan's closure body moved here
//! from swarm.rs's wave loop, which now calls `run_wave`; `one_ruler_grade` moved here with the
//! flip (the wave is its only consumer) and returns the graded findings, not a count.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use goose_swarm::{DispatchRequest, EventSink, TaskDispatcher};

use super::attribution::parse_handoffs;
use super::decisions::BriefDecisions;
use super::findings::{
    dedupe_findings_exact, missing_deliverable_finding, parse_finding_verdicts, FileGroup,
    FindingCheck, FindingProvenance, FindingSource,
};
use super::fleet_order::{order_fleet_by_speed, resolved_fleet_speed_weights};
use super::ledger_block::read_ledger_rollup;
use super::ledger_writers::{write_repair_ledger, RepairLedgerRow};
use super::transcripts::read_calls_capture;
use super::{
    activity_digest_key, app_scope_py, copy_created_source_files, copy_tree_excluding,
    cross_module_drift, css_coherence_scan, dom_id_scan, http_timeout_scan,
    is_intentional_empty_marker, is_test_path, load_config, render_repair_history, run_smoke_gate,
    run_spec_contract, shard_beats_baseline, smoke_fix_description, spawn_fix_progress_sampler,
    swarm_gate_cfg, FixAttemptProgress, GooseAgentDispatcher, TargetLang,
};

/// One open finding and the shard that will work it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct OpenFinding {
    pub(super) text: String,
    /// The check that produced it (findings.rs `FindingProvenance::check_of`): the brief's first
    /// action and the promoter's ruler. None = no authoring check recorded → `finding_unverifiable`.
    pub(super) check: Option<FindingCheck>,
    /// The attributed file (the shard's name) and the files the shard owns.
    pub(super) file: String,
    pub(super) owned: Vec<String>,
    pub(super) order_note: String,
    /// Index within its file, so `complete-fix::<file>#<k>` is unique per wave.
    pub(super) k: usize,
    /// The tree version this finding was last tried against and promoted nothing; None = never
    /// tried, or re-armed (conflict / reshard) — dispatchable now.
    pub(super) attempted_at: Option<u64>,
    /// The quoted conflict hunks from a promotion that overlapped a sibling's landed fix.
    pub(super) conflict_note: Option<String>,
    /// S5d (ii): the finding came from a server-response probe or the render gate, so a NOT REAL
    /// verdict must quote the replayed request and response or it is `dismissed_without_replay`.
    pub(super) replay_required: bool,
}

/// (a) One shard per FINDING: every finding of a file group becomes its own OpenFinding carrying
/// the group's resolved ownership and order note.
pub(super) fn explode_groups(
    groups: &[FileGroup],
    owned_by_shard: &[Vec<String>],
    order_notes: &[String],
    replay_required: &dyn Fn(&str) -> bool,
    check_of: &dyn Fn(&str) -> Option<FindingCheck>,
) -> Vec<OpenFinding> {
    let mut out = Vec::new();
    for (i, g) in groups.iter().enumerate() {
        let owned = owned_by_shard
            .get(i)
            .cloned()
            .unwrap_or_else(|| vec![g.file.clone()]);
        let note = order_notes.get(i).cloned().unwrap_or_default();
        for (k, f) in g.findings.iter().enumerate() {
            out.push(OpenFinding {
                text: f.clone(),
                check: check_of(f),
                file: g.file.clone(),
                owned: owned.clone(),
                order_note: note.clone(),
                k,
                attempted_at: None,
                conflict_note: None,
                replay_required: replay_required(f),
            });
        }
    }
    out
}

/// The result of merging one owned file three-way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Merge {
    Clean(Vec<u8>),
    /// The merged text WITH markers, and each conflict hunk verbatim.
    Conflict {
        hunks: Vec<String>,
    },
    /// `git merge-file` could not run — said, never guessed clean.
    Unavailable(String),
}

/// (b) THREE-WAY MERGE via `git merge-file -p ours base theirs` (exit 0 clean, N>0 conflicts,
/// otherwise an error). Fast paths first: no edit → nothing; tree unchanged since the base →
/// ours as-is.
pub(super) fn three_way_merge(base: &[u8], ours: &[u8], theirs: &[u8]) -> Merge {
    if theirs == base {
        return Merge::Clean(ours.to_vec());
    }
    if ours == theirs {
        return Merge::Clean(ours.to_vec());
    }
    let dir = match tempfile::tempdir() {
        Ok(d) => d,
        Err(e) => return Merge::Unavailable(format!("tempdir: {e}")),
    };
    let (b, o, t) = (
        dir.path().join("base"),
        dir.path().join("ours"),
        dir.path().join("theirs"),
    );
    for (p, bytes) in [(&b, base), (&o, ours), (&t, theirs)] {
        if let Err(e) = std::fs::write(p, bytes) {
            return Merge::Unavailable(format!("write {}: {e}", p.display()));
        }
    }
    let out = std::process::Command::new("git")
        .args([
            "merge-file",
            "-p",
            "-L",
            "this shard",
            "-L",
            "base at dispatch",
            "-L",
            "tree now",
        ])
        .arg(&o)
        .arg(&b)
        .arg(&t)
        .output();
    let out = match out {
        Ok(o) => o,
        Err(e) => return Merge::Unavailable(format!("git merge-file: {e}")),
    };
    match out.status.code() {
        Some(0) => Merge::Clean(out.stdout),
        Some(n) if n > 0 && n < 128 => {
            let text = String::from_utf8_lossy(&out.stdout);
            let mut hunks = Vec::new();
            let mut cur: Option<String> = None;
            for line in text.lines() {
                if line.starts_with("<<<<<<<") {
                    cur = Some(String::new());
                }
                if let Some(h) = cur.as_mut() {
                    h.push_str(line);
                    h.push('\n');
                }
                if line.starts_with(">>>>>>>") {
                    if let Some(h) = cur.take() {
                        hunks.push(h);
                    }
                }
            }
            Merge::Conflict { hunks }
        }
        other => Merge::Unavailable(format!(
            "git merge-file exited {other:?}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )),
    }
}

/// What a shard's owned files compose to when landed on the tree NOW.
#[derive(Debug, Default)]
pub(super) struct Composed {
    /// (file, merged bytes) for every owned file whose landed content differs from the tree.
    pub(super) merged: Vec<(String, Vec<u8>)>,
    /// (file, hunks) — overlapping edits; nothing of this shard lands while any exist.
    pub(super) conflicts: Vec<(String, Vec<String>)>,
    /// Files whose merge tool was unavailable (reported, treated like a conflict).
    pub(super) unavailable: Vec<(String, String)>,
    /// How many files were merged three-way (vs. copied because the tree had not moved).
    pub(super) three_way: usize,
}

impl Composed {
    fn changed(&self) -> bool {
        !self.merged.is_empty() || !self.conflicts.is_empty() || !self.unavailable.is_empty()
    }
    fn landable(&self) -> bool {
        !self.merged.is_empty() && self.conflicts.is_empty() && self.unavailable.is_empty()
    }
}

fn safe_rel(rel: &str) -> Option<&Path> {
    let p = Path::new(rel);
    if p.is_absolute()
        || p.components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return None;
    }
    Some(p)
}

/// The tree a shard's composition is graded in: a fresh temp dir holding the real tree NOW. A
/// failure here is the HARNESS failing (no temp space, an unreadable tree) and never the shard's
/// edit — it is returned BY NAME so the wave counts it as a failed shard instead of reading the
/// ungraded shard as "beat nothing" (VA-080: both arms returned `(None, …)` silently).
fn preview_workspace(real_root: &Path) -> Result<tempfile::TempDir, String> {
    let preview = tempfile::TempDir::new().map_err(|e| format!("preview temp dir: {e}"))?;
    copy_tree_excluding(real_root, preview.path())
        .map_err(|e| format!("preview copy of {}: {e}", real_root.display()))?;
    Ok(preview)
}

/// What grading a shard's composition returned.
#[derive(Debug, Default)]
pub(super) struct Preview {
    /// The one ruler's grade of the preview tree, per check. None = not graded: nothing changed,
    /// a conflicted or tool-less composition, no shadow, or `setup_error` says the preview itself
    /// could not be built.
    pub(super) graded: Option<TreeGrade>,
    /// The shard's owned files differ from the tree (merged, conflicted or unavailable).
    pub(super) changed: bool,
    pub(super) composed: Composed,
    /// The preview workspace could not be set up — the harness's failure, named.
    pub(super) setup_error: Option<String>,
}

impl GooseAgentDispatcher {
    /// Compose the shard's owned files as a promotion would land them: each merged three-way
    /// against the tree now, from the base snapshotted at dispatch.
    pub(super) fn compose_merged(&self, task_id: &str, real_root: &Path) -> Option<Composed> {
        let (shadow_root, owned) = {
            let g = self.spec_shadows.lock().unwrap();
            g.get(task_id)
                .map(|(shadow, owned)| (shadow.path().to_path_buf(), owned.clone()))?
        };
        let bases: HashMap<String, Vec<u8>> = self
            .shard_bases
            .lock()
            .unwrap()
            .get(task_id)
            .cloned()
            .unwrap_or_default();
        let mut out = Composed::default();
        for f in &owned {
            let Some(rel) = safe_rel(f) else { continue };
            // OURS must exist: a shard that deleted or corrupted its own owned file made no
            // landable change to it — said by name, never landed as an EMPTY file (gate 1; the
            // old copy_owned_files skipped a missing src the same way, silently).
            let ours = match std::fs::read(shadow_root.join(rel)) {
                Ok(b) => b,
                Err(e) => {
                    self.events.write_value(serde_json::json!({
                        "event": "shard_file_unreadable",
                        "task_id": task_id,
                        "file": f,
                        "error": e.to_string(),
                    }));
                    continue;
                }
            };
            // THEIRS / BASE absent means the file did not exist (in the tree now / at dispatch):
            // an honest empty — the shard CREATED the file. Any OTHER read error on the tree's
            // copy is said, and the file is skipped rather than merged against a guess.
            let theirs = match std::fs::read(real_root.join(rel)) {
                Ok(b) => b,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                Err(e) => {
                    self.events.write_value(serde_json::json!({
                        "event": "shard_file_unreadable",
                        "task_id": task_id,
                        "file": f,
                        "side": "tree",
                        "error": e.to_string(),
                    }));
                    continue;
                }
            };
            let base = match bases.get(f) {
                Some(b) => b.clone(),
                None => Vec::new(),
            };
            if ours == base || ours == theirs {
                continue; // the shard did not change this file (or matches the tree already)
            }
            if theirs != base {
                out.three_way += 1;
            }
            match three_way_merge(&base, &ours, &theirs) {
                Merge::Clean(bytes) => {
                    if bytes != theirs {
                        out.merged.push((f.clone(), bytes));
                    }
                }
                Merge::Conflict { hunks } => out.conflicts.push((f.clone(), hunks)),
                Merge::Unavailable(why) => out.unavailable.push((f.clone(), why)),
            }
        }
        Some(out)
    }

    /// Grade EXACTLY what a promotion would land — the tree now plus the shard's MERGED files
    /// (and its created source files when it owns several). A conflicted composition cannot be
    /// graded and never lands; a preview that could not be built is a named `setup_error`.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn grade_merged_preview(
        &self,
        task_id: &str,
        real_root: &Path,
        prompt: &str,
        lang: TargetLang,
        all_files: &[String],
        composite: bool,
        missing_gate: bool,
    ) -> Preview {
        // No shadow: the shard never ran in one (`make_shadow` bailed the dispatch Transient —
        // `agent_ok: false` and `agent_error` on `complete_fix_completed` carry that) or it was
        // already promoted/discarded; nothing to compose is honest here.
        let Some(mut composed) = self.compose_merged(task_id, real_root) else {
            return Preview::default();
        };
        let shadow_root = {
            let g = self.spec_shadows.lock().unwrap();
            g.get(task_id).map(|(s, _)| s.path().to_path_buf())
        };
        let owned_n = {
            let g = self.spec_shadows.lock().unwrap();
            g.get(task_id).map(|(_, o)| o.len()).unwrap_or(0)
        };
        let preview = match preview_workspace(real_root) {
            Ok(t) => t,
            Err(e) => {
                return Preview {
                    graded: None,
                    changed: composed.changed(),
                    composed,
                    setup_error: Some(e),
                }
            }
        };
        let mut created = 0;
        if let Some(shadow_root) = shadow_root {
            if owned_n > 1 {
                created = copy_created_source_files(&shadow_root, preview.path());
            }
        }
        if !composed.changed() && created == 0 {
            return Preview {
                graded: None,
                changed: false,
                composed,
                setup_error: None,
            };
        }
        if !composed.conflicts.is_empty() || !composed.unavailable.is_empty() {
            return Preview {
                graded: None,
                changed: true,
                composed,
                setup_error: None,
            };
        }
        for (f, bytes) in &composed.merged {
            if let Some(rel) = safe_rel(f) {
                if let Some(parent) = preview.path().join(rel).parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(preview.path().join(rel), bytes);
            }
        }
        let graded = one_ruler_grade(
            preview.path(),
            prompt,
            lang,
            all_files,
            composite,
            missing_gate,
        )
        .await;
        if created > 0 && composed.merged.is_empty() {
            // created files only: changed, landable through the created-files copy below
            composed.three_way += 0;
        }
        Preview {
            graded,
            changed: true,
            composed,
            setup_error: None,
        }
    }

    /// Land the shard: write every merged file into the real tree (never outside it), copy its
    /// created source files when it owns several, drop the shadow and the base. Returns the
    /// files written.
    pub(super) fn promote_merged(&self, task_id: &str, real_root: &Path) -> Vec<String> {
        let Some(composed) = self.compose_merged(task_id, real_root) else {
            return Vec::new();
        };
        let mut written = Vec::new();
        if composed.landable() {
            for (f, bytes) in &composed.merged {
                let Some(rel) = safe_rel(f) else { continue };
                let dst = real_root.join(rel);
                if let Some(parent) = dst.parent() {
                    if std::fs::create_dir_all(parent).is_err() {
                        continue;
                    }
                    match (parent.canonicalize(), real_root.canonicalize()) {
                        (Ok(cp), Ok(ct)) if cp.starts_with(&ct) => {}
                        _ => continue,
                    }
                }
                if std::fs::write(&dst, bytes).is_ok() {
                    written.push(f.clone());
                }
            }
        }
        let entry = self.spec_shadows.lock().unwrap().remove(task_id);
        self.shard_bases.lock().unwrap().remove(task_id);
        if let Some((shadow, owned)) = entry {
            let created = if owned.len() > 1 {
                copy_created_source_files(shadow.path(), real_root)
            } else {
                0
            };
            self.events.write_value(serde_json::json!({
                "event": "shard_promoted",
                "task_id": task_id,
                "files": written,
                "three_way_merged": composed.three_way,
                "created_copied": created,
            }));
        }
        written
    }

    /// Drop a shard's shadow and base without landing anything.
    pub(super) fn discard_shard(&self, task_id: &str) {
        let _ = self.spec_shadows.lock().unwrap().remove(task_id);
        self.shard_bases.lock().unwrap().remove(task_id);
    }
}

/// Everything one wave needs, named once at the call site.
pub(super) struct WaveInputs {
    pub(super) round: u32,
    /// The round's opening gate, graded per check (`TreeGrade::of` over the round's findings and
    /// their provenance) — what every shard's preview is compared against until a promotion
    /// re-grades the tree.
    pub(super) baseline: TreeGrade,
    pub(super) findings: Vec<OpenFinding>,
    pub(super) fleet_slots: Vec<String>,
    pub(super) all_files: Vec<String>,
    pub(super) cwd: PathBuf,
    pub(super) prompt: String,
    pub(super) lang: TargetLang,
    pub(super) composite: bool,
    pub(super) missing_gate: bool,
    pub(super) device_id: String,
    pub(super) user_decisions: String,
    pub(super) brief_decisions: BriefDecisions,
    pub(super) doc_facts: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct WaveOutcome {
    pub(super) shards: usize,
    pub(super) promoted: usize,
    pub(super) conflicts: usize,
    pub(super) reshards: usize,
    /// Shards whose preview workspace could not be built — the harness's failures, counted apart
    /// from shards that ran, were graded and beat nothing.
    pub(super) setup_failed: usize,
    pub(super) findings_left: usize,
}

/// What one finding-shard returned to the driver.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct ShardOutcome {
    /// Bytes LANDED on the real tree (S13-c: `promote_merged` wrote at least one file).
    pub(super) promoted: bool,
    /// The finding's check no longer fails on the tree now — a sibling's promotion closed it
    /// while this shard worked; the driver retires the finding instead of parking it.
    pub(super) already_closed: bool,
    /// Overlapping hunks — re-dispatch at once on the merged base with this note.
    pub(super) conflict_note: Option<String>,
    /// The merge tool could not run — no progress on this tree (parked, never re-armed).
    pub(super) unavailable: bool,
    pub(super) handoff_files: Vec<String>,
    /// The preview workspace could not be built (`grade_merged_preview`'s `setup_error`): the
    /// shard was never graded, so its edit could not promote — the driver says so with the
    /// finding and counts it, instead of parking it as a quiet no-op.
    pub(super) setup_error: Option<String>,
}

/// The wave's seam: what runs ONE finding-shard and what re-grades the tree. The production
/// runner is the dispatcher (`DispatcherShardRunner`); a test runner drives `drive_wave` with
/// scripted completions and arrival orders (S13's refutation ran through exactly that seam).
#[async_trait]
pub(super) trait ShardRunner: Send + Sync + 'static {
    async fn run_shard(
        &self,
        f: &OpenFinding,
        slot: &str,
        baseline: Arc<tokio::sync::RwLock<TreeGrade>>,
    ) -> ShardOutcome;
    /// The tree's grade NOW, per check (None = the gate could not run).
    async fn regrade(&self) -> Option<TreeGrade>;
}

fn conflict_note(conflicts: &[(String, Vec<String>)]) -> String {
    let mut s = String::from(
        "YOUR PREVIOUS EDIT OVERLAPPED A SIBLING'S FIX THAT LANDED WHILE YOU WORKED. The file you \
         now see is the MERGED tree (the sibling's fix is in it); redo YOUR change on it, keeping \
         theirs. The overlapping hunks, as git marked them (`this shard` is your previous version, \
         `tree now` is what landed):\n",
    );
    for (f, hunks) in conflicts {
        for h in hunks {
            s.push_str(&format!("--- {f}\n{h}"));
        }
    }
    s
}

/// (c) THE WAVE, without a barrier — every finding keyed by its TEXT, never by its position
/// (S13: a promotion once removed a row while sibling indices were in flight, so the next fill
/// re-dispatched a running finding under the same task id and its shadow was replaced under the
/// running agent). Dispatch open findings as slots free, land each shard as it returns, re-grade
/// the tree after every promotion and hand the NEW count to every shard still running (the
/// baseline is shared, S13-b), re-shard on a handoff or a conflict at once, park a finding on the
/// current tree when it made no progress, and stop when nothing is dispatchable and nothing runs.
/// MILD (S14): two findings with IDENTICAL text collapse to one key — one shard fixes both, and
/// the second row is closed by the same promotion, never dispatched twice.
pub(super) async fn drive_wave<R: ShardRunner>(
    runner: Arc<R>,
    sink: Arc<dyn EventSink>,
    round: u32,
    baseline: TreeGrade,
    findings: Vec<OpenFinding>,
    fleet_slots: Vec<String>,
) -> WaveOutcome {
    let mut open = findings;
    // S14-5: a LOCK, not an atomic — the driver holds the WRITE guard across the regrade so a
    // sibling finishing meanwhile parks on `read()` and compares against the re-graded tree.
    let baseline = Arc::new(tokio::sync::RwLock::new(baseline));
    let mut tree_version: u64 = 0;
    let mut outcome = WaveOutcome::default();
    let slots = if fleet_slots.is_empty() {
        vec![String::new()]
    } else {
        order_fleet_by_speed(fleet_slots, &resolved_fleet_speed_weights(&load_config()))
    };
    let mut free: VecDeque<String> = slots.into_iter().collect();
    let mut in_flight: std::collections::HashSet<String> = Default::default();
    let mut done: std::collections::HashSet<String> = Default::default();
    let mut task_keys: HashMap<tokio::task::Id, (String, String)> = HashMap::new();
    let mut tasks: tokio::task::JoinSet<(String, String, ShardOutcome)> =
        tokio::task::JoinSet::new();
    loop {
        // FILL: every free slot takes the next dispatchable finding — not running, not done,
        // not already tried against this very tree.
        while let Some(slot) = free.pop_front() {
            let next = open
                .iter()
                .find(|f| {
                    !in_flight.contains(&f.text)
                        && !done.contains(&f.text)
                        && f.attempted_at != Some(tree_version)
                })
                .cloned();
            let Some(f) = next else {
                free.push_front(slot);
                break;
            };
            in_flight.insert(f.text.clone());
            outcome.shards += 1;
            let key = f.text.clone();
            let key_for_map = key.clone();
            let runner = runner.clone();
            let baseline = baseline.clone();
            let slot_for_task = slot.clone();
            let handle = tasks.spawn(async move {
                let out = runner.run_shard(&f, &slot_for_task, baseline).await;
                (key, slot_for_task, out)
            });
            task_keys.insert(handle.id(), (key_for_map, slot));
        }
        if in_flight.is_empty() {
            break;
        }
        let Some(joined) = tasks.join_next().await else {
            break;
        };
        let (key, slot, res) = match joined {
            Ok(v) => v,
            Err(e) => {
                // A panicked lane: release ONLY its finding (parked on this tree) and its slot;
                // every other running finding keeps running.
                let (key, slot) = task_keys
                    .remove(&e.id())
                    .unwrap_or_else(|| (String::new(), String::new()));
                sink.write_value(serde_json::json!({
                    "event": "lane_panicked",
                    "context": "complete-fix",
                    "round": round,
                    "finding": super::findings::elide_middle(&key, 150, 400),
                    "error": e.to_string(),
                }));
                in_flight.remove(&key);
                if let Some(f) = open.iter_mut().find(|o| o.text == key) {
                    f.attempted_at = Some(tree_version);
                }
                if !slot.is_empty() {
                    free.push_back(slot);
                }
                continue;
            }
        };
        task_keys.retain(|_, (k, _)| *k != key);
        in_flight.remove(&key);
        free.push_back(slot);
        let Some(f) = open.iter_mut().find(|o| o.text == key) else {
            continue;
        };
        if res.promoted {
            outcome.promoted += 1;
            tree_version += 1;
            done.insert(key.clone());
            // RE-VERIFY after each promotion: the next shard — and every shard still running —
            // is judged against the tree it lands on, never the round's opening count (S13-b).
            // The WRITE guard is held for the whole regrade (S14-5): a sibling that finishes
            // during it parks on `baseline.read()` instead of reading the PRE-promotion count —
            // a preview that already contains this fix graded 8 < 9 and landed a
            // fix-one-break-one as an improvement. When the gate cannot run the previous count
            // stays in force and the event SAYS which count that is.
            let mut guard = baseline.write().await;
            let regraded = runner.regrade().await;
            let verified = regraded.as_ref().map(|g| g.count);
            if let Some(g) = regraded {
                *guard = g;
            }
            let baseline_in_force = guard.count;
            drop(guard);
            sink.write_value(serde_json::json!({
                "event": "repair_tree_regraded",
                "round": round,
                "after_finding": super::findings::elide_middle(&key, 150, 400),
                "findings": verified,
                "baseline_in_force": baseline_in_force,
                "tree_version": tree_version,
            }));
            // Siblings parked on the old tree may try again on the new one.
            for o in open.iter_mut() {
                if o.attempted_at.is_some_and(|v| v < tree_version) {
                    o.attempted_at = None;
                }
            }
            continue;
        }
        if res.already_closed {
            // A sibling's landed fix closed this finding's check meanwhile (the shard's event
            // `finding_closed_by_sibling` says so): retired, never re-dispatched on the new tree.
            done.insert(key.clone());
            continue;
        }
        if let Some(error) = &res.setup_error {
            // The HARNESS failed this shard (no preview temp dir / the tree copy failed): said
            // with the finding and counted as a failed shard — never read as "ran and beat
            // nothing". The finding then takes the same road as any ungraded shard below
            // (conflict re-arm, handoff reshard, or parked until the tree moves): an environment
            // fault re-dispatched at once would spin, and the tree changing is the retry signal.
            outcome.setup_failed += 1;
            sink.write_value(serde_json::json!({
                "event": "repair_shard_setup_failed",
                "round": round,
                "finding": super::findings::elide_middle(&f.text, 150, 400),
                "shard": f.file,
                "error": error,
            }));
        }
        if let Some(note) = res.conflict_note {
            outcome.conflicts += 1;
            f.conflict_note = Some(note);
            f.attempted_at = None;
            continue;
        }
        if res.unavailable {
            // No merge tool → no progress possible on this tree; parked like a no-op shard,
            // never re-armed (unbounded re-dispatch when git is absent was the S13 finding).
            f.attempted_at = Some(tree_version);
            continue;
        }
        if !res.handoff_files.is_empty() {
            let mut added = false;
            for h in &res.handoff_files {
                if !f.owned.contains(h) {
                    f.owned.push(h.clone());
                    added = true;
                    sink.write_value(serde_json::json!({
                        "event": "handoff_reshard",
                        "round": round,
                        "finding": super::findings::elide_middle(&f.text, 150, 400),
                        "file": h,
                        "owned_now": f.owned,
                    }));
                }
            }
            if added {
                outcome.reshards += 1;
                f.attempted_at = None;
                continue;
            }
        }
        f.attempted_at = Some(tree_version);
    }
    outcome.findings_left = open.iter().filter(|o| !done.contains(&o.text)).count();
    outcome
}

/// The production runner: one finding-shard through the dispatcher, the regrade through the one
/// ruler.
pub(super) struct DispatcherShardRunner {
    pub(super) me: Arc<GooseAgentDispatcher>,
    pub(super) sink: Arc<dyn EventSink>,
    pub(super) round: u32,
    pub(super) all_files: Vec<String>,
    pub(super) cwd: PathBuf,
    pub(super) prompt: String,
    pub(super) lang: TargetLang,
    pub(super) composite: bool,
    pub(super) missing_gate: bool,
    pub(super) device_id: String,
    pub(super) user_decisions: String,
    pub(super) brief_decisions: BriefDecisions,
    pub(super) doc_facts: String,
}

#[async_trait]
impl ShardRunner for DispatcherShardRunner {
    async fn run_shard(
        &self,
        f: &OpenFinding,
        slot: &str,
        baseline: Arc<tokio::sync::RwLock<TreeGrade>>,
    ) -> ShardOutcome {
        run_finding_shard(self, f, slot, baseline).await
    }
    async fn regrade(&self) -> Option<TreeGrade> {
        one_ruler_grade(
            &self.cwd,
            &self.prompt,
            self.lang,
            &self.all_files,
            self.composite,
            self.missing_gate,
        )
        .await
    }
}

pub(super) async fn run_wave(
    me: Arc<GooseAgentDispatcher>,
    sink: Arc<dyn EventSink>,
    inputs: WaveInputs,
) -> WaveOutcome {
    let WaveInputs {
        round,
        baseline,
        findings,
        fleet_slots,
        all_files,
        cwd,
        prompt,
        lang,
        composite,
        missing_gate,
        device_id,
        user_decisions,
        brief_decisions,
        doc_facts,
    } = inputs;
    let runner = Arc::new(DispatcherShardRunner {
        me,
        sink: sink.clone(),
        round,
        all_files,
        cwd,
        prompt,
        lang,
        composite,
        missing_gate,
        device_id,
        user_decisions,
        brief_decisions,
        doc_facts,
    });
    drive_wave(runner, sink, round, baseline, findings, fleet_slots).await
}

/// Does a NOT REAL verdict QUOTE the replay the gate's finding calls for? (A QUOTE check — the
/// engine reads the words, it does not re-run the request.) A probe finding names a request
/// (`POST /api/drafts/<id>/submit …`): the detail must carry a path whose segments match it —
/// a `<id>`/`{id}`/`:id` segment matches any one real segment — and an HTTP status (`HTTP 200`,
/// `200 OK`, `→ 201`, a curl line). A render finding names no path: the detail must carry the
/// probe's own numbers (`renderedRowCount`/`rows=` or a console-error count). Words alone
/// ("re-probed with realistic variants, saw JSON every time") are not a quote.
pub(super) fn quotes_replay(detail: &str, finding: &str) -> bool {
    let has_status = detail.contains("curl")
        || detail
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|t| t.len() == 3 && t.chars().all(|c| c.is_ascii_digit()))
            .any(|t| t.starts_with(['2', '3', '4', '5']));
    let path = finding
        .split(|c: char| c.is_whitespace() || "`'\"(),;".contains(c))
        .find(|t| t.starts_with('/') && t.len() > 1)
        .map(|t| t.split('?').next().unwrap_or(t).trim_end_matches("'s"));
    match path {
        Some(pattern) => has_status && detail_names_path(detail, pattern),
        None => {
            let low = detail.to_lowercase();
            has_status
                || low.contains("renderedrowcount")
                || low.contains("rows=")
                || (low.contains("console") && low.chars().any(|c| c.is_ascii_digit()))
        }
    }
}

/// Does `detail` carry a path matching `pattern` segment-wise, where a parameter segment
/// (`<id>`, `{id}`, `:id`) matches any one non-empty real segment?
fn detail_names_path(detail: &str, pattern: &str) -> bool {
    let want: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let is_param = |seg: &str| seg.starts_with('<') || seg.starts_with('{') || seg.starts_with(':');
    detail
        .split(|c: char| c.is_whitespace() || "`'\"(),;".contains(c))
        .filter_map(|t| {
            // A URL's path starts after its host: `http://127.0.0.1:8741/api/x` → `/api/x`.
            match t.find("://") {
                Some(i) => {
                    let rest = t.split_at(i + 3).1;
                    rest.find('/').map(|j| rest.split_at(j).1)
                }
                None => t.find('/').map(|i| t.split_at(i).1),
            }
        })
        .map(|t| t.split('?').next().unwrap_or(t))
        .any(|cand| {
            let have: Vec<&str> = cand.split('/').filter(|s| !s.is_empty()).collect();
            have.len() == want.len()
                && want
                    .iter()
                    .zip(have.iter())
                    .all(|(w, h)| (is_param(w) && !h.is_empty()) || w == h)
        })
}

/// One finding's shard, start to finish: dispatch (speculative shadow), grade the merged
/// preview against the baseline AS IT IS when the shard returns, land or discard, persist the
/// repair row, say what happened.
async fn run_finding_shard(
    r: &DispatcherShardRunner,
    f: &OpenFinding,
    model: &str,
    baseline: Arc<tokio::sync::RwLock<TreeGrade>>,
) -> ShardOutcome {
    let (sink, me, round, all_files, cwd) = (&r.sink, &r.me, r.round, &r.all_files, &r.cwd);
    let task_id = format!("complete-fix::{}#{}", f.file, f.k);
    let baseline_at_dispatch = baseline.read().await.count;
    sink.write_value(serde_json::json!({
        "event": "complete_fix_dispatched",
        "round": round, "shard": f.file, "finding_index": f.k, "model": model,
        "task_id": task_id, "baseline_findings": baseline_at_dispatch,
        "owned": f.owned,
        "conflict_retry": f.conflict_note.is_some(),
    }));
    let started = std::time::Instant::now();
    let shard_decisions = r.brief_decisions.for_files(&f.owned);
    sink.write_value(serde_json::json!({
        "event": "shard_decisions",
        "round": round, "shard": f.file, "task_id": task_id,
        "owners": shard_decisions.owners,
        "chars": shard_decisions.block.chars().count(),
    }));
    let findings = vec![f.text.clone()];
    let conflict_block = f
        .conflict_note
        .as_deref()
        .map(|n| format!("\n\n{n}"))
        .unwrap_or_default();
    let shard_desc = format!(
        "{}{}{}{}{}",
        repro_block(f),
        f.order_note,
        smoke_fix_description(
            &findings,
            r.lang,
            &r.prompt,
            &render_repair_history(
                read_ledger_rollup(cwd).as_ref(),
                &f.owned,
                &findings,
                round as usize,
            ),
        ),
        conflict_block,
        shard_decisions.block
    );
    let req = DispatchRequest {
        task_id: task_id.clone(),
        description: shard_desc.clone(),
        device_id: r.device_id.clone(),
        model_id: model.to_string(),
        context_slice: String::new(),
        attempt: round,
        owned_files: f.owned.clone(),
        all_files: all_files.clone(),
        prior_hint: None,
        subsplit: Vec::new(),
        speculative: true,
        user_decisions: r.user_decisions.clone(),
        doc_facts: r.doc_facts.clone(),
        neighborhood: Vec::new(),
        shard_of: None,
        merger_of: None,
    };
    let progress = Arc::new(std::sync::Mutex::new(FixAttemptProgress::default()));
    let sampler = spawn_fix_progress_sampler(me.clone(), task_id.clone(), progress.clone());
    let ran = me.run(req).await;
    sampler.abort();
    {
        let st = progress.lock().unwrap().clone();
        sink.write_value(serde_json::json!({
            "event": "fix_attempt_progress",
            "round": round, "shard": f.file, "task_id": task_id,
            "samples": st.samples,
            "changed_samples": st.changed,
            "first_change_secs": st.first_change_secs,
            "longest_still_secs": st.longest_still_secs,
        }));
    }
    // REPAIR v2 §1: did the shard re-run the finding's own check BEFORE its first edit? Read from
    // its durable calls capture — the shadow's primary row file, the real tree's mirror when the
    // primary is empty (the mirror accumulates rounds under one task id, so rows are filtered to
    // this attempt). Said by name in every case, never blocked.
    let calls_file = format!("{}.calls.jsonl", activity_digest_key(&task_id));
    let mirror = cwd.join(".swarm").join("activity").join(&calls_file);
    let capture = match me.speculative_root(&task_id) {
        Some(shadow) => read_calls_capture(
            &shadow.join(".swarm").join("activity").join(&calls_file),
            Some(mirror),
        ),
        None => read_calls_capture(&mirror, None),
    };
    let (rows, unparseable_rows) = match capture.as_deref() {
        Some(text) => parse_call_rows(text, round),
        None => (Vec::new(), 0),
    };
    let check_command = f.check.as_ref().and_then(|c| c.command.clone());
    let finding_short = super::findings::elide_middle(&f.text, 150, 400);
    let (repro_event, repro_detail) = match repro_verdict(&rows, f.check.as_ref()) {
        Repro::Confirmed { call } => ("repro_confirmed", serde_json::json!({ "call": call })),
        Repro::EditedFirst { first_edit } => (
            "edit_before_repro",
            serde_json::json!({ "first_edit": first_edit }),
        ),
        Repro::NeverRan => ("repro_never_ran", serde_json::json!({})),
        Repro::NoCommand => (
            "repro_unobservable",
            serde_json::json!({ "why": "the finding carries no command to re-run by hand (a static scan or stat); the engine re-runs its check on the merged preview" }),
        ),
        Repro::NoCalls => (
            "repro_unobservable",
            serde_json::json!({ "why": "no calls capture for this shard (primary and mirror empty)" }),
        ),
    };
    sink.write_value(serde_json::json!({
        "event": repro_event,
        "round": round, "shard": f.file, "task_id": task_id,
        "finding": finding_short,
        "check": check_command,
        "calls": rows.len(),
        "unparseable_rows": unparseable_rows,
        "detail": repro_detail,
    }));
    let Preview {
        graded,
        changed: shard_changed,
        composed,
        setup_error,
    } = me
        .grade_merged_preview(
            &task_id,
            cwd,
            &r.prompt,
            r.lang,
            all_files,
            r.composite,
            r.missing_gate,
        )
        .await;
    let conflicted = !composed.conflicts.is_empty();
    let unavailable = !composed.unavailable.is_empty();
    // S13-b: compare against the baseline AS IT IS NOW — a sibling that landed while this shard
    // ran re-graded it, and a preview that already contains the sibling's fix is judged against
    // the post-sibling tree, never the opening one (or a regression lands as an improvement).
    // S14-5: `read()` parks while the driver holds the write guard across a sibling's regrade.
    let baseline_now = baseline.read().await.clone();
    let verified = graded.as_ref().map(|g| g.count);
    // REPAIR v2 §2: PROMOTE ON THE FLIP — the finding's own check fails fewer times on the merged
    // preview than on the tree now, and no check fails more. The count rule survives only for a
    // finding with no authoring check, labelled.
    let decision = decide_promotion(f.check.as_ref(), graded.as_ref(), &baseline_now);
    if shard_changed && !conflicted && !unavailable {
        let check_key = f.check.as_ref().map(|c| c.key.clone());
        match &decision {
            Promotion::Flipped { before, after, .. } => sink.write_value(serde_json::json!({
                "event": "finding_flipped",
                "round": round, "shard": f.file, "task_id": task_id,
                "finding": finding_short, "check": check_key, "command": check_command,
                "fails_before": before, "fails_after": after,
            })),
            Promotion::StillFailing { fails, quote, .. } => sink.write_value(serde_json::json!({
                "event": "finding_still_failing",
                "round": round, "shard": f.file, "task_id": task_id,
                "finding": finding_short, "check": check_key, "fails_on_preview": fails,
                "quote": super::findings::elide_middle(quote, 150, 400),
            })),
            Promotion::Regressed { new_failures, .. } => sink.write_value(serde_json::json!({
                "event": "preview_regressed",
                "round": round, "shard": f.file, "task_id": task_id,
                "finding": finding_short, "check": check_key,
                "new_failures": new_failures.iter().map(|(k, q)| serde_json::json!({
                    "check": k, "quote": super::findings::elide_middle(q, 150, 400)
                })).collect::<Vec<_>>(),
            })),
            Promotion::AlreadyClosed { .. } => sink.write_value(serde_json::json!({
                "event": "finding_closed_by_sibling",
                "round": round, "shard": f.file, "task_id": task_id,
                "finding": finding_short, "check": check_key,
            })),
            Promotion::Unverifiable { promote, verified, baseline } => {
                sink.write_value(serde_json::json!({
                    "event": "finding_unverifiable",
                    "round": round, "shard": f.file, "task_id": task_id,
                    "finding": finding_short,
                    "rule": "count strictly lower — LABELLED: no authoring check is recorded for this finding, so nothing can be re-run for it",
                    "verified_findings": verified, "baseline_findings": baseline,
                    "promote": promote,
                }))
            }
            Promotion::Ungraded => {}
        }
    }
    let would_promote = shard_changed && !conflicted && !unavailable && decision.promotes();
    let mut written = Vec::new();
    if would_promote {
        written = me.promote_merged(&task_id, cwd);
        if written.is_empty() {
            // S13-c: the tree moved between grade and landing and the re-composition was not
            // landable — nothing was written, so nothing was fixed; said by name.
            sink.write_value(serde_json::json!({
                "event": "shard_promotion_lost",
                "round": round, "shard": f.file, "task_id": task_id,
            }));
        }
    } else {
        me.discard_shard(&task_id);
    }
    let promoted = !written.is_empty();
    if conflicted {
        sink.write_value(serde_json::json!({
            "event": "merge_conflict",
            "round": round, "shard": f.file, "task_id": task_id,
            "files": composed.conflicts.iter().map(|(f, h)| serde_json::json!({"file": f, "hunks": h.len()})).collect::<Vec<_>>(),
        }));
    }
    if unavailable {
        sink.write_value(serde_json::json!({
            "event": "merge_unavailable",
            "round": round, "shard": f.file, "task_id": task_id,
            "said": composed.unavailable.iter().map(|(f, w)| serde_json::json!({"file": f, "why": w})).collect::<Vec<_>>(),
        }));
    }
    let output = ran.as_ref().map(|o| o.output.as_str()).unwrap_or("");
    // S5d (iv): a FIXED verdict from a shard whose shadow never diverged from the tree is a
    // claim without an edit — r6c's r1 read a zero-edit FIXED as a regression. Loud, MILD.
    let verdicts = parse_finding_verdicts(output);
    if !shard_changed {
        for (n, verdict, said) in &verdicts {
            if *verdict == "FIXED" {
                sink.write_value(serde_json::json!({
                    "event": "fix_claimed_without_edit",
                    "round": round, "shard": f.file, "task_id": task_id,
                    "finding_n": n,
                    "finding": super::findings::elide_middle(&f.text, 150, 400),
                    "said": said,
                }));
            }
        }
    }
    // S5d (ii): NOT REAL on a probe/render finding must QUOTE the replayed request AND response.
    let unreplayed: Vec<u32> = verdicts
        .iter()
        .filter(|(_, v, said)| {
            *v == "NOT REAL" && f.replay_required && !quotes_replay(said, &f.text)
        })
        .map(|(n, _, _)| *n)
        .collect();
    for n in &unreplayed {
        let said = verdicts
            .iter()
            .find(|(m, _, _)| m == n)
            .map(|(_, _, s)| s.as_str())
            .unwrap_or("");
        sink.write_value(serde_json::json!({
            "event": "dismissed_without_replay",
            "round": round, "shard": f.file, "task_id": task_id,
            "finding_n": n,
            "finding": super::findings::elide_middle(&f.text, 150, 400),
            "said": said,
        }));
    }
    if let Some(path) = write_repair_ledger(
        cwd,
        RepairLedgerRow {
            round: round as usize,
            shard: &f.file,
            owned_files: &f.owned,
            all_files,
            description: &shard_desc,
            output,
            promoted,
            baseline: baseline_now.count,
            agent_ok: ran.is_ok(),
            edited: shard_changed,
            unreplayed: &unreplayed,
        },
    ) {
        sink.write_value(serde_json::json!({
            "event": "ledger_written",
            "kind": "repair",
            "round": round,
            "shard": f.file,
            "path": path.display().to_string(),
        }));
    }
    let handoff_files: Vec<String> = parse_handoffs(output, all_files, &f.owned)
        .into_iter()
        .map(|h| h.path)
        .collect();
    sink.write_value(serde_json::json!({
        "event": "complete_fix_completed",
        "round": round, "shard": f.file, "finding_index": f.k, "model": model,
        "task_id": task_id,
        "secs": started.elapsed().as_secs(),
        "agent_ok": ran.is_ok(),
        "agent_error": ran.as_ref().err().map(|e| e.to_string()),
        "setup_failed": setup_error,
        "verified_findings": verified,
        "baseline_findings": baseline_now.count,
        "shard_changed": shard_changed,
        "three_way_merged": composed.three_way,
        "conflicted": conflicted,
        "merge_unavailable": unavailable,
        "handoffs": handoff_files,
        "files_written": written,
        "claimed_fixed_without_edit": !shard_changed && verdicts.iter().any(|(_, v, _)| *v == "FIXED"),
        "dismissed_without_replay": unreplayed,
        "promoted": promoted,
    }));
    ShardOutcome {
        promoted,
        already_closed: matches!(decision, Promotion::AlreadyClosed { .. }),
        conflict_note: conflicted.then(|| conflict_note(&composed.conflicts)),
        unavailable,
        handoff_files,
        setup_error,
    }
}

/// A tree's grade PER CHECK (findings.rs `check_key`): how many findings fail each check and one
/// finding's text per check for quoting, plus the plain count the old ruler used. Built from a
/// finding list and its provenance — the round's opening gate at the wave's start, the one
/// ruler's re-run after every promotion, and every shard's merged preview.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct TreeGrade {
    failing: BTreeMap<String, (usize, String)>,
    /// Every finding, sourced or not — the labelled count rule for a finding with no check.
    pub(super) count: usize,
}

impl TreeGrade {
    pub(super) fn of(findings: &[String], prov: &FindingProvenance) -> Self {
        let mut failing: BTreeMap<String, (usize, String)> = BTreeMap::new();
        for f in findings {
            // An unsourced finding has no check: it counts, it keys nothing (finding_unverifiable).
            if let Some(c) = prov.check_of(f) {
                let slot = failing.entry(c.key).or_insert_with(|| (0, f.clone()));
                slot.0 += 1;
            }
        }
        TreeGrade {
            failing,
            count: findings.len(),
        }
    }

    #[cfg(test)]
    pub(super) fn unkeyed(count: usize) -> Self {
        TreeGrade {
            failing: BTreeMap::new(),
            count,
        }
    }

    /// How many findings fail `key` on this tree (0 = the check passes here).
    pub(super) fn fails(&self, key: &str) -> usize {
        self.failing.get(key).map(|(n, _)| *n).unwrap_or(0)
    }

    pub(super) fn quote(&self, key: &str) -> Option<&str> {
        self.failing.get(key).map(|(_, q)| q.as_str())
    }

    /// The checks this tree fails MORE than `before` did — what a shard's edit broke.
    pub(super) fn regressions_from(&self, before: &TreeGrade) -> Vec<(String, String)> {
        self.failing
            .iter()
            .filter(|(k, (n, _))| *n > before.fails(k))
            .map(|(k, (_, q))| (k.clone(), q.clone()))
            .collect()
    }
}

/// THE ONE RULER — the same checks the round loop composes (the F862 law: a check enters the
/// round ruler and this grader TOGETHER), run on `root` and returned graded PER CHECK with each
/// finding's provenance, so a preview is judged on whether THIS finding's check flipped, not on
/// a count. None when the smoke gate could not run there: nothing is known, and nothing promotes
/// on unknown. Moved here from swarm.rs under the incremental-split law — the wave is its only
/// consumer.
pub(super) async fn one_ruler_grade(
    root: &Path,
    prompt: &str,
    lang: TargetLang,
    all_files: &[String],
    composite: bool,
    missing_gate: bool,
) -> Option<TreeGrade> {
    let g = run_smoke_gate(root, lang).await;
    if !g.ran {
        return None;
    }
    let mut prov = FindingProvenance::default();
    let mut findings: Vec<String> = Vec::new();
    prov.tag(FindingSource::SmokeGate, &g.findings);
    findings.extend(g.findings);
    if composite {
        let sc = run_spec_contract(root, prompt, lang).await;
        prov.absorb(sc.provenance);
        findings.extend(sc.findings);
    }
    let app_only: Vec<String> = all_files
        .iter()
        .filter(|f| !is_test_path(lang, f))
        .cloned()
        .collect();
    let timeouts = http_timeout_scan(root, lang, &app_scope_py(root, &app_only)).await;
    prov.tag(FindingSource::HttpTimeoutScan, &timeouts.findings);
    findings.extend(timeouts.findings);
    let dom = dom_id_scan(root, all_files).await;
    prov.tag(FindingSource::DomIdScan, &dom.findings);
    findings.extend(dom.findings);
    let css = css_coherence_scan(root, all_files).await;
    prov.tag(FindingSource::CssCoherenceScan, &css.findings);
    findings.extend(css.findings);
    if swarm_gate_cfg(
        "GOOSE_SWARM_CROSS_MODULE_CHECK",
        load_config().cross_module_check,
    ) {
        let drift = cross_module_drift(root, lang, &app_scope_py(root, all_files)).await;
        prov.tag(FindingSource::CrossModuleDrift, &drift.findings);
        findings.extend(drift.findings);
    }
    if missing_gate {
        let missing: Vec<String> = all_files
            .iter()
            .filter(|f| {
                lang.is_source_file(f) && !lang.is_test_file(f.rsplit('/').next().unwrap_or(f))
            })
            .filter(|f| !is_intentional_empty_marker(f))
            .filter(|f| {
                !root
                    .join(f)
                    .metadata()
                    .map(|m| m.len() > 0)
                    .unwrap_or(false)
            })
            .map(|f| missing_deliverable_finding(f.as_str()))
            .collect();
        prov.tag(FindingSource::MissingDeliverable, &missing);
        findings.extend(missing);
    }
    let findings = dedupe_findings_exact(&findings, &std::collections::HashSet::new());
    Some(TreeGrade::of(&findings, &prov))
}

/// What the promoter decided about one shard's merged preview (REPAIR v2 §2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Promotion {
    /// The finding's own check fails fewer times on the preview than on the tree now, and no
    /// check fails more: the fix landed and broke nothing the gate can see.
    Flipped {
        key: String,
        before: usize,
        after: usize,
    },
    /// The finding's own check still fails on the preview — quoted, so the words say what the
    /// shard did not close (r6c: `TypeError: Illegal invocation` under a promoted DOM-id fix).
    StillFailing {
        key: String,
        fails: usize,
        quote: String,
    },
    /// The finding's check flipped, but a check that passed on the tree now fails on the preview.
    Regressed {
        key: String,
        new_failures: Vec<(String, String)>,
    },
    /// The finding's check no longer fails on the tree either — a sibling's landed fix closed it
    /// while this shard worked; nothing to credit, nothing to re-dispatch.
    AlreadyClosed { key: String },
    /// No authoring check is recorded for the finding, so nothing can be re-run for it: the count
    /// rule (`shard_beats_baseline`) decides, LABELLED as such in the event.
    Unverifiable {
        promote: bool,
        verified: usize,
        baseline: usize,
    },
    /// The preview was not graded (nothing changed, a conflict, no shadow, a setup failure).
    Ungraded,
}

impl Promotion {
    pub(super) fn promotes(&self) -> bool {
        matches!(
            self,
            Promotion::Flipped { .. } | Promotion::Unverifiable { promote: true, .. }
        )
    }
}

/// PROMOTE ON THE FLIP. Pure over the finding's check, the preview's grade and the tree's grade
/// now, so the r6c sequence is a unit test: the shard sent for the exception whose preview closed
/// only a DOM id is `StillFailing` with the exception quoted; the DOM id's own shard is `Flipped`.
pub(super) fn decide_promotion(
    check: Option<&FindingCheck>,
    preview: Option<&TreeGrade>,
    baseline: &TreeGrade,
) -> Promotion {
    let Some(preview) = preview else {
        return Promotion::Ungraded;
    };
    let Some(check) = check else {
        return Promotion::Unverifiable {
            promote: shard_beats_baseline(Some(preview.count), baseline.count),
            verified: preview.count,
            baseline: baseline.count,
        };
    };
    let key = check.key.clone();
    let before = baseline.fails(&key);
    let after = preview.fails(&key);
    if before == 0 && after == 0 {
        return Promotion::AlreadyClosed { key };
    }
    if after >= before {
        let quote = preview.quote(&key).unwrap_or("").to_string();
        return Promotion::StillFailing {
            key,
            fails: after,
            quote,
        };
    }
    let new_failures = preview.regressions_from(baseline);
    if !new_failures.is_empty() {
        return Promotion::Regressed { key, new_failures };
    }
    Promotion::Flipped { key, before, after }
}

/// One row of the shard's durable `<task>.calls.jsonl`: the tool's name and the argument summary
/// the dispatcher wrote when the call was made (`inflight_args_preview`'s shapes —
/// `developer__shell: <line>`, `str_replace <path>`, `view <path>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CallRow {
    pub(super) name: String,
    pub(super) summary: String,
}

/// The rows of one attempt, in call order, plus how many lines did not parse (said in the event,
/// never dropped silently). Rows without a name and summary — the `attempt_end` snapshot the
/// terminal flush appends — are not calls. `attempt` filters the mirror, which accumulates every
/// round's rows under one task id.
pub(super) fn parse_call_rows(jsonl: &str, attempt: u32) -> (Vec<CallRow>, usize) {
    let mut rows = Vec::new();
    let mut unparseable = 0usize;
    for line in jsonl.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            unparseable += 1;
            continue;
        };
        if v.get("attempt").and_then(|a| a.as_u64()) != Some(u64::from(attempt)) {
            continue;
        }
        let (Some(name), Some(summary)) = (
            v.get("name").and_then(|x| x.as_str()),
            v.get("summary").and_then(|x| x.as_str()),
        ) else {
            continue;
        };
        rows.push(CallRow {
            name: name.to_string(),
            summary: summary.to_string(),
        });
    }
    (rows, unparseable)
}

/// What the shard did first (REPAIR v2 §1), read from its own calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Repro {
    /// A shell call re-ran the finding's check before any edit.
    Confirmed { call: String },
    /// The first edit came before any re-run of the check.
    EditedFirst { first_edit: String },
    /// Neither: the shard never re-ran the check and never edited.
    NeverRan,
    /// The finding carries no command to re-run by hand.
    NoCommand,
    /// No calls were captured for the shard.
    NoCalls,
}

/// An edit, by the tool's own name — the rows carry the SHORT name (`edit`, `write`, `shell`: r5
/// and r6c's archived `complete-fix::*.calls.jsonl`) or the prefixed one (`developer__edit`), and
/// a `text_editor` names its verb first in the summary (`str_replace <path>`; `view` is a read).
fn is_edit_call(row: &CallRow) -> bool {
    let name = row.name.rsplit("__").next().unwrap_or(&row.name);
    let verb = row.summary.split_whitespace().next().unwrap_or("");
    let edit_verb = |v: &str| {
        matches!(
            v,
            "write" | "str_replace" | "insert" | "create" | "edit" | "undo_edit"
        )
    };
    edit_verb(name) || (name == "text_editor" && edit_verb(verb))
}

fn is_shell_call(row: &CallRow) -> bool {
    row.name.contains("shell") || row.summary.starts_with("developer__shell")
}

/// Does a shell line re-run the check's command? The command's LAST backticked span is the probe
/// or request the gate ran (an earlier span is its boot); its program, its URL path (segment-wise,
/// a `<id>`/`{id}`/`:id` placeholder matching any real id — `detail_names_path`) and its plain
/// words (no flags, no digits — the gate's port and ids are the gate's, not the check's) must all
/// appear in the line. `node /opt/probe.mjs load http://127.0.0.1:54321` is re-run by `node
/// /opt/probe.mjs load http://127.0.0.1:9000`; `curl -s -w '%{http_code}' -X POST -m 20
/// http://127.0.0.1:8931/api/payments/<id>/note` by r6c's own `curl -s -i -X POST
/// http://127.0.0.1:8099/api/payments/pay_00042/note …`.
pub(super) fn shell_reruns_check(shell_line: &str, command: &str) -> bool {
    let parts: Vec<&str> = command.split('`').collect();
    let span = if parts.len() >= 3 {
        parts
            .iter()
            .enumerate()
            .filter(|(i, _)| i % 2 == 1)
            .map(|(_, s)| *s)
            .next_back()
            .unwrap_or(command)
    } else {
        command
    };
    let basename = |t: &str| t.rsplit('/').next().unwrap_or(t).to_lowercase();
    let mut program: Option<String> = None;
    let mut url_path: Option<String> = None;
    let mut words: Vec<String> = Vec::new();
    let mut skipping_cd = false;
    for t in span.split_whitespace() {
        if matches!(t, "&&" | ";" | "|" | "||") {
            program = None;
            skipping_cd = false;
            continue;
        }
        if skipping_cd {
            continue;
        }
        if program.is_none() {
            if t == "cd" {
                skipping_cd = true;
                continue;
            }
            if t.contains('=') && !t.starts_with('-') {
                continue; // an ENV=VALUE prefix
            }
            program = Some(basename(t));
            continue;
        }
        if t.starts_with('-') {
            continue;
        }
        if let Some((_, rest)) = t.split_once("://") {
            // `find` offsets are char boundaries; `get` says so — `None` leaves no path, as an absent `/` does.
            url_path = rest
                .find('/')
                .and_then(|j| rest.get(j..))
                .map(|p| p.trim_end_matches(['\'', '"', '`', ';']).to_lowercase());
            continue;
        }
        if t.chars().any(|c| c.is_ascii_digit()) || t.contains(['%', '{', '}', '\\']) {
            continue;
        }
        words.push(basename(t.trim_matches(['\'', '"'])));
    }
    let line = shell_line.to_lowercase();
    program.as_deref().is_some_and(|p| line.contains(p))
        && url_path
            .as_deref()
            .is_none_or(|p| detail_names_path(&line, p))
        && words.iter().all(|w| line.contains(w.as_str()))
}

/// REPAIR v2 §1, the reading: walk the shard's calls in order — a shell re-run of the check before
/// the first edit is `Confirmed`; an edit first is `EditedFirst`; neither is `NeverRan`.
pub(super) fn repro_verdict(rows: &[CallRow], check: Option<&FindingCheck>) -> Repro {
    let Some(command) = check.and_then(|c| c.command.as_deref()) else {
        return Repro::NoCommand;
    };
    if rows.is_empty() {
        return Repro::NoCalls;
    }
    for row in rows {
        if is_shell_call(row) && shell_reruns_check(&row.summary, command) {
            return Repro::Confirmed {
                call: row.summary.clone(),
            };
        }
        if is_edit_call(row) {
            return Repro::EditedFirst {
                first_edit: row.summary.clone(),
            };
        }
    }
    Repro::NeverRan
}

/// The search stem for a misnamed symbol's sibling: `onBrushChangeTracked` → `onBrushChange`
/// (all camel-case words but the last), `compute_total_v2` → `compute_total` (up to the last
/// `_`), a one-word name → itself.
fn sibling_search_stem(name: &str) -> String {
    if let Some((head, _)) = name.rsplit_once('_') {
        if !head.is_empty() {
            return head.to_string();
        }
    }
    let mut cuts: Vec<usize> = name
        .char_indices()
        .filter(|(i, c)| *i > 0 && c.is_ascii_uppercase())
        .map(|(i, _)| i)
        .collect();
    match cuts.pop() {
        // `last` is a `char_indices` offset, so a char boundary by construction.
        Some(last) if !cuts.is_empty() || last > 1 => name.split_at(last).0.to_string(),
        _ => name.to_string(),
    }
}

/// `path:line` and `File "path", line N` frames the finding quotes, in order, deduplicated.
fn frame_locations(text: &str) -> Vec<(String, u32)> {
    let toks: Vec<&str> = text.split_whitespace().collect();
    let is_path = |p: &str| {
        !p.is_empty()
            && super::findings::FINDING_PATH_EXTS
                .iter()
                .any(|e| p.ends_with(e))
    };
    let mut out: Vec<(String, u32)> = Vec::new();
    let mut push = |file: &str, line: u32| {
        if !out.iter().any(|(f, l)| f == file && *l == line) {
            out.push((file.to_string(), line));
        }
    };
    for (i, raw) in toks.iter().enumerate() {
        let t = raw.trim_matches(|c: char| "()[],;'\"`".contains(c));
        if let Some((file, rest)) = t.split_once(':') {
            let line = rest.split(':').next().unwrap_or("");
            if is_path(file) && !line.is_empty() && line.chars().all(|c| c.is_ascii_digit()) {
                if let Ok(n) = line.parse::<u32>() {
                    push(file, n);
                }
            }
        }
        if *raw == "File" {
            if let (Some(f), Some(kw), Some(n)) =
                (toks.get(i + 1), toks.get(i + 2), toks.get(i + 3))
            {
                let file = f.trim_matches(|c: char| "\",".contains(c));
                let n = n.trim_matches(|c: char| ",:".contains(c));
                if *kw == "line" && is_path(file) {
                    if let Ok(n) = n.parse::<u32>() {
                        push(file, n);
                    }
                }
            }
        }
    }
    out
}

/// REPAIR v2 §1, the localization CARRIED IN THE BRIEF from the evidence the finding already
/// quotes: an exception class names what to grep for, a frame names the file and line. Every
/// sentence is built from this finding's own symbol or path — nothing here names a bench.
pub(super) fn localization_hints(text: &str) -> Vec<String> {
    let mut hints: Vec<String> = Vec::new();
    let toks: Vec<&str> = text.split_whitespace().collect();
    let ident = |t: &str| {
        t.trim_matches(|c: char| !(c.is_alphanumeric() || "_$.".contains(c)))
            .to_string()
    };
    for (i, t) in toks.iter().enumerate() {
        if t.trim_start_matches(['(', '[']) == "ReferenceError:" {
            if let Some(x) = toks.get(i + 1).map(|x| ident(x)).filter(|x| !x.is_empty()) {
                let stem = sibling_search_stem(&x);
                hints.push(format!(
                    "`{x}` is referenced but never defined (the ReferenceError): `grep -n '{x}'` in \
                     your owned files finds the call site; `grep -n '{stem}'` finds the sibling that \
                     IS defined under a nearby name. ONE edit: make the call use the name that exists, \
                     or define `{x}`."
                ));
            }
        }
    }
    if let Some((before, _)) = text.split_once(" is not a function") {
        let callee = before
            .split_whitespace()
            .next_back()
            .map(ident)
            .filter(|x| !x.is_empty());
        if let Some(x) = callee {
            let last = x.rsplit('.').next().unwrap_or(&x).to_string();
            hints.push(format!(
                "`{x}` exists where it is called but is not callable (the TypeError): `grep -n '{last}'` \
                 for its definition and every caller — one side holds a different type than the other \
                 assumes. Fix that one site."
            ));
        }
    }
    if text.contains("Illegal invocation") {
        hints.push(
            "`Illegal invocation`: a browser API method was called DETACHED from its object — \
             `const f = obj.method; f(…)`, or a `cond ? gl.a : gl.b` that picks a method and then \
             calls it bare. Grep the owned scripts for methods stored in variables or chosen by \
             `?:` and call them ON their object (`obj.method(…)` / `.call(obj, …)`)."
                .to_string(),
        );
    }
    if text.contains("Cannot read propert") || text.contains("Cannot set propert") {
        let prop = text
            .split_once("(reading '")
            .or_else(|| text.split_once("property '"))
            .and_then(|(_, rest)| rest.split('\'').next())
            .unwrap_or("");
        hints.push(format!(
            "`{prop}` was read on null/undefined: a `getElementById`/`querySelector` whose id or \
             selector the served page does not define, or a handle that failed to create. Grep the \
             owned files for the lookup that yields the null and the served html for the id."
        ));
    }
    for (file, line) in frame_locations(text).into_iter().take(4) {
        hints.push(format!(
            "open `{file}` at line {line} — the frame the evidence names; the FIRST edit goes there."
        ));
    }
    hints
}

/// REPAIR v2 §1/§6 — the head of a fix shard's brief: the finding's own check, verbatim, as the
/// FIRST action; the localization the evidence already names; then the first edit at that
/// location. Assembled from THIS finding's check and evidence; the instruction sentences are
/// constants branched on what the finding carries.
pub(super) fn repro_block(f: &OpenFinding) -> String {
    let check_line = match f.check.as_ref() {
        Some(FindingCheck {
            command: Some(cmd), ..
        }) => format!(
            "THE CHECK THAT PRODUCED IT, as the gate ran it: {cmd}\nYOUR FIRST ACTION: run that \
             check yourself against YOUR booted copy of the app (your own port in place of the \
             gate's) and quote the failing line. Reading files before it is fine; editing before it \
             is not — the engine reads your call log and records which came first \
             (`repro_confirmed` / `edit_before_repro`)."
        ),
        Some(FindingCheck { key, command: None }) => format!(
            "THE CHECK THAT PRODUCED IT: {key} — a check the engine runs on the tree, not a command \
             you run by hand; the finding's text below is its whole output, and the engine re-runs \
             it on your merged result to decide whether the finding closed."
        ),
        None => "THE CHECK THAT PRODUCED IT: none recorded — no authoring check is known for this \
                 finding, so nothing can be re-run for it; the engine will grade your edit by the \
                 total finding count alone (labelled unverifiable)."
            .to_string(),
    };
    let hints = localization_hints(&f.text);
    let localize = if hints.is_empty() {
        "LOCALIZE FROM THE EVIDENCE: the finding names its subject (the endpoint, the DOM id, the \
         test, the file) — the FIRST edit goes where that subject is implemented in your owned files."
            .to_string()
    } else {
        format!(
            "LOCALIZE FROM THE EVIDENCE — the finding already names where:\n{}",
            hints
                .iter()
                .map(|h| format!("- {h}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    };
    format!(
        "REPRODUCE FIRST, THEN LOCALIZE, THEN EDIT. This shard exists for ONE finding (numbered 1 \
         below).\n{check_line}\n{localize}\nTHE FIRST EDIT goes at that location — before any survey \
         of the rest of the codebase and without a refactor around it; then re-run the check. The \
         finding is closed when the check stops failing, and the engine confirms that on your merged \
         result before anything lands.\n\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The promoter never writes outside the real tree: a `..` component or an absolute owned
    /// path is refused before any byte moves (the guard copy_owned_files used to carry).
    #[test]
    fn owned_paths_that_escape_the_tree_are_refused() {
        assert!(safe_rel("../SENTINEL").is_none());
        assert!(safe_rel("/tmp/abs-escape").is_none());
        assert!(safe_rel("sub/../../x").is_none());
        assert_eq!(safe_rel("web/app.js"), Some(Path::new("web/app.js")));
    }

    /// S5d (ii): a NOT REAL is accepted only with the replayed request AND response quoted —
    /// r6c's "re-probed with 8 realistic variants and saw JSON every time" is words; the gate's
    /// own line with its status is a replay.
    #[test]
    fn a_not_real_needs_the_replayed_request_and_response() {
        let probe = "POST /api/drafts's response could not be read as a JSON object on either probe — the spec documents a JSON response";
        assert!(!quotes_replay(
            "re-probed with 8 realistic variants (tokens, bodies) and saw JSON every time",
            probe
        ));
        assert!(quotes_replay("`curl -s -w '\n%{http_code}' -X POST -m 20 http://127.0.0.1:8741/api/drafts` → HTTP 401 {\"error\":{\"code\":\"unauthorized\"}}", probe));
        assert!(
            !quotes_replay("GET /api/health returned HTTP 200 {\"ok\":true}", probe),
            "a different path is not this finding's replay"
        );
        let render = "the served page renders NO data rows in a real browser — the API works but the frontend shows a user nothing. (in `web/viz.js`)";
        assert!(!quotes_replay("the page looks fine to me", render));
        assert!(quotes_replay(
            "ran the probe: renderedRowCount=50, consoleErrors 0",
            render
        ));
        assert!(quotes_replay(
            "node probe.js load http://127.0.0.1:8741 → rows=50",
            render
        ));
    }

    /// S5d (iv): the history a later shard reads names a FIXED that landed no edit and a NOT REAL
    /// that quoted no replay — r6c r1 read a zero-edit FIXED as a regression.
    #[test]
    fn the_repair_history_names_unedited_fixed_and_unreplayed_not_real() {
        let rollup = serde_json::json!({"repair": {"rounds": [
            {"round": 0, "shard": "web/app.js", "owned_files": ["web/app.js"], "edited": false,
             "verdicts": [
                {"n": 1, "finding": "POST /api/drafts's response does not carry the documented field(s)", "verdict": "FIXED", "detail": "all fields present on my probes"},
                {"n": 2, "finding": "the page renders NO data rows", "verdict": "NOT REAL", "detail": "looks fine", "unreplayed": true}
             ]}
        ]}});
        let text = super::super::render_repair_history(
            Some(&rollup),
            &["web/app.js".to_string()],
            &["POST /api/drafts's response does not carry the documented field(s)".to_string()],
            1,
        );
        assert!(
            text.contains("FINDING 1 FIXED — CLAIMED FIXED WITHOUT AN EDIT"),
            "{text}"
        );
        assert!(text.contains("FINDING 2 NOT REAL — NOT ACCEPTED (no replayed request+response quoted; the finding stays open)"), "{text}");
    }

    /// r5 put six findings on ONE shard (group by file); now six findings on one file are six
    /// shards sharing that file's resolved ownership, each numbered for a unique task id.
    #[test]
    fn a_file_group_explodes_into_one_shard_per_finding() {
        let groups = vec![
            FileGroup {
                file: "app/httpapi.py".into(),
                findings: vec!["f1".into(), "f2".into(), "f3".into()],
            },
            FileGroup {
                file: "web/viz.js".into(),
                findings: vec!["f4".into()],
            },
        ];
        let owned = vec![
            vec!["app/httpapi.py".to_string(), "app/__main__.py".to_string()],
            vec!["web/viz.js".to_string(), "web/index.html".to_string()],
        ];
        let notes = vec!["A".to_string(), "B".to_string()];
        let open = explode_groups(&groups, &owned, &notes, &|t: &str| t == "f4", &|t: &str| {
            (t == "f4").then(|| FindingCheck {
                key: "render gate rows | f4".into(),
                command: None,
            })
        });
        assert_eq!(open.len(), 4);
        assert!(open[3].replay_required && !open[0].replay_required);
        assert_eq!(
            open[3].check.as_ref().map(|c| c.key.as_str()),
            Some("render gate rows | f4")
        );
        assert!(open[0].check.is_none(), "no authoring check → unverifiable");
        assert_eq!(open[0].k, 0);
        assert_eq!(open[2].k, 2);
        assert_eq!(open[2].owned, owned[0]);
        assert_eq!(open[3].file, "web/viz.js");
        assert_eq!(open[3].order_note, "B");
        assert!(open
            .iter()
            .all(|o| o.attempted_at.is_none() && o.conflict_note.is_none()));
    }

    /// (b) Two shards edit ONE file from the same base: non-overlapping hunks land in either
    /// order (the second merges three-way against the first's landed tree); overlapping hunks
    /// conflict and the hunk is quoted with git's markers; an unchanged tree is a plain copy.
    #[test]
    fn non_overlapping_hunks_merge_and_overlapping_ones_are_quoted() {
        let base = b"a\nb\nc\nd\ne\nf\ng\nh\n";
        let ours = b"a\nB\nc\nd\ne\nf\ng\nh\n"; // shard 1 edits line 2
        let theirs = b"a\nb\nc\nd\ne\nf\nG\nh\n"; // shard 2 landed line 7
        match three_way_merge(base, ours, theirs) {
            Merge::Clean(m) => assert_eq!(m, b"a\nB\nc\nd\ne\nf\nG\nh\n".to_vec()),
            other => panic!("expected a clean merge: {other:?}"),
        }
        match three_way_merge(base, ours, base) {
            Merge::Clean(m) => assert_eq!(m, ours.to_vec(), "tree unchanged → ours as-is"),
            other => panic!("{other:?}"),
        }
        let theirs2 = b"a\nBETA\nc\nd\ne\nf\ng\nh\n"; // both edited line 2
        match three_way_merge(base, ours, theirs2) {
            Merge::Conflict { hunks } => {
                assert_eq!(hunks.len(), 1);
                assert!(hunks[0].contains("<<<<<<< this shard"), "{}", hunks[0]);
                assert!(hunks[0].contains("B\n"), "{}", hunks[0]);
                assert!(hunks[0].contains("BETA"), "{}", hunks[0]);
                assert!(hunks[0].contains(">>>>>>> tree now"), "{}", hunks[0]);
            }
            other => panic!("expected a conflict: {other:?}"),
        }
        let note = conflict_note(&[(
            "web/app.js".into(),
            vec!["<<<<<<< this shard\nx\n=======\ny\n>>>>>>> tree now\n".into()],
        )]);
        assert!(note.contains("--- web/app.js\n<<<<<<< this shard"));
        assert!(note.contains("redo YOUR change on it, keeping theirs"));
    }

    /// S13: a parameter segment matches any real id — a genuine replay with a real id is a
    /// QUOTE; a missing or different segment is not this path.
    #[test]
    fn a_quoted_replay_matches_parameter_segments_against_real_ids() {
        let submit =
            "POST /api/drafts/<id>/submit's response could not be read as JSON on either probe";
        assert!(quotes_replay("curl -X POST http://127.0.0.1:8741/api/drafts/d_7f3a/submit → HTTP 200 {\"state\":\"submitted\"}", submit));
        assert!(
            !quotes_replay(
                "curl -X POST http://127.0.0.1:8741/api/drafts/submit → HTTP 200",
                submit
            ),
            "a missing segment is not this path"
        );
        assert!(!quotes_replay(
            "HTTP 200 on /api/drafts/d_7f3a/approve",
            submit
        ));
    }

    /// S13: the driver keys every finding by its TEXT. Two findings on ONE file, three slots,
    /// both promoted, in BOTH arrival orders: each finding is dispatched exactly once (no
    /// duplicate task id, no shadow replaced under a running agent), both promotions are
    /// counted, the tree version bumps per promotion and the regrade runs after each, a later
    /// shard reads the refreshed baseline, and a third finding parked on the old tree is
    /// re-armed by the promotions — never dispatched twice on one tree version.
    #[tokio::test]
    async fn two_shards_on_one_file_both_land_in_either_arrival_order() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Mutex;
        #[derive(Default)]
        struct Rec(Mutex<Vec<serde_json::Value>>);
        impl EventSink for Rec {
            fn emit(&self, _e: &goose_swarm::SwarmEvent) {}
            fn write_value(&self, v: serde_json::Value) {
                self.0.lock().unwrap().push(v);
            }
        }
        struct Scripted {
            script: HashMap<String, (u64, bool)>,
            dispatched: Mutex<Vec<String>>,
            regrades: AtomicUsize,
            baselines_seen: Mutex<Vec<(String, usize)>>,
        }
        #[async_trait]
        impl ShardRunner for Scripted {
            async fn run_shard(
                &self,
                f: &OpenFinding,
                _slot: &str,
                baseline: Arc<tokio::sync::RwLock<TreeGrade>>,
            ) -> ShardOutcome {
                self.dispatched.lock().unwrap().push(f.text.clone());
                let (delay, promoted) = self.script.get(&f.text).copied().unwrap_or((0, false));
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                let seen = baseline.read().await.count;
                self.baselines_seen
                    .lock()
                    .unwrap()
                    .push((f.text.clone(), seen));
                ShardOutcome {
                    promoted,
                    ..ShardOutcome::default()
                }
            }
            // S14-5: a SLOW gate — the sibling finishing during it must still read the re-graded
            // tree (the driver holds the write guard), never the pre-promotion one.
            async fn regrade(&self) -> Option<TreeGrade> {
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                let n = self.regrades.fetch_add(1, Ordering::SeqCst) + 1;
                Some(TreeGrade::unkeyed(9 - n))
            }
        }
        let finding = |text: &str, file: &str, k: usize| OpenFinding {
            text: text.into(),
            check: None,
            file: file.into(),
            owned: vec![file.into()],
            order_note: String::new(),
            k,
            attempted_at: None,
            conflict_note: None,
            replay_required: false,
        };
        for (d1, d2) in [(10u64, 80u64), (80u64, 10u64)] {
            let findings = vec![
                finding("S1", "web/app.js", 0),
                finding("S2", "web/app.js", 1),
                finding("S3", "app/api.py", 0),
            ];
            let runner = Arc::new(Scripted {
                script: [
                    ("S1".to_string(), (d1, true)),
                    ("S2".to_string(), (d2, true)),
                    ("S3".to_string(), (5, false)),
                ]
                .into_iter()
                .collect(),
                dispatched: Mutex::new(Vec::new()),
                regrades: AtomicUsize::new(0),
                baselines_seen: Mutex::new(Vec::new()),
            });
            let sink = Arc::new(Rec::default());
            let sink_dyn: Arc<dyn EventSink> = sink.clone();
            let out = drive_wave(
                runner.clone(),
                sink_dyn,
                0,
                TreeGrade::unkeyed(9),
                findings,
                vec!["m1".into(), "m2".into(), "m3".into()],
            )
            .await;
            assert_eq!(
                out.promoted, 2,
                "both fixes to one file land ({d1},{d2}): {out:?}"
            );
            let dispatched = runner.dispatched.lock().unwrap().clone();
            assert_eq!(
                dispatched.iter().filter(|t| *t == "S1").count(),
                1,
                "{dispatched:?}"
            );
            assert_eq!(
                dispatched.iter().filter(|t| *t == "S2").count(),
                1,
                "{dispatched:?}"
            );
            let s3 = dispatched.iter().filter(|t| *t == "S3").count();
            assert!(
                (1..=3).contains(&s3),
                "S3 runs at most once per tree version: {dispatched:?}"
            );
            assert_eq!(
                runner.regrades.load(Ordering::SeqCst),
                2,
                "one regrade per promotion"
            );
            let events = sink.0.lock().unwrap().clone();
            let versions: Vec<u64> = events
                .iter()
                .filter(|e| e["event"] == "repair_tree_regraded")
                .map(|e| e["tree_version"].as_u64().unwrap())
                .collect();
            assert_eq!(versions, vec![1, 2], "{events:?}");
            let seen = runner.baselines_seen.lock().unwrap().clone();
            let mut s12: Vec<usize> = seen
                .iter()
                .filter(|(t, _)| t == "S1" || t == "S2")
                .map(|(_, b)| *b)
                .collect();
            s12.sort_unstable();
            assert_eq!(
                s12,
                vec![8, 9],
                "the second of S1/S2 finishes DURING the first's 300 ms regrade and must read the \
                 re-graded 8, never the pre-promotion 9 ({d1},{d2}): {seen:?}"
            );
            let in_force: Vec<u64> = events
                .iter()
                .filter(|e| e["event"] == "repair_tree_regraded")
                .map(|e| e["baseline_in_force"].as_u64().unwrap())
                .collect();
            assert_eq!(in_force, vec![8, 7], "{events:?}");
            assert_eq!(out.findings_left, 1, "S3 stays open; S1/S2 are done");
        }
    }

    /// VA-080 item 2: a shard whose preview workspace could not be built is the HARNESS failing,
    /// not "ran and beat nothing" — the wave says so with the finding and the error, counts it
    /// as a failed shard, and does not spin it: parked until the tree moves, like any ungraded
    /// shard. `preview_workspace` itself returns the copy failure by name (the arm that returned
    /// `(None, …)` silently).
    #[tokio::test]
    async fn a_failed_preview_setup_is_said_with_its_finding_and_counted() {
        use std::sync::Mutex;
        let err = preview_workspace(Path::new("/nonexistent/swarm-va080/tree")).unwrap_err();
        assert!(
            err.starts_with("preview copy of /nonexistent/swarm-va080/tree: "),
            "{err}"
        );

        #[derive(Default)]
        struct Rec(Mutex<Vec<serde_json::Value>>);
        impl EventSink for Rec {
            fn emit(&self, _e: &goose_swarm::SwarmEvent) {}
            fn write_value(&self, v: serde_json::Value) {
                self.0.lock().unwrap().push(v);
            }
        }
        struct Broken {
            dispatched: Mutex<Vec<String>>,
            error: String,
        }
        #[async_trait]
        impl ShardRunner for Broken {
            async fn run_shard(
                &self,
                f: &OpenFinding,
                _slot: &str,
                _baseline: Arc<tokio::sync::RwLock<TreeGrade>>,
            ) -> ShardOutcome {
                self.dispatched.lock().unwrap().push(f.text.clone());
                ShardOutcome {
                    setup_error: Some(self.error.clone()),
                    ..ShardOutcome::default()
                }
            }
            async fn regrade(&self) -> Option<TreeGrade> {
                panic!("nothing promoted, nothing to regrade")
            }
        }
        let runner = Arc::new(Broken {
            dispatched: Mutex::new(Vec::new()),
            error: err.clone(),
        });
        let sink = Arc::new(Rec::default());
        let sink_dyn: Arc<dyn EventSink> = sink.clone();
        let out = drive_wave(
            runner.clone(),
            sink_dyn,
            2,
            TreeGrade::unkeyed(9),
            vec![OpenFinding {
                text: "GET /api/drafts returns HTML".into(),
                check: None,
                file: "app/api.py".into(),
                owned: vec!["app/api.py".into()],
                order_note: String::new(),
                k: 0,
                attempted_at: None,
                conflict_note: None,
                replay_required: false,
            }],
            vec!["m1".into(), "m2".into()],
        )
        .await;
        assert_eq!(out.setup_failed, 1, "{out:?}");
        assert_eq!(out.promoted, 0, "{out:?}");
        assert_eq!(
            out.shards, 1,
            "parked on this tree, not re-dispatched: {out:?}"
        );
        assert_eq!(out.findings_left, 1, "{out:?}");
        assert_eq!(runner.dispatched.lock().unwrap().len(), 1);
        let events = sink.0.lock().unwrap().clone();
        let said = events
            .iter()
            .find(|e| e["event"] == "repair_shard_setup_failed")
            .unwrap_or_else(|| panic!("{events:?}"));
        assert_eq!(said["round"], 2);
        assert_eq!(said["finding"], "GET /api/drafts returns HTML");
        assert_eq!(said["shard"], "app/api.py");
        assert_eq!(said["error"], err);
    }

    fn sourced(prov: &mut FindingProvenance, source: FindingSource, text: &str) -> String {
        let t = text.to_string();
        prov.tag(source, std::slice::from_ref(&t));
        t
    }

    /// REPAIR v2 §2, r6c's round 0 as a unit test. The `web/viz.js` shard was dispatched for
    /// the exception finding (`TypeError: Illegal invocation`, RenderGateRows) and its preview
    /// closed only the DOM id (`viz-labels`): the count rule promoted it 9→8 and the next verify
    /// carried the exception verbatim. On the flip: that shard is `StillFailing` with the
    /// exception QUOTED; the DOM id's own shard, same preview, is `Flipped`; a preview that also
    /// breaks the note endpoint is `Regressed`; a finding with no check keeps the labelled count
    /// rule; an ungraded preview promotes nothing; a check a sibling already closed is
    /// `AlreadyClosed`.
    #[test]
    fn promotion_flips_only_when_the_findings_own_check_passes_and_nothing_regresses() {
        let mut prov = FindingProvenance::default();
        let f0 = sourced(
            &mut prov,
            FindingSource::RenderGateRows,
            "the served page renders NO data rows in a real browser — the API works but the \
             frontend shows a user nothing. First console error: TypeError: Illegal invocation. \
             Open web/index.html end to end: the page must fetch the documented endpoints and \
             render the rows. GATE COMMAND (run it yourself against your booted app; it prints \
             renderedRowCount and consoleErrors): `node /opt/probe.mjs load \
             http://127.0.0.1:52001`. (in `viz.js`)",
        );
        let f2 = sourced(
            &mut prov,
            FindingSource::DomIdScan,
            "web/viz.js:533 references DOM id `viz-labels` which NO html file in the app defines \
             — getElementById returns null there and the page throws at runtime (the \
             rendered-nothing class). Either add the id to the HTML or fix the reference to an \
             id that exists.",
        );
        let f3 = sourced(
            &mut prov,
            FindingSource::EndpointContractProbe,
            "POST /api/payments/<id>/note's response does not carry the documented field(s) \
             `id`, `note`, `version` — the spec's endpoint table names them for exactly this \
             endpoint.",
        );
        let baseline = TreeGrade::of(&[f0.clone(), f2.clone(), f3.clone()], &prov);
        assert_eq!(baseline.count, 3);
        // The gate re-run on the preview: its own provenance, a fresh port, the DOM id gone.
        let mut preview_prov = FindingProvenance::default();
        let f0_again = sourced(
            &mut preview_prov,
            FindingSource::RenderGateRows,
            &f0.replace("52001", "52777"),
        );
        let f3_again = sourced(&mut preview_prov, FindingSource::EndpointContractProbe, &f3);
        let preview = TreeGrade::of(&[f0_again.clone(), f3_again.clone()], &preview_prov);
        assert_eq!(
            preview.count, 2,
            "the count rule would have promoted: 2 < 3"
        );
        let exception_shard = prov.check_of(&f0).expect("sourced");
        let dom_shard = prov.check_of(&f2).expect("sourced");
        match decide_promotion(Some(&exception_shard), Some(&preview), &baseline) {
            Promotion::StillFailing { fails, quote, key } => {
                assert_eq!(fails, 1);
                assert!(quote.contains("TypeError: Illegal invocation"), "{quote}");
                assert_eq!(key, exception_shard.key);
            }
            other => panic!("the exception's shard must not promote on a DOM id: {other:?}"),
        }
        assert_eq!(
            decide_promotion(Some(&dom_shard), Some(&preview), &baseline),
            Promotion::Flipped {
                key: dom_shard.key.clone(),
                before: 1,
                after: 0
            },
            "the DOM id's own shard lands the DOM id fix"
        );
        // The same edit also broke the note endpoint: flipped, but a regression — refused.
        let broke = sourced(
            &mut preview_prov,
            FindingSource::EndpointContractProbe,
            "POST /api/payments/<id>/note returned 500 — the spec advertises this endpoint and \
             it errors (server 5xx). Nothing downstream of it can work.",
        );
        let preview2 = TreeGrade::of(&[f0_again, f3_again, broke], &preview_prov);
        match decide_promotion(Some(&dom_shard), Some(&preview2), &baseline) {
            Promotion::Regressed { new_failures, .. } => {
                assert_eq!(new_failures.len(), 1, "{new_failures:?}");
                assert!(
                    new_failures[0].1.contains("returned 500"),
                    "{new_failures:?}"
                );
            }
            other => panic!("a new failure must refuse the promotion: {other:?}"),
        }
        // No authoring check: the LABELLED count rule, exactly the old `shard_beats_baseline`.
        assert_eq!(
            decide_promotion(None, Some(&preview), &baseline),
            Promotion::Unverifiable {
                promote: true,
                verified: 2,
                baseline: 3
            }
        );
        assert_eq!(
            decide_promotion(None, Some(&baseline), &baseline),
            Promotion::Unverifiable {
                promote: false,
                verified: 3,
                baseline: 3
            }
        );
        assert_eq!(
            decide_promotion(Some(&dom_shard), None, &baseline),
            Promotion::Ungraded
        );
        // A sibling landed the DOM id meanwhile: the tree now no longer fails it — retired.
        let tree_after_sibling = TreeGrade::of(&[f0, f3], &prov);
        assert!(matches!(
            decide_promotion(Some(&dom_shard), Some(&preview), &tree_after_sibling),
            Promotion::AlreadyClosed { .. }
        ));
        assert!(!Promotion::Ungraded.promotes());
        assert!(Promotion::Flipped {
            key: "k".into(),
            before: 1,
            after: 0
        }
        .promotes());
        assert!(!Promotion::Unverifiable {
            promote: false,
            verified: 3,
            baseline: 3
        }
        .promotes());
    }

    /// REPAIR v2 §1: the shard's own calls say whether it re-ran the check before it edited.
    /// The render probe re-run on the shard's own port is a re-run; a grep is not; an edit
    /// before any re-run is `EditedFirst`; the POST probe's replay is its LAST span (the
    /// curl), never the boot; rows are this attempt's only, the `attempt_end` snapshot is not a
    /// call, and a corrupt line is counted, not dropped.
    #[test]
    fn repro_first_is_read_from_the_shards_own_calls() {
        let check = FindingCheck {
            key: "k".into(),
            command: Some(
                "GATE COMMAND (run it yourself; it prints consoleErrors.texts): `node \
                 /opt/probe.mjs load http://127.0.0.1:54321`"
                    .into(),
            ),
        };
        let row = |name: &str, summary: &str| CallRow {
            name: name.into(),
            summary: summary.into(),
        };
        let rerun =
            "developer__shell: cd /tmp/shadow && node /opt/probe.mjs load http://127.0.0.1:9000";
        let confirmed = vec![
            row("developer__text_editor", "view web/viz.js"),
            row(
                "developer__shell",
                "developer__shell: grep -n 'onBrushChangeTracked' web/viz.js",
            ),
            row("developer__shell", rerun),
            row("developer__text_editor", "str_replace web/viz.js"),
        ];
        assert_eq!(
            repro_verdict(&confirmed, Some(&check)),
            Repro::Confirmed { call: rerun.into() }
        );
        let edited_first = vec![
            row("developer__text_editor", "view web/viz.js"),
            row("developer__text_editor", "str_replace web/viz.js"),
            row("developer__shell", rerun),
        ];
        assert_eq!(
            repro_verdict(&edited_first, Some(&check)),
            Repro::EditedFirst {
                first_edit: "str_replace web/viz.js".into()
            }
        );
        let never = vec![
            row("developer__text_editor", "view web/viz.js"),
            row("developer__shell", "developer__shell: ls web"),
        ];
        assert_eq!(repro_verdict(&never, Some(&check)), Repro::NeverRan);
        assert_eq!(repro_verdict(&confirmed, None), Repro::NoCommand);
        let scan = FindingCheck {
            key: "k".into(),
            command: None,
        };
        assert_eq!(repro_verdict(&confirmed, Some(&scan)), Repro::NoCommand);
        assert_eq!(repro_verdict(&[], Some(&check)), Repro::NoCalls);

        let replay = "REPLAY IT: boot exactly as the gate did — `cd <tree> && PYTHONPATH=src \
                      python3 -m app.ledgerd --db-dir D` — then `curl -s -w '\\n%{http_code}' -X \
                      POST -m 20 http://127.0.0.1:8931/api/drafts`; a NOT REAL verdict must quote \
                      that command's status and body";
        assert!(shell_reruns_check(
            "developer__shell: curl -X POST -d '{}' http://127.0.0.1:9000/api/drafts",
            replay
        ));
        assert!(
            !shell_reruns_check(
                "developer__shell: curl http://127.0.0.1:9000/api/drafts",
                replay
            ),
            "a GET is not the POST the gate sent"
        );
        assert!(
            !shell_reruns_check(
                "developer__shell: PYTHONPATH=src python3 -m app.ledgerd --db-dir D",
                replay
            ),
            "booting is not re-running the check"
        );
        assert!(shell_reruns_check(
            "developer__shell: pytest -q tests/",
            "pytest -q"
        ));

        // THE ARCHIVE'S OWN ROW SHAPES (r6c `complete-fix::app~sledgerd~s__init__.py.calls.jsonl`
        // and `complete-fix::web~sviz.js.calls.jsonl`): short tool names, raw summaries — a curl
        // with a REAL id re-runs the placeholder check; an `edit` row whose summary is only the
        // path is an edit.
        let note_check = FindingCheck {
            key: "k".into(),
            command: Some(
                "REPLAY IT: boot exactly as the gate did — `cd <tree> && PYTHONPATH=src python3 -m \
                 app.ledgerd --db-dir D` — then `curl -s -w '\\n%{http_code}' -X POST -m 20 \
                 http://127.0.0.1:8931/api/payments/<id>/note`; a NOT REAL verdict must quote that \
                 command's status and body"
                    .into(),
            ),
        };
        let r6c_ledgerd = vec![
            row("shell", "cd /var/folders/T/.tmpYg9SG8 2>/dev/null; grep -n 'note' app/api.py"),
            row("shell", "sed -n '370,450p' app/api.py"),
            row(
                "shell",
                "echo '=== note ==='; curl -s -i -X POST http://127.0.0.1:8099/api/payments/pay_00042/note -H 'Content-Type: application/json' -d '{}'",
            ),
        ];
        match repro_verdict(&r6c_ledgerd, Some(&note_check)) {
            Repro::Confirmed { call } => assert!(call.contains("pay_00042/note"), "{call}"),
            other => panic!("a real-id curl re-runs the placeholder check: {other:?}"),
        }
        let r6c_viz = vec![
            row(
                "shell",
                "grep -rn 'viz-labels' web/ ; echo '---'; sed -n '515,560p' web/viz.js",
            ),
            row("edit", "/var/folders/T/.tmpWtl6Iu/web/index.html"),
            row("edit", "/var/folders/T/.tmpWtl6Iu/web/viz.js"),
            row("shell", "node /opt/probe.mjs load http://127.0.0.1:8099"),
        ];
        assert_eq!(
            repro_verdict(&r6c_viz, Some(&check)),
            Repro::EditedFirst {
                first_edit: "/var/folders/T/.tmpWtl6Iu/web/index.html".into()
            },
            "the archive's `edit` row is an edit"
        );

        let jsonl = concat!(
            "{\"ts\":\"t\",\"attempt\":0,\"name\":\"developer__shell\",\"summary\":\"developer__shell: ls\",\"ok\":true,\"result_tail\":\"\"}\n",
            "{\"ts\":\"t\",\"attempt\":1,\"name\":\"developer__text_editor\",\"summary\":\"view web/viz.js\",\"ok\":true,\"result_tail\":\"\"}\n",
            "{\"kind\":\"attempt_end\",\"attempt\":1}\n",
            "not json\n",
            "{\"ts\":\"t\",\"attempt\":1,\"name\":\"developer__shell\",\"summary\":\"developer__shell: node /opt/probe.mjs load http://127.0.0.1:9000\",\"ok\":true,\"result_tail\":\"{}\"}\n",
        );
        let (rows, unparseable) = parse_call_rows(jsonl, 1);
        assert_eq!(
            unparseable, 1,
            "a corrupt row is counted, never dropped silently"
        );
        assert_eq!(
            rows.len(),
            2,
            "attempt 0's row and the attempt_end snapshot are not calls"
        );
        assert_eq!(rows[0].summary, "view web/viz.js");
        assert!(rows[1].summary.contains("probe.mjs load"));
        assert_eq!(
            repro_verdict(&rows, Some(&check)),
            Repro::Confirmed {
                call: rows[1].summary.clone()
            }
        );
    }

    /// REPAIR v2 §1/§6: the brief opens with the gate's own replay as the first action and
    /// carries the localization the evidence already names — r5's `ReferenceError` yields the
    /// grep for the symbol and for its defined sibling (`onBrushChange`), r6c's `Illegal
    /// invocation` names the detached-method shape, a frame names its file and line — and never
    /// opens a numbered block of its own (parse_numbered_findings reads the first `1. `).
    #[test]
    fn the_brief_opens_with_the_check_and_the_localization_from_the_evidence() {
        let mut prov = FindingProvenance::default();
        let console = sourced(
            &mut prov,
            FindingSource::RenderGateException,
            "the page renders but the browser console carries 4 error(s) in normal use (first: \
             ReferenceError: onBrushChangeTracked is not defined) — fix the JS errors; users hit \
             them as broken interactions. GATE COMMAND (run it yourself; it prints \
             consoleErrors.texts): `node /opt/probe.mjs load http://127.0.0.1:54321`. (in \
             `web/viz.js`)",
        );
        let open = |text: &str, check: Option<FindingCheck>| OpenFinding {
            text: text.into(),
            check,
            file: "web/viz.js".into(),
            owned: vec!["web/viz.js".into(), "web/index.html".into()],
            order_note: String::new(),
            k: 0,
            attempted_at: None,
            conflict_note: None,
            replay_required: true,
        };
        let block = repro_block(&open(&console, prov.check_of(&console)));
        assert!(
            block.starts_with("REPRODUCE FIRST, THEN LOCALIZE, THEN EDIT."),
            "{block}"
        );
        assert!(
            block.contains(
                "THE CHECK THAT PRODUCED IT, as the gate ran it: GATE COMMAND (run it yourself; \
                 it prints consoleErrors.texts): `node /opt/probe.mjs load http://127.0.0.1:54321`"
            ),
            "{block}"
        );
        assert!(
            block.contains("`grep -n 'onBrushChangeTracked'`"),
            "{block}"
        );
        assert!(
            block.contains("`grep -n 'onBrushChange'`"),
            "the defined sibling's stem: {block}"
        );
        assert!(
            block.contains("THE FIRST EDIT goes at that location"),
            "{block}"
        );
        assert!(block.ends_with("\n\n"), "prepends cleanly to the brief");
        assert!(
            block.lines().all(|l| !l.trim_start().starts_with("1. ")),
            "the head must never open a numbered block: {block}"
        );

        let hints = localization_hints(
            "the served page renders NO data rows in a real browser — the API works but the \
             frontend shows a user nothing. First console error: TypeError: Illegal invocation. \
             (in `viz.js`)",
        );
        assert!(
            hints.iter().any(|h| h.contains("DETACHED from its object")),
            "{hints:?}"
        );
        assert_eq!(
            localization_hints(
                "web/viz.js:533 references DOM id `viz-labels` which NO html file in the app \
                 defines — fix it"
            ),
            vec!["open `web/viz.js` at line 533 — the frame the evidence names; the FIRST edit goes there."]
        );
        let hints = localization_hints(
            "`pytest -q` failed:\n  File \"/Users/x/runs/unit/app/api.py\", line 40, in \
             list_payments\napp/store.py:88: in query\nE   TypeError: Cannot read properties of \
             null (reading 'getContext')\nE   TypeError: this.draw is not a function",
        );
        assert!(
            hints
                .iter()
                .any(|h| h.contains("open `/Users/x/runs/unit/app/api.py` at line 40")),
            "{hints:?}"
        );
        assert!(
            hints
                .iter()
                .any(|h| h.contains("open `app/store.py` at line 88")),
            "{hints:?}"
        );
        assert!(
            hints
                .iter()
                .any(|h| h.contains("`getContext` was read on null")),
            "{hints:?}"
        );
        assert!(
            hints.iter().any(|h| h.contains("`grep -n 'draw'`")),
            "{hints:?}"
        );
        assert_eq!(sibling_search_stem("onBrushChangeTracked"), "onBrushChange");
        assert_eq!(sibling_search_stem("compute_total_v2"), "compute_total");
        assert_eq!(sibling_search_stem("draw"), "draw");

        // A static scan carries no command: the check is NAMED, and the model is told the
        // engine re-runs it — never a substituted command.
        let scan_block = repro_block(&open(
            "web/viz.js:533 references DOM id `viz-labels` which NO html file in the app defines",
            Some(FindingCheck {
                key: "dom-id contract scan | web/viz.js:# references dom id `viz-labels`".into(),
                command: None,
            }),
        ));
        assert!(
            scan_block.contains(
                "THE CHECK THAT PRODUCED IT: dom-id contract scan | web/viz.js:# references dom \
                 id `viz-labels` — a check the engine runs on the tree"
            ),
            "{scan_block}"
        );
        assert!(
            repro_block(&open("a finding nothing authored", None)).contains("none recorded"),
            "unsourced is said, not filled"
        );
    }

    /// A finding whose check a sibling's promotion already closed is RETIRED by the driver, not
    /// parked and re-dispatched on the next tree version.
    #[tokio::test]
    async fn a_finding_closed_by_a_sibling_is_retired_not_redispatched() {
        use std::sync::Mutex;
        #[derive(Default)]
        struct Rec(Mutex<Vec<serde_json::Value>>);
        impl EventSink for Rec {
            fn emit(&self, _e: &goose_swarm::SwarmEvent) {}
            fn write_value(&self, v: serde_json::Value) {
                self.0.lock().unwrap().push(v);
            }
        }
        struct Closed {
            dispatched: Mutex<Vec<String>>,
        }
        #[async_trait]
        impl ShardRunner for Closed {
            async fn run_shard(
                &self,
                f: &OpenFinding,
                _slot: &str,
                _baseline: Arc<tokio::sync::RwLock<TreeGrade>>,
            ) -> ShardOutcome {
                self.dispatched.lock().unwrap().push(f.text.clone());
                ShardOutcome {
                    already_closed: true,
                    ..ShardOutcome::default()
                }
            }
            async fn regrade(&self) -> Option<TreeGrade> {
                panic!("nothing promoted, nothing to regrade")
            }
        }
        let runner = Arc::new(Closed {
            dispatched: Mutex::new(Vec::new()),
        });
        let sink: Arc<dyn EventSink> = Arc::new(Rec::default());
        let out = drive_wave(
            runner.clone(),
            sink,
            0,
            TreeGrade::unkeyed(2),
            vec![OpenFinding {
                text: "web/viz.js:533 references DOM id `viz-labels`".into(),
                check: Some(FindingCheck {
                    key: "dom-id contract scan | web/viz.js:# references dom id `viz-labels`"
                        .into(),
                    command: None,
                }),
                file: "web/viz.js".into(),
                owned: vec!["web/viz.js".into()],
                order_note: String::new(),
                k: 0,
                attempted_at: None,
                conflict_note: None,
                replay_required: false,
            }],
            vec!["m1".into(), "m2".into()],
        )
        .await;
        assert_eq!(out.shards, 1);
        assert_eq!(out.promoted, 0);
        assert_eq!(out.findings_left, 0, "retired, not left open: {out:?}");
        assert_eq!(runner.dispatched.lock().unwrap().len(), 1);
    }
}
