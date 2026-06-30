# SPECULATIVE EXECUTION — design (from workflow wf_e5700d25-b36, 2026-06-30)

User-approved (idle node races the in-flight chokepoint, first-wins). Default-OFF (GOOSE_SWARM_SPECULATE).
CONSTRAINT: 1 task per node — speculative twin only on a GENUINELY IDLE device (in_flight==0), its 1 task.

## CONFIDENCE SPLIT (honest)
- SCHEDULER-side machinery: MED-HIGH (maps 1:1 onto judge/pre-review/replan + the attempts-epoch trick; scheduler_mock-testable, ZERO filesystem).
- DISPATCHER-side shadow+promote (swarm.rs): MED-LOW, CATASTROPHIC blast radius (6000-line file; thread `root` through ~8 cwd sites consistently or a guard re-reads the real tree -> infinite ContentRetry; cp -r of a live tree; on-disk corruption is the deepest risk). PHASE IT: Phase-1 scheduler foundation (flag OFF, byte-identical, tested) THEN Phase-2 dispatcher verified standalone before the flag is ever on.

## SCHEME
"scheme": "END-TO-END, default-OFF behind GOOSE_SWARM_SPECULATE (with the flag off every new map is empty and every new block is skipped, so the validated path is byte-identical).

KEY SIMPLIFICATION vs the DISPATCH+LOCK investigation: because the twin runs in a SHADOW dir and NEVER enters held_files, the file-lock representation is untouched. Only the PRIMARY ever holds the real owned_files (one held_by entry). So NO refcount / last-instance-release is needed. The invariant "two DIFFERENT tasks never co-write a real file" holds trivially: the twin is the same task id, isolated to a shadow; the real tree is written only by the primary until promote, and promote happens under the primary's STILL-active hold after the primary is fully cancelled.

DETECT: a new idle block in scheduler.rs run() inserted between :1498 and :1499 (AFTER replan/judge/pre-review so they get first refusal). Under the State lock, fire ONLY when: self.speculation_enabled && s.ready.is_empty() (pick_assignments :1291 already drained leftovers) && s.total_in_flight()>0 (:200-202) && s.idle_capacity()>0 (:205-211) && s.idle_jobs<s.idle_capacity() && the pre-review scan (:698-705) finds no candidate && s.spec_count<SPEC_CAP.

TARGET: new pick_speculation_target cloned from pick_judge_target (:595-685). Scan TaskState::Claimed (:614) tasks NOT in `speculating`, with elapsed (:617-621) >= a min-age floor (never duplicate a near-done task); pick max by node.fan_out (dag.rs:43) then elapsed. Place on a free device via the enabled&&in_flight<weight predicate (:259/:603/:696) with index != claimed_device[tid] (different host). On pick: devices[dev].in_flight+=1; spec_device[tid]=dev; spec_started_at[tid]=now; spec_count+=1; speculating.insert(tid); build a DispatchRequest cloning description/owned_files/all_files + ctx.slice_for(deps), attempt=n.attempts, speculative:true; emit SpeculativeDispatche

## FILE_ISOLATION
"file_isolation": "The twin NEVER writes the real owned_files. A new field `speculative: bool` is added to DispatchRequest (dispatch.rs:38-57; set false in do_claim's build at scheduler.rs:387-400, true only in pick_speculation_target). In GooseAgentDispatcher::run (swarm.rs:4429-4430), at the very top compute ONE effective root: if req.speculative, create a per-task self-cleaning shadow via tempfile::TempDir (tempfile is already a goose-cli dep, Cargo.toml:41) seeded with `cp -r` of std::env::current_dir(), remember it in a `Mutex<HashMap<task_id,(TempDir,Vec<owned>)>>` on the dispatcher, and use its path as `root`; else root = std::env::current_dir(). Then replace EVERY std::env::current_dir() inside run() with `root`: the layout/owned-path builder (:4449), the owned-parent pre-create (:4474), read_prereview_findings (:4520), existing-owned-content injection (:4530), dependency-source injection (:4568), the hallucination guard (:4708), and the done-gate (:4751). Thread `root` into run_agent to replace self.working_dir.clone() at the create_session call (:2203) (add a working_dir param to run_agent) so the developer extension's resolve_path (developer/edit.rs:215-226) and shell cwd (developer/shell.rs:621-622,649-650) land in the shadow. CRITICAL: leave the .swarm telemetry heartbeat (:2285) on the REAL cwd — the live judge reads it from the real tree; if it followed the shadow the judge goes blind to the twin. ROOT CONSISTENCY: the path-builder, the session working_dir, AND both guards must all use the same `root`, or the guard re-reads the real tree, sees the shadow-written files absent, and fires infinite ContentRetry.

WINNER COPY-BACK: a new trait method `async fn promote_speculative(&self, task_id: &str) {}` with a no-op default (so the MockDispatcher and any non-speculating impl need zero change) (dispatch.rs:85-89). GooseAgentDispatcher's impl copies ONLY t

## FIRST_WINS_ABORT
"first_wins_abort": "The (task,attempt) single-accept is enforced by the existing epoch guard at scheduler.rs:408 PLUS the n.attempts bump on the twin-win path. The hazard the investigation flagged — Done never advances n.attempts (:463-468), so a stale completion could pass :408 — is closed because exactly one of two things happens to the loser, and both make its late complete() a no-op:

PRIMARY WINS: complete(primary) runs the Ok arm (:436-491) ONCE. The appended abort-loser hook (after :491) calls spec_abort[tid].abort() — the twin future is cancelled before it can call resolve_speculation, so resolve_speculation never double-merges/double-relaxes. ctx.merge (:469) and the indegree decrement (:480-491) run exactly once. Device nets to zero: complete() decrements the primary's claimed_device slot (:414-418); the abort-loser hook decrements the twin's spec_device slot. No held_files double-release (twin never held real files).

TWIN WINS: resolve_speculation aborts the primary (:846-848 idiom) and bumps n.attempts (:876 idiom). The primary's spawned future either (i) was already past dispatcher.run and calls complete(primary_tid, old_attempt) — rejected at :408 because n.attempts is now old+1 — OR (ii) is aborted mid-run and never calls complete. Either way the primary contributes NOTHING: no AttemptRecord, no device_speed, no TaskCompleted. The twin then awaits the primary JoinHandle (guarantees the primary has stopped executing, so its writes have ceased before promote), promotes, and runs the accept (Done/merge/relax) ONCE. Device nets to zero: resolve releases the primary slot (:414-418 idiom) and, in phase-2, the twin slot.

RETRY-BUDGET INTEGRITY: a speculative LOSER is pure waste — it pushes NO AttemptRecord, does NOT n.attempts+=1, does NOT count toward max_attempts (:516-524), does NOT pollute device_speed (:438-442). The only n.attempts bump is on tw

## BOUNDING
"bounding": "FOUR independent bounds, all collapsing to the default-OFF no-op when the flag is off:
1. DEFAULT-OFF FLAG: Scheduler.speculation_enabled (new field, false by default) set only via with_speculation(); goose-cli calls it iff GOOSE_SWARM_SPECULATE is truthy at swarm.rs:5316. With it false the new idle block, pick_speculation_target, and all spec_* maps are never touched — byte-identical to today (same discipline as with_judge/with_pre_reviewer).
2. ONLY WHEN NO OTHER WORK: the gate requires ready.is_empty() AND idle_capacity()>0 AND idle_jobs<idle_capacity() AND no pre-review candidate (:698-705) AND an in-flight Claimed chokepoint exists. So replan (more work), judge (unstick), and pre-review (verify) each get first refusal; speculation is strictly last-resort idle use.
3. AT MOST ONE TWIN PER TASK: the `speculating: HashSet<TaskId>` guard — pick_speculation_target skips any task already in it; it is removed only on resolve/abort. A task can never have two concurrent twins.
4. SMALL GLOBAL CAP: spec_count (u32) incremented on dispatch, decremented on resolve/abort, gated by a const SPEC_CAP (start at 1, i.e. at most one speculative copy fleet-wide in the first increment). The twin consumes a real device slot (in_flight++), so it correctly lowers idle_capacity for subsequent ticks — keep its accounting on spec_count + a genuinely-free device, never conflated with idle_jobs (which doesn't decrement in_flight). A distinct SpeculativeDispatched event (NOT dispatched_per_device) keeps the speed-weighted router (:302-308) unskewed.",
      "edit_plan": [
        "dispatch.rs:38-57 — add `pub speculative: bool` to DispatchRequest; dispatch.rs:85-89 — add `async fn promote_speculative(&self, _task_id: &str) {}` no-op default to the TaskDispatcher trait (mocks/existing impls need no change).",
        "scheduler.rs:182-189 — add State fields: spec_device: HashM

## RISKS_AND_TESTS
"risks_and_tests": "CONFIDENCE SPLIT (brutally honest): the SCHEDULER-side machinery (detect/target/gate/bounding/parallel spec_* maps/resolve_speculation/abort-loser/epoch-bump) is MED-HIGH — it maps 1:1 onto the existing judge/pre-review/replan code and the proven attempts-epoch trick, and is fully testable with scheduler_mock with ZERO filesystem involvement. The DISPATCHER-side shadow+promote (swarm.rs) is MED-LOW and is the catastrophic-blast-radius half: it lives in a 6000-line file, must thread one `root` through 8 cwd sites + the session working_dir consistently (a single missed site makes the guards re-read the real tree and fire infinite ContentRetry), and `cp -r` snapshots a live tree. This is why I recommend the phased landing and keeping the flag OFF until Phase 2 is verified standalone.

DEEPEST RISK: on-disk corruption. Mitigated by (1) shadow isolation so the twin never writes the real tree; (2) promote ONLY owned_files, never the whole stale snapshot (else sibling writes are clobbered); (3) await the loser's JoinHandle before promote, defeating tokio's best-effort abort (abort() only drops at the next await, so the primary's last write can otherwise land mid-promote); (4) promote under the primary's still-active held_files lock so files_conflict blocks any sibling claim. Residual: a `cp -r` of a large produced-app dir per twin (acceptable for swarm app dirs, not the 73GB data); and telemetry MUST stay on the real tree or the judge goes blind.

SCHEDULER_MOCK TESTS (no FS; mock promote is the no-op default; mock varies delay by req.speculative):
1. spec_twin_wins_copies_back_and_loser_aborts: chain with a `slow` chokepoint (delay*8) and a free idle device; twin (speculative, fast) wins -> assert Recorder.runs[chokepoint]==2 but the task is Done exactly once, dependents relaxed once (final indegree correct, no early-ready), primary aborted, total i

## EDIT_PLAN
"edit_plan": [
        "dispatch.rs:38-57 — add `pub speculative: bool` to DispatchRequest; dispatch.rs:85-89 — add `async fn promote_speculative(&self, _task_id: &str) {}` no-op default to the TaskDispatcher trait (mocks/existing impls need no change).",
        "scheduler.rs:182-189 — add State fields: spec_device: HashMap<TaskId,usize>, spec_started_at: HashMap<TaskId,Instant>, spec_abort: HashMap<TaskId,AbortHandle>, primary_join: HashMap<TaskId,JoinHandle<()>>, speculating: HashSet<TaskId>, spec_count: u32; init all in run()'s State{...} at scheduler.rs:1256-1287.",
        "scheduler.rs:1194-1206 region — add Scheduler.speculation_enabled: bool (default false) + `pub fn with_speculation(mut self)->Self`. Mirror with_judge.",
        "scheduler.rs:387-400 — in do_claim's DispatchRequest build set `speculative: false`.",
        "scheduler.rs:~686 — add pick_speculation_target(&mut self, min_age_secs, spec_cap) -> Option<(DispatchRequest, usize)> cloned from pick_judge_target (:595-685): Claimed scan skipping `speculating`, min-age floor, max by fan_out then elapsed, free device via :259 predicate with idx!=claimed_device[tid]; on pick bump in_flight + set spec_device/spec_started_at, spec_count+=1, speculating.insert, emit SpeculativeDispatched; NO Claimed/held_files/attempts.",
        "scheduler.rs:~590 — add resolve_speculation: (A) twin Err or task!=Claimed -> spec cleanup only (in_flight-- on spec_device, drop spec_* , spec_count--, speculating.remove); (B) twin Ok && Claimed -> abort primary (:846-848), n.attempts+=1 (:876), release primary device (:414-418), keep held_files, take primary_join handle, return it + (so the future awaits it, then calls dispatcher.promote_speculative, then a phase-2 fn: release held_files :419-423, Done+result, ctx.merge :469, relax-dependents :480-491, emit TaskCompleted, spec cleanup).",
        "scheduler.rs:491 — append abort-loser to complete()'s Ok arm: if spec_abort.remove(tid) -> handle.abort(); decrement spec_device in_flight; drop spec_started_at/primary_join/speculating/spec_count for tid.",
        "scheduler.rs:1301-1318 — when speculation_enabled, also store the PRIMARY JoinHandle in primary_join (and keep abort_handles populated, currently judge-only at :1311-1317) so the twin-win path can await the loser's full cancellation.",
        "scheduler.rs:1498->1499 — insert the gated speculative idle block (gate per `bounding`), spawn modeled on :1301-1308, store spec_abort[tid]=jh.abort_handle(); the future runs twin, resolves, awaits primary_join on win, promotes, finalizes, notifies.",
        "scheduler.rs:1505 — extend short-tick gate with `|| self.speculation_enabled`.",
        "scheduler.rs — in pick_judge_target (:613) and apply_split selection, skip any Claimed task in `speculating` (judge/split coherence).",
        "event.rs — add SpeculativeDispatched + SpeculativeResolved events (excluded from dispatched_per_device / device_speed).",
        "swarm.rs:4430 (GooseAgentDispatcher::run) — compute one `root`: req.speculative -> tempfile::TempDir cp -r of current_dir, remembered in a new Mutex<HashMap<task_id,(TempDir,Vec<String>)>> field; else current_dir. Replace std::env::current_dir() at :4449,:4474,:4520,:4530,:4568,:4708,:4751 with `root`. Leave the .swarm telemetry at :2285 on the REAL cwd.",
        "swarm.rs:2203 — thread `root` into run_agent (add a working_dir param) t

## PHASE-2 FEASIBILITY — CONFIRMED in-scope (the key de-risking finding, 2026-06-30)
The Phase-2 blocker was: can a speculative TWIN be isolated to a shadow workspace WITHOUT modifying core
goose (out of scope)? Researched the dispatcher + agent + developer extension. ANSWER: YES, in scope.
- The developer extension writes via a PER-CALL working_dir: crates/goose/.../developer/mod.rs passes
  ctx.working_dir to shell_with_cwd / file_write_with_cwd / file_edit_with_cwd / tree_with_cwd (tested at
  developer_client_uses_working_dir_for_file_tools). So file tools are NOT hardcoded to process cwd.
- ctx.working_dir comes from session.working_dir (agent.rs reads session.working_dir into the ToolCallContext).
- session.working_dir is set at SESSION CREATION: SessionManager::create_session(working_dir: PathBuf, ...)
  (session_manager.rs:372). The swarm dispatcher ALREADY calls self.session_manager.create_session(...) with
  self.working_dir (swarm.rs ~2298-2300). So the dispatcher controls the agent file-write root via the
  working_dir it passes to create_session.
=> PHASE-2 DESIGN (in scope, no core change): in GooseAgentDispatcher::run, when req.speculative, (1) build a
shadow = cp -r of self.working_dir into a tempfile::TempDir (EXCLUDE node_modules/.git/.swarm for speed), (2)
create the twin agent session with working_dir = shadow (instead of self.working_dir) -> the agent writes ONLY
the shadow, (3) thread the shadow as `root` through the dispatcher OWN post-check reads (the
current_dir().unwrap_or(self.working_dir) sites at ~2382/4059/4271/4854) so the twin syntax-check/prereview
read the shadow, (4) store task_id -> TempDir on the dispatcher, (5) promote_speculative(task_id) copies ONLY
the winner owned_files shadow->real (never a blind cp -r). The PROCESS cwd is never changed -> thread-safe with
concurrent normal tasks. STANDALONE TEST before any flag-on: a speculative dispatch writes only inside the
TempDir + the real tree is byte-identical; promote copies exactly the owned_files. Confidence now MED (was
MED-LOW) — the isolation mechanism is a supported API, not a hack. STILL the riskiest change (promote touches
the real tree) -> flag OFF until the standalone test + adversarial review are GREEN.

## PHASE-2 VERDICT (2026-06-30): the cwd-shadow is NOT a real jail -> GOOSE_SWARM_SPECULATE STAYS OFF
Adversarial review of the Phase-2 dispatcher shadow (commit e54b98285) found the headline invariant FALSE:
a cwd shadow contains only RELATIVE-path editor writes. The developer SHELL tool (sets child cwd, no
chroot/seccomp) and ABSOLUTE-path editor writes (passed through verbatim by core goose edit.rs) BYPASS the
shadow -> a misbehaving twin could write outside it. The root cause is in CORE GOOSE (developer edit.rs /
shell.rs), OUT OF the editable swarm scope. So a true isolation GUARANTEE is not achievable in scope.
Mitigating reality (why it is not as scary as "catastrophic" first sounds): (1) the shadow is in OS temp, so
`cd ..` reaches /tmp, not the project; (2) the worker is shown ONLY the shadow path (the layout cwd = root =
shadow), so it has no real-path to target; (3) the "real tree" for a swarm run is the SCRATCH APP DIR the
user cd-d into (an eval app being built), NOT ~/.important files; (4) the PRIMARY worker ALREADY has
unsandboxed shell today, so a twin adds NO new risk CLASS — only a second concurrent writer, and only if it
uses an absolute path it never sees. So the PRACTICAL containment is strong; the GUARANTEE is absent.
DECISION (made as the operator, honoring the user safety emphasis + the catastrophic framing): keep
GOOSE_SWARM_SPECULATE OFF. The plumbing stays LANDED + DORMANT (default-OFF, byte-identical, both halves
built + reviewed, the file-ops + flag-gating proven) so it is ready IF a real sandbox is ever added (a
container/chroot per twin, or an abs-path reject in the developer extension — both core-goose, out of scope).
The remaining wire-up (resolve_speculation -> promote) is NOT done, since completing an un-enableable feature
is low-value; it is a ~20-line scheduler change documented in the loop notes if isolation is ever solved.
IDLE-NODE PROBLEM (what speculative was meant to solve) — safe in-scope status: Phase-1 (idle-node judge +
pre-review) already fills idle nodes whenever there is review work. The remaining gap is a serial chokepoint
with NO review work left. The SAFE alternative is FLATTER DAGS (the architect already caps dependency depth
at 2 + discourages chokepoints; can be strengthened), which reduces idle moments with ZERO corruption risk.
The recursive-ceiling apps (APP6/APP8) have an IRREDUCIBLE chokepoint (one hard algorithm) that no DAG
flattening removes — there the idle is inherent to a capability-bound task.
