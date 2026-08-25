# Post-V19 time/value ledger

## Scope and invariants

This ledger records evidence for the first local run after V19. It does not authorize any
change to the immutable live V19 tree, stable-SB7 specification, scorer, thresholds, era,
label, publication identities, strict schemas, or lifecycle guarantees.

V19 evidence identity:

- run: `swarm-20260825-074635151`
- source: `4dac4737dd0c8386bebc3feb4efd1cb9f6989995`
- binary: `0a2b49258a4a3e383d9aaaa28174a8fcf211147324e0fe8da8fcee3aed2b0729`
- terminal boundary: sequence 1736 at `2026-08-25T11:56:39.688122Z`

The optimization objective is lower wall time to a runnable application without buying speed
with weaker evidence, looser validation, arbitrary call/time/token caps, reduced benchmark
requirements, or hidden degraded authority.

## Measured V19 critical path

- Research ran from `07:46:35.173942Z` to `10:37:06.211424Z`: 10,231.037 seconds
  (2:50:31). It used 122 provider starts and produced 172 accepted targets plus 25 typed
  degraded targets out of 197.
- Canonical skeleton ran for 372.025 seconds and produced one accepted 4,823-character plan
  skeleton.
- Three planning audits ran concurrently for 886.253 seconds of wall time. Acceptance/evidence
  found 9 issues, DAG/interfaces found 0, and requirements/coverage found 4.
- Canonical revision began at `10:58:04.509602Z`. Three generations consumed 1,900.654
  provider-seconds and were cancelled after semantic recurrence review. The fourth finished in
  263.364 seconds; strict compilation rejected one duplicate semantic-role assignment, and two
  short correction calls allowed requirement binding to begin.
- No build task was ever dispatched. V19 terminated 15,004.534 seconds (4:10:04.534) after run
  start while still in pre-scheduler requirement binding.

## Proven terminal failure

Workhorse admission `00000128` started exactly one provider request for
`reqbind-artifact-evidence-v3:pre-scheduler:128` at `11:36:56.443312Z`. The last stream progress
was sequence 1733 at `11:46:39.678859Z`. After 600.009 seconds with no further progress, the local
work ended `error` with one provider start and zero proven provider terminals (sequences
1734-1735). Sequence 1736 quarantined the admission because the outstanding request had no proven
cancelled terminal. Goose then exited without `run_finished`; the authenticated monitor captured
that exact incident at `11:56:40.008466Z`.

This is an ambiguous-transport abort, not a model-quality verdict or a context-window error. The
engine correctly refused to release unresolved lifecycle authority, but typed task compilation
failed before dispatch. Consequently V19 has no build, repair, score, publication, or no-cache
result and must never be represented as a completed baseline. Evidence: `run.jsonl` sequences
1732-1736, `engine-console.log`, and `.swarm-monitor/watch.jsonl` in the frozen V19 run tree.

## Context-window and compaction evidence

- Authenticated preflight sealed the loaded windows as Local/Mihai `262144`, Mac/Gabee `135936`,
  and Workhorse `262144` tokens (`fleet-seal.json`, sealed `07:46:31.914849Z`). V19 did not bind
  those per-host allocations into sessions: `levers_resolved.context_cap` was null and all 128
  hidden V19 sessions persisted `model_config.context_limit=null`.
- With no global hard cap, the unbound Goose model fallback was `128000` tokens and the default
  0.8 auto-compaction threshold was `102400`. V19 emitted no per-call resolved-limit evidence, so
  the fallback is reconstructible from source and session state but a host-specific effective
  limit is not. This observability gap is itself a V20 requirement.
- Completed prompt-token maxima were Local `38894`, Mac/Gabee `38888`, and Workhorse `43397`.
  Gabee's largest completed prompt plus output was `46655`, 34.3% of its loaded allocation. The
  aborted requirement-binding request had `208169` serialized input bytes and no terminal usage
  row, so its exact token count is unavailable.
- No compaction event, diagnostic, or context overflow occurred in the run log, matching CLI log,
  activity snapshots, or session metadata. That is not proof that continuity works: pre-scheduler
  calls were predominantly fresh response-only requests, and V19 dispatched zero execution
  workers. The longest persisted pre-scheduler conversation was session `20260825_298` with 11
  messages and a `43397`-token final prompt.

## Demonstrated low-marginal-value work

### Exhausted research branches

Two partitions consumed 16 provider turns and 5,584.578 cumulative provider-seconds without
producing one accepted target:

- `target-section-78825fa61d34cb1d`: 18 targets, 9 turns, 3,582.048 provider-seconds.
- `target-section-639e33adff0c5ff4`: 7 targets, 7 turns, 2,002.530 provider-seconds.

The typed degraded closure is valuable and must remain fail-closed. The candidate change is to
recognize empirically unchanged per-host correction/schema failure earlier and reach the same typed
closure without replaying a branch that cannot gain new authority. This must be semantic-state and
host-history based, never an arbitrary count or elapsed-time cap.

### Same-host correction before successful failover

The final accepted `target-section-bd14130383f38bcb` jury-2 Mac branch used four turns and
1,351.145 provider-seconds before repeat detection. Its Local failover then succeeded in two turns
and 446.956 seconds. The candidate change is earlier distinct-host failover after corroborated
unchanged correction state. A materially improving correction must remain on the current host.

### Canonical revision regeneration

Three Workhorse revision generations used 1,900.654 provider-seconds, entered no accepted
structured output, and were cancelled after semantic recurrence review. The judges prevented worse
looping and are not the waste. The candidate change is to make revision consume a smaller,
deterministic merge input and preserve accepted skeleton content, so a nudge does not force the
model to re-derive the entire plan. The strict one-plan compiler and semantic recurrence guard
remain mandatory.

### DAG/interfaces audit observation

The DAG/interfaces audit cost 375.470 seconds and found no issue in V19. One run is insufficient
evidence to remove an assurance gate. Treat conditional or deterministic replacement as an
experiment only after cross-run evidence proves it redundant; do not delete it for the next run
solely from this zero.

## Demonstrated quality-bearing work to preserve

- Three adjudication calls resolved 20 disagreements into 20 ledger corrections.
- Twenty-five citation calls verified 172 targets and rejected one; the final 551.140-second
  citation tail alone verified 18 targets.
- The requirements/coverage audit found 4 plan defects even though it determined the audit tail.
- The acceptance/evidence audit found 9 plan defects.
- The accepted canonical skeleton, one shared-plan authority, typed degraded evidence, strict
  schema validation, full lifecycle drain, host distinction, recurrence detection, compiler checks,
  concrete task contracts, build, repair, hermetic scoring, and guarded publication remain intact.
- Twenty-two stale-host admission rejections started no provider call and therefore are scheduler
  bookkeeping, not a meaningful latency target.

## Swarm-only implementation map

All candidate changes are confined to `crates/goose-cli/src/commands/swarm.rs`.

### High-confidence candidates

1. Compact successful research evidence without removing any call or authority check. Keep
   `rationale` in `research_closure_schema` and `research_closure_citation_schema`, permit an empty
   value only for positive `complete`/`supported` verdicts, and synthesize deterministic provenance
   from target, evidence, and physical-host IDs during compilation. Continue requiring substantive
   rationale for incomplete, gap, adjudication, and unsupported verdicts. The relevant compilers are
   `compile_research_closure_partition` and `compile_research_closure_citations`; semantic agreement
   already ignores positive rationale text.
2. Preserve all three planning audits but route the broad requirements/coverage role to the fastest
   physical host. `run_planning_pod_audits` currently derives priority partly from the short role
   brief, while `order_fleet_by_speed` cannot resolve V19 device-keyed speed weights against model
   IDs and falls back to equal weights. Replace accidental prompt-length ordering with explicit
   semantic cost and resolve configured device/model identity before applying speed weight. Add a
   V19-shaped assignment regression.
3. Keep early semantic-judge observation, but require two consecutive `looping_high` verdicts over
   the same provider request and recurring tail before destructive cancel/re-prefill. A `continue`
   verdict, request change, advancing tail, or structured-output progress clears the candidate.
   Reuse the existing two-verdict/tail policy rather than inventing a time, turn, token, or repeat-
   share cap. The production path is `PreSchedulerSemanticRuntime::try_spawn_recurrence_review`.

### Measure before adoption

- Compact the repeated immutable research JSON serialization in
  `run_research_closure_semantic_pass_on_lane` and the citation path, then compare schema-correction
  quality before adoption. Semantic content is unchanged, but compact JSON may be harder for a weak
  local model to read.
- Do not coalesce authored section partitions until a bounded A/B proves that mixed-section output
  preserves or improves compiler acceptance. Do not skip a juror, verifier, citation check, or
  planning audit based on provisional `SpecSufficient` evidence.

### Required regression families

- Positive empty rationale accepted and given deterministic provenance; incomplete or unsupported
  empty rationale rejected.
- Requirements/coverage assigned to the fastest mapped host while all three audits still run and
  compile.
- One `looping_high` followed by `continue` does not cancel; two corroborated highs over the same
  recurring tail cancel once; advancing tail or structured output prevents cancellation.
- A forced low-cap, long-lived execution worker crosses the compaction threshold and emits
  task/session/host-linked pre/post token and message counts, resolved loaded/effective limits, the
  trigger reason, and retained-tail evidence. It must preserve tool request/response pairing,
  requirement and file-contract continuity, avoid duplicate calls, and finish the runnable task.
- A separate oversized fresh response-only request is rejected or routed before admission from
  loaded-window plus output-reserve evidence; conversational compaction is not credited for that
  case.
- Existing false-spec citation reopening, distinct-host failover, structured-output race, and full
  lifecycle-drain regressions remain green.

## Next-run acceptance gates

Any post-V19 change must prove all of the following before an append-only launch:

1. Exact production regressions preserve improving corrections but bound unchanged semantic-state
   replay and use existing distinct-host failover or typed degraded closure.
2. Research retains all 197 immutable target authorities, reports accepted and degraded targets
   truthfully, and does not increase V19's 25 degraded targets.
3. Planning produces exactly one compiler-valid, specific shared plan while retaining every issue
   found by the acceptance/evidence and requirements/coverage audits.
4. The first build dispatch occurs materially earlier than V19 without weakening any stable-SB7
   rule or hiding provider work.
5. Lifecycle, tool-call, recurrence, compiler, build, repair, hermetic-score, publication, and
   no-cache verification gates all remain exact.
6. At least one forced low-cap worker compaction is directly observed with continuity telemetry;
   absence of a context failure or a low final prompt is not evidence that compaction works.
7. The final result is a runnable application; latency improvement alone is not success.

## Terminal disposition

The terminal addendum is closed as an impaired pre-build failure. There are no task/build/repair,
score, publication, or no-cache values to append. Preserve the frozen incident and context evidence;
V20 must prove natural terminal lifecycle balance and forced compaction continuity before any score
or publication decision.
