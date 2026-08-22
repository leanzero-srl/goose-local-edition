# Engine task creation and staged-fan ledger

## Scope and safety boundary

This slice changes task construction and the PLAN detail/contract schedule. It does not change SB7, any
scorer, F924 recurrence behavior, EXECUTE judge decisions, repair, LM Studio configuration, or a running
fleet. No live benchmark was started or signalled.

The implementation is in `crates/goose-cli/src/commands/swarm.rs`. The corrected incident shape used by
the scheduler replay is preserved in `evals/swarm-bench/fixtures/f924-detail-tail-shape.json`.

## Evidence that selected the mechanism

The source of record is `SWARM-ENGINE-AUDIT-2026-08-22.md`; numbers below are copied from its corrected
r2 reconstruction, not from the quarantined F924 narrative.

- r2 entered 27 detail calls. Twenty-six completed; one `api-server-api` request remained logically
  outstanding.
- That request's activity existed for about 60.7 minutes and was the sole unfinished detail for about
  17.7 minutes. It reached 203,447 reasoning characters before interruption.
- The other 26 calls were not uniformly short: median duration was 521 seconds, p90 about 1,791 seconds,
  maximum 2,140 seconds. Completed reasoning reached 63,878 characters.
- The 26 returned task descriptions totalled 146,576 characters, median 4,810. A request for roughly 150
  words did not bound either reasoning or output.
- The engine did not record correlated provider occupancy. “One logical item left” is not proof that two
  physical decoders were idle. The user saw idle nodes, but this slice keeps that as an observation rather
  than laundering it into an exact interval.
- Historical F923 proves detail agents had developer tools in the real project tree and did prototype/
  remove application files. That is unauthorized planning side effect even where cleanup happened.
- The old structural selector rewarded root count, task count, shallow depth, and fan-in relative to fleet
  width. r1 then loaded 48 tasks with every dependency list empty despite real imports and joins.
- Existing `split_fat_modules` is file/role based. It cannot establish semantic independence and cannot
  help a large single-file/API task. It was not generalized into a filename or file-count heuristic here.
- Historical F623 supports one decode lane per Apple host for prologue generations: stacking two concurrent
  generations on one host reduced aggregate progress. “Use all nodes” therefore means distinct physical-host
  lanes for this stage, not blindly filling every configured parallel slot.

## Root causes addressed

1. **Roster-shaped task construction.** The same requested system could receive a different task graph
   because more slots happened to be attached. Structural selection then rewarded the widest/flattest graph.
2. **Free-form, write-capable detail agents.** A task compiler was actually a normal Goose developer agent
   with the full goal/research body, real-tree tools, free-form output, and silent one-line fallback.
3. **A global detail-to-contract barrier.** Required contract work could not start until every detail had
   returned, even when a completed module and a released host lane already existed.
4. **Activity/control coupling.** The same `activity_key` that made a compiler visible also armed volume,
   progress, repetition, and synchronous judge abort paths. Required plan input could be deleted in the name
   of observability.
5. **Hidden turn fallback.** `SessionConfig.max_turns = None` means “use the global default,” not unbounded.
   A compiler could therefore be capped by configuration outside the swarm's effective echo.
6. **Unsafe contract replacement.** The contract phase treated absent output as missing demand and could
   retry after a local error without provider-terminal evidence.

## Implemented behavior

### Task construction

- Task existence and dependencies are roster-blind. The architect is asked for cohesive requirement/
  acceptance ownership; it is not given a worker-count target.
- Skeleton ranking validates the DAG and penalizes ownership overlap. It no longer scores task count, root
  count, depth, choke points, or fan-in as proxies for quality.
- Parallel tests are created only for independent acceptance scope, never because capacity exists.
- Planner prompts name the actual runtime roster model identifiers; stale hardcoded Qwen3.6 identifiers are
  gone from the task-construction path.

### Typed task compiler

- Each detailed task must return typed fields for objective, exact requirement citations, interfaces,
  implementation steps, edge cases, and acceptance checks.
- Every requirement quote must occur verbatim in the supplied goal, brief, or research evidence. The engine
  owns and renders the exact file list. Empty objective/steps/acceptance or fabricated citations reject the
  plan before dispatch.
- Detail and contract compilers have a response-only tool surface. They cannot read, write, edit, shell, or
  create scratch in the project tree.
- A rejected detail never collapses to the architect's brief. The plan fails honestly before build dispatch.
- Compiler sessions use no wall-clock, reasoning-volume, or practical turn ceiling. Their activity remains
  visible, but progress/repetition/spiral breakers and the synchronous omni-judge cannot abort them until a
  provider-terminal continuation protocol exists.

### Required-work pipeline

- `fanout_staged` owns one central queue over distinct-host lanes.
- Pending detail has strict admission priority. The scheduler does not delay a detail to run auxiliary work.
- Once every detail is admitted, a lane released by a successful completed production-module detail may run
  that module's already-required frozen-contract compilation while remaining details continue.
- Contract eligibility is one shared predicate at both producer sites: a real non-test source owner, never
  `integrate-verify`, read-only work, docs, or a test-only task. No work is fabricated to occupy a lane.
- Every admitted future is awaited. There is no straggler grace, timeout, kill, or replacement in the staged
  fan.
- The later CONTRACTS phase drains prefetched results by module ID and generates only genuinely absent demand.
  An admitted terminal empty result is cached as empty and is not silently reclassified as missing.
- An explicitly enabled contract retry may follow only a clean terminal empty stream. A local stream error is
  not proof of provider termination and is never retried here.
- Bundle order follows requested module order rather than response completion order.
- The old stray-stub cleanup was removed. Response-only contract compilers cannot create stubs; scanning and
  deleting “new” source files could instead delete an unrelated concurrent planner side effect.

## Runtime evidence

`plan_compile_resolved` now states:

- typed detail format and response-only tool surface;
- no detail or contract straggler abort;
- no wall/volume/practical-turn cap for the compilers;
- activity-only/no-abort compiler supervision;
- whether contract pipelining is effective;
- `required-production-contracts-only` auxiliary policy;
- task count source `job` and distinct-model-host lane identity.

`plan_compile_tail_state` records logical admitted-request state: outstanding detail ID; pending, in-flight,
and completed counts for detail and contract work; and logically free lanes. It emits
`physical_idle_lanes: null` because this scheduler still lacks correlated provider occupancy.

`contract_compile_started`, `contract_compile_completed`, `contract_prefetch_cached`, and
`contract_pipeline_resolved` make pipeline stage, model, tool surface, attempts, result size, prefetch hits,
missing demand, and effective abort/timeout policy auditable.

## Regression and verification contract

The F924-shape replay reads the preserved JSON fixture rather than embedding a corrected narrative in the
test. It blocks the 27th admitted detail with a notification, lets 26 complete, and proves two released host
lanes begin existing contract demand while the tail remains admitted. It then releases the tail and proves
all 27 details and all 27 required auxiliaries finish. Two-second waits are test deadlock guards, not engine
policy.

Other tests enforce:

- a roster cannot influence the task-count ask;
- structural ranking cannot flatten real dependencies to buy width;
- exact requirement/file/acceptance preservation and fabricated-citation rejection;
- contract demand excludes test-only/read-only/sink work but includes mixed production+test ownership;
- an empty admitted contract is not regenerated and canonical bundle order is stable;
- authority-bearing detail/contract calls have no reasoning-volume breaker.

Required gate:

```bash
source bin/activate-hermit
cargo fmt --all
cargo test -p goose-cli fan_order_tests
cargo test -p goose-cli typed_task_detail
cargo test -p goose-cli skeleton
cargo test -p goose-cli spiral_budget_never_cuts_a_measured_healthy_call
cargo test -p goose-cli swarm_control_registry
cargo clippy -p goose -p goose-cli -p goose-swarm --all-targets -- -D warnings
```

The full gate above passed on 2026-08-23 in the isolated `codex/swarm-engine-overhaul` worktree.

## Explicit non-claims and remaining prerequisites

- The replay proves scheduler state transitions. It does not prove a counterfactual wall-time or score win.
- Physical occupancy remains unproven until the request broker records host/model instance, request ID,
  prefilling/decoding state, cancellation requested, and provider terminal. This slice never reports logical
  capacity as physical idleness.
- This does not implement F924 recurrence, a deterministic loop decision, or same-session judge nudging. The
  audit showed the current nudge can start a new request before the old provider request is proven terminal.
- Full requirement-inventory slicing remains a later prerequisite. The typed compiler validates exact
  citations and concrete acceptance, but it still receives the full goal and research body. A future slice
  must create stable requirement/interface IDs and pass only the relevant slice; a filename/token threshold is
  not a safe substitute.
- `split_fat_modules` remains an existing file/role heuristic. It is not evidence of semantic task balance and
  was not used as the tail fix.
- No judge work is fabricated when contract demand drains. Useful late-fan supervision needs version-current
  evidence plus verified physical capacity; absent those, the honest action is no request.
- Build, judge, repair, campaign, and live LM Studio behavior were not exercised by this isolated slice.
