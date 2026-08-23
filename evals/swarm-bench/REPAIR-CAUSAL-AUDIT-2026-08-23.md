# Swarm repair causal audit — 2026-08-23

## Scope, authority, and non-actions

This is the durable, read-only audit of the swarm's repair, verification, salvage, and ship-best
slice. The current source was inspected at `10ab496465a7b9cdff94298ec64cde8e8f87c03e`
(`codex/swarm-engine-overhaul`). The historical Qwen3.8 r1 engine identifies itself separately as
`build_sha: 2543a01ab-dirty`; historical facts below come from that run's own event and artifact
records, not from an assumption that its binary equals current HEAD.

The audit did not change SB7, any scorer, engine source, LM Studio, the roster, or a running fleet. It
did not start or resume a benchmark. The requested safety boundary remains binding: the SB7 task and
hermetic scorer are external outcome oracles and must not be modified or used as a repair prompt.

Primary evidence:

- current engine: `crates/goose-cli/src/commands/swarm.rs`,
  `crates/goose-swarm/src/{scheduler,dag,dispatch,event}.rs`;
- r1 events:
  `/Users/mihaiperdum/Projects/goose/evals/swarm-bench/runs/sb7-fleet38/swarm-3node-r1/runs/sb7-fleet38/swarm-3node-r1/run.jsonl`;
- r1 round-1 snapshot and shipped tree:
  `/Users/mihaiperdum/Projects/goose/evals/swarm-bench/runs/sb7-fleet38/swarm-3node-r1/.swarm/best-tree`
  and its parent run directory;
- r2 events:
  `/Users/mihaiperdum/Projects/goose/evals/swarm-bench/runs/sb7-fleet38/swarm-3node-r2/runs/sb7-fleet38/swarm-3node-r2/run.jsonl`;
- the previously sealed whole-engine audit:
  `evals/swarm-bench/SWARM-ENGINE-AUDIT-2026-08-22.md`.

## Verdict

Repair is not one mechanism. It is a set of mutually inconsistent paths with different isolation,
scheduling, supervision, verification, and stopping semantics. The path Qwen r1 actually used was the
hand-rolled file fan, not the real scheduler. It spent almost five hours in post-execute gates, left at
least roughly half of the three physical hosts' available node-minutes idle, promoted four candidates
on an unsafe scalar comparison, remained red, and then let two direct writers mutate six source files
after the last complete verification. The final hermetic score was `0.0169`.

The decisive repair defects are:

1. A full verify does not seal the tree. Boot repair and AST wire repair mutate the real tree after
   ship-best and/or `complete_result`, then run only their narrow probe.
2. Finding count is not causal identity, severity, or monotonic quality. A lower count can close a cheap
   symptom while introducing a different critical failure. Independently promoting several candidates
   against one stale baseline does not prove their composition improves anything.
3. A stalled worker with bytes on disk can be laundered into `Done`; the final report then drops the
   salvage fact. File existence is not task completion and cannot license green.
4. The default repair fan bypasses the scheduler and semantic judge. Its progress sampler observes but
   cannot supervise; completed nodes cannot be reassigned to useful, version-current review of the tail.
5. Fan width is derived from logical slots and file attribution, not from causal independence plus
   physical-host occupancy. Qwen r1 sent two simultaneous long repairs to one Workhorse model instance
   while other physical nodes later idled behind the tail.
6. `uncapped` removes some walls but leaves structural termination ceilings and a one-week stand-in.
   Several of those ceilings decide that no more value exists using count or attempt number rather than
   causal evidence and semantic judgement.

The minimum safe implementation order is therefore fixed:

1. prohibit post-gate mutation;
2. make salvage an explicit provisional state;
3. introduce causal finding identity and severity evidence;
4. use one hermetic repair ruler and one transactional promotion path;
5. move all repair through the scheduler and physical request broker;
6. remove hard structural ceilings in favour of evidence-based continuation.

Changing fan width or turning `fix_sched` on before steps 1–4 would make unsafe mutation faster. Removing
ceilings before causal progress exists would make non-value-producing loops longer. This ordering is about
correctness confidence and blast radius, not effort.

## Current source map

Line numbers below identify the audited current commit. Symbols are the durable locator if concurrent work
moves the lines.

### Verification and finding construction

- `run_swarm` enters COMPLETE at `swarm.rs:38967`. It builds one `SmokeResult`, then appends failed-task,
  missing-deliverable, HTTP-timeout, DOM-id, CSS-coherence, cross-module, and optional spec-contract
  strings before emitting `complete_verify` (`39141–39587`).
- `one_ruler_grade` (`35780–35849`) re-runs the same categories for a candidate but returns only
  `(Option<usize>, established)`. All identity, provenance, severity, and before/after relation are erased.
- `group_findings_by_file` (`29416–29449`) deduplicates only byte-identical strings and attributes each
  finding to at most one file via `extract_file_from_finding` (`29291–29413`). A cross-module root cause is
  therefore reduced to a subject-first file guess.
- `fix_evidence_pointers` (`29624–29676`) truncates the evidence path list to 12. Smoke/test findings are
  themselves commonly tail-truncated to 40 lines by the call sites around `run_smoke_gate`. Evidence not
  fitting those presentation bounds is unavailable to the repair prompt instead of remaining addressable
  by ID.
- `shard_owned_files` (`29515–29566`) expands a test file to a same-basename implementation sibling and
  JS/CSS to an HTML sibling. These are useful heuristics, but they are not proof of causal ownership and do
  not generalize to arbitrary applications.

### Repair mutation paths

| Path | Current predicate | Writes | Scheduler / judge | Promotion authority |
|---|---|---|---|---|
| whole-tree race | `spec_repair && fleet_models.len()>1 && !shard_this_round`, `39860–40102` | isolated shadows | hand fan; no scheduler judge | `pick_repair_winner`: lowest count below baseline |
| scheduled file DAG | `fix_sched && shard_this_round`, `40103–40277` | isolated shadows | fresh Scheduler; judge is observe-only in fix rounds | each task: count below round baseline |
| hand file fan | `complete_parallel || shard_this_round`, `40279–40431` | isolated shadows, independently promoted | hand fan; progress sampler only | each shard: count below same baseline |
| unassigned join | after hand fan, `40432–40528` | whole-tree shadow | direct dispatcher call | count below post-wave baseline |
| serial COMPLETE fix | fallback, `40529–40595` | real tree directly | none | agent bytes land immediately; next round may verify |
| ship-best restore | `40610–40681` | real tree via rsync | none | minimum count with established preference |
| Python boot repair | after ship-best, `40682–40783` | real tree directly; owns no files | none | boot probe only |
| standalone smoke fix | COMPLETE off, `40903–40985` | real tree directly; owns no files | none | smoke gate only; advisory exit |
| AST wire fix | after COMPLETE result, `41144–41191` | real tree directly; owns no files | none | AST import review only |

The `run_fix_task` scheduled path (`30373–30570`) is the most disciplined existing writer: forced shadow,
grade-what-lands preview, and one-shot scheduler parity. It is not sufficient as-is:

- `Scheduler::apply_judge_outcome` deliberately turns all fix-round split/terminal/redispatch actions into
  observation (`scheduler.rs:2331–2335`), so “scheduler path” does not currently mean useful supervision;
- fix-run sink-review findings are drained and explicitly dropped as informational at
  `swarm.rs:40217–40235`;
- each file candidate still compares a scalar total to the same stale baseline;
- concurrent successful candidates land separately instead of being composed and verified atomically.

### Promotion and ship-best logic

- `shard_beats_baseline` (`35075–35084`) accepts `verified < baseline`.
- `pick_repair_winner` (`35115–35134`) selects the minimum count, with earliest task as tie breaker.
- `grade_promotion_preview` (`16614` in `GooseAgentDispatcher`) correctly previews the bytes that would
  land and refuses a byte-identical no-op. That byte-level improvement should be preserved.
- Neither helper proves that the candidate's assigned defect disappeared. Neither records which old defects
  persist or which new defects appeared. Severity is absent.
- Hand shards all grade against the round-opening baseline and then promote independently. “A improves B”
  and “C improves B” does not entail “compose(A,C) improves B”. The next round catches some interactions,
  but only after the real tree has already mutated.
- ship-best selects by scalar count plus `established`; it cannot express incomparable trees such as “boot
  fixed but data corruption introduced” versus “one noncritical style finding remains”.

### Salvage and verification paths

There are three separate degraded-completion mechanisms:

1. Dispatcher progress-watchdog salvage in `swarm.rs:32099–32131` returns
   `TaskRunOutput { salvaged: true }` when every owned file is present/nonempty/non-skeleton after a
   “no productive progress” error. It does not exclude test tasks and performs no acceptance check.
2. Scheduler `degrade_on_stall` in `scheduler.rs:1569–1640` changes an exhausted task to `Done` when
   `should_degrade_on_stall` accepts it. It is stricter for critical owned files but treats owns-nothing
   verification work as degradable.
3. Scheduler finalize-spin salvage in `scheduler.rs:2427–2489` changes `Looping` to `Done`. It excludes
   test tasks, but default `GOOSE_SWARM_SALVAGE_REQUIRE_CRITICAL=false` makes
   `owned_file_written` use `.any()` rather than require every critical deliverable.

All three relax dependents as though the task completed. `TaskState` has only Pending/Ready/Claimed/Done/
Failed (`dag.rs:92–99`), so provisional work has nowhere honest to live. `TaskCompleted` carries a boolean,
but `TaskOutcome` and `RunReport` do not (`scheduler.rs:611–651`). This is why the final artifact loses the
fact even when the transient event recorded it.

### Current caps and hidden ceilings in this slice

| Mechanism | Default / hard shape | Why it is not causal progress |
|---|---|---|
| COMPLETE fix rounds | default 2; clamped 1–6 (`complete_rounds_from`) | task/finding number is not evidence of exhaustion |
| dynamic extension | hard `round < 6` | uncapped still stops at six |
| stall exit | any configured value >=1 stops after one non-decreasing count | equal count with different identities/severity may be major progress |
| strategy switch | one global boolean switch | a new causal hypothesis may require the same mechanism again |
| fix attempt wall | 1200s, clamped 120–3600; uncapped returns one week; tree scaling up to the sink scale | time does not distinguish a slow valuable call from a loop |
| COMPLETE wall | default 3000s, lifted to fit advertised rounds; uncapped starts at one week | finite stand-in and round geometry remain |
| scheduled fix retries | `Scheduler::new(..., 1)` | one attempt regardless of new evidence |
| scheduled stillborn | zero tool calls at 300s | only the scheduled path has it; a fixed time should summon semantic review, not decide termination |
| boot repair | 3 attempts, or 1 after deadline; identical traceback stops | traceback equality is useful evidence, not proof no new repair hypothesis exists |
| standalone smoke | exactly 1 fix | arbitrary structural ceiling |
| AST wire repair | exactly 1 fix | arbitrary structural ceiling and happens after COMPLETE result |
| repair evidence paths | 12 | a presentation bound discards causal evidence |
| event finding texts | 12, each middle-elided | acceptable only for display if full evidence remains addressable elsewhere; currently it does not |

Pure dead-stream/provider-terminal failsafes are a separate safety class. They may remain as infrastructure
protection, but must produce a resumable/provisional outcome; they must not become a correctness verdict.

## Qwen3.8 r1 reconstruction

### Phase and quality outcome

The r1 `run_finished` event reports:

- research `6.1m`;
- planning `129.9m`;
- execute `139.4m`;
- gates `298.0m`;
- total `573.5m`.

The sealed whole-engine audit records the final hermetic score as `0.0169`: inner quality `0.404`, pre-
severity excellence `0.3625`, critical multiplier `0.0467`. After 9.6 hours, required surfaces including
`web/app.js`, `web/viz.js`, and `/api/events` were absent. Repair time did not convert into central value.

Resolved levers relevant to this phase were:

- `complete=true`, `spec_repair=true`, `sink_shard=true`, `ship_best=true`;
- `fix_sched=false`, so the actual multi-file rounds used the hand fan;
- `judge_nudge=false`, `supervision_pool=false`;
- `read_on_fix=true` in r1 even though its current config default is false;
- `salvage_spin=true`, `salvage_require_critical=false`, `degrade_on_stall=false`;
- uncapped effective values (`max_turns=100000`; observed repair calls exceed the ordinary 3600s clamp).

No `judge_verdict` appears after `integrate-verify` was accepted at
`2026-08-22T08:14:05.608910Z`. The entire COMPLETE/boot/wire tail was semantically unsupervised.

### Exact repair chronology

| UTC | Event | Evidence |
|---|---|---|
| 08:14:10 | round-0 verify | 5 findings; established snapshot saved |
| 08:14:10 | four file fixes dispatched | ledgerd→gabee, config→workhorse, styles→mihai, api→workhorse |
| 08:39:17 | styles completes | 1507s, 5→4, promoted |
| 08:47:51 | ledgerd completes | 2020s, 5→4, promoted |
| 09:36:10 | config completes | 4919s, agent not OK, 5→4, promoted from partial tree |
| 10:33:42 | api completes | 8371s, 5→3, promoted |
| 10:33:46–10:35:55 | unassigned join | 128s, no established grade/promotion |
| 10:36:00 | round-1 verify | 3 findings; new best snapshot |
| 10:36:00–10:49:23 | db fix | 802s, 3→3, discarded |
| 10:36:00–11:48:05 | notifierd fix | 4325s, 3→18, discarded |
| 11:48:10–12:26:18 | unassigned join | 2287s, 3→3, discarded |
| 12:26:22 | round-2 verify/stall exit | still 3 findings |
| 12:26:27–12:56:28 | boot repair | direct real-tree write; narrow boot probe reports repaired |
| 12:56:28 | `complete_result` | `passed=false`, `verified=false`, 3 findings |
| 12:56:29–13:12:07 | AST wire fix | direct real-tree write; AST-only review reports clean |
| 13:12:07 | `run_finished` | no post-wire complete verification |

The eight recorded fix intervals sum to `24,359s = 405.98 request-minutes`: six file-target attempts plus
the two unassigned joins. Boot adds approximately `1,801.8s = 30.03m`; wire adds
`938.3s = 15.64m`.

Across 298.0 gate minutes, three physical hosts provide 894 physical node-minutes. Treating every recorded
fix interval, including its local grade tail, as if it occupied a distinct model host, and treating all of
boot/wire wall as model-busy, gives an intentionally generous upper bound of about 451.7 occupied
node-minutes. Therefore at least about `442 physical node-minutes` were idle. This is a lower bound: two
round-0 requests overlapped on the same Workhorse model, and event `secs` includes non-decode local grading,
both of which overstate physical model occupancy.

The progress events expose the value problem, not just the wall:

- `app/api.py`: first shadow change at 5,340s (89m), longest still window 5,220s (87m), total 8,371s;
- `app/config.py`: first change 840s, longest still 1,320s;
- `app/notifierd.py`: first change 300s, longest still 2,280s, then grades 18 findings against 3;
- completed nodes were not assigned a semantic review, targeted verification, or causal analysis of the
  long tail. The fan waited at its barrier.

The pool recorded three devices, each `weight:2` but `instances:1`. The hand file fan uses
`fleet_slot_models`, so a physical model appears twice. Round 0 consequently sent both `app/config.py` and
`app/api.py` to the Workhorse model concurrently. The current source already acknowledges elsewhere that
stacking two Apple-host generations degrades aggregate progress and uses `one_lane_per_host` for prologue
fans; repair does not apply the same physical-host discipline.

### Unsafe scalar promotion proven by r1

All four round-0 shards compared against the same count of 5 and promoted separately. The events establish
only these scalar statements: three candidates measured 4 and one measured 3. They do not establish:

- that the finding assigned to each candidate disappeared;
- that a candidate did not replace it with a more severe defect;
- that all four promoted byte sets compose to the candidate trees that were graded;
- that duplicated whole-app diagnosis across four agents produced independent value.

The next real-tree verify measured 3 findings, so the composition happened not to increase the scalar total
in that round. It remained unable to boot and still failed collection. That is not evidence that all four
promotions were safe or valuable.

### Salvage contradiction proven by r1

`test-webhook` emitted:

```text
task_completed status=done salvaged=true elapsed_ms=1885796
```

It took the dispatcher progress-watchdog salvage path even though scheduler finalize-spin explicitly says
test tasks are never salvaged. In the final `run_finished.report.tasks` record for `test-webhook`, the
`salvaged` field is absent/null; it appears as ordinary `done` with an `ok` attempt. The transient event and
final report therefore disagree about the nature of completion.

### Cryptographic artifact proof of post-gate mutation

The round-1 best snapshot and shipped files have different SHA-256 values:

| File | round-1 snapshot SHA-256 | shipped SHA-256 | shipped mtime (Europe/Bucharest) |
|---|---|---|---|
| `app/db.py` | `ab9b6e3c989d48133b6194bd00e5334e1e771a9c3661b85e96b15e12b1ed05ae` | `13114f83ec10a0ec2590c565770951d04bf6c26bd53b4c8f374ce4262429b189` | 15:39:40 |
| `app/events.py` | `afc3fd21fb560981668363e5c600cfe510613b623b8c3b1e15e38a5085ea5dcc` | `df183e89ef24fa8bce8da9291e6c8c4b1d3afe8b0ece769dfb194367f0a244e7` | 15:40:31 |
| `app/sync.py` | `9aa7b297fc271e0c8b43cb58e953fdcde5cf3b9584d83a934b7d77a8b57aaf96` | `a81442977f9e047e54577a494cbcacd40af84f87a05f5cfe1b422ef1de9f46a3` | 15:56:06 |
| `app/ledgerd.py` | `54aafdf259c3598d5010081fc163e1c811b346682bd93dc85e1bc60e2b4b96dc` | `715e8a6f24a933c8f8e97fe11e72338fe0ef9a42443bf8f07c10dd4b289a5713` | 16:05:38 |
| `app/__main__.py` | `79ed915772b91f767509b4429fefd0680124ee52c3494de43d30a9c2b9aa3850` | `50eaca18643afd6de68a500fdde6a4c66c6f4e6a18acb4a98bd63ff5d69ba76a` | 16:07:10 |
| `app/notifierd.py` | `132ff1a55d3947c1fba7038c10fd4915eea1aa12dc6f72873e108fa958c48470` | `c2bd475ee1321a8a278354caf76392bfcf339fc154ac8f7a803e72fb4a79a697` | 16:07:10 |

The first three shipped mtimes fall inside the boot-repair window (15:26:27–15:56:28 local). The last
three fall inside the wire-fix window (15:56:29–16:12:07 local). No candidate promotion occurred after the
round-1 snapshot; the round-1 file candidates and join all emitted `promoted:false`. The only model writers
after that point were boot repair and wire fix.

Mtime caveat: rsync snapshots preserve source mtimes, so a snapshot file's mtime is not the time the
snapshot was taken. Mtime alone cannot attribute authorship. The conclusion rests on the combination of
different cryptographic bytes, exact event windows, no intervening successful promotion, current direct-
write path semantics, and shipped-file mtimes falling within those two windows. The evidence proves the
shipped tree was not the last fully verified tree and identifies the only recorded writer windows; it does
not claim which individual tool call changed each line because repair tool calls were not emitted.

## Qwen3.8 r2 reconciliation

R2 is negative evidence for repair. Its `run.jsonl` contains 26 `detail_completed` events and no
`plan_loaded`, `task_dispatched`, `complete_verify`, `complete_fix_*`, `complete_result`, or `run_finished`.
It was stopped in planning while one of 27 detail requests remained outstanding. Therefore:

- r2 proves the same general long-generation/barrier risk exists before repair;
- r2 does not validate or falsify any repair promotion, salvage, ship-best, or repair scheduler theory;
- combining r2's planning tail with r1's repair timings as though they were two repair samples would be a
  category error.

The r2 detail-tail fixture remains useful for the shared physical request broker and staged idle-work
policy, but the repair implementation must be falsified against r1 and new frozen-spec runs.

## Target design: causal defect ledger

### Evidence model

Add `crates/goose-cli/src/commands/swarm/repair.rs` and keep `swarm.rs` as the phase orchestrator. The new
module should own these engine types (names may vary, semantics may not):

```rust
struct DefectObservation {
    id: DefectId,
    gate: GateId,
    requirement_ids: BTreeSet<RequirementId>,
    subjects: BTreeSet<SubjectRef>,
    kind: DefectKind,
    impact: ImpactEvidence,
    evidence: Vec<EvidenceRef>,
    first_seen_tree: TreeEpoch,
    last_seen_tree: TreeEpoch,
}

struct RepairCandidate {
    id: CandidateId,
    base_tree: TreeEpoch,
    targets: BTreeSet<DefectId>,
    changed_subjects: BTreeSet<SubjectRef>,
    shadow: PathBuf,
    worker_evidence: WorkerEvidence,
}

struct CandidateDelta {
    closed: BTreeSet<DefectId>,
    persisted: BTreeSet<DefectId>,
    introduced: BTreeSet<DefectId>,
    changed_evidence: BTreeSet<DefectId>,
    established: bool,
}
```

`DefectId` must be stable across volatile ports, temp paths, traceback line numbers, ordering, and changing
failure summaries. It derives from gate identity plus a normalized invariant/requirement and all causal
subjects, not the full rendered string. Full stdout/stderr belongs in immutable evidence blobs under
`.swarm/repair/evidence/`; prompts and events carry references, so there is no need for a 12-path or 800-
character truth cap.

Severity is evidence, not a magic integer. Mechanically established impact records facts such as “advertised
entry did not bind”, “required artifact absent”, “named test node failed”, or “check did not establish a
verdict”. Semantic importance relative to the user's normalized requirements is assigned by the semantic
judge with requirement/evidence citations. The engine validates references and state transitions; it does
not invent a deterministic semantic-correctness decision.

Finding-to-task construction becomes many-to-many and causal:

- engine supplies the full defect ledger, typed requirement/interface graph, changed files, and available
  physical hosts;
- a response-only semantic planner/judge proposes causal clusters and exact target defect IDs;
- structural validation requires every target to have one primary repair owner, permits shared verification
  consumers, rejects overlapping writes, and preserves dependency/interface edges;
- no rule creates a task because a filename count, token count, or idle node exists;
- if one defect is indivisible, idle capacity performs alternative-hypothesis review, evidence collection,
  candidate review, or targeted verification. It does not fabricate a second writer.

### One promotion transaction

Replace scalar `one_ruler_grade`, `shard_beats_baseline`, and `pick_repair_winner` as mutation authority with
one transaction:

1. Freeze `TreeEpoch { content_hash, ledger_hash }`.
2. Generate every repair candidate in a shadow rooted at that epoch.
3. Run the target defect's cheapest exact probe in the shadow for feedback.
4. Give a semantic judge the target IDs, requirement slices, exact diff, target-probe evidence, and current
   ledger. It may accept, nudge/revise, propose a causally valid split, or abstain. It never writes.
5. Compose all independently accepted, non-overlapping candidates in one preview. A candidate completing
   early need not wait idle: review and targeted probes start immediately; the coordinator may promote a
   ready subset after composing against the current epoch. Outstanding candidates are rebased and regraded,
   never blindly landed from a stale baseline.
6. Run one full repair ruler on the exact composed preview. It must call the same implementation that opens
   the round and return a structured ledger, not a count.
7. Promotion requires: the base epoch still matches; every targeted defect is closed or explicitly returned
   for revision; no new mechanically blocking observation exists; all required gate legs established; and
   semantic review accepted the requirement-level delta. An equal or lower count is irrelevant.
8. Atomically land the composed bytes once, then run the same ruler on the real tree. The real-tree ledger
   and tree hash must equal the preview result. On mismatch, restore the pre-promotion epoch and record it.

This preserves deterministic mechanical evidence without allowing a deterministic heuristic to declare
semantic correctness. The judge remains the semantic quality authority; the engine remains the mutation,
evidence-integrity, and rollback authority.

Ship-best becomes a checkpoint chain, not a minimum count. Every accepted promotion creates an immutable
parented epoch. A later candidate that introduces a blocker never lands. If a post-promotion real-tree check
does not match its preview, rollback is exact. There is consequently no “best” scalar ordering across
incomparable trees and no need to choose a boot-broken tree over a requirement-broken tree by arithmetic.

### Scheduler and useful-idle policy

All repair work must enter `goose-swarm::Scheduler`; delete the hand fan and direct serial writer after
parity. Add an explicit repair task kind to `TaskSpec`/`DispatchRequest` instead of inferring repair from
`fix::` names and empty ownership. The scheduler owns one pipeline:

```text
candidate generation -> target probe -> semantic review -> composed full ruler -> promotion/rebase
```

The device model must distinguish:

- physical host / endpoint;
- loaded model instance;
- active decode permit;
- scheduling share/weight;
- supervision capability;
- provider request state: queued, prefilling, decoding, cancellation requested, provider terminal.

R1 had `weight:2, instances:1` on every host. `weight` must not silently mean two simultaneous physical
decoders. Use endpoint/roster-reported instances and measured concurrency throughput for decode permits;
keep weight as scheduling share. Fill distinct physical hosts before a second lane on one host. Permit more
concurrency only when the backend reports it and telemetry shows aggregate progress improves.

When a host becomes physically idle and version-current useful work exists, the priority is:

1. target probe for a completed candidate;
2. semantic review of a candidate or long-running attempt's current evidence snapshot;
3. evidence/research needed by an open defect;
4. an alternative repair hypothesis the semantic judge identified as genuinely independent.

Do not synthesize judge work merely to report occupancy. Every review carries the tree epoch and evidence
fingerprint; an unchanged snapshot is not reviewed twice. A long generation is not deterministically killed
for duration or volume. Recurrence/no-progress evidence summons the semantic judge and, where the provider
supports safe continuation, sends an in-session nudge. A replacement request starts only after provider-
terminal evidence releases the physical permit. This preserves the user's judge-nudge behavior and avoids a
second request racing an unproven live one.

### Evidence-based continuation, no hard structural ceilings

After transactional promotion and causal identity are proven, replace repair control values with optional
operator walls and progress state:

- `None`, not `UNCAPPED_SECS`, represents no deadline;
- remove the six-round ceiling, one-switch ceiling, boot 3/1, standalone one-shot, and wire one-shot;
- do not stop on a non-decreasing count;
- continue while a new candidate closes a target, changes its evidence, produces a new semantic hypothesis,
  or the judge identifies actionable work;
- when tree epoch, defect ledger, attempted hypothesis, and evidence fingerprint all recur unchanged, ask the
  semantic judge whether to nudge, revise scope, try an alternative, or declare no actionable hypothesis;
- an infrastructure-terminal event may pause/requeue provisionally, never become “correct” or “Done”.

The loop may run a long time when it is acquiring evidence or closing causal defects. It must not spend a
node repeating a hypothesis against an unchanged epoch with no new evidence.

## Minimum safe implementation sequence

Each increment is separately revertible and must pass its gate before the next begins.

### 1. Seal the verified tree before any further writer

Files/functions:

- `crates/goose-cli/src/commands/swarm.rs`: move boot observations and AST observations into the COMPLETE
  ledger before final result; route boot, wire, standalone smoke, and serial fixes through existing shadow/
  preview mechanics; emit `complete_result` only after the final full ruler.
- `GooseAgentDispatcher::run_task_inner`: reject a repair-mode `speculative:false` dispatch.
- introduce `TreeEpoch` and a type-level `RepairMutationLease` owned only by the promotion transaction.

Events:

- `repair_tree_opened { epoch, hash }`;
- `repair_candidate_created/graded/promoted`;
- `repair_tree_sealed { epoch, hash, ledger_hash }`;
- `repair_post_seal_mutation_detected` (must remain zero; rollback/fail honest if seen).

Tests/falsification:

- replay r1 chronology and prove boot/wire bytes cannot change the sealed tree without a full ruler and
  promotion event;
- hash the tree at `complete_result` and immediately before `run_finished`; unexplained drift fails;
- candidate whose narrow boot probe passes but full ruler regresses remains in shadow;
- no source search hit for a post-COMPLETE repair `DispatchRequest { speculative:false, .. }`.

### 2. Add explicit `Salvaged` / provisional state

Files/functions:

- `crates/goose-swarm/src/dag.rs`: add `TaskState::Salvaged` (or `Provisional`, but not `Done`);
- `scheduler.rs`: change dispatcher-watchdog, `degrade_on_stall`, and finalize-spin transitions; dependents may
  run, but final truth retains the provisional state;
- `dispatch.rs`: replace the boolean-only `TaskRunOutput.salvaged` with a typed completion disposition;
- `event.rs`, `RunReport`, and `TaskOutcome`: persist status, salvage reason, artifact hashes, and required
  verification; keep compatibility fields only as derived values;
- `swarm.rs`: create ledger observations for every provisional task and close them only through the causal
  verification pipeline.

Tests/falsification:

- exact r1 `test-webhook` fixture remains `salvaged` in `run_finished`, never ordinary `done`;
- test tasks, manifest-only tasks, partial multi-file owners, and owns-nothing verifiers cannot license green
  merely through salvage;
- a genuinely complete artifact may proceed to its dependents and later close through verification without
  paying a cold rebuild.

### 3. Introduce causal finding identity and severity evidence

Files/functions:

- new `crates/goose-cli/src/commands/swarm/repair.rs`;
- extend `SmokeResult`, `run_smoke_gate`, `run_spec_contract`, `http_timeout_scan`, `dom_id_scan`,
  `css_coherence_scan`, `cross_module_drift`, failed-task and missing-deliverable producers to emit structured
  observations plus backward-compatible rendered text;
- replace single-file `extract_file_from_finding` authority with many-subject evidence links;
- persist full evidence blobs; remove truth truncation from repair inputs.

Events:

- `repair_ledger_opened`;
- `repair_defect_observed { defect_id, gate, requirements, subjects, impact, evidence_refs }`;
- `repair_ledger_delta { closed, persisted, introduced, changed_evidence }`;
- `repair_semantic_review { candidate, verdict, cited_requirements, cited_evidence }`.

Tests/falsification:

- volatile temp paths, random ports, order, and traceback lines retain one defect ID;
- two different defects with count 1 never compare equal;
- a candidate closing two minor observations while introducing one boot blocker is rejected;
- a cross-module finding retains both producer and consumer subjects;
- semantic impact without valid requirement/evidence citations is rejected as malformed, not treated as
  engine truth.

### 4. One hermetic ruler and one atomic promotion

Files/functions:

- make one `run_repair_ruler(root, inventory) -> DefectLedger` implementation and call it from round open,
  candidate composition, post-promotion verification, and final seal;
- replace mutation use of `one_ruler_grade`, `shard_beats_baseline`, `pick_repair_winner`, and scalar
  ship-best;
- preserve `grade_promotion_preview`'s exact-byte composition and no-op detection inside `RepairTransaction`.

Tests/falsification:

- every gate category appears exactly once in all four ruler sites;
- independently good A/B candidates whose composition is bad never mutate the real tree;
- test weakening that lowers pytest failures but violates cited requirements is surfaced to semantic review
  and cannot win on count;
- promoted preview ledger/hash equals immediate real-tree ledger/hash; mismatch restores the parent epoch;
- SB7/scorer files have zero diff.

### 5. Route repair through Scheduler and the physical broker

Files/functions:

- `crates/goose-swarm/src/dag.rs` / `dispatch.rs`: explicit task kind and repair stage;
- `scheduler.rs`: candidate/probe/review queues, epoch-aware hints, physical decode leases, useful-idle work;
- `swarm.rs`: remove `fanout_over_fleet` repair branches and fresh one-off scheduler construction after the
  unified scheduler has parity;
- provider/LM Link telemetry seam: record physical host, instance, request ID/state, and terminal release.

Events:

- `provider_request_state`;
- `physical_lease_acquired/released`;
- `repair_tail_state { generation, probe, review, promotion, physically_idle_hosts }`;
- `repair_idle_work_started/completed/skipped { epoch, evidence_fingerprint, reason }`.

Tests/falsification:

- r1 pool fixture (`3 hosts × weight2 × instances1`) schedules one decode per host before any second lane;
- a four-candidate wave does not stack two on Workhorse while a distinct host is available;
- when one generation remains, released hosts begin version-current targeted probes/reviews without mutating;
- no unchanged epoch/evidence pair is reviewed twice;
- cancellation does not release a lease until provider-terminal evidence;
- scheduler reports physical occupancy, never infers it from logical outstanding count.

### 6. Remove hard structural ceilings

Files/functions:

- replace deadline stand-ins with `Option<Instant>`;
- retire six-round, count-stall, one-strategy-switch, boot, standalone-smoke, wire, and 12-evidence ceilings;
- keep operator/harness deadline support and provider-terminal/dead-stream safety as explicit external stops;
- make recurrence/no-progress a semantic-judge trigger with durable hypothesis history.

Tests/falsification:

- uncapped resolves to no deadline, not one week;
- a flat count with different defect IDs/severity continues;
- a seventh evidence-producing repair is reachable;
- unchanged epoch + unchanged ledger + unchanged hypothesis cannot silently spin: it emits a judge decision and
  either obtains new actionable work or ends honestly as no actionable hypothesis;
- no fixed duration/character/tool threshold produces a correctness, promotion, or failure verdict.

## Archived regression fixtures to add during implementation

Do not copy the entire SB7 prompt or scorer into unit tests. Extract engine facts only:

1. `evals/swarm-bench/fixtures/qwen38-r1-repair-tail.json`
   - events 1382–1456 relevant to verify/fix/stall/boot/review;
   - device/model assignment, durations, progress samples, promoted flags;
   - six snapshot/shipped hash pairs and the mtime caveat;
   - final `passed=false`, `verified=false` and score metadata as observation only.
2. `evals/swarm-bench/fixtures/qwen38-r1-salvage.json`
   - `test-webhook` transient `salvaged:true` versus final report omission.
3. `evals/swarm-bench/fixtures/qwen38-r2-phase-boundary.json`
   - 26 completed details, one outstanding, and explicit absence of repair events.
4. `crates/goose-cli/tests/repair_causal_ledger.rs`
   - identity, severity evidence, composition, seal, and r1 chronology replay.
5. `crates/goose-swarm/tests/repair_scheduler.rs`
   - provisional state, physical leases, useful-idle scheduling, provider-terminal cancellation.

Fixture extraction must record source log SHA-256 and be generated by a deterministic read-only script so a
changed archive cannot silently rewrite the theory.

## Campaign falsification gates

After implementation and only when no protected local run is active:

1. Run source/unit gates first: fmt, focused repair/scheduler tests, full relevant crate tests, clippy with
   warnings denied. A green proxy test is insufficient; inspect the exact emitted chronology and shipped hash.
2. Replay the r1 fixtures. Every old promotion must now explain target closure and introduced defects; all six
   post-gate mutations must be impossible.
3. Run frozen-spec shadow comparisons without changing SB7 or its scorer. Use sequential replication rather
   than declaring a win from one stochastic run.
4. Reject a speed win if hermetic quality falls, a critical requirement disappears, verification becomes less
   established, or the semantic judge is bypassed. Reject a quality win that merely spends more node-minutes
   repeating unchanged hypotheses.
5. Measure physical host occupancy from provider-correlated events. Logical task count, semaphore permits, and
   model self-report are not occupancy.
6. Primary value metric: closed causal defects weighted by evidenced impact per physical model node-minute,
   accompanied by final hermetic score and wall time. Raw task completion and raw finding count are diagnostic
   only.
7. The autonomous monitor stops a benchmark on a new engine defect, preserves the run and event evidence,
   fixes one causal mechanism, passes the source/replay gates, and restarts from a clean run directory. It never
   patches a benchmark artifact mid-run or changes the scorer to obtain green.

Success means faster time-to-evidenced-value with equal or better hermetic quality, every mutation traceable to
one reviewed causal transaction, salvage remaining explicit, and useful physical idle capacity feeding the
semantic judge or verification whenever version-current work genuinely exists.
