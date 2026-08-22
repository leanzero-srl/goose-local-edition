# Goose Local Swarm Engine Audit and Implementation Plan — 2026-08-22

Status: read-only engine audit complete; implementation and paid cloud runs are awaiting approval.

This audit did not change SB7, its scorer, LM Studio, a running process, or executable source. The Qwen3.8 r2 engine disappeared before `plan_loaded` and never emitted `run_finished`; this audit did not signal it. Its artifacts are therefore forensic evidence, not a completed or scoreable benchmark. During this audit, a separate Claude Code session committed and pushed `388792522` (F924/F925). Independent review found material evidence, arithmetic, lifecycle, and control-flow defects in that commit. It is quarantined concurrent work, not an accepted engine baseline; the implementation plan begins by preserving it for forensics, reverting it from the implementation line, and reconstructing only the parts that pass review.

## Conclusions that change the previous plan

1. The semantic judge is the useful mechanism. It must not be replaced by a deterministic judge. Deterministic measurements may summon a judge, describe state, protect ownership, or confirm an objective test result; they may not decide that arbitrary software work is correct, drifting, looping, or complete.
2. The 30–40k-token generations are not proven to have one cause. Detail planning has a clear task-construction and tool-scope defect, but r2 whole-plan drafts were also extreme (29,324, 48,718, and 156,447 reasoning characters), while two research scouts stayed near 2–3k and one reached 15,270. Task grain, full-context prompts, output schema, tool menu, model template, sampler, and role prompt must therefore be separated in matched experiments. The typed task compiler remains necessary for specificity and safe fanning; it is not yet claimed as the sole tail cure.
3. Fleet use cannot be derived only from the count of ready implementation tasks. It depends on contract-safe semantic grain, dependencies, artifact ownership, physical decoder state, measured same-host contention, retry affinity, and a second queue of evidence-producing research, review, test, integration, and live semantic supervision.
4. There is no defensible universal Qwen3.8-27B sampler preset in the available evidence. The current `medium / 0.7 / 0.8 / min-p 0` configuration is not wrong merely because it differs from a model card, but it combines the official non-thinking temperature/top-p pair with thinking enabled. Community reports conflict, including reports of both success and >30k loops at temperature 0.7. The exact rendered LM Studio prompt and request profile must be certified before sampler comparisons mean anything.
5. Claude found a real long-period recurrence symptom, but its incident narrative and committed fix do not survive adversarial review. The final call ran about 61 minutes, was the sole unfinished detail for about 18 minutes, and the committed recurrence statistic is mislabelled. More importantly, the fix can synchronously summon the judge on every stream chunk and can start a replacement request before the old stream is dropped or proven terminal.
6. The current cloud-provider paths cannot validly run the five requested models yet: all three families fall through to incorrect model metadata or have current tool/thinking-contract gaps. The cloud lane therefore starts with provider conformance and a pinned build-only harness, not paid SB7 episodes.

## Audit standard and invariants

Every proposed treatment below is tied to:

1. an exact engine path;
2. event, activity, session, benchmark, runtime, or upstream-source evidence;
3. a user-visible time or quality consequence;
4. a falsifiable experiment using the unchanged SB7 task and hermetic scorer.

The optimization target is quality-preserving useful throughput: requirements proven correct per physical node-minute, with hermetic quality and critical checks as gates. Raw task count, file count, token count, model self-report, and nominal node occupancy are not success metrics.

The implementation invariants are:

- SB7 and its scorer remain unchanged.
- No production hard token, turn, wall-clock, task-count, research-count, or repair-round cap is used to manufacture speed.
- A semantic stopping condition may conclude that further work has no new hypothesis or evidence; an operational watchdog may protect a crashed transport. Neither is allowed to pretend incomplete work is correct.
- Task specifications remain implementation-specific and acceptance-specific. Structured does not mean generic.
- A physical node is considered busy when its decoder is serving or still unwinding a request, not merely when a Rust future is registered.
- Semantic judge interventions remain generic because they reason over generated contracts and evidence, not hardcoded languages, frameworks, filenames, or tool counts.
- Changes are isolated, committed, and benchmarked one causal lever at a time.
- Concurrent cloud generation is isolated from engine development by immutable worktrees, a copied release binary, unique state roots and ports, and an exact instrument manifest. Hermetic browser scoring remains serial and quiet.
- The frozen scorer's own version and calibration state are reported exactly. Current bytes emit `sb-7.0-rc`; the publication layer may not relabel them `sb-7.0`.

## Evidence baseline

### Completed Qwen3.8 r1

The completed r1 run took about 573.5 minutes:

- research: about 6.1 minutes;
- planning before execution: about 129.9 minutes;
- execution: about 139.4 minutes;
- gates and repair: about 298 minutes.

Its final hermetic score was `0.0169`: inner quality was `0.404`, excellence composition produced a pre-severity score of `0.3625`, and the critical multiplier was `0.0467` (`0.3625 × 0.0467 ≈ 0.0169`). After 9.6 hours, required surfaces including `web/app.js`, `web/viz.js`, and `/api/events` were absent. This falsifies “more completed tasks means more value”: 47 task-completion events and almost five hours of repair did not close central requirements.

The loaded r1 plan contained 48 tasks. Twenty-six non-verification/build task descriptions totalled 139,059 characters, and the largest was 9,796 characters. Every one of the 48 tasks had an empty dependency list, despite explicit imports, shared interfaces, services, and integration joins. The descriptions were often individually detailed but the graph was structurally incoherent.

### Interrupted Qwen3.8 r2

The r2 evidence path is `evals/swarm-bench/runs/sb7-fleet38/swarm-3node-r2/`.

- Research finished in about 11 minutes.
- Three skeleton drafts were requested; two returned after 2,741 seconds.
- Round-one structural convergence was reported as 97%, and the event said the ladder could be skipped, yet the full-fleet backbone round still ran for 5,545 seconds.
- Detail planning then created 27 calls. Twenty-six completed; their aggregate agent time was 18,831 seconds, median 521 seconds, 90th percentile about 1,791 seconds, and maximum 2,140 seconds.
- Those 26 returned 146,576 task-specification characters, median 4,810, despite a prompt requesting roughly 150 words. One specification was 21,545 characters.
- The final `detail-api-server-api` activity reached 203,447 reasoning characters, four tool calls, one tool error, and no final specification before the engine disappeared. Its activity file existed from 21:32:55 to 22:33:37 (about 60.7 minutes). The last other detail file completed at 22:15:58, so this was the sole unfinished detail for about 17.7 minutes—not two hours.
- Completed detail calls reached 63,878 reasoning characters (large examples: 31,471, 34,760, 39,896, 55,473, 58,205, and 63,878). Across all 27 detail activities, including the interrupted outlier, the minimum was 1,211, median 12,303, mean 25,255, and maximum 203,447; 25 exceeded 1,384 and 16 were at least 8,000. Whole-plan drafts separately reached 156,447. The tail is systemic across planning roles, not a single corrupt detail sample.
- The user observed idle LM Studio nodes, and the fan had only one logical item left, but the engine did not record enough correlated provider occupancy to reconstruct an exact physical-idle interval. A fan-item count is not proof that every other decoder is idle.

At the time of read-only inspection, LM Studio reported three loaded instances: Mihai and Workhorse at 262,144 context and parallelism 2, Gabee at 169,728 context and parallelism 2. The native model-list endpoint did not expose the active temperature, top-p, min-p, repeat penalty, or rendered reasoning-effort instruction. Its `reasoning_budget_message` was empty. This is why the rendered prompt remains the sampling gate rather than an assumption.

## Incident F924/F925 — verified, corrected, and quarantined

The 9,304-character fixture does demonstrate recurrence that a final 2,000-character tail can miss. It does not support the numbers or control logic currently committed:

- The fixture has 9,257 stride-one 48-character windows and 5,524 distinct windows. `3,733 / 9,257 = 0.4033` is the share of windows beyond the first occurrence. The committed `0.6758` is `(total - distinct) / distinct`; it is a repeat-to-distinct load factor, can exceed 100%, and is incorrectly described to both tests and judge as “percent of windows repeated.”
- The committed fixture is the first 9,304 bytes of a 9,839-byte scratch capture, ends mid-sentence, and samples only roughly 22:31–22:33. The raw ten-second samples, timestamps, and overlap-splicing program were not preserved. It cannot establish that the preceding roughly 57 minutes or 191k characters were in the same loop.
- “Healthy detail calls top out at 1,384 characters across 58 samples” is not a valid current negative control. In r2, completed details reached 63,878 reasoning characters; 25 of 27 exceeded 1,384. The session database does not retain the full streams needed for a clean completed-call recurrence distribution.
- Zero `judge_nudge` events do not prove a failed nudge detector because r2 resolved `judge_nudge=false`. `omni_judge=true` is a different mechanism.
- `now >= omni_next_look || recur.recurring()` becomes level-triggered once recurrence crosses its threshold. An `OK` leaves the meter high, so the synchronous worker loop may call the planner-routed judge again on every incoming chunk, bypassing the intended 300-second cadence and look budget.
- Deterministic recurrence is counted as corroboration, and an intervening `OK` does not clear the prior semantic looping case. This lets a measurement become part of the semantic vote and lets non-consecutive `LOOPING` decisions act as if consecutive.
- The replacement expression awaits `agent.reply(...)` while the old stream is still owned; assignment drops the old stream only after the new reply future returns. The code can therefore overlap two requests, and even an earlier local drop would not prove LM Studio terminated the old decoder request or that partial reasoning was safely committed.
- `fan_last_outstanding` reports logical fan items, then claims all other nodes are idle. It must be renamed to factual item/age/block-state telemetry and correlated with the physical request broker before making occupancy claims.
- Streaming console output and emitting judge lifecycle events are useful. The runner still needs explicit process-group termination and descendant/port verification: `subprocess.run(..., start_new_session=True)` does not by itself kill the whole group on timeout.

Post-approval disposition: preserve the commit and fixture as incident evidence, revert the behavioral patch on the implementation branch, retain live unbuffered logging and factual lifecycle events in separately reviewed commits, and rebuild recurrence only as debounced neutral evidence. The current meter is bounded recent-stream reach (65,536 windows), not whole-call memory; that reach is justified only by one short captured positive and a synthetic negative. Its replacement gets a correctly named bounded statistic, preserved incident inputs, real slow-healthy/problem distributions and longer-period controls, edge-triggering, one outstanding review per state, and no vote or kill authority. Same-session nudge work cannot begin until provider-terminal cancellation is proven end to end.

## Slice 1 — task creation is the first bottleneck

### Current mechanism

`parallel_plan` creates coarse skeleton tasks. `detail_plan` then asks a normal Goose agent session to turn each skeleton into about 150 words of implementation-ready prose. That session:

- receives the full overall goal and full research body;
- has no typed final-output schema;
- is allowed planner-side turns that reach 60 in the uncapped configuration;
- receives the standard developer tool surface (`write`, `edit`, `shell`, and `tree`) even when no extensions are passed;
- runs in the real working directory.

The live activities show detailers writing application modules, tests, README content, and decisions before `plan_loaded`. Historical F923 evidence refines this: many workers later removed their own scratch files and no cross-worker deletion was proven, but cleanup was inconsistent and survivors can contaminate the build or score. The defect is unauthorized shared-tree side effects during planning, not a claim that every prototype survives.

The current plan schema contains only `id`, `description`, `difficulty`, `model`, `depends_on`, and `files`. It cannot represent exact requirement ownership, interfaces, inputs, outputs, invariants, forbidden overlap, acceptance evidence, promotion scope, or the version of a dependency contract consumed by a task.

The structural skeleton scorer also contradicts the prompt's claim that fleet width is irrelevant: it rewards independent roots up to worker count, rewards task count at least worker count, and penalizes depth/choke points. This biases weak planners toward a flat, fleet-shaped graph. The r1 all-zero-dependency plan is the observed result. `split_fat_modules` then splits primarily by file/role, which does not establish independent semantic acceptance boundaries.

There are also hardcoded Qwen3.6 model names in planner prompts while the active fleet is Qwen3.8. That is both stale and non-generic.

### Planned replacement: task compiler, not free-form detail agents

The skeleton/detail phase becomes a typed task compiler:

- A task owns requirement IDs and an independent acceptance closure, not merely a file.
- Use a small universal core—objective, requirement references, ownership, dependency contracts, acceptance closure, and overlap policy—plus task-kind-specific sections. Exact literals and behaviors live once in a normalized requirement/interface registry and are referenced by ID; copying every field into every task would create generic schema boilerplate and silent divergence.
- A detailer receives only the requirement slice, relevant evidence ledger entries, upstream contracts, and read-only repository excerpts it actually needs.
- Ordinary detailers have no write/edit/shell capability. Repository inspection uses a purpose-built read-only surface. Structured output is validated before it can enter the graph.
- Missing or generic fields trigger a semantic repair of that specification. Deterministic checks enforce schema, references, cycles, and ownership only; the semantic judge grades task-specific sufficiency. Failure does not trigger a fixed token cutoff or silently fall back to the original one-line skeleton.
- The semantic split authority is acceptance closure. If one specification contains separable outcomes, an idle judge proposes children with full contracts, dependency edges, and a non-overlap proof. Splitting by filenames, node count, or current queue pressure is insufficient.
- Plan prompts use capability roles from the actual roster rather than hardcoded model names.

This is a high-confidence correction for planning contamination, specificity, dependency integrity, and safe task fanning. It is only a candidate tail treatment. Matched replay arms must separately remove the tool menu, add the schema, slice the goal/research context, change task grain, and change the role prompt; sampler/template arms remain separate. That design can identify which mechanism reduces long reasoning without converting correlation into a story.

## Slice 2 — research and planning should collaborate on one evolving truth

### Current mechanism

Research is a short fixed fan over architecture, libraries, and edge cases. Planning then asks multiple models for whole competing plans, selects structurally, and runs another fleet-wide backbone/convergence pass. Every task is detailed before contracts begin, and every contract completes before `plan_loaded`. This creates repeated whole-problem reasoning followed by global barriers.

The 97%-converged r2 still spending another 92 minutes on a backbone round shows that the current convergence signal does not control work. Conversely, the research stage is too brief and fixed to resolve the specification's actual unknowns. Models repeatedly rediscover those unknowns inside task detail and build sessions.

### Planned replacement: evidence ledger plus canonical plan patches

1. Preserve the raw frozen specification as the authority. Generate a semantic inventory with stable IDs, bidirectional raw-clause citations, exact literals, expected surfaces, acceptance evidence, and unresolved/contested state. Any raw clause without a trace is uncovered; an inventory omission can never redefine scope.
2. Maintain a canonical evidence ledger. Each claim records requirement IDs, source/provenance, affected interfaces, confidence, conflict state, and what decision it enables.
3. Fan research by unresolved material questions, not three permanent lenses. Nodes claim different questions, and the queue expands when research reveals new unknowns. Research converges when the ledger has no unresolved material conflict or uncovered requirement—not after a fixed lookup or time count.
4. Before exposing one canonical graph, collect blind independent requirement/contract inventories or alternate-seam critiques to reduce seed/groupthink bias. Then seed the canonical graph. Other nodes submit typed patches in distinct roles—requirement/coverage critic, dependency/interface critic, and execution/verification critic—instead of paying for repeated full plans.
5. The semantic judge resolves incompatible patches against the requirements and evidence. Deterministic validation checks only schema completeness, cycles, ownership collisions, dangling contract versions, and missing coverage.
6. Contracts move through `PROVISIONAL`, `FROZEN`, and `SUPERSEDED`. A superseding semantic change invalidates affected descendants, propagates re-review, and blocks promotion of stale consumers. Downstream detail work and eligible implementation can begin against frozen contracts while unrelated branches are refined, removing global all-details/all-contracts barriers without pretending version labels alone make work safe.
7. A second whole-plan convergence round runs only when a material unresolved conflict remains. Structural similarity alone neither forces nor forbids it.

This keeps planning specific while moving useful multi-model effort into research, criticism, interface design, and verification rather than redundant complete plans.

## Slice 3 — full-fleet use requires two queues and physical admission

### Current mechanism

The scheduler already uses more than task count: DAG readiness, files/claims, speed, retry history, affinity, pre-review, tail review, Q&A, speculation, dynamic replanning, and judge work all affect dispatch. The prior idea that fanning should depend only on tasks was therefore incomplete.

However, current “free” capacity is derived from logical `in_flight < weight`. It is not physical decoder idleness. On the active topology, a weight of two can make the scheduler send supervision to a host already generating. Historical F623 evidence says two concurrent Apple decode streams can reduce aggregate throughput. Conversely, during r2 Gabee was physically idle behind a single global detail barrier.

Historical F122→F123→F162→F163 findings are explicit counterexamples to simplistic “idle means run judge” logic: judge rates were twice misread, useful supervision demand often appeared while all decoders were occupied, tail idleness sometimes arrived only after the review target had gone stale, and a deterministic flat-progress predicate killed a worker that later produced its artifact. The broker must therefore match useful, version-current work to verified physical capacity; neither task count nor idle count is a sufficient policy.

The prologue deduplicates lanes by model string rather than an explicit physical host identity. The scheduler also releases occupancy and file claims when it aborts the Rust future, although LM Studio may still be unwinding the remote generation. This can create phantom-free capacity.

### Planned replacement: adaptive physical-host broker

The broker tracks physical host and loaded model instance separately from logical aliases and slots. It records queued, prefilling, decoding, cancellation-requested, provider-terminal, and idle states.

Its default value order is:

1. ready critical-path implementation or repair with a frozen contract and safe ownership;
2. ready non-critical implementation with a frozen contract;
3. evidence-producing auxiliary work: unresolved research, contract review, acceptance-oracle/test authoring in isolation, integration preflight, completed-artifact review, or live semantic supervision;
4. no request when additional concurrency has measured negative marginal throughput and no useful isolated work exists.

Auxiliary work is preemptible only before request admission: newly ready critical implementation normally outranks queued low-value judge, QA, pre-review, tail-review, test-generation, and speculation. This is not a permanent “implementation always wins” rule. A version-current semantic review whose measured marginal quality/value has become higher may claim the next verified-terminal slot even under sustained build backlog; no host is permanently reserved for it, and an admitted generation is not killed merely to make room. The current fixed review priority and all-free-device tail review can starve build work, while strict idle-only supervision can starve the judge; the unified broker schedules both from evidence.

An idle physical node does not blindly duplicate an unsplittable task. It inspects that task's contract and current trace, then either semantically supervises it, develops independent acceptance evidence, prepares an isolated dependency, or proposes a contract-complete split. This is how the judge adds quality when implementation parallelism is structurally blocked.

Concurrency per host is learned from controlled one-versus-two-stream measurements for that exact runtime, quantization, context regime, and role. “Use every node” means avoid physical host idleness when useful work exists; it does not mean saturate every configured parallel lane regardless of contention.

The scheduler reports productive versus supervisory occupancy, queue wait, prefill/decode time, interference, contract-blocked time, tail duration, and requirements closed per node-minute. Node occupancy without a useful role is not credited.

## Slice 4 — the semantic judge becomes an asynchronous control plane

### What the data proves

The user is correct that the semantic nudge can turn a run. In a historical Fable run, `verify-e2e::1` tested the wrong command. The judge identified the exact task-position drift and named the correct pagination cases; the retry acknowledged the error and performed the right checks. A same-session nudge would preserve that value without discarding a long attempt.

The completed r1 run also proves deterministic semantic decisions are unsafe:

- A judge returned `VERDICT|OK|HIGH|... no looping or drift`; the parser scanned rationale keywords, classified “looping,” killed the valid attempt after 148 seconds, and spent another 206 seconds retrying the same conclusion.
- A semantic judge said `integrate-verify` was healthy and still mapping the contract. Nineteen seconds later, while reasoning was advancing and `app/api.py` remained missing, a deterministic inactivity heuristic accepted the task, aborted it, and marked it done with null output.

The current inner omni-judge is semantic, but it is synchronously awaited inside the worker stream loop and always routed to `planner_model`. Its question is largely repeat-versus-progress, so it cannot detect an advancing worker that is solving an oversized or wrong problem. In r2, 121 likely omni probes on Workhorse consumed about 53.5 node-minutes during detail. The exact purpose is inferred because that running binary predates purpose-labelled judge telemetry.

The r1 outer review system issued 22 pre-reviews and eight tail reviews, consumed about 136.4 node-minutes, and reported zero findings. Repeated identical OK reviews are activity, not value. Archived `judge_nudge` arms emitted zero actual nudge events, so their score does not validate the mechanism yet.

### Planned judge behavior

- Keep the judge semantic. Neutral recurrence, progress, tool, artifact, occupancy, and transport signals only decide when and what to inspect.
- Run reviews asynchronously on a genuinely idle physical host; never pause worker stream consumption and never silently add a second decode to a busy planner endpoint.
- Give the judge the actual task contract, acceptance oracle, dependency and sibling contract versions, targeted artifact excerpts, recent reasoning/tool trace, previous intervention, and observed response to it.
- Require a strict structured action enum: `CONTINUE`, `NUDGE`, `SPLIT_PROPOSAL`, `ROUTE_FINDING`, or `ACCEPT_CANDIDATE`. Parse only the enum field. Rationale text cannot change the action.
- Add `ABSTAIN` / `INSUFFICIENT_EVIDENCE`. Unknown, malformed, ambiguous, or stale output continues the worker and records a parser failure; it can never trigger an intervention.
- Prefer a same-session semantic nudge. Kill/re-dispatch is reserved for transport/session failure or a separately verified inability to continue, not ordinary drift.
- A split is a proposal until all child contracts are specific, acceptance-complete, dependency-correct, and non-overlapping.
- `ACCEPT_CANDIDATE` requires semantic requirement-coverage review and is then confirmed only within the objective oracle's actual scope. Silence, file existence, elapsed time, reasoning shape, or a narrow passing test never makes arbitrary software correct. Exhausted evidence with unmet coverage yields `INCOMPLETE`, not acceptance.
- Bind every review to immutable artifact and trace snapshot hashes as well as task/contract versions. If work changes, stale verdicts are discarded or revalidated against the current artifact. If a valid finding arrives after completion, it is routed to the owning integration/repair task rather than dropped.
- Record the causal chain: review request, physical reviewer, snapshot hashes, latency/tokens, verdict, nudge delivery, next worker action, artifact/evidence delta, and final outcome. Review value is precision, actionability, unique evidence consumed, and verified downstream correction—not finding count.

The committed F924 recurrence detector may be useful only as one neutral summons after its metric, provenance, debounce, and state machine are replaced. It cannot corroborate a verdict, become a deterministic killer, or start a second request. No same-session nudge ships before end-to-end cancellation and provider-terminal admission pass their gate.

## Slice 5 — inner Goose and LM Studio transport hide abandoned work

The 30–40k generation is not primarily an HTTP latency problem. When a model alias has no canonical output limit, Goose omits `max_tokens`, so the generation is allowed by design. Adding a hard cap would hide the unresolved task/prompt/template/sampler causes and convert long-but-valid reasoning into arbitrary failures.

Cancellation is nevertheless incomplete:

- the swarm scheduler aborts the Rust task;
- `run_agent_in` passes no cancellation token to `Agent::reply`, even though the inner agent supports one;
- occupancy and file claims can be released before the provider proves termination;
- dropping the local stream does not prove LM Studio released its decode slot.

The implementation must carry cooperative cancellation from scheduler attempt to agent stream and provider request, then release capacity only after a provider-terminal observation. LM Studio slot-release behavior will be measured; confidence is medium until that experiment is possible.

Telemetry is survivor-biased because timed provider telemetry is written after a stream drains. Judge-aborted, timeout-dropped, and speculative-loser calls can disappear. Required lifecycle events are request started, first item, cancellation requested, local stream dropped, provider terminal, response/request ID, finish reason, and classified error. Prompt content need not be logged.

Thinking control is also role-inconsistent. Qwen is served through the OpenAI-compatible path, whose documented standard Chat Completions fields do not include `reasoning_effort`. LM Studio's local model YAML maps its UI selector into the `reasoning_effort` Jinja variable, but the native loaded-instance response does not prove which rendered branch was used. `ThinkingEffort::Off` in Goose is therefore not proof that a Qwen call was non-thinking. Detail, judge, summary, compaction, label, and implementation roles need explicit tested request profiles.

Lifecycle telemetry and terminal admission are Engine 1 correctness work, not a later optimization. A retry before a first streamed item, semantic nudge, speculative replacement, or timeout restart is forbidden until the old request has a correlated request ID and provider-terminal observation. Otherwise a local future drop can create the very phantom occupancy and double-decoding that the scheduler is meant to remove.

## Slice 6 — Qwen3.8 sampling is an experiment, not a folklore preset

### What is established

The [official Qwen3.8-27B model card](https://huggingface.co/Qwen/Qwen3.8-27B) enables thinking by default. Its published thinking sampler is temperature 1.0, top-p 0.95, top-k 20, min-p 0, presence penalty 0, repeat penalty 1.0; its non-thinking sampler is temperature 0.7, top-p 0.8, top-k 20, min-p 0, presence penalty 1.5, repeat penalty 1.0. Those are controls, not proof of the best agentic configuration.

The current user profile—medium thinking, temperature 0.7, top-p 0.8, min-p 0—is a legitimate hybrid to test. It should not be “corrected” from a model card without evidence.

Model-specific field evidence is mixed:

- [Qwen issue #216](https://github.com/QwenLM/Qwen3.8/issues/216) reports much higher non-completion under extra-high reasoning and failures at 8,294–26,137 generated tokens rather than merely at the context ceiling. In one limited arm, repeat penalty 1.1 reduced empty completions from 8/48 to 0/23 without changing F1; that is promising but not enough to standardize.
- A [community reasoning-effort comparison](https://www.reddit.com/r/LocalLLaMA/comments/1vpuh7m/qwen38_27b_reasoning_effort_lowmediumxhigh/) reported roughly 4.4k, 5.9k, and 39.4k reasoning tokens for low, medium, and extra-high, with extra-high taking about 6.4 times the wall time for a modest quality gain. This is anecdotal and workload-specific.
- A [temperature discussion](https://www.reddit.com/r/LocalLLaMA/comments/1vr3cma/weirdly_no_one_talks_about_temperature_setting/) contains mutually incompatible reports: temperature 0.7 fixed one setup, while another still exceeded 30k tokens; a third favored 0.6/0.95/top-k 20/presence 1.5. That disagreement is evidence that prompt/template/runtime interactions matter.
- Reports that the same medium/0.7 profile works in some agent harnesses and loops in another point back to rendered system prompts, tool schemas, preserved reasoning, and task shape rather than a universal temperature.
- The original [min-p paper](https://arxiv.org/abs/2407.01082) motivates adaptive truncation, but a [controlled reanalysis](https://arxiv.org/abs/2506.13681) disputes broad superiority over controlled baselines. Min-p is therefore an exploratory later lever, not the first fix.

LM Studio's [Chat Completions documentation](https://lmstudio.ai/docs/developer/openai-compat/chat-completions) confirms that the server applies a model chat template. Its [log-stream command](https://lmstudio.ai/docs/cli/serve/log-stream) exposes rendered model input, not the complete serialized HTTP body or every Jinja variable; both must be captured and correlated by request ID. The [native model-list endpoint](https://lmstudio.ai/docs/developer/rest/list) exposes loaded-instance configuration. The local model YAML defaults to extra-high reasoning, thinking enabled, preserved thinking enabled, and the official 1.0/0.95 sampler; the user's UI overrides must be verified rather than inferred.

### Experimental matrix

Gate 0 captures, per role and physical host:

- exact request body and rendered chat template, correlated by request ID;
- actual `reasoning_effort`, `enable_thinking`, and `preserve_thinking` template variables;
- temperature, top-p, top-k, min-p, presence/repeat penalties;
- tool schema and role prompt size;
- runtime version, quantization, context length, KV/cache settings, parallelism, and MTP state.
- SB7 fixture seed and model sampling seed as separate values. Goose currently has no verified sampling-seed plumbing, so LM Studio's supported `seed` field must be carried to the wire and tested before deterministic paired comparisons are claimed.

No sampler arm is interpreted until Gate 0 passes.

The first arm is an exact clone of the captured current request, including omissions; it is not silently normalized to top-k 20, presence 0, or repeat 1.0. After that control is proven, core paired arms hold verified medium reasoning, top-k 20, min-p 0, presence penalty 0, and repeat penalty 1.0 constant:

1. `0.7 / 0.8` — normalized comparison arm (the exact current control is the preceding omission-preserving clone);
2. `0.7 / 0.95`;
3. `1.0 / 0.8`;
4. `1.0 / 0.95` — official thinking control.

Then test independently:

- repeat penalty 1.0 versus 1.05 versus 1.1;
- presence penalty 0 versus 0.5 versus 1.0, with 1.5 as the non-thinking control only when recurrence is present;
- min-p 0 versus 0.02 versus 0.05 only after the stronger levers;
- low versus medium reasoning by phase, with extra-high reserved for targeted high-value adjudication unless evidence justifies it;
- thinking disabled only for separately validated clerical/schema roles;
- preserved prior thinking on versus off for multi-turn sessions;
- MTP, quantization, runtime version, and one-versus-two concurrent streams as separate experiments.

Runs use matched prompts/tasks, multiple seeds, host rotation, and ABBA or Latin-square ordering to avoid thermal/host/order confounds. A pre-change request corpus is frozen now; it characterizes sampler/template effects without the new compiler prompt, and any winning profile is then revalidated on the new role prompts. Metrics are hermetic task quality, critical checks, wall time, reasoning/output distribution and tail, valid tool calls, retries, first-pass acceptance, and useful requirements closed per 1,000 reasoning tokens and per node-minute. There is no production generation cap; the experiment observes natural stopping behavior. Judge-interrupted calls are recorded as censored/intervened observations and never presented as natural stopping lengths.

## Slice 7 — build and repair must close requirements, not generate activity

Build is relatively stronger than planning and repair, but r1 proves nominal completion can coexist with missing central surfaces. Verification tasks often checked imports or public names rather than the exact user-visible contract. Some correctly reported missing frontend artifacts, yet the global run still spent hours in repair without closing them.

The current repair system groups findings largely by files. Attributable findings become shards; ambiguous findings can race whole-tree repair twins. In r1:

- round zero had four shards lasting about 25, 33.6, 82, and 139.5 minutes; all promoted;
- Workhorse ran two same-host repair streams concurrently, while the slowest became a 2h19 tail and other hosts idled;
- later db/notifier/cross-file attempts consumed substantial time without promotion;
- findings remained, then the fixed round structure stopped;
- boot repair and final review added about 46 minutes.

Uncapped mode expands individual attempt time to days but repair still has a fixed round floor and hard maximum. That is the wrong combination: enormous attempt allowance plus a hard iteration count unrelated to new evidence.

The replacement is a causal defect ledger:

- Every planned task produces an artifact/evidence receipt tied to requirement IDs and acceptance checks. A missing required output cannot be counted done.
- Join points run contract and integration checks against exact dependency versions before downstream promotion.
- A failure is normalized into reproduction, expected result, observed result, implicated requirement/contracts, evidence, current hypothesis, owner, safe write scope, and verifier.
- Findings with one root cause are repaired once, even when many tests expose them. Independent causal hypotheses fan out; alternative hypotheses race only when ambiguity makes that informative.
- Repair occurs in isolated shadows or owned workspaces. Promotion is progressive and evidence-gated, not wholesale because an agent said it fixed something.
- Idle physical nodes provide semantic hypothesis review, reproduction tightening, oracle construction, and integration checking rather than duplicate whole-tree edits.
- An attempt continues while it produces a new falsifiable hypothesis, new evidence, or verified requirement closure. It concludes when gates are green or the semantic judge records evidence-backed hypothesis exhaustion. There is no fixed repair-round or wall-clock correctness cutoff.

## Slice 8 — applicable parent-Goose work

The fork is thousands of local commits away from upstream and has already adapted several upstream fixes under different commit identities. No upstream change should be blindly cherry-picked.

Behavior-level ports to evaluate independently, never as one bundled arm:

- Split [`b7ddf933`](https://github.com/aaif-goose/goose/commit/b7ddf933c429c2553713dc6d5e0347c1cec43872) into cancellation-select and pre-first-item transient retry. Cancellation is evaluated after request-terminal telemetry exists. Retry is admitted only when the request ID/provider proves the prior decoder stopped; otherwise it can create a second live request.
- [`85aac194`](https://github.com/aaif-goose/goose/commit/85aac194044aadbb58cfb62b1b927e919be89652) is a narrow structured-context-overflow delta. The fork already classifies message text; port nested `error.code` and `n_prompt_tokens > n_ctx` only against a captured local failing payload.
- The fork already has late `recover_mangled_tool_name`. Evaluate the useful part of [`f2e6e9ed`](https://github.com/aaif-goose/goose/commit/f2e6e9ed05ec22508f13403a52f654c11e395cfd): canonicalize before permission/hook inspection, reject ambiguous matches, and prove permission metadata remains attached.
- [`1f6c7524`](https://github.com/aaif-goose/goose/commit/1f6c7524e1ad1b3b46f5653390af4b79614d17d8) fills a real representation gap: `collect_stream` coalesces text but not consecutive thinking inside one response; later `Conversation::push` cannot repair an already multi-block message. Require a captured Qwen stream and block-count, memory, and session-serialization tests. This can reduce representation overhead, not generated-token duration.
- The current parser already handles empty `choices` usage frames and explicit error objects. Evaluate only the absent-`choices` metadata/error delta from [`1844d3fb`](https://github.com/aaif-goose/goose/commit/1844d3fb4aed0ec7f2e3806829cb887981f15ead) against an actual LM Studio frame.

Secondary candidates are bounded partial-`<think>` state (`f3ab1557`), a composite session-message index (`701e93ab`), and selected request/response lifecycle metadata from `f45ccd46`. They do not outrank task compilation, cancellation, judge admission, or complete request telemetry.

Do not port the 88-file unrolled/state-machine agent-loop refactor, main-model compaction by default, broad cache/request restructuring, or unrelated toolshim/UI/OpenAI Responses work. The fork already adapted inactivity timeout, prefix-cache turn-context isolation, output-limit markers, reasoning carryover, one malformed-tool recovery path, empty-turn retry, and local proactive compaction; reapplying them would add risk without new behavior.

Each accepted port gets its own mechanism test, commit, and campaign arm. A combined “upstream improvements” result would be causally uninterpretable.

## Slice 9 — every existing lever needs a disposition, not an “all on” run

The current lever machinery cannot support causal claims as written:

- `SwarmConfig` has 116 fields. `~/goose-builds/loop-state/arm_config.py` catalogs 87 names, but only 85 are real; `repro_demotes_verified` and `review_repro` are stale/inert.
- The campaign catalog omits 31 real fields: `act_now_nudge`, `complete`, `complete_cap_secs`, `contracts`, `devices`, `doc_fetch`, `dynamic_replan`, `e2e_oracle`, `endpoint`, `fan_e2e`, `fix_sched`, `force_write_tool`, `judge_nudge`, `kind_prompt`, `lm_extra_body`, `planner_model`, `read_on_fix`, `research_planning`, `sink_cap_ref_bytes`, `sink_review`, `smoke`, `spec_sized_plan`, `speed_weights`, `split`, `split_secs`, `supervision_pool`, `think_off_test_authors`, `top_k`, `uncapped`, `verify_commands`, and `worker_extensions`.
- `levers_resolved` semantically covers about 105 of 116 config fields, completely misses 11, renames `dynamic_replan` to `dynamic_replan_cfg`, and often emits raw rather than effective uncapped values. It reports `split_inherit_spec=false` when the scheduler's unset default executes as true. Default-on env-only judge, review, QA, repair, salvage, and ship-best paths are not comprehensively echoed.
- `APP_FORCED` is obsolete: the provider no longer force-enables its six listed behaviors, but the campaign still refuses them. `ENV_ONLY` is stale because `ask_replan` and `complete` now have config fields.
- There are another 50 real env-only controls. Default-on judge, pre-review, tail-review, QA, salvage, ship-best, and benchmark-derived prompt paths are behavioral levers even when they never became `SwarmConfig` fields.

The first implementation artifact is a generated lever disposition register. Its source set is `SwarmConfig`, `Default`, every env read and resolver, CLI/UI overrides, provider-forced defaults, direct routing/topology inputs, hardcoded behavioral constants and prompt injections, `levers_resolved`, `arm_config.py`, historical `LEDGER.tsv`, and `FINDINGS.md`. Every row carries canonical name, aliases, default, effective resolved value, phase/use sites, interactions, evidence, disposition, migration, and exact isolated arm. Tests fail when a config field, env gate, override, routing input, behavioral constant, or resolved event lacks a register row or when the event differs from effective runtime behavior.

Initial disposition, to be encoded and then verified:

The complete audited disposition is recorded now; implementation makes it generated/enforced rather than rediscovering it. Moving deterministic `repeat_break`, `straggler_stop`, `backbone_skip_confident`, and `degrade_on_stall` from “keep” to semantic/evidence-gated redesign yields 30 retain/enabled, eight retain/disabled pending evidence, 32 modify, 12 remove/merge, and 34 runtime/profile controls across all 116 config fields:

- **Retain/enabled (30):** `stream_decode_retry`, `planner_also_works`, `sink_lean_prefill`, `e2e_oracle`, `spec_sized_plan`, `delegated_decisions_ok`, `clarify_spec_bound`, `spec_wins`, `clarity_fail_closed`, `spec_contract`, `retarget_stall_guard`, `answers_win_floor`, `cross_module_check`, `smoke`, `verify_commands`, `fan_e2e`, `no_tools_means_ask`, `author_pitfalls`, `grounded_research_only`, `ts_smoke_tests`, `failed_tasks_block_green`, `sink_prebuild`, `user_notes`, `contract_validate`, `kind_prompt`, `occupancy` as always-on observation, `doc_prefetch`, `dep_signatures`, `act_now_nudge`, `require_tests`.
- **Retain/disabled pending evidence (8):** `straggler_stop_degrade`, `goals`, `ask_replan`, `contract_retry`, `incremental_replan`, `ask_away`, `write_first`, `think_off_test_authors`.
- **Modify before testing (32):** `max_attempts`, `max_research_questions`, `dynamic_replan`, `max_replans`, `research_scouts`, `parallel_planning`, `best_of_n_skeletons`, `progress_watchdog_secs`, `omni_judge`, `converge`, `diverse_plan`, `retarget`, `supervision_pool`, `judge_nudge`, `fix_sched`, `ask_max_q`, `split`, `contracts`, `complete`, `backbone`, `review`, `unwired_demotes_verified`, `persona`, `relax_contracted_deps`, `split_fat`, `doc_fetch`, `fan_verify`, `parallel_tests`, `repeat_break`, `straggler_stop`, `backbone_skip_confident`, and `degrade_on_stall`. Repetition and artifact presence become neutral review evidence; elapsed grace cannot abort the final draft; semantic ledger coverage—not structural similarity—decides whether a backbone round is redundant; exhaustion yields `INCOMPLETE` unless semantic/evidence-gated salvage proves coverage.
- **Remove/merge (12):** `sink_review`, `detail_memo`, `spiral_break_chars`, `homogeneous_models`, `speed_weights`, `delivery`, `owned_file_fence`, `spiral_thinking_chars`, `read_on_fix`, `force_write_tool`, `scoped_contracts`, `split_secs`.
- **Runtime/profile, not causal arms (34):** `endpoint`, `planner_model`, `devices`, `worker_max_turns`, `straggler_grace_secs`, `worker_extensions`, `planner_weight`, `context_cap`, `research_planning`, `worker_timeout_secs`, `planner_timeout_secs`, `allow_model_load`, `temperature`, `top_p`, `top_k`, `min_p`, `repeat_penalty`, `max_tool_response_chars`, `scout_budget_secs`, `scout_max_lookups`, `sink_cap_secs`, `sink_cap_ref_bytes`, `uncapped`, `lm_extra_body`, `ask_floor`, `struct_stop`, `clarity_probe_secs`, `sink_max_turns`, `draft_timeout_secs`, `retarget_rounds`, `complete_cap_secs`, `draft_temp`, `ask_rounds_max`, `research_tools`. Time/turn/count/byte values resolve adaptively and are not quality levers.

The 50 env-only controls also reconcile completely:

- **Retain/enabled (14):** `BOUNDARY_PROBE`, `CLI_CONTRACT`, `COMPILE_GATE`, `CSS_COHERENCE`, `DOM_ID_SCAN`, `DONE_GATE`, `OVERVIEW`, `QA`, `REQUIRE_SERVABLE`, `RESUME`, `SALVAGE_REQUIRE_CRITICAL`, `SCOUT_DOC_URLS`, `SKELETON_FIRST`, `SPLIT_INHERIT_SPEC`.
- **Retain/disabled pending evidence (3):** `DOC_EXAMPLES`, `SPECULATE`, `TESTGEN`.
- **Modify (9):** `COMPLETE_ROUNDS`, `COMPLETE_STALL_ROUNDS`, `JUDGE`, `PREREVIEW`, `SALVAGE_SPIN`, `SHIP_BEST`, `SINK_SHARD`, `SPEC_REPAIR`, `TAIL_REVIEW`.
- **Remove/merge (8):** `ASK_SCALE`, `ASSURED`, `COMPLETE_PARALLEL`, `FILL_FAN`, `PREREVIEW_DIMS`, `PROBE_ADVERTISED_POST`, `SPLIT_FAT_FILES`, `WEB_VOCAB`.
- **Runtime/profile (16):** `AI_NAME`, `ASK_FILE`, `ASK_WAIT_SECS`, `DETAIL_BUDGET_SECS`, `FIX_CAP_SECS`, `INHERIT_HINTS`, `MAX_NODES`, `NAME_TIMEOUT_SECS`, `PIN_DEVICE`, `RENDER_NODE`, `RENDER_PROBE`, `RETARGET_DRAFT_STEP`, `RETARGET_STALL_TOLERANCE`, `RUN_DEADLINE_UNIX_MS`, `TAIL_REVIEW_SECS`, `TELEMETRY_FILE`.

Two comment-only names, `GOOSE_SWARM_REVIEW_FANOUT` and `GOOSE_SWARM_REVIEW_REPRO`, are obsolete/inert and are removed from operator-facing documentation.

- **Retain or bake in as correctness/provenance rails:** stream decode retry after terminal-safe semantics, lean sink prefill, e2e oracle, spec-sized planning intent, delegated-decision/spec/answer guards, smoke/spec-contract/verify/fan-e2e evidence, grounded research/doc prefetch, failed-task/no-tests truth, user notes, contract validation, kind prompt, dependency signatures, and act-now signaling. Exact repeated tool/result patterns remain neutral semantic-review evidence; they cannot abort, fail, accept, or corroborate arbitrary work. Objective transport-level duplicate suppression is a separate mechanism. These rails become dependable behavior only where there is no legitimate “off” meaning.
- **Keep off until isolated evidence exists:** destructive straggler degradation, goals, ask-replan, contract retry, incremental replan, ask-away, write-first, thinking-off test authors, test generation, fill-fan, and speculative twins. A switch existing is not evidence it should be enabled.
- **Redesign before retesting:** parallel/convergent/diverse/backbone planning, best-of-N, retargeting, dynamic replan, parallel tests, split/split-fat/relaxed dependencies, every judge/supervision path, completion/fix scheduling/spec repair/sink shard/complete-parallel, fan verification/review/persona, document fetch, occupancy, and all caps/watchdogs. These currently mix multiple mechanisms, block the main scheduler, or use logical rather than physical state.
- **Remove, merge, or make non-optional:** duplicate `sink_review`; retired `detail_memo`; delivery bundle; restore-at-sink `owned_file_fence`; current flat-DAG `scoped_contracts`; static `homogeneous_models`/`speed_weights`; fixed-time `split_secs`; deterministic spiral volume controls; `read_on_fix` as a switch (replace with typed repair scope); and the combined `force_write_tool` lever (retain only separately justified thinking-preclose recovery). Numeric provider/runtime profile values remain configuration, not causal on/off levers.

Three defects outrank merely toggling the list:

1. Task existence is still fleet-shaped despite `spec_sized_plan`: skeleton scoring rewards roots/count relative to worker count, `parallel_tests` explicitly scales tasks to the fleet, and best-of-N grows with fleet size. `relax_contracted_deps` can then delete real module edges. The task graph must be job-defined; the broker may adapt execution slicing to resources without rewriting semantic work to fit three machines.
2. `WEB_VOCAB` and `PROBE_ADVERTISED_POST` leak old benchmark-specific frontend/vendor semantics into generic workers. Replace both with interfaces derived from the active specification; do not carry `sync-button`, `payments-table`, or vendor-sync assumptions into arbitrary projects.
3. Repair's “monotonic”/ship-best choice compares finding count rather than finding identity and severity, so one critical defect can beat two minor ones. Promotion must compare requirement closure and severity-aware causal evidence.

The campaign will never switch all levers on together. Foundational corrections are baked or removed first; remaining hypotheses are tested one mechanism per arm, including interaction arms only after both components have independent evidence.

## Slice 10 — five isolated cloud baselines, serial hermetic scoring, direct truthful publication

All requested model families exist, but the current Goose adapters are not benchmark-ready:

- Z.AI's exact ID is [`glm-5.3`](https://z.ai/blog/glm-5.3). It uses enabled thinking with `low|high|max` effort and recommends `max` for coding. Goose's Z.AI catalog stops at GLM-5.2, so GLM-5.3 falls through to a generic 4,096-token output limit, and Z.AI-specific effort semantics are lost through the current declarative Anthropic formatter. The supplied key must be probed after approval against the exact general or Coding Plan entitlement; endpoint substitution is not allowed. Z.AI documents the endpoint families separately in its [API introduction](https://docs.z.ai/api-reference/introduction) and [Goose setup](https://docs.z.ai/scenario-example/develop-tools/goose).
- Google's exact IDs are [`gemini-3.7-flash`](https://ai.google.dev/gemini-api/docs/models/gemini-3.7-flash) and [`gemini-3.1-pro-preview`](https://ai.google.dev/gemini-api/docs/models/gemini-3.1-pro-preview); there is no stable `gemini-3.1-pro` ID. Current unknown-model fallback incorrectly limits both to 4,096 output tokens. The Google formatter does not retain the provider call identifier correctly across tool results, maps global medium thinking to low, and omits thought-token usage. Gemini 3.7 requires exact call ID/name handling, positional thought-signature replay, removal of temperature/top-p/top-k overrides, and native medium thinking. The custom-tools preview is a different entrant and will not be silently substituted.
- DeepSeek's exact IDs are [`deepseek-v4-flash` and `deepseek-v4-pro`](https://api-docs.deepseek.com/updates/). Both support a 1M context and up to 384K output, but the declarative catalog lists legacy aliases and the generic formatter does not emit V4 reasoning effort. DeepSeek requires reasoning content to be replayed throughout tool-bearing turns; sequential and parallel tool loops need contract fixtures before a paid episode.

Provider conformance therefore precedes cloud spend. Each adapter gets local request/response fixtures, exact model metadata, output/context limits, reasoning semantics, tool-call identity, thought/reasoning preservation, streamed usage, and error/finish classification. Then the pinned release binary performs one minimal paid two-step tool smoke per exact model. Alias fallback, 4K truncation, schema errors, missing usage, or response-version drift fails the gate.

Fair predeclared profiles omit temperature/top-p/top-k and use provider-native documented reasoning: GLM-5.3 enabled/max, Gemini 3.7 Flash native medium, Gemini 3.1 Pro Preview native high, and DeepSeek V4 Flash/Pro enabled/high. All five receive the same frozen SB7 prompt, Goose binary, tools, permissions, and clean-tree policy. A manifest records response-reported model/version, endpoint family, thinking profile, context/output limits, binary/adapter/prompt/tool-schema hashes, independently pre-drawn fixture seed, advertised port, runtime, timestamps, usage, and cost. Provider versions cannot be silently mixed.

Paid authority is explicit: at most USD 400 total—USD 250 Google, USD 100 Z.AI, and USD 50 DeepSeek—including capability smokes and restarts—and at most two full paid episodes per model (the initial episode plus one restart for a verified infrastructure/instrument defect). A failed request gets at most three classified retries only when it was never admitted by the provider or the exact request is proven terminal, respecting `Retry-After`; outcome-driven and ambiguous-stream retries are forbidden. Before every provider request/turn, the coordinator reserves worst-case cost from exact input tokens, the pinned price/version, configured provider maximum output, and any admitted continuation exposure. It admits the request only if that reserve fits both envelopes, then releases unused reserve from reported usage. Missing usage, unknown pricing, or an unbounded request fails closed before admission. Crossing a provider or total envelope therefore stops before the next model turn and seals the episode `INCOMPLETE`; it is never scored as model failure. No admitted response is cut off merely to hit the budget, and no correctness conclusion is inferred from the spend gate.

After approval, the three supplied secrets are written once to `/Users/mihaiperdum/.agents/skills/goose-benchmark-iteration/secrets/cloud-providers.env` in a `0700` directory with file mode `0600`. `SKILL.md` records only the path and variable names. The coordinator parses rather than shell-sources it, injects only one relevant key into each child, redacts logs, and never writes a secret to git, arguments, manifests, state, or site data.

`run_build.py` becomes a generic entrant-manifest harness with build-only separated from scoring. A detached, resumable coordinator launches five supervisors together while engine development continues in another worktree. Every lane has an immutable copied binary, unique clean tree, reserved vendor/application ports, `GOOSE_PATH_ROOT`, config/data/state roots, process group, logs, atomic state, and attempt directory. A resource-conflict graph governs request admission: two models sharing a key run simultaneously only after authenticated quota/concurrency evidence proves no throttling or latency interference; otherwise those two provider lanes serialize while other vendors continue. CPU/I/O-heavy local tool phases take a shared resource lease, and heavy engine builds/tests run on the workhorse or wait. It exposes `start/status/watch/results/stop/resume`, survives coordinator loss, streams logs, and proves whole-process-group cleanup. There is no model-quality wall cap. Retry is allowed only before provider admission is possible or after an explicit correlated provider-terminal response (for example a terminal 408/429/5xx/529 with no live generation), respecting `Retry-After`. Connection reset or premature stream loss after admission is ambiguous: the lane seals `INCOMPLETE` unless the provider proves that exact request terminal; it never starts a replacement merely because a retry budget remains. Model mistakes and poor applications are benchmark outcomes, not reasons to repair or rerun. Contended wall time is labelled and is never presented as intrinsic model latency.

The five raw trees are sealed before scoring. Once builds are finished and a quiet-window lock is held, disposable clones are scored one at a time with the unchanged CLI scorer, exact original seed, and exact advertised port. The scorer is mutating, so originals remain untouched. Any provider-adapter code fix changes the shared binary and invalidates all five publishable entries. Affected-lane-only invalidation is limited to external entitlement/configuration/version drift that leaves every instrument hash unchanged. Common binary/prompt/vendor/runner defects invalidate all five; scoring-only failures rerun scoring; no attempt is overwritten.

There is a publication-integrity defect to correct before launch. Current `score_sb7.py` has `CALIB_SHA256="TBD-AT-FREEZE"`, `CALIBRATED=false`, and emits `sb-7.0-rc` with an explicit uncalibrated warning. The website seed accepts that verdict and hardcodes `sb-7.0`, concealing its status. SB7/scorer bytes stay frozen as instructed; the generic publisher instead preserves the verdict's exact version/calibration state and visibly labels the current era provisional. Existing SB7 rows receive the same truthful metadata treatment so the board has no unlabeled mixed convention.

After each serial valid score, the publisher dry-runs, performs idempotent `createOrReplace`, calls the revalidation endpoint, and verifies the rendered leaderboard and run page for exact score, RC label, model/version, wall time, notes, checks, screenshots, and methodology. It does not report “published” until rendered desktop and mobile truth pass. Website code changes generalize “Anthropic baseline” to “cloud baseline” and use data-driven entrant manifests rather than five more hardcoded branches.

## Post-approval execution plan and falsification gates

Engine implementation, cloud baselines, scoring, and publication are separate state machines. They share hashes and evidence, never mutable worktrees or live binaries. Every behavioral mechanism lands in its own commit and test arm; dependency scaffolding can precede an arm, but no bundled phase receives a causal claim.

### Foundation 0 — quarantine, branches, and immutable comparators

1. Tag/preserve `388792522` and its fixture as incident evidence. Create the engine implementation branch from a clean line with that behavioral commit reverted, then reconstruct accepted logging separately.
2. Create a cloud-benchmark worktree from the pre-F924 source base. Provider/harness conformance work happens there, then the reviewed Engine 1 terminal-admission changes are applied without F924 behavior. No paid-smoke/campaign binary is built or frozen until both gates pass.
3. Define the transitive SB7 instrument manifest inputs: spec, thresholds, scorer, vendor, fixtures, schedule, probe/shared helpers, browser/runtime versions, runner, provider adapter, prompt/tool schema, seed, and port. The final hashes are frozen only after cloud conformance and before paid smoke.
4. Freeze the current local request corpus before prompt/compiler changes so tail and later sampler hypotheses retain a real comparator.

Gate: git status, source-base commit, branch ownership, and manifest input list are machine-verifiable. Later cloud preflight must add the binary/instrument hashes. No benchmark uses the quarantined F924 behavior.

### Engine 1 — configuration truth, lifecycle truth, and terminal-safe cancellation

1. Generate the complete control registry and effective `levers_resolved` event; delete manual campaign catalogs and stale aliases only after parity tests pass.
2. Add request ID, physical host/model instance, role, immutable snapshot/profile hashes, queued/prefill/decode/cancel-requested/local-drop/provider-terminal states, first item, finish reason, classified error, and orphan detection.
3. Carry cooperative cancellation through scheduler, agent, formatter/provider stream, and LM Studio. Capacity and claims remain occupied until correlated provider-terminal evidence exists.
4. Add owned-PGID termination with descendant and port verification to all runners.
5. Replay r1/r2, F122→F123→F162→F163, the Fable correction, parser false positive, deterministic false acceptance, repair promotion, and F924 fixture. Replay validates facts and state transitions; it is not used to claim a counterfactual speed win.

Gate: effective config equals executed behavior; abandoned/failed calls remain visible; a replacement or retry cannot start before the old request is terminal; no process or decoder slot survives an owned stop. No judge nudge or retry behavior is enabled before this gate.

### Engine 2 — isolate the planning-tail causes, then compile specific tasks

1. On matched frozen requests, independently test tool-menu removal, schema-only output, sliced versus full goal/research, task grain, and role prompt while holding the exact captured request profile fixed. Sampler/template arms wait for Engine 8 Gate 0.
2. Add role profiles one at a time with explicit tools, request settings, and schemas.
3. Introduce the small-core/task-kind typed compiler and read-only repository surface. Remove fleet-width skeleton scoring, stale model names, generic one-line fallback, `WEB_VOCAB`, and benchmark-specific probe semantics.
4. Add requirement/interface registries with raw-spec citations and semantic sufficiency review; deterministic validation remains structural.

Gate: detail work cannot mutate the tree; task existence and dependencies are invariant under one-, two-, and three-node roster simulations; varied non-SB7 tasks retain exact domain literals and acceptance closures. Tail improvement is credited only to the matched arm that produced it.

### Engine 3 — evidence-led research and canonical patch planning

1. Build the unresolved-question/evidence ledger from blind independent inventories and alternate-seam critiques.
2. Introduce a canonical graph only after that blind pass; subsequent models submit typed coverage, dependency/interface, and execution/verification patches.
3. Add provisional/frozen/superseded contract states, invalidation propagation, and progressive release of safe branches.

Gate: offline replay proves parser/state behavior only. Matched shadow simulations and live non-SB7/SB7 arms must prove earlier safe release, full raw-clause trace coverage, non-flat dependencies where required, and no rise in stale-contract or ownership conflicts.

### Engine 4 — physical broker and semantic observation, with no intervention yet

1. Unify implementation and auxiliary work under the physical-host broker with verified-terminal admission, learned same-host marginal concurrency, and preemptible queued work.
2. Replace judge keyword parsing with strict `CONTINUE|NUDGE|SPLIT_PROPOSAL|ROUTE_FINDING|ACCEPT_CANDIDATE|REQUEST_EVIDENCE|ABSTAIN|INCOMPLETE`; malformed/unknown output abstains and continues.
3. Run immutable-snapshot, asynchronous, deduplicated judge observation without delivering nudges, killing work, accepting tasks, or changing scheduling.
4. Measure an evidence/value policy that lets a current high-value review claim a future slot without reserving a host or starving critical implementation.

Gate: a corpus covers true long recurrence, slow healthy reasoning, tool-payload silence, advancing-but-wrong work, genuinely complete work, uncertain/missing evidence, historical Fable correction, and F163 false-positive negatives. Precision, actionability, unique evidence, and downstream-relevant coverage are measured; raw finding count and repeated OKs earn nothing.

### Engine 5 — safe nudge, semantic split, and neutral recurrence

1. Deliver ordinary guidance at a natural tool/turn boundary. A high-confidence interrupt uses cooperative cancel, waits for provider terminal, commits a valid partial session state, and resumes that same session; it never overlaps requests.
2. Rebuild recurrence as a correctly named, bounded, edge-triggered, debounced signal with real positive/negative distributions and one outstanding review per state. It never votes, kills, accepts, or corroborates.
3. Admit split proposals only with task-specific contracts, dependency edges, acceptance closure, and non-overlap proof; resource availability never invents architecture.
4. Route late findings through current-snapshot revalidation and exact ownership.

Gate: intervention fixtures prove request non-overlap, retained session context, corrective next action, artifact/evidence delta, semantic coverage, and objective oracle results. Slow healthy and uncertain cases continue. A useful nudge must beat observation-only on matched cases before production enablement.

### Engine 6 — artifact truth and causal repair

1. Add requirement/artifact receipts and join-time contract checks; no missing mandatory surface can be marked complete.
2. Consolidate the four competing repair systems into a causal defect ledger: isolated shards for independent causes, whole-tree alternatives only for coupled ambiguity, one hermetic ruler, and one graded promotion.
3. Replace finding-count “monotonicity” with severity, invariant preservation, exact finding identity, and raw-requirement closure.
4. Continue only while a new falsifiable hypothesis, evidence item, or verified closure appears. Hypothesis exhaustion yields `INCOMPLETE`; it is not a correctness cap.

Gate: r1 replay prioritizes missing critical surfaces and rejects a one-critical-for-two-minor regression. Repeated non-promoting work without new evidence is impossible, and the causal root rather than every symptom owns repair.

### Engine 7 — selective upstream behavior ports

Each of cancellation-select, terminal-safe first-item retry, structured local overflow, pre-permission unambiguous tool-name canonicalization, thinking-delta coalescing, and absent-choice SSE handling is evaluated from its own captured fixture, commit, and arm. No broad merge and no combined “upstream” verdict.

Gate: the exact fork gap is reproduced before the port; targeted positive/negative tests and existing compaction/tool/permission behavior pass; the mechanism event proves the arm exercised.

### Engine 8 — Qwen/LM Studio role-profile campaign

1. Correlate exact HTTP bodies, rendered model input, model sampling seed, SB7 fixture seed, runtime/quant/context/cache/parallel/MTP state, and role prompt.
2. Run the exact observed current profile first, then the paired sampler, penalty, min-p, effort, preservation, MTP, quantization, and concurrency arms. Pre-change corpus results are revalidated on new role prompts.
3. Treat judge-interrupted samples as censored. Promote no profile from one host, seed, anecdote, or faster-but-worse result.

Gate: a preregistered matched design shows quality non-inferiority and improved total physical work/tail on both development and untouched holdout tasks.

### Cloud lane — provider conformance through verified publication

Provider/harness/site work starts in parallel with Engine 1 after approval. Paid smokes and full episodes depend on Engine 1's correlated provider-terminal admission gate:

1. Store secrets securely, implement Z.AI/Google/DeepSeek conformance fixtures and exact model metadata, generalize the build-only harness and SB7 publisher, and correct the site's RC/cloud-baseline metadata.
2. Pass local adapter/harness tests and website dry run; apply and verify the terminal-admission lifecycle patch; force all unproven retry/replacement/timeout redispatch paths off. Only then build once, copy the release binary to the immutable campaign path, and freeze its SHA-256 plus the full instrument manifest.
3. Against that exact binary, pass golden reference, severity self-test, hermeticity, authenticated model-entitlement discovery, terminal-safe two-step paid tool smoke, coordinator crash/resume, PGID cleanup, and instrument-drift checks. A deliberately dropped admitted stream must not start a replacement without proof that the exact provider request ended.
4. Start all five supervisors together for GLM-5.3, Gemini 3.7 Flash, Gemini 3.1 Pro Preview, DeepSeek V4 Flash, and DeepSeek V4 Pro. The resource-conflict graph admits all five concurrently only where key quota and local-resource isolation are proven; otherwise same-key or heavy local sections wait while independent provider calls continue. Engine work cannot change their binaries or state.
5. When all build lanes are terminal, acquire a quiet scoring lock, score disposable clones serially, and directly publish/revalidate/verify each valid RC result. Model-generated failure remains the score; provider/runner defects follow the invalidation policy in Slice 10.

Approval of this plan authorizes the initial five paid episodes, their minimal capability smokes, one verified-defect restart per model, up to three transient retries per failed request, the USD 400/provider sub-envelopes above, the described provider/site changes, and direct idempotent RC publication. It does not authorize outcome-driven reruns or silent model/endpoint substitution.

### Engine 9 — unattended one-lever campaign and autonomous defect loop

Before arms, preregister the comparator commit/config/provider/model, exact prompts and seeds, host order, repetitions, censored/aborted-work accounting, critical-check vector, total-score rule, physical-work/tail metrics, untouched holdouts, and a versioned defect-predicate allowlist. “Requirements closed” is reconciled against frozen raw-spec clauses and scorer checks, with false/missed inventory entries reported separately.

The initial unattended defect allowlist is: engine panic/crash with matching process exit; a local future ending while its correlated provider request remains active **and** the broker releases occupancy/claims, loses correlation, admits a replacement, or violates a predeclared provider-terminal transport invariant; a replacement admitted before prior provider terminal; config echo differing from the value actually executed; artifact write outside the frozen ownership contract; owned PGID/descendant/port surviving a completed stop; frozen binary/spec/scorer/seed/port hash drift; reproducible protocol/parser violation against a captured valid frame; or scheduler state violating a registered DAG/ownership invariant. A provider still unwinding while the broker safely retains its claim is an expected transitional state, not a defect. Model recurrence, long reasoning, low score, missing application feature, provider throttling, and ordinary tool mistakes are explicitly not engine defects. A newly discovered predicate can be tested and versioned for the next instrument, but cannot be invented post hoc to stop the current run.

The persistent monitor uses `OBSERVE → SUSPECT → VERIFIED`:

- `SUSPECT` makes no mutation. Duration, a quiet log, a low score, provider throttling, or strange model output are not engine defects.
- `VERIFIED` requires correlated request/engine/process/port evidence proving one frozen allowlisted predicate on the same object, or its predeclared reproducible fixture with positive and negative controls.
- Before a stop, write an incident bundle. Stop only the exact owned PGID, verify descendants/ports/decoder terminal state, seal the run `INVALID`, and create a new run ID/tree. Never broad-kill or reuse a dirty tree.
- One causal fix gets targeted regression tests, controls, a commit and new instrument hash. Restart the supervisor itself before the clean episode so it cannot retain old code.
- Repeated identical defects, crash loops, instrument drift, or exhausted declared cloud-retry/spend authority fail closed with a human-readable blocker. The monitor never edits SB7/scorer, LM Studio global state, credentials, or site code; publication is a separate validated state machine.
- Every unit ends as valid result, invalid incident, transient retry, or explicit failure. Silence is never success.

Exploratory arms use matched repetitions; candidates are confirmed against the frozen comparator on untouched tasks/seeds to avoid winner's curse. The unchanged SB7 CLI scorer remains serial and hermetic.

Final acceptance is preregistered before candidate results are visible and requires all of:

- no loss in the comparator's passed critical-check vector and no lower median hermetic score on confirmation runs;
- at least 20% lower median end-to-end time and total physical decoder-minutes on matched confirmation cases, counting abandoned/cancelled provider work;
- at least 25% lower 90th-percentile planning/detail/repair physical-request tail, with censored samples reported rather than dropped;
- at least 20% less verified avoidable physical-node idle time, without a negative same-host marginal-throughput regression;
- no increase in raw-spec missed/false requirement traces, stale-contract promotions, ownership collisions, or generic task-spec failures across holdouts;
- real same-session semantic corrections whose downstream artifact/oracle result beats observation-only controls.

If no design meets every quality and speed gate, no weaker arm is promoted merely because it is faster.

## Confidence and unresolved empirical risks

High confidence:

- task-detail role/tool mismatch causes unauthorized planning side effects and role contamination; its contribution to long reasoning remains unresolved;
- fleet-shaped structural scoring and weak schema produced an unusable flat graph;
- global detail/contract barriers create avoidable fleet idleness;
- current judge parsing and deterministic acceptance have made provably wrong decisions;
- semantic supervision can correct real task drift;
- current repair spends node-hours without proportional requirement closure;
- request telemetry omits abandoned work.
- the F924 narrative overstates duration/idleness and the committed recurrence/nudge control is unsafe;
- the lever catalog and `levers_resolved` event are incomplete and sometimes disagree with effective runtime behavior;
- the current Z.AI, Google, and DeepSeek adapter paths cannot produce fair runs for the five requested model IDs without conformance fixes;
- the website currently hides the scorer's `sb-7.0-rc` calibration state.

Medium confidence:

- canonical patch planning, progressive contract release, and the auxiliary supervision queue will improve useful occupancy without new integration conflicts;
- cooperative cancellation will promptly release LM Studio slots;
- terminal-safe same-session nudges will preserve more useful work than kill/re-dispatch;
- individual selected upstream behavior ports will improve local-model resilience without interfering with fork-specific compaction.

Lower confidence until experiments run:

- which Qwen sampler wins by phase;
- whether low or medium reasoning is optimal for each role;
- whether repeat penalty 1.1 transfers from the published arm;
- whether MTP helps this Apple fleet and workload;
- whether a second same-host stream ever has positive marginal value at the active contexts and quants.
- which planning input/tool/schema factor causes the long-tail distribution;
- whether the Z.AI key exposes the general or Coding Plan endpoint for GLM-5.3 and what response version it reports;
- how much model-version drift affects the Gemini 3.1 Pro Preview baseline during the campaign.

Low confidence is handled by controlled measurement, not by asking the operator to choose a folklore preset.
