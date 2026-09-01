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
//!     The tree is RE-GRADED after each promotion (`one_ruler_grade`) so the next shard is judged
//!     against the tree it will actually land on — never the round's opening count.
//! (c) NO ROUND BARRIER: findings dispatch as nodes free (a slot pool, `JoinSet`); a handoff naming
//!     a non-owned tree file re-shards that finding at once with the file added
//!     (`handoff_reshard{finding, file}`); a finding that was tried against THIS tree and promoted
//!     nothing waits for the tree to change (progress, not a count). The wave ends when no finding
//!     is dispatchable and no shard runs; the outer loop's `complete_fix_converged` (existing) ends
//!     the phase when a wave changed nothing on the tree.
//!
//! Sibling module under the incremental-split law; the per-file fan's closure body moved here
//! from swarm.rs's wave loop, which now calls `run_wave`.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use goose_swarm::{DispatchRequest, EventSink, TaskDispatcher};

use super::attribution::parse_handoffs;
use super::decisions::BriefDecisions;
use super::findings::{parse_finding_verdicts, FileGroup};
use super::fleet_order::{order_fleet_by_speed, resolved_fleet_speed_weights};
use super::ledger_block::read_ledger_rollup;
use super::ledger_writers::{write_repair_ledger, RepairLedgerRow};
use super::{
    copy_created_source_files, copy_tree_excluding, load_config, one_ruler_grade,
    render_repair_history, shard_beats_baseline, smoke_fix_description, spawn_fix_progress_sampler,
    FixAttemptProgress, GooseAgentDispatcher, TargetLang,
};

/// One open finding and the shard that will work it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct OpenFinding {
    pub(super) text: String,
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
    /// (and its created source files when it owns several). Returns (findings, changed,
    /// composition): a conflicted composition cannot be graded and never lands.
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
    ) -> (Option<usize>, bool, Composed) {
        let Some(mut composed) = self.compose_merged(task_id, real_root) else {
            return (None, false, Composed::default());
        };
        let shadow_root = {
            let g = self.spec_shadows.lock().unwrap();
            g.get(task_id).map(|(s, _)| s.path().to_path_buf())
        };
        let owned_n = {
            let g = self.spec_shadows.lock().unwrap();
            g.get(task_id).map(|(_, o)| o.len()).unwrap_or(0)
        };
        let preview = match tempfile::TempDir::new() {
            Ok(t) => t,
            Err(_) => return (None, composed.changed(), composed),
        };
        if copy_tree_excluding(real_root, preview.path()).is_err() {
            return (None, composed.changed(), composed);
        }
        let mut created = 0;
        if let Some(shadow_root) = shadow_root {
            if owned_n > 1 {
                created = copy_created_source_files(&shadow_root, preview.path());
            }
        }
        if !composed.changed() && created == 0 {
            return (None, false, composed);
        }
        if !composed.conflicts.is_empty() || !composed.unavailable.is_empty() {
            return (None, true, composed);
        }
        for (f, bytes) in &composed.merged {
            if let Some(rel) = safe_rel(f) {
                if let Some(parent) = preview.path().join(rel).parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(preview.path().join(rel), bytes);
            }
        }
        let (verified, _established) = one_ruler_grade(
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
        (verified, true, composed)
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
    pub(super) baseline: usize,
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
    pub(super) findings_left: usize,
}

/// What one finding-shard returned to the driver.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct ShardOutcome {
    /// Bytes LANDED on the real tree (S13-c: `promote_merged` wrote at least one file).
    pub(super) promoted: bool,
    /// Overlapping hunks — re-dispatch at once on the merged base with this note.
    pub(super) conflict_note: Option<String>,
    /// The merge tool could not run — no progress on this tree (parked, never re-armed).
    pub(super) unavailable: bool,
    pub(super) handoff_files: Vec<String>,
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
        baseline: Arc<tokio::sync::RwLock<usize>>,
    ) -> ShardOutcome;
    /// The tree's finding count NOW (None = the gate could not run).
    async fn regrade(&self) -> Option<usize>;
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
    baseline: usize,
    findings: Vec<OpenFinding>,
    fleet_slots: Vec<String>,
) -> WaveOutcome {
    let mut open = findings;
    // S14-5: a LOCK, not an atomic — the driver holds the WRITE guard across the regrade so a
    // sibling finishing meanwhile parks on `read()` and compares against the re-graded count.
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
            let verified = runner.regrade().await;
            if let Some(v) = verified {
                *guard = v;
            }
            let baseline_in_force = *guard;
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
        baseline: Arc<tokio::sync::RwLock<usize>>,
    ) -> ShardOutcome {
        run_finding_shard(self, f, slot, baseline).await
    }
    async fn regrade(&self) -> Option<usize> {
        one_ruler_grade(
            &self.cwd,
            &self.prompt,
            self.lang,
            &self.all_files,
            self.composite,
            self.missing_gate,
        )
        .await
        .0
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
    baseline: Arc<tokio::sync::RwLock<usize>>,
) -> ShardOutcome {
    let (sink, me, round, all_files, cwd) = (&r.sink, &r.me, r.round, &r.all_files, &r.cwd);
    let task_id = format!("complete-fix::{}#{}", f.file, f.k);
    let baseline_at_dispatch = *baseline.read().await;
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
        "{}{}{}{}",
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
    let (verified, shard_changed, composed) = me
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
    // ran lowered it, and a preview that already contains the sibling's fix must beat the
    // post-sibling count, never the opening one (or a regression lands as an improvement).
    // S14-5: `read()` parks while the driver holds the write guard across a sibling's regrade.
    let baseline_now = *baseline.read().await;
    let would_promote = shard_changed
        && !conflicted
        && !unavailable
        && shard_beats_baseline(verified, baseline_now);
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
            baseline: baseline_now,
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
        "verified_findings": verified,
        "baseline_findings": baseline_now,
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
        conflict_note: conflicted.then(|| conflict_note(&composed.conflicts)),
        unavailable,
        handoff_files,
    }
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
        let open = explode_groups(&groups, &owned, &notes, &|t: &str| t == "f4");
        assert_eq!(open.len(), 4);
        assert!(open[3].replay_required && !open[0].replay_required);
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
                baseline: Arc<tokio::sync::RwLock<usize>>,
            ) -> ShardOutcome {
                self.dispatched.lock().unwrap().push(f.text.clone());
                let (delay, promoted) = self.script.get(&f.text).copied().unwrap_or((0, false));
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                let seen = *baseline.read().await;
                self.baselines_seen
                    .lock()
                    .unwrap()
                    .push((f.text.clone(), seen));
                ShardOutcome {
                    promoted,
                    conflict_note: None,
                    unavailable: false,
                    handoff_files: Vec::new(),
                }
            }
            // S14-5: a SLOW gate — the sibling finishing during it must still read the re-graded
            // count (the driver holds the write guard), never the pre-promotion one.
            async fn regrade(&self) -> Option<usize> {
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                let n = self.regrades.fetch_add(1, Ordering::SeqCst) + 1;
                Some(9 - n)
            }
        }
        let finding = |text: &str, file: &str, k: usize| OpenFinding {
            text: text.into(),
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
                9,
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
}
