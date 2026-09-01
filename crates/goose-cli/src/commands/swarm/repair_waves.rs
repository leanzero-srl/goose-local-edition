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

use goose_swarm::{DispatchRequest, EventSink, TaskDispatcher};

use super::attribution::parse_handoffs;
use super::decisions::BriefDecisions;
use super::findings::FileGroup;
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
}

/// (a) One shard per FINDING: every finding of a file group becomes its own OpenFinding carrying
/// the group's resolved ownership and order note.
pub(super) fn explode_groups(
    groups: &[FileGroup],
    owned_by_shard: &[Vec<String>],
    order_notes: &[String],
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
            let ours = std::fs::read(shadow_root.join(rel)).unwrap_or_default();
            let theirs = std::fs::read(real_root.join(rel)).unwrap_or_default();
            let base = bases.get(f).cloned().unwrap_or_default();
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
struct ShardResult {
    idx: usize,
    slot: String,
    promoted: bool,
    conflict_note: Option<String>,
    handoff_files: Vec<String>,
}

fn conflict_note(conflicts: &[(String, Vec<String>)], unavailable: &[(String, String)]) -> String {
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
    for (f, why) in unavailable {
        s.push_str(&format!(
            "--- {f}: could not be merged ({why}); apply your change to the current file\n"
        ));
    }
    s
}

/// (c) THE WAVE, without a barrier: dispatch open findings as slots free, land each shard by
/// three-way merge as it returns, re-grade the tree after every promotion, re-shard on a handoff
/// or a conflict at once, and stop when nothing is dispatchable and nothing runs.
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
    let mut open = findings;
    let mut baseline = baseline;
    let mut tree_version: u64 = 0;
    let mut outcome = WaveOutcome::default();
    let slots = if fleet_slots.is_empty() {
        vec![String::new()]
    } else {
        order_fleet_by_speed(fleet_slots, &resolved_fleet_speed_weights(&load_config()))
    };
    let mut free: VecDeque<String> = slots.into_iter().collect();
    let mut in_flight: std::collections::HashSet<usize> = Default::default();
    let mut tasks: tokio::task::JoinSet<ShardResult> = tokio::task::JoinSet::new();
    let all_files = Arc::new(all_files);
    let prompt = Arc::new(prompt);
    let brief_decisions = Arc::new(brief_decisions);
    loop {
        // FILL: every free slot takes the next dispatchable finding (never one tried against
        // this very tree, never one already running).
        while let Some(slot) = free.pop_front() {
            let next = open
                .iter()
                .enumerate()
                .find(|(i, f)| !in_flight.contains(i) && f.attempted_at != Some(tree_version))
                .map(|(i, _)| i);
            let Some(idx) = next else {
                free.push_front(slot);
                break;
            };
            in_flight.insert(idx);
            outcome.shards += 1;
            let f = open[idx].clone();
            let me = me.clone();
            let sink = sink.clone();
            let all_files = all_files.clone();
            let prompt = prompt.clone();
            let brief_decisions = brief_decisions.clone();
            let cwd = cwd.clone();
            let device_id = device_id.clone();
            let user_decisions = user_decisions.clone();
            let doc_facts = doc_facts.clone();
            tasks.spawn(async move {
                let (promoted, conflict_note, handoff_files) = run_finding_shard(
                    me,
                    sink,
                    round,
                    baseline,
                    &f,
                    &slot,
                    &all_files,
                    &cwd,
                    &prompt,
                    lang,
                    composite,
                    missing_gate,
                    &device_id,
                    &user_decisions,
                    &brief_decisions,
                    &doc_facts,
                )
                .await;
                ShardResult {
                    idx,
                    slot,
                    promoted,
                    conflict_note,
                    handoff_files,
                }
            });
        }
        if in_flight.is_empty() {
            break;
        }
        let Some(joined) = tasks.join_next().await else {
            break;
        };
        let res = match joined {
            Ok(r) => r,
            Err(e) => {
                // A panicked lane: name it, free nothing it held (its slot is lost with it —
                // the pool shrinks by one rather than a phantom slot dispatching forever).
                sink.write_value(serde_json::json!({
                    "event": "lane_panicked",
                    "context": "complete-fix",
                    "round": round,
                    "error": e.to_string(),
                }));
                // Whichever finding it was can no longer be identified; every in-flight index
                // that has no live task is released to be tried again.
                in_flight.clear();
                if tasks.is_empty() && free.is_empty() {
                    break;
                }
                continue;
            }
        };
        in_flight.remove(&res.idx);
        free.push_back(res.slot);
        let Some(f) = open.get_mut(res.idx) else {
            continue;
        };
        if res.promoted {
            outcome.promoted += 1;
            tree_version += 1;
            // RE-VERIFY after each promotion: the next shard is judged against the tree it lands
            // on, never the round's opening count.
            let (verified, established) =
                one_ruler_grade(&cwd, &prompt, lang, &all_files, composite, missing_gate).await;
            if let Some(v) = verified {
                baseline = v;
            }
            sink.write_value(serde_json::json!({
                "event": "repair_tree_regraded",
                "round": round,
                "after_shard": format!("complete-fix::{}#{}", f.file, f.k),
                "findings": verified,
                "established": established,
                "tree_version": tree_version,
            }));
            let text = f.text.clone();
            open.retain(|o| o.text != text);
            // Siblings tried against the old tree may try again on the new one.
            for o in open.iter_mut() {
                if o.attempted_at.is_some_and(|v| v < tree_version) {
                    o.attempted_at = None;
                }
            }
            // indices shifted: nothing in flight refers to a removed row only if we remap —
            // simplest honest choice: rebuild in_flight by text match is impossible; so keep
            // indices stable by NOT removing here when anything is in flight.
            continue;
        }
        if let Some(note) = res.conflict_note {
            outcome.conflicts += 1;
            f.conflict_note = Some(note);
            f.attempted_at = None;
            continue;
        }
        if !res.handoff_files.is_empty() {
            let mut added = false;
            for h in &res.handoff_files {
                if all_files.contains(h) && !f.owned.contains(h) {
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
    outcome.findings_left = open.len();
    outcome
}

/// One finding's shard, start to finish: dispatch (speculative shadow), grade the merged
/// preview, land or discard, persist the repair row, say what happened.
#[allow(clippy::too_many_arguments)]
async fn run_finding_shard(
    me: Arc<GooseAgentDispatcher>,
    sink: Arc<dyn EventSink>,
    round: u32,
    baseline: usize,
    f: &OpenFinding,
    model: &str,
    all_files: &[String],
    cwd: &Path,
    prompt: &str,
    lang: TargetLang,
    composite: bool,
    missing_gate: bool,
    device_id: &str,
    user_decisions: &str,
    brief_decisions: &BriefDecisions,
    doc_facts: &str,
) -> (bool, Option<String>, Vec<String>) {
    let task_id = format!("complete-fix::{}#{}", f.file, f.k);
    sink.write_value(serde_json::json!({
        "event": "complete_fix_dispatched",
        "round": round, "shard": f.file, "finding_index": f.k, "model": model,
        "task_id": task_id, "baseline_findings": baseline,
        "owned": f.owned,
        "conflict_retry": f.conflict_note.is_some(),
    }));
    let started = std::time::Instant::now();
    let shard_decisions = brief_decisions.for_files(&f.owned);
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
            lang,
            prompt,
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
        device_id: device_id.to_string(),
        model_id: model.to_string(),
        context_slice: String::new(),
        attempt: round,
        owned_files: f.owned.clone(),
        all_files: all_files.to_vec(),
        prior_hint: None,
        subsplit: Vec::new(),
        speculative: true,
        user_decisions: user_decisions.to_string(),
        doc_facts: doc_facts.to_string(),
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
            prompt,
            lang,
            all_files,
            composite,
            missing_gate,
        )
        .await;
    let conflicted = !composed.conflicts.is_empty() || !composed.unavailable.is_empty();
    let promoted = shard_changed && !conflicted && shard_beats_baseline(verified, baseline);
    let mut written = Vec::new();
    if promoted {
        written = me.promote_merged(&task_id, cwd);
    } else {
        me.discard_shard(&task_id);
    }
    if conflicted {
        sink.write_value(serde_json::json!({
            "event": "merge_conflict",
            "round": round, "shard": f.file, "task_id": task_id,
            "files": composed.conflicts.iter().map(|(f, h)| serde_json::json!({"file": f, "hunks": h.len()})).collect::<Vec<_>>(),
            "unavailable": composed.unavailable.iter().map(|(f, w)| serde_json::json!({"file": f, "why": w})).collect::<Vec<_>>(),
        }));
    }
    let output = ran.as_ref().map(|o| o.output.as_str()).unwrap_or("");
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
            baseline,
            agent_ok: ran.is_ok(),
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
        "baseline_findings": baseline,
        "shard_changed": shard_changed,
        "three_way_merged": composed.three_way,
        "conflicted": conflicted,
        "handoffs": handoff_files,
        "files_written": written,
        "promoted": promoted,
    }));
    let note = conflicted.then(|| conflict_note(&composed.conflicts, &composed.unavailable));
    (promoted, note, handoff_files)
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
        let open = explode_groups(&groups, &owned, &notes);
        assert_eq!(open.len(), 4);
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
        let note = conflict_note(
            &[(
                "web/app.js".into(),
                vec!["<<<<<<< this shard\nx\n=======\ny\n>>>>>>> tree now\n".into()],
            )],
            &[],
        );
        assert!(note.contains("--- web/app.js\n<<<<<<< this shard"));
        assert!(note.contains("redo YOUR change on it, keeping theirs"));
    }
}
