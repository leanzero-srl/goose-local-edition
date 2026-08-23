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
- Requirement-inventory/context slicing is implemented in the next increment documented below. Its live
  quality and latency effect remains unmeasured until the frozen campaign gate allows a non-SB7 shadow run.
- `split_fat_modules` remains an existing file/role heuristic. It is not evidence of semantic task balance and
  was not used as the tail fix.
- No judge work is fabricated when contract demand drains. Useful late-fan supervision needs version-current
  evidence plus verified physical capacity; absent those, the honest action is no request.
- Build, judge, repair, campaign, and live LM Studio behavior were not exercised by this isolated slice.

## Increment 2 — normalized requirements and semantic context slices

Implementation commit: `8f5c320c9` (`swarm: bind task details to normalized requirements`).

### Authority boundary

The live `opts.prompt` is not a safe requirement source. The engine appends model-authored “settled defaults”
from targeted research to it, then may append human clarify answers. Treating that composite as the raw spec
would let research prose silently acquire requirement authority.

The final post-plan seam now has three separate channels:

- the frozen operator specification captured before the first model call;
- verbatim human Q&A under `USER DECISIONS — BINDING`;
- research findings, normalized separately as `advisory-research` evidence.

Only the first two form the binding inventory. Research may inform a semantic slice by evidence ID; it cannot
create, remove, own, or override a requirement.

### Normalized source of truth

`normalized_markdown_units` follows authored Markdown boundaries—paragraphs, list items, Q/A pairs, and fenced
examples. It does not read filenames, token counts, requested task counts, worker counts, model IDs, or fleet
capacity. Section headings remain provenance on each contained clause. Each non-empty clause receives a stable
content ID; duplicate verbatim clauses retain distinct ordered occurrences.

The selected skeleton is sanitized before the binder sees it: only task id, brief, dependency ids, and owned
files remain. Preferred model and difficulty are omitted, so the semantic slice count cannot key off roster or
capability metadata.

A response-only canonical binder returns:

- exactly one primary owner for every raw requirement ID;
- cross-cutting/applicable and verification consumers without weakening primary ownership;
- dependency and owned-file corrections on the fixed selected task IDs;
- stable engine-generated interface IDs, producer/consumer endpoints, requirement references, contract text,
  and whether a completed artifact is required;
- one or more per-task slices, each justified by a distinct semantic acceptance outcome with concrete evidence.

The binder prompt explicitly forbids splitting by filename count, text length, token budget, fleet size, or idle
nodes. One slice is valid when one acceptance outcome is genuinely indivisible; extra work is never manufactured
to occupy capacity.

### Structural admission checks

Before any detail request is admitted, the engine rejects:

- an uncovered raw requirement or more than one primary owner;
- missing/repeated model-authored tasks;
- unknown, repeated, or cross-role requirement IDs;
- unknown evidence IDs;
- slices that do not exactly cover the task's owner/applicable/verifier roles;
- empty objectives or acceptance evidence;
- unsafe/repeated slice IDs;
- dropped files from the selected graph, any post-binding file overlap, invalid dependencies, or a cycle;
- invalid interface endpoints/references; and
- a consumer marked as requiring a completed artifact without a dependency on its producer.

These are structural checks. They do not deterministically decide whether an architecture or requirement is
semantically good; the response-only binder makes that semantic proposal, and a failure aborts honestly instead
of falling back to a generic brief.

### Context and fan behavior

Each detail call receives only:

- its semantic slice's requirement records;
- evidence IDs selected for that slice;
- interfaces relevant to its task/endpoints/requirements;
- exact owned files; and
- the slice objective and acceptance evidence.

It no longer receives the full goal, full research body, or architect brief. The detail model cites requirement
IDs only; the engine renders the authoritative raw text, preventing a weak model from paraphrasing an exact
route/header/value and then validating its own paraphrase. Multiple slices for one execution task run as
independent detail calls and are merged in declared semantic order into one implementation contract.

The existing staged fan still admits detail before auxiliary work and uses one lane per distinct host. A
single-slice task may prefetch its required contract once its detail finishes. A multi-slice task cannot compile
an interface from a partial task contract, so its contract waits for the ordinary contract phase after all
slices merge. No admitted request is killed/replaced, and no auxiliary work is invented.

`requirement_binding_started`, `requirement_binding_resolved`, `plan_compile_resolved`, and per-slice
`detail_completed`/`detail_compile_failed` events expose authority, inventory/evidence/task/slice/interface
counts, stable IDs, coverage, roster blindness, context characters, exact requirement/evidence IDs, and
`full_goal_context:false`. The final plan persists the requirement registry, advisory evidence registry,
interface registry, task roles, and slice-to-evidence mapping. `Dag` ignores these additive metadata objects,
while each execution `TaskSpec.description` carries the merged exact requirement contract.

### Phase interaction audit

- **Research:** findings are now usable without becoming requirements. The existing fixed-lens/retarget research
  policy is unchanged; unresolved-question expansion and conflict state remain Engine 3 work.
- **Planning:** the binder runs once on the final post-ask selected graph, so redraft/backbone rounds do not pay
  repeated detail cost. It can repair file ownership and dependency edges and split detail-generation work by
  acceptance closure, but it deliberately keeps selected task IDs fixed. It does not yet turn slices into new
  execution DAG tasks or repair the earlier structural-confidence metric.
- **Contracts/build:** exact requirement text and interface facts enter the worker through the merged task
  description. Single-slice contract prefetch remains safe; multi-slice tasks wait for the complete contract.
- **Judge:** current outer judge and pre-review already receive `TaskSpec.description`, so they see the exact
  merged requirement contract. They do not yet consume the typed registries/snapshot versions directly. No
  judge verdict, split, kill, acceptance, cadence, priority, or nudge behavior changed here.
- **Repair:** current fix-round specifications are still generated from findings plus the composite run prompt;
  they do not yet address stable requirement IDs or a causal requirement/defect ledger. This slice therefore
  improves the initial build contract but makes no claim that repair is requirement-complete.
- **Scheduler/occupancy:** semantic slice count is job-derived, and detail fan concurrency uses available
  distinct hosts. Logical tail telemetry remains explicitly non-physical. No broker, same-host concurrency,
  cancellation, or provider-terminal behavior changed.

### F924/F925 re-verification and disposition

Commit `388792522` was re-read separately against its captured fixture, current controls, and the corrected
audit before this increment was accepted.

- The 9,304-character fixture has 9,257 stride-one 48-character windows and 5,524 distinct windows. The correct
  share of windows beyond first occurrence is `3,733 / 9,257 = 0.4033`. The committed `0.6758` is
  `(total-distinct)/distinct`, a repeat-to-distinct load factor that can exceed 100%; it is not a percentage of
  repeated windows.
- The fixture proves a long-period recurrence can be invisible to the final 2,000-character view. It does not
  prove the preceding ~57 minutes or 191k characters were the same loop, and it is not a negative-control
  distribution for slow healthy Qwen3.8 calls.
- The proposed trigger is gated by `omni_judge`, but once over threshold it is level-triggered on every stream
  chunk under uncapped. Recurrence also becomes semantic corroboration, non-consecutive model verdicts persist,
  and the replacement reply can overlap the still-owned prior stream without provider-terminal proof.
- `fan_last_outstanding` measured logical items and then claimed physical idleness it could not observe.

Disposition: retain revert `6b4de01b7`. Do not restore F924/F925 behavioral code, deterministic corroboration,
replacement, or occupancy claims. Commit `388792522` and its raw fixture remain incident evidence in history;
the corrected scheduler fixture remains live. Unbuffered console output and factual judge/request lifecycle
events are useful only as separately reviewed observability changes. A future recurrence instrument must be a
correctly named neutral statistic, use real slow-healthy/problem distributions, be edge-triggered/debounced with
one outstanding review, and have no vote/kill/accept/replacement authority. No same-session intervention ships
before correlated provider-terminal cancellation.

### LM Studio and upstream queues discovered during this slice

- LM Studio native `/api/v1/chat` is an observability/control probe, not a drop-in Goose transport: it exposes
  chat start/end, `model_instance_id`, prompt-processing events, reasoning/message/tool boundaries, TTFT/TPS,
  and reasoning usage, but does not support custom tools or assistant messages. OpenAI `/v1/responses` supports
  custom tools/stateful history and needs a separately captured adapter evaluation; `/v1/chat/completions`
  must not be replaced blindly.
- `/api/v1/models` exposes loaded-instance parallelism, context, and engine configuration. LM Link routing is
  preferred-device/per-machine, so the future broker must correlate physical instance identity rather than
  infer it from a logical alias.
- Upstream `bb539f7d6` remains a selective-port candidate: this fork marks length-terminated text as truncated,
  but a `finish_reason=length` tool-call branch may still execute a syntactically valid token-guillotined call.
  No port is authorized without a captured LM Studio length-terminated tool-call frame, a normal-tool negative,
  invalid-params behavior, and persisted output-limit metadata.

### Increment-2 verification and non-claims

Focused tests cover stable exact-source normalization (including Q/A), full ownership, semantic context
isolation, stable interfaces, fabricated/out-of-slice requirement rejection, unowned/duplicate requirements,
file overlap, artifact dependency integrity, and the existing real 27-item logical-tail replay. `cargo fmt`,
focused tests, `cargo check -p goose-cli`, and `cargo clippy -p goose-cli --all-targets -- -D warnings` passed
before the implementation commit.

No LM Studio request, benchmark, scorer, SB7 file, campaign state, running fleet, judge behavior, or repair
behavior was changed or exercised. Offline tests establish mechanism truth, not a wall-time or quality win.

## Adversarial implementation closure — authoritative DAG, binder, topology, and runtime review

The completed engine series was red-teamed after implementation rather than accepted from its green tests.
Four P0 contract failures were found and corrected in separate commits:

- `946fc16f8` makes post-binder package-entry injection reparse and revalidate into the exact DAG used by
  contracts, the integration sink, `plan_loaded`, dispatch, and resume. The advertised plan can no longer omit
  a scheduled entry-point owner.
- `798ae2770` makes binder objective, files, requirement quotes, advisory evidence, interfaces, and required
  acceptance evidence authoritative inputs to the typed detail compiler. Model-authored generic prose can add
  useful procedure but cannot erase or replace those facts.
- `8d3617cd3` makes the binder fail closed on incomplete implementation topology and unsafe project paths.
  Generic role/surface and interface/artifact preservation checks cover missing source/config entry surfaces,
  disconnected multi-integration graphs, exact root artifacts, extensionless files, and dotfiles without
  treating runtime databases, API routes, flags, or URLs as delivered source. Absolute, parent, drive-prefixed,
  empty-segment, and root-escaping owned paths are rejected.
- `a61c4181a` removes the raw dynamic-replan escape hatch. Runtime additions are now read-only acceptance
  reviews compiled from an already-completed task and one frozen semantic binder slice. Their objective,
  requirements, evidence, interfaces, acceptance evidence, source paths, and sole dependency are engine-derived;
  the model can supply only review procedure and exact citations. The scheduler verifies the compiler receipt,
  atomically inserts admitted reviews before `integrate-verify`, emits the complete post-splice DAG, and resume
  restores that same DAG and receipt. Review results are typed, requirement-complete, source-file-bound, and
  reach the integration sink through an explicit uncapped typed-evidence channel rather than a lossy generic
  dependency slice.

### Why runtime review remains task-derived, never capacity-derived

Idle capacity may decide **when** the engine asks whether useful evidence work exists; it never decides **what**
work exists or how many reviews are created. The complete candidate set is the frozen Cartesian identity of an
already-bound task and one of its semantic acceptance slices. Each pair has one stable engine-generated review
ID and one exact contract. The selector never receives worker count or idle capacity and cannot author an ID,
file, dependency, objective, requirement, evidence item, interface, or acceptance condition. A capacity-2 and a
capacity-8 request compile byte-for-byte equivalent `TaskSpec`s. Extra machines therefore expose scheduling
opportunity; they do not rewrite architecture, multiply semantic work, or create a bonus tail merely to keep a
decoder busy.

The same rule applies to judge splitting. Existing semantic judge observation and nudge behavior was not made
deterministic or changed by this series. Split execution is explicitly guarded off while `ChildSpec` lacks full
binder metadata. That is an Engine 5 prerequisite, not a permanent ban: a future split may be admitted only when
each child preserves requirement/evidence/interface/path/acceptance closure through the ordinary typed compiler
and appears in the exact observable DAG.

### Remaining findings after the P0 fixes

No additional P0 was found in this implementation slice after the adversarial fixtures and full gates. The
following unresolved risks remain and are deliberately not hidden by this closure:

- **P1 — semantic planner selection:** `score_skeleton`/`select_best_skeleton` validate structure and file
  overlap but do not measure requirement or interface improvement. An equally shaped but semantically better
  convergence/backbone candidate can still lose because replacement requires a strictly higher structural
  score. Planning fan size is also still influenced by the available model pool.
- **P1 — physical scheduling truth:** one-lane-per-host currently deduplicates model identifiers, not correlated
  LM Studio host/model-instance occupancy. Distinct aliases on one physical host can be treated as independent
  lanes, while `physical_idle_lanes` correctly remains unknown. Speed and idle claims need the Engine 1/4 broker
  telemetry before they are causal evidence.
- **P1 — staged-fan failure handling:** an error returned from one `fanout_staged` child can still unwind the
  caller before all other admitted futures are explicitly drained. Provider-terminal accounting and structured
  collection of every admitted result remain required.
- **P1 — research value:** fixed scout lenses, `max_research_questions`, and bounded question generation still
  limit research by configured count rather than unresolved evidence. Markdown normalization also preserves
  heading provenance in records but flattened evidence quotes can lose the question/referent that made a fact
  meaningful.
- **P1 — hardcoded genericity leak:** default-on `WEB_VOCAB` still injects old frontend/vendor terms such as
  `sync-button` and `payments-table`; it must be replaced with active-spec interfaces before generic campaigns.
- **P1 — repair correctness and time:** repair retains fixed round ceilings, fixed timeout/kill paths, flat
  finding-count stall detection, and promotion by finding count rather than identity, severity, invariant
  preservation, and requirement closure. Historical r1/r2 evidence shows this can spend hours and still prefer a
  candidate with one critical regression over multiple minor improvements.
- **P2 — residual trust and slicing limits:** runtime receipts are trusted in-process compiler artifacts rather
  than cryptographically bound records; ordinary legacy dependency contexts retain byte caps; binder semantic
  quality still depends on the model after structural checks; project-relative lexical validation does not by
  itself prove that an existing ancestor is not a symlink. These are explicit trust boundaries, not claims of
  complete hermeticity.

These findings map to Engines 1, 3, 4, 5, 6, and 8 in the main audit. They were not folded into this P0 series,
because doing so would destroy causal attribution and exceed the task-compiler/DAG contract boundary.

### Final verification state

On commit `a61c4181a`, `cargo fmt`, all `goose-swarm` unit/integration/doc tests (65 + 6 + 39), the full
`goose-cli` suite (585 passed, 1 ignored), `cargo clippy --all-targets -- -D warnings`, and `git diff --check`
passed. The tests include scheduled-vs-advertised DAG identity, package-entry end-to-end dispatch, adversarial
binder fact erasure, missing source/config surfaces, disconnected interfaces, SB7 runtime-artifact distinction,
unsafe paths, idle-capacity 2-versus-8 invariance, raw/generic/path-invalid dynamic additions, typed review
evidence at the sink, exact resume, and the judge-split guard.

This worktree did not start or signal a fleet, call LM Studio, edit SB7 or its scorer, score a run, or touch the
cloud benchmark worktrees.
