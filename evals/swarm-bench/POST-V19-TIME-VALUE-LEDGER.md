# Post-V19 time/value ledger

## Scope and invariants

This ledger records evidence for the first local run after V19. It does not authorize any
change to the immutable live V19 tree, stable-SB7 specification, scorer, thresholds, era,
label, publication identities, strict schemas, or lifecycle guarantees.

V19 evidence identity:

- run: `swarm-20260825-074635151`
- source: `4dac4737dd0c8386bebc3feb4efd1cb9f6989995`
- binary: `0a2b49258a4a3e383d9aaaa28174a8fcf211147324e0fe8da8fcee3aed2b0729`
- snapshot boundary: sequence 1687 at `2026-08-25T11:22:19.305030Z`

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
- Canonical revision began at `10:58:04.509602Z`. Its first two generations consumed
  1,454.707 provider-seconds, were both cancelled after semantic `looping_high` verdicts, and
  produced no accepted plan. The third generation was live at the snapshot boundary.
- No build task had been dispatched 3:35:44 after run start at the snapshot boundary.

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

Two Workhorse revision generations used 1,454.707 provider-seconds, entered no structured output,
and were cancelled by semantic judges. The judges prevented worse looping and are not the waste.
The candidate change is to make revision consume a smaller, deterministic merge input and preserve
accepted skeleton content, so a nudge does not force the model to re-derive the entire plan. The
strict one-plan compiler and semantic recurrence guard remain mandatory.

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
6. The final result is a runnable application; latency improvement alone is not success.

## Terminal addendum required

Append V19's final phase durations, task/build/repair results, exact score, telemetry, impairment
disclosure, and publication verification before selecting or implementing the next-run changes.
