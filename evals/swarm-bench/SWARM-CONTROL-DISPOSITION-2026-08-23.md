# Swarm control disposition register — 2026-08-23

## Decision

This is the no-run control audit for integration commit
`c697e8ecd7a8a197cc826a8e2db6683805337add`. It does not authorize a benchmark. The external campaign is
stopped, its `ACTIVE` and PID files are stale, and it has not adopted a schema-2 plan/lock/receipt. Therefore
the current campaign state of every behavior control below is **UNRUN**. Nothing in the legacy queue can be
resumed as a causal arm.

Read-only state check: `~/goose-builds/loop-state/STOP` exists; neither PID recorded by `daemon.pid` nor
`ACTIVE` is live; `ACTIVE` is stale residue `allon-1 9897`; the 28-row `QUEUE` contains `allon-2/3` plus 26
historical ablations; `LEDGER.tsv` ends at the fleet-swap marker with no post-swap baseline. The schema-2
migration exists only as source/a migration staging directory: there is no adopted `CAMPAIGN.inventory.json`
or `QUEUE.schema2.jsonl`. This makes **UNRUN** both the per-control campaign state and the only honest result.

The engine has **166 canonical controls**: 116 persisted `SwarmConfig` fields and 50 environment-only
controls. Nine historical spellings are aliases, not additional controls. The source catalogue classifies
30/8/32/12/34 config controls as retain-enabled/retain-disabled/modify/remove-or-merge/runtime-profile and
14/4/9/8/15 environment controls in the same categories. This audit keeps the exhaustive source set but
overrides catalogue dispositions where scheduler use sites or retained runs prove the earlier label unsafe.

This corrects a current documentation drift. Commit `9966c54e6` added `physical_broker` after the 165-row
ledgers were written without updating their 49/165 and 26/95 environment/behavior totals. Current source is
50 environment-only controls: 27 behavior, 15 runtime-profile, and eight removal controls; overall it is 96
behavior, 49 runtime-profile, 20 removal, and one telemetry control.

No semantic timeout, character count, token count, round count, fleet size, file count, or flat-progress
predicate may decide that work is correct, failed, or replaceable. Operational I/O timeouts may remain only
when they produce an explicit infrastructure/inconclusive state and do not release physical capacity before
a provider-terminal receipt.

## Evidence and dependency keys

| Key | Meaning |
|---|---|
| `REG` | Exhaustive source registry and resolution tests in `crates/goose-cli/src/commands/swarm_control_registry.rs`; registration proves existence, not value. |
| `RUN` | Corrected r1/r2 evidence in `SWARM-ENGINE-AUDIT-2026-08-22.md`: r1 573.5m (129.9m planning, 298.0m gates/repair); r2 27 details, 26 complete, one 203,447-reasoning-character tail; no proven physical-idle interval. |
| `FAN` | Typed requirement/task compiler and F924 replay in `ENGINE-TASK-FAN-LEDGER.md`; replay proves state transitions, not throughput or quality. |
| `PHY` | Physical admission invariants in `ENGINE-4-PHYSICAL-BROKER-LEDGER.md`; terminal receipt, verified host/model instance, common queue. |
| `OBS` | Observation-only typed semantic plane in `ENGINE-SEMANTIC-OBSERVATION-LEDGER.md`; implemented and tested but not wired to a production reviewer or live summons. |
| `REP` | Repair evidence in `REPAIR-CAUSAL-AUDIT-2026-08-23.md` and `REPAIR-BOUNDARY-LEDGER.md`; post-gate mutation sealing is real, causal candidate selection is not. |
| `CAM` | `CAMPAIGN-CONTROL-HANDSHAKE-LEDGER.md`; old 87-name catalogue has two nonexistent names, omits 31 persisted fields, and 23/26 queued ablations are default-on no-ops. |
| `UP` | `UPSTREAM-GOOSE-AUDIT-2026-08-23.md`; typed provider-terminal failure/context/output-limit propagation is a prerequisite for trustworthy retry and admission. |

Disposition vocabulary: **ON** means preserve enabled as a correctness invariant; **OFF** means preserve
disabled pending evidence; **MODIFY** means do not causally test the current mechanism; **REMOVE/MERGE** means
retire the independent control; **PROFILE** means pin and record it in every arm but never call it the causal
delta. `null→X` records a nullable source default whose current resolver supplies `X`.

## Persisted controls — correctness, behavior, and removal

| Control | Source default | Mechanism / evidence / dependency | Audited disposition |
|---|---:|---|---|
| `stream_decode_retry` | `true` | Provider stream recovery; must distinguish pre-dispatch, partial stream, and provider-terminal failure (`UP`,`PHY`). | **ON**, after typed terminal errors; never retry an unconfirmed live request. |
| `planner_also_works` | `true` | Lets planner endpoint execute ready build work (`RUN`,`PHY`). | **ON** through common broker; role eligibility, not a reserved lane. |
| `sink_lean_prefill` | `true` | Reduces integration prompt duplication (`RUN`). | **ON**; verify requirement/evidence completeness. |
| `e2e_oracle` | `true` | Runs end-to-end acceptance evidence (`REP`). | **ON**, bake as invariant. |
| `spec_sized_plan` | `null→true` | Sizes decomposition to requirements rather than fleet (`FAN`). | **ON**, bake into compiler; node count must not create tasks. |
| `delegated_decisions_ok` | `true` | Allows explicit low-risk delegated decisions. | **ON** only under normalized spec authority and auditable decision records. |
| `clarify_spec_bound` | `true` | Bounds clarification to unresolved spec facts. | **ON**; remove time-based correctness coupling. |
| `spec_wins` | `true` | Makes user specification authoritative. | **ON**, bake as invariant. |
| `clarity_fail_closed` | `true` | Refuses planning on unresolved critical ambiguity. | **ON**; status must be blocked/inconclusive, never timeout-green. |
| `spec_contract` | `true` | Materializes normalized specification contract (`FAN`). | **ON**, bake as compiler input. |
| `retarget_stall_guard` | `true` | Stops repeated plan retargeting after flat scalar progress. | **MODIFY**: use unresolved requirement/evidence change, not stall counts. |
| `answers_win_floor` | `true` | Preserves explicit user answers over inferred defaults. | **ON**, bake as authority rule. |
| `cross_module_check` | `true` | Checks cross-module compatibility. | **ON** as acceptance evidence; version-bind findings. |
| `smoke` | `true` | Runs smoke gates. | **ON**; capability-derived commands, explicit inconclusive status. |
| `verify_commands` | `true` | Executes declared verification commands. | **ON**, bake as invariant. |
| `fan_e2e` | `true` | Fans independent acceptance evidence. | **MODIFY**, then ON: fan only requirement-independent scopes through broker. |
| `no_tools_means_ask` | `true` | Prevents unsupported claims when required tools are absent. | **ON**, bake as capability invariant. |
| `author_pitfalls` | `true` | Injects `DOMAIN_PITFALLS`, an 18-domain hard-coded rule library. Comment/default drift also exists. | **MODIFY**: replace with retrieved, cited, versioned evidence; do not test current prompt leakage. |
| `grounded_research_only` | `true` | Rejects ungrounded research claims. | **ON**; provenance must survive planning and repair. |
| `ts_smoke_tests` | `true` | Adds TypeScript smoke evidence. | **MODIFY/MERGE** into language-capability gate, not a global product heuristic. |
| `failed_tasks_block_green` | `true` | Prevents failed tasks from yielding green. | **ON**, bake as invariant. |
| `sink_prebuild` | `true` | Prepares integration context before final sink. | **ON** if snapshot-bound and non-mutating. |
| `user_notes` | `true` | Carries user decisions into prompts. | **ON**, normalized and provenance-bound. |
| `contract_validate` | `true` | Validates contracts before execution. | **ON**, bake as invariant. |
| `kind_prompt` | `true` | Specializes prompt by task kind. | **MODIFY** into typed task schemas; avoid hard-coded domain vocabulary. |
| `occupancy` | `false` | Emits logical occupancy telemetry; cannot prove physical idleness (`RUN`,`PHY`). | **ON** as telemetry, renamed/qualified until broker provides physical truth. |
| `doc_prefetch` | `true` | Fetches declared documentation ahead of tasks. | **ON** when URL/provenance is spec-derived and snapshot-bound. |
| `dep_signatures` | `true` | Carries dependency interface signatures. | **ON**, derive from typed contracts, not byte excerpts. |
| `act_now_nudge` | `true` | Prompt nudge against passive narration. | **MODIFY/MERGE** into task protocol; measure before preserving a global phrase. |
| `require_tests` | `true` | Requires test evidence for testable requirements. | **ON**, bake as requirement-aware invariant. |
| `straggler_stop_degrade` | `null→false` | Converts killed straggler into degraded output. | **OFF**; killing without semantic replacement and provider terminal is forbidden. |
| `goals` | `null→false` | Assured-mode goal bundle. | **REMOVE/MERGE** into normalized requirements; keep OFF meanwhile. |
| `ask_replan` | `null→false` | Replans after an operator answer. | **MODIFY** into incremental contract patch; keep OFF meanwhile. |
| `contract_retry` | `false` | Retries failed contract generation. | **OFF** until typed defect and terminal-safe retry exist. |
| `incremental_replan` | `false` | Adds plan mutations during execution. | **MODIFY** into versioned DAG patch; keep OFF meanwhile. |
| `ask_away` | `false` | Repeated clarification rounds. | **OFF** until unresolved-question ledger, dedupe, and human-blocking state exist. |
| `write_first` | `false` | Forces mutation before adequate inspection. | **REMOVE**; conflicts with evidence-first work. |
| `think_off_test_authors` | `null→false` | Disables reasoning for test authors. | **OFF**; model/runtime profile experiment only after task quality baseline. |
| `max_attempts` | `3` | Retry ceiling. | **MODIFY**: hypothesis/typed-failure state, no semantic retry count; operational circuit breaker yields inconclusive. |
| `max_research_questions` | `4` | Caps fixed research lenses. | **MODIFY**: unresolved-question/evidence queue, no fixed question count. |
| `dynamic_replan` | `true` | Synchronously asks planner for bonus work when ≥2 logical slots are free. Can stall dispatch and invent capacity-shaped tasks. | **MODIFY** into asynchronous contract-bound DAG patch; incompatible with `physical_broker` now. |
| `max_replans` | `2` | Caps dynamic-replan rounds. | **REMOVE/MERGE** into evidence-state convergence after replanner redesign. |
| `research_scouts` | `true` | Fans research calls. | **MODIFY**, then ON: dynamic evidence questions, broker admission, provenance. |
| `parallel_planning` | `true` | Produces redundant whole-plan work. | **MODIFY** into one canonical plan plus independent semantic patches. |
| `best_of_n_skeletons` | `1` | Drafts N whole skeletons and scalar-selects. | **REMOVE/MERGE** with canonical-plan patches; N is not evidence. |
| `progress_watchdog_secs` | `900` | Time-based watchdog can classify slow work. | **MODIFY** to observation summons/infra status only; never semantic kill. |
| `omni_judge` | `true` | Enables legacy idle judge over all tasks (`OBS`). | **MODIFY**: replace with brokered typed observation; current legacy path may oversubscribe and abort. |
| `converge` | `true` | Reconciles multiple whole plans. | **MODIFY/MERGE** into evidence-backed patch adjudication. |
| `diverse_plan` | `false` | Requests diverse whole plans. | **REMOVE/MERGE**; diversity should be semantic role evidence, not duplicate plans. |
| `retarget` | `true` | Rewrites plans after checks. | **MODIFY** into versioned, requirement-addressed patch with semantic review. |
| `supervision_pool` | `false` | Reserves nodes excluded by `MAX_NODES` for legacy supervision. | **REMOVE**; all roles must share one physical broker and priority queue. |
| `judge_nudge` | `false` | Legacy same-session nudge, capped twice, then abort. | **OFF/MODIFY** until typed observation, cooperative delivery, and terminal-safe state machine. |
| `fix_sched` | `false` | Routes a subset of repair through legacy scheduler. | **MODIFY**, then ON as the only brokered repair route. |
| `ask_max_q` | `3` | Caps questions per ask. | **MODIFY**: prioritize unresolved critical facts; no semantic count cap. |
| `split` | `true` (requested but hard-disabled) | Splits plan modules. Current judge passes literal `false` for child contract completeness, so execution cannot split. | **MODIFY**, then ON through requirement compiler; semantic boundaries, not fleet/file count. |
| `contracts` | `true` | Generates task contracts. | **MODIFY**, then bake ON: typed compiler currently improves structure but live quality is unmeasured (`FAN`). |
| `complete` | `true` | Runs iterative COMPLETE repair. | **MODIFY**: causal defect ledger, shadow candidates, judge adjudication, atomic promotion (`REP`). |
| `backbone` | `true` | Builds/revises a whole-plan backbone. | **MODIFY/MERGE** into canonical plan; avoid another full-plan generation. |
| `review` | `false` | Adds planning review. | **MODIFY** into independent semantic patch evidence; old campaign's only actual behavior delta (`CAM`). |
| `unwired_demotes_verified` | `true` | Deterministically demotes verified tasks from wiring heuristics. | **OFF/MODIFY**; require semantic observation and stable defect evidence. |
| `persona` | `true` | Adds persona prompts. | **REMOVE/MERGE** unless a matched arm proves requirement-quality gain independent of verbosity. |
| `relax_contracted_deps` | `false` | Loosens dependency barriers after contracts. | **OFF/MODIFY**; only proven interface compatibility may release a dependency. |
| `split_fat` | `true` | Pre-splits file-heavy tasks using file/role thresholds. | **MODIFY/MERGE** into semantic requirement slices; file count is not task grain. |
| `doc_fetch` | `false` | Fetches external docs during planning. | **MODIFY**, then merge with research evidence queue; provenance and capability gating required. |
| `fan_verify` | `true` | Fans verification work. | **MODIFY**, then ON for independent acceptance scopes through broker. |
| `parallel_tests` | `true` | Fans test work. | **MODIFY**, then ON only for isolated acceptance scopes and tree epochs. |
| `repeat_break` | `true` | Detects repeated tool/output windows and intervenes. | **MODIFY**: neutral judge summons with stream-wide evidence; no deterministic semantic kill. |
| `straggler_stop` | `true` | Stops redundant draft stragglers after a fixed grace. | **MODIFY/OFF**: no fixed grace for detail/build work; any draft cancellation needs terminal proof. |
| `backbone_skip_confident` | `true` | Skips backbone revision on a scalar confidence condition. | **MODIFY/MERGE** into unresolved-conflict criterion. |
| `degrade_on_stall` | `true` | Marks stalled owns-nothing work provisionally done. | **OFF/MODIFY**: add explicit `Provisional/Salvaged` state; never report verified green. |
| `sink_review` | `null→false` | Whole-tree idle review during sink. | **REMOVE/MERGE** with one version-current semantic-review queue. |
| `detail_memo` | `true` | Legacy detail memoization. | **REMOVE**; retired typed compiler path makes it redundant. |
| `spiral_break_chars` | `12000` | Character-volume intervention threshold. | **REMOVE** as semantic decision; retain recurrence metrics only as observation evidence. |
| `homogeneous_models` | `false` | Treats roster as homogeneous. | **REMOVE**; capabilities and physical instances are explicit. |
| `speed_weights` | `{}` | Manual logical speed weighting. | **REMOVE/MERGE** into measured physical broker service data; not a causal quality lever. |
| `delivery` | `false` | Legacy delivery-mode bundle. | **REMOVE**; explicit gate invariants supersede it. |
| `owned_file_fence` | `false` | Heuristic file ownership fence. | **REMOVE/MERGE** into shadow transactions and tree epochs. |
| `spiral_thinking_chars` | `0` | Character threshold for thinking spiral. | **REMOVE** as semantic decision; stream fingerprint is observation input only. |
| `read_on_fix` | `false` | Repair prompt mode requiring reads. | **REMOVE/MERGE** into evidence-bearing repair contract. |
| `force_write_tool` | `null→false` | Forces a write tool call. | **REMOVE**; tool occurrence is not correctness. |
| `scoped_contracts` | `null→false` | Alternate scoped contract mode. | **REMOVE/MERGE** into the single typed contract compiler. |
| `split_secs` | `300` (currently inert) | Split-phase wall; child splitting is hard-disabled. | **REMOVE**; no semantic duration cap. |

## Persisted controls — runtime profile, never causal arms

| Control | Source default | Intended profile role / dependency | Disposition |
|---|---:|---|---|
| `endpoint` | `http://localhost:1234` | Provider route; pins topology and transport (`PHY`). | **PROFILE**; lock exact bytes and probe identity before arm. |
| `planner_model` | `qwen/qwen3.6-27b` | Planner identity; stale example, not current-model truth. | **PROFILE**; explicit model/version/quantization. |
| `devices` | two baked qwen3.6 examples | Logical roster; not physical capacity. | **PROFILE**; explicit host/provider/instance identity and snapshot. |
| `worker_max_turns` | `40` | Agent-loop operational budget. | **PROFILE/MODIFY**; no semantic cutoff; infra circuit yields inconclusive. |
| `straggler_grace_secs` | `null` | Grace before legacy draft stop. | **PROFILE**, keep null; retire with straggler mechanism. |
| `worker_extensions` | `[]` | Tool capability set. | **PROFILE**; exact extensions and versions are benchmark inputs. |
| `planner_weight` | `1` | Logical planner concurrency. | **PROFILE**; broker must enforce physical capacity. |
| `context_cap` | `null` | Context window profile. | **PROFILE**; pin rendered/effective context, structured overflow. |
| `research_planning` | `on` | Enables research phase. | **PROFILE** initially ON; future invariant when unresolved external facts exist. |
| `worker_timeout_secs` | `900` | Worker wall timeout. | **PROFILE/MODIFY**: hang circuit only, terminal-safe, inconclusive. |
| `planner_timeout_secs` | `900` | Planner wall timeout. | **PROFILE/MODIFY** under same rule. |
| `allow_model_load` | `false` | Allows runtime model loading. | **PROFILE**, false for hermetic benchmark. |
| `temperature` | `null` | Sampling parameter. | **PROFILE**; record rendered provider value, never mix with engine arm. |
| `top_p` | `null` | Sampling parameter. | **PROFILE**; same. |
| `top_k` | `null` | Sampling parameter. | **PROFILE**; same. |
| `min_p` | `null` | Sampling parameter. | **PROFILE**; same. |
| `repeat_penalty` | `null` | Sampling parameter. | **PROFILE**; same. |
| `max_tool_response_chars` | `null` | Tool spill threshold. | **PROFILE**; truncation must preserve artifact/provenance pointer. |
| `scout_budget_secs` | `900` | Research scout wall. | **PROFILE/MODIFY**: remove semantic cutoff; infra circuit only. |
| `scout_max_lookups` | `10` | Research tool-call ceiling. | **PROFILE/MODIFY**: evidence-state completion, not count. |
| `sink_cap_secs` | `1800` | Sink wall, tree-size scaled. | **PROFILE/MODIFY**: remove semantic cutoff. |
| `sink_cap_ref_bytes` | `30000` | Scales sink wall by tree bytes. | **REMOVE/MERGE** with removed sink wall; bytes do not predict correctness. |
| `uncapped` | `false` | Expands limits to a one-week/100k-turn stand-in. | **REMOVE/MERGE**: represent absent semantic deadlines explicitly, not fake infinity. |
| `lm_extra_body` | `null` | Provider-specific request body. | **PROFILE**; canonicalize/hash exact body and rendered reasoning settings. |
| `ask_floor` | `80` | Confidence threshold for clarification. | **PROFILE/MODIFY**: unresolved critical fact, not scalar confidence. |
| `struct_stop` | `80` | Structural confidence stop. | **PROFILE/MODIFY**: evidence closure, not score threshold. |
| `clarity_probe_secs` | `null→180` | Clarity probe wall, clamped 30–900. | **PROFILE/MODIFY**: no semantic time decision. |
| `sink_max_turns` | `120` | Sink agent-turn ceiling. | **PROFILE/MODIFY**: evidence-state completion. |
| `draft_timeout_secs` | `null→480` | Plan draft wall, clamped 60–1800. | **PROFILE/MODIFY**: no semantic time decision. |
| `retarget_rounds` | `null→2` | Retarget count, cap 4. | **PROFILE/MODIFY**: versioned unresolved-evidence convergence. |
| `complete_cap_secs` | `3000` | COMPLETE phase wall. | **PROFILE/MODIFY**: remove semantic cutoff (`REP`). |
| `draft_temp` | `null` | Planning sampling override. | **PROFILE**; lock with provider sampling. |
| `ask_rounds_max` | `null→3` | Clarification-round ceiling, cap 6. | **PROFILE/MODIFY**: unresolved critical facts, explicit blocked state. |
| `research_tools` | `false` | Enables research tool surface. | **PROFILE**; capability/provenance input, normally ON when external truth is required. |

## Environment-only controls — behavior and removal

These controls are real production readers but only 11 currently have a run-level effective echo:
`judge`, `prereview`, `qa`, `salvage_require_critical`, `salvage_spin`, `ship_best`, `sink_shard`,
`spec_repair`, `split_inherit_spec`, `tail_review`, and `testgen`. The other 39 require phase-event evidence;
the schema-2 campaign correctly refuses to pretend their ambient environment value is execution proof.

| Control / environment | Current default | Mechanism / evidence / dependency | Audited disposition |
|---|---:|---|---|
| `boundary_probe` / `GOOSE_SWARM_BOUNDARY_PROBE` | ON | Probes declared external boundaries. | **ON**, but derive protocol/capability from spec contract and return inconclusive on unavailable dependency. |
| `cli_contract` / `GOOSE_SWARM_CLI_CONTRACT` | ON | Checks CLI shape; currently carries Python/argparse assumptions. | **MODIFY**, then ON through normalized interface contract/language adapter. |
| `compile_gate` / `GOOSE_SWARM_COMPILE_GATE` | ON | Runs compile/syntax gate. | **ON**, capability-derived and baked as invariant. |
| `css_coherence` / `GOOSE_SWARM_CSS_COHERENCE` | ON | Applies web-specific CSS checks. | **MODIFY/MERGE** into artifact-kind acceptance rules; never global. |
| `dom_id_scan` / `GOOSE_SWARM_DOM_ID_SCAN` | ON | Scans DOM IDs. | **MODIFY/MERGE** into web artifact contract; no vendor vocabulary. |
| `done_gate` / `GOOSE_SWARM_DONE_GATE` | OFF | Requires final done-state gate. | **MODIFY**, then ON as one typed final ruler; avoid duplicate gate stacks. |
| `overview` / `GOOSE_SWARM_OVERVIEW` | ON | Generates run overview/report. | **ON** as reporting; must distinguish verified, provisional, failed, blocked, inconclusive. |
| `qa` / `GOOSE_SWARM_QA` | ON | Services operator questions before background review. | **ON**, human-blocking priority through broker; effective echo exists. |
| `require_servable` / `GOOSE_SWARM_REQUIRE_SERVABLE` | ON | Requires advertised service to boot/respond. | **ON** when spec advertises a service; capability-derived. |
| `resume` / `GOOSE_SWARM_RESUME` | OFF | Resumes an interrupted run. | **OFF/PROFILE** until exact tree epoch, binary, profile, provider terminal, and event continuity are sealed. |
| `salvage_require_critical` / `GOOSE_SWARM_SALVAGE_REQUIRE_CRITICAL` | OFF | Requires critical acceptance before salvage. | **MODIFY**, then ON; explicit `Salvaged/Provisional` status plus critical evidence (`REP`). |
| `scout_doc_urls` / `GOOSE_SWARM_SCOUT_DOC_URLS` | ON | Fetches URLs found in spec for research. | **ON/MODIFY**: preserve provenance, remove fixed three-URL/24k-byte truth ceilings. |
| `skeleton_first` / `GOOSE_SWARM_SKELETON_FIRST` | ON | Builds a plan skeleton before details. | **MODIFY/MERGE** into canonical requirement graph; avoid another whole-plan pass. |
| `split_inherit_spec` / `GOOSE_SWARM_SPLIT_INHERIT_SPEC` | ON (currently inert) | Propagates spec context into split tasks, but contract-bound child splitting is hard-disabled. | **MODIFY**, then ON with typed requirement slices rather than copied prompt text. |
| `doc_examples` / `GOOSE_SWARM_DOC_EXAMPLES` | OFF | Adds documentation examples to prompts. | **OFF** until cited, version-matched retrieval proves value without prompt bloat. |
| `physical_broker` / `GOOSE_SWARM_PHYSICAL_BROKER` | OFF | Enforces verified physical admission (`PHY`). Current entry rejects default-on judge, prereview and dynamic replan, so simply enabling it fails before execution. | **OFF/MODIFY**, then foundational ON after all work roles use its common queue. |
| `speculate` / `GOOSE_SWARM_SPECULATE` | OFF | Races duplicate writer and aborts loser. | **OFF** until shadow transaction, provider terminal, and semantic promotion exist; likely remove if supervision is superior. |
| `testgen` / `GOOSE_SWARM_TESTGEN` | OFF | Uses idle node to write tests, fixed fan cap 3. | **OFF/MODIFY**: contract-derived, snapshot-bound shadow tests through broker before testing. |
| `complete_rounds` / `GOOSE_SWARM_COMPLETE_ROUNDS` | `2` (clamp 1–6) | Repair-round ceiling (`REP`). | **REMOVE/MERGE** into unresolved stable-defect ledger. |
| `complete_stall_rounds` / `GOOSE_SWARM_COMPLETE_STALL_ROUNDS` | `2` | Scalar flat-round stop; current semantics effectively stop after one flat round. | **REMOVE** as correctness decision. |
| `judge` / `GOOSE_SWARM_JUDGE` | ON | Legacy idle judge; first look/interval/min-text/max-look constants, not physically admitted, may abort (`OBS`). | **MODIFY** to typed observation-only production reviewer; keep legacy path OFF under broker. |
| `prereview` / `GOOSE_SWARM_PREREVIEW` | ON | Reviews completed tasks on logical idle slots. | **MODIFY**: snapshot/version-bound evidence through broker; preserve priority behind human-blocking work. |
| `salvage_spin` / `GOOSE_SWARM_SALVAGE_SPIN` | ON | Additional salvage repair loop. | **MODIFY/MERGE** into one causal repair queue. |
| `ship_best` / `GOOSE_SWARM_SHIP_BEST` | ON | Selects/promotes the scalar “best” repair candidate. | **REMOVE/MODIFY**: semantic judge adjudicates stable defects; atomic composed preview (`REP`). |
| `sink_shard` / `GOOSE_SWARM_SINK_SHARD` | ON | Shards integration repair. | **MODIFY**: shadow candidates, stable defect ownership, common broker; no count winner. |
| `spec_repair` / `GOOSE_SWARM_SPEC_REPAIR` | ON | Repairs spec-contract defects; source comments still claim default OFF. | **MODIFY** into the single causal repair protocol; fix comment/default drift. |
| `tail_review` / `GOOSE_SWARM_TAIL_REVIEW` | ON | Fills all logically free nodes with fixed whole-tree dimensions; fixed fan cap 8 and 240s review wall. | **MODIFY**: version-current, requirement-derived observations through broker; no blind idle fill. |
| `ask_scale` / `GOOSE_SWARM_ASK_SCALE` | ON when ask floor exists | Alters ask behavior from scalar confidence. | **REMOVE/MERGE** into unresolved-critical-fact protocol. |
| `assured` / `GOOSE_SWARM_ASSURED` | OFF | Bundles many unrelated “assured” behaviors. | **REMOVE**; violates single-control causality and hides defaults. |
| `complete_parallel` / `GOOSE_SWARM_COMPLETE_PARALLEL` | OFF | Alternate direct parallel repair fan. | **REMOVE**; all repair goes through one brokered causal scheduler. |
| `fill_fan` / `GOOSE_SWARM_FILL_FAN` | OFF | Creates work to fill fleet. | **REMOVE**; capacity must never create semantics. |
| `prereview_dims` / `GOOSE_SWARM_PREREVIEW_DIMS` | ON | Enables fixed prereview dimension set. | **REMOVE/MERGE** into requirement/evidence-derived semantic roles. |
| `probe_advertised_post` / `GOOSE_SWARM_PROBE_ADVERTISED_POST` | OFF | Probes advertised POST endpoints. | **MODIFY/MERGE** into spec-derived protocol contract, not a global HTTP switch. |
| `split_fat_files` / `GOOSE_SWARM_SPLIT_FAT_FILES` | `3` | File-count threshold for task splitting. | **REMOVE**; semantic acceptance closure determines grain. |
| `web_vocab` / `GOOSE_SWARM_WEB_VOCAB` | ON | Injects fixed DOM/vendor tokens such as app-root/sync/payment identifiers (`FAN`). | **REMOVE**; benchmark/domain leakage and genericity violation. |

## Environment-only controls — runtime profile

| Control / environment | Current default | Intended profile role / dependency | Disposition |
|---|---:|---|---|
| `ai_name` / `GOOSE_SWARM_AI_NAME` | ON | Detached model-generated session title. Its fallback `swarm.ai_session_name` YAML key is outside `SwarmConfig` and the registry. | **PROFILE/OFF for benchmark**; remove the unregistered fallback or register one canonical input. |
| `ask_file` / `GOOSE_SWARM_ASK_FILE` | unset | Operator-answer transport. | **PROFILE**; isolated path, hashed initial state, no ambient mutation. |
| `ask_wait_secs` / `GOOSE_SWARM_ASK_WAIT_SECS` | `1800` | Human wait wall. | **PROFILE**; timeout yields blocked/inconclusive, never guessed answer. |
| `fix_cap_secs` / `GOOSE_SWARM_FIX_CAP_SECS` | `1200` (clamp 120–3600) | Repair-attempt wall; uncapped substitutes one week. | **PROFILE/MODIFY**: no semantic duration cap. |
| `inherit_hints` / `GOOSE_SWARM_INHERIT_HINTS` | OFF | Inherits ambient task hints. | **PROFILE/OFF** for hermetic runs unless exact hint bytes are sealed. |
| `max_nodes` / `GOOSE_SWARM_MAX_NODES` | unset | Limits logical worker count. | **PROFILE**; pin topology but do not infer physical capacity. |
| `name_timeout_secs` / `GOOSE_SWARM_NAME_TIMEOUT_SECS` | `600`; `0` waits forever | Detached AI-title timeout. | **PROFILE/OFF for benchmark** with `ai_name`; not an engine lever. |
| `pin_device` / `GOOSE_SWARM_PIN_DEVICE` | unset | Forces routing to logical device. | **PROFILE** for diagnostics only; incompatible with adaptive broker arm unless declared. |
| `render_node` / `GOOSE_SWARM_RENDER_NODE` | `node` | Render-probe executable. | **PROFILE**; pin toolchain identity. |
| `render_probe` / `GOOSE_SWARM_RENDER_PROBE` | unset/OFF | Optional rendering probe. | **PROFILE**; capability/evaluation input, not engine delta. |
| `retarget_draft_step` / `GOOSE_SWARM_RETARGET_DRAFT_STEP` | `1` (clamp 1–3) | Draft-count increment per retarget. | **REMOVE/MERGE** with canonical patch workflow. |
| `retarget_stall_tolerance` / `GOOSE_SWARM_RETARGET_STALL_TOLERANCE` | `1` (clamp 1–3) | Flat-progress tolerance. | **REMOVE** as semantic decision. |
| `run_deadline_unix_ms` / `GOOSE_SWARM_RUN_DEADLINE_UNIX_MS` | unset | Operator outer deadline. | **PROFILE**; may stop orchestration only as incomplete/inconclusive, never score semantic failure/green. |
| `tail_review_secs` / `GOOSE_SWARM_TAIL_REVIEW_SECS` | `240` (minimum 30) | Per-tail-review wall. | **REMOVE/MODIFY** with legacy tail-review path; no semantic time cap. |
| `telemetry_file` / `GOOSE_SWARM_TELEMETRY_FILE` | `.swarm/telemetry.jsonl` | Provider/swarm telemetry sink. | **PROFILE/ON**; isolate per run and seal bytes/digest. |

## Aliases, drift, and unobservable settings

The nine accepted aliases are `act_now→act_now_nudge`, `ask_maxq→ask_max_q`,
`ask_rounds→ask_rounds_max`, `delegated_ok→delegated_decisions_ok`,
`dynamic_replan_cfg→dynamic_replan`, `force_write→force_write_tool`,
`stream_retry→stream_decode_retry`, `temp→temperature`, and
`think_off→think_off_test_authors`. They must never become arm names or independent deltas.

There are no registered controls with no production reader: source tests compare the 142 literal
`GOOSE_SWARM_*` readers bidirectionally with the registry and aliases. There are, however, **39 real
environment-only controls without a run-level effective echo**. Those are settable but not provable from
`levers_resolved`; their mechanism-specific events are the only present evidence. A future campaign may not
arm one until it has a shared resolver/effective echo and a required mechanism event.

The external campaign catalogue is not source truth: its 87 names include nonexistent
`repro_demotes_verified` and `review_repro`, omit 31 persisted controls, and its `ALL-ON.env` has 32 tokens of
which only `review=true` differs from current engine defaults. `author_pitfalls` is ON while a nearby comment
says OFF; `spec_repair` is ON while stale comments say OFF; catalogue “retain enabled” is a desired
disposition, not proof of current default (`done_gate`, `resume`, and `salvage_require_critical` currently
default OFF). The older engine/campaign ledgers also report 49/165 because they predate the registered
`physical_broker`; current source is 50/166. These drifts require generated truth, never another
hand-maintained campaign list.

Five registry-boundary defects remain despite exact name coverage:

1. Environment-only manifest rows expose neither value type/default nor phase/event contract. The 39 values
   without an effective echo cannot be reconstructed from `goose swarm controls`.
2. `split=true` is echoed as executed while `contract_bound_child_split_enabled(judge_split_requested(),
   false)` hard-disables it. `split_secs` and `split_inherit_spec` are consequently inert. The registry test
   proves a reader exists, not that the requested mechanism can fire.
3. `straggler_grace_secs=null` is emitted even though execution derives a concrete grace inside the fan. It is
   a serialized input echo, not an effective execution value.
4. Provider code reads raw `swarm.ai_session_name` as a fallback outside `SwarmConfig` and the registry. The
   control-environment digest covers only `GOOSE_SWARM_*`, excluding runtime-affecting inputs such as
   `GOOSE_LOCAL_CONTEXT_CAP`, `GOOSE_MAX_TOOL_RESPONSE_SIZE`, `GOOSE_COMPACT_KEEP_TAIL`,
   `CONTEXT_FILE_NAMES`, `GOOSE_DEFAULT_EXTENSION_TIMEOUT`, `SWARM_COMMAND`, and `SWARM_LMS_PATH`.
5. Schema-2 reference generation uses serialized nullable defaults. It can stage `null` for behavior controls
   that execute as booleans (`spec_sized_plan null→true`; `straggler_stop_degrade`, `goals`, `ask_replan`, and
   `think_off_test_authors null→false`) and then compare the requested null with the effective boolean. A
   reference or arm can therefore fail verification or become a misleading no-op before any benchmark.

## Source anchors

- Persisted registry: `crates/goose-cli/src/commands/swarm_control_registry.rs:108`; environment-only
  registry: `:240`; effective environment echoes: `:493`; aliases: `:508`; literal reader inventory and
  bidirectional tests: `:550` and `:1039`. Current source contains 142 unique literal readers.
- Serialized source defaults: `crates/goose-cli/src/commands/swarm.rs:1245`; merged config resolution:
  `:1412`; provider/profile/effective `levers_resolved` projection: `:38317`.
- Typed task-detail compiler and staged fan: `crates/goose-cli/src/commands/swarm.rs:19988`, `:20088`,
  `:25099`, and `:34198`.
- Physical control/terminal interface: `crates/goose-swarm/src/control_plane.rs:147` and `:904`; production
  admission entry and its legacy-path rejection: `crates/goose-swarm/src/scheduler.rs:3611`; CLI opt-in:
  `crates/goose-cli/src/commands/swarm.rs:37740` and `:40150`.
- Legacy idle work ordering—dynamic replan, judge, Q&A, prereview, review/testgen, speculation—is
  `crates/goose-swarm/src/scheduler.rs:4078–4440`. The judge comment at `:4154` explicitly says it is not
  capacity-bounded.
- Typed semantic observation schema: `crates/goose-swarm/src/semantic_observation.rs:290`; brokered plane:
  `crates/goose-swarm/src/semantic_control.rs:173`. The ledger records that no production reviewer/summons is
  wired yet.
- Hard-coded prompt policy: `DOMAIN_PITFALLS` at `crates/goose-cli/src/commands/swarm.rs:14686` and
  `WEB_VOCAB` use at `:30252`; fake infinity at `:3577`; legacy judge/repeat intervention at `:17300`;
  repair count/round/winner rules at `:34969`, `:34991`, `:35217`, and `:35258`.

## Retained-run disposition evidence

The old ledger contains no valid current ON/OFF causal result. Its same-binary/config replicate moved from
8/15 to 13/15; every possible 1v1 result has Fisher `p=1`; the retained estimate was a 37.5% chance for an
inert lever to manufacture a win. Mechanism firing, correctness protection, negative evidence, and absence
must therefore remain separate:

- Direct protective observations support preserving `stream_decode_retry`, `no_tools_means_ask`,
  `grounded_research_only`, `answers_win_floor`, `contract_validate`, `failed_tasks_block_green`,
  `ts_smoke_tests`, `spec_sized_plan`, and `doc_prefetch`. They do not establish matched-arm outcome lift.
- `fan_e2e` produced four n3 shards costing 1,842–1,856 task-seconds versus 809–870 for two n1 shards;
  `spec_sized_plan` caught the same spec requesting 6–12 modules on n3 versus 2–4 on n1. This supports
  requirement-sized work, not fleet-shaped fan width.
- `unwired_demotes_verified`/legacy `review` falsely demoted the best working app; `force_write_tool` failed
  to elicit calls and its named-tool mode returned HTTP 400; static `speed_weights` were backwards relative
  to measured host rates. These mechanisms stay OFF or are replaced.
- Fixed progress/volume policy killed healthy Qwen work: the 420-second watchdog killed active
  compaction/prefill, and normal planners were killed twice at exactly 60,005 reasoning characters. Corrected
  r2 detail durations were median 521s, p90 1,791s, maximum 2,140s. This invalidates fixed straggler grace and
  semantic wall/character stops.
- The corrected recurrence fixture rate is 0.4033, not 0.6758; it proves only that a 2,000-character tail can
  miss a longer recurrence. No deterministic recurrence verdict was validated.
- Legacy prereview consumed 72,029 node-seconds and was empty-handed 81% of the time; tail-review findings
  were discarded; `sink_review` fired zero times in 21,805 archived runs. Useful semantic supervision remains
  the design target, but these implementations are not evidence for blindly filling idle slots.
- Legacy judge produced 49 verdicts/four redispatches in one run, with one demonstrable recovery among three
  affected tasks; later it hot-spun around 40 cycles/second until a scheduler correction reduced roughly
  36,000 observations to 55. `judge_nudge` was enabled in another run but emitted zero nudges. The judge is
  promising mechanism evidence, not established efficacy or authority.
- r1 repair ran with `complete`, `spec_repair`, `sink_shard`, `ship_best`, `salvage_spin`, and `read_on_fix`
  together while `fix_sched` was false. It scored 0.0169 after 573.5 minutes; gates/repair consumed 298.0
  minutes and at least about 442 of 894 physical node-minutes were idle. This adverse co-occurrence supports
  unifying and instrumenting repair, never blaming or enabling one switch from that run.
- Retained findings explicitly mark `retarget_stall_guard`, `user_notes`, `sink_prebuild`, and
  `author_pitfalls` as inert/unobservable and `persona` as firing with `lessons:0`. Current disabled controls
  have no valid legacy causal arms. Their disposition comes from correctness and architecture dependency,
  not fabricated outcome evidence.

## Hard caps and deterministic semantic decisions to retire

| Family | Current controls/constants | Required replacement |
|---|---|---|
| Planning | research questions 4; best-of-N UI cap 5; replans 2/UI cap 6; draft 480s; clarity 180s; retarget rounds 2/cap 4; retarget drafts cap 6; ask rounds 3/cap 6 | Requirement/evidence ledger with explicit blocked or inconclusive state. |
| Research | four fixed lenses; scout lookups 10; scout wall 900s; three-document/24k-byte fetch limits | Dynamic unresolved-question queue with cited evidence and retrieval artifacts. |
| Build | worker/planner 900s; worker turns 40; progress watchdog 900s; spiral 12k/60k chars; split wall 300s | Typed terminal/infrastructure state plus semantic observation; no volume/time correctness rule. |
| Judge/review | first look 45s, interval 60s, minimum 2k chars, six looks; nudge max 2; tail fan max 8; tail wall 240s; testgen max 3; speculation max 8 | Brokered observation requests derived from current requirements/evidence, one typed action state machine, terminal-safe delivery. |
| Repair | COMPLETE rounds 2/cap 6; one-flat-round stop; COMPLETE wall 3000s; fix wall 1200s; sink turns 120/cap 200; sink wall 1800s scaled to 2× | Stable defect IDs, shadow candidates, semantic adjudication, atomic composition/promotion, full-ruler closure. |
| Fake infinity | `uncapped` substitutes 604800 seconds and 100,000 turns | `Option<Deadline>`/`Option<Budget>` where `None` is genuinely absent; operator deadline is external and inconclusive. |
| Evidence truncation | scan 512 files/2 MiB; dependency/finding/prompt byte excerpts | Content-addressed artifacts plus lossless provenance; bounded prompt view must point to full evidence. |

## Causal implementation and test order

Order is by correctness dependency and false-result risk, never by effort.

1. **Generated truth and frozen reference.** Land this integration line; export registry schema 2; seal
   binary/build identity, registry digest, ambient environment digest, runtime baseline bytes, provider/model
   profile, spec/scorer hashes, and a zero-delta reference. Refuse the legacy queue.
2. **Typed provider terminal boundary.** Propagate structured dispatch/stream/context/output-limit/cancel states
   end to end. A locally dropped future does not release capacity. Replays must prove retry cannot overlap an
   unconfirmed request.
3. **One physical admission broker.** Make the default judge, prereview, dynamic replanner, test/review, and
   repair roles submit typed opportunities to the common queue instead of making `physical_broker=1` fail.
   Verify physical host/model-instance identity and one-stream versus two-stream service behavior before
   claiming idle time or speedup.
4. **Observation-only semantic judge.** Wire a production reviewer to the existing typed plane. Summons come
   from current unresolved requirements, recurrence evidence, failing gates, or stale-risk evidence—not from
   node idleness. Prove snapshot rejection, dedupe, single flight, no mutation, no abort, and usefulness
   telemetry before adding any intervention.
5. **Canonical planning and research.** One normalized requirement graph and one plan; models contribute
   evidence, conflicts, semantic slices, and patches. Replace fixed lenses/counts and whole-plan consensus
   with an evolving unresolved-question ledger. Keep plans/tasks specific through requirement IDs,
   acceptance evidence, ownership, interfaces, dependencies, and forbidden shortcuts.
6. **Build/task fan.** Replay F924 and other archived shapes, then run matched references to verify compiler
   task quality. Dispatch contract-ready critical work first; use verified spare capacity for current
   evidence/review work. No file-count/fleet-count splitting, fixed grace, or blind duplicate writer.
7. **One terminal-safe intervention.** First test a non-mutating observation. Then, separately, a cooperative
   nudge whose receipt, worker response, artifact/evidence delta, and provider terminal are correlated. Keep
   deterministic kill and speculation disabled.
8. **Causal repair.** Route every repair path through the broker; replace finding counts and “ship best” with
   stable defect hypotheses, isolated candidate trees, semantic judge adjudication, exact composed preview,
   atomic promotion, new tree epoch, and one full ruler. Preserve post-gate mutation sealing and expose
   provisional/salvaged state.
9. **Remove semantic caps and duplicate controls.** Delete/merge only after their replacement state machines
   and replay coverage exist. Operational backstops must emit infrastructure/inconclusive state.
10. **Matched campaign.** Run reference replicates first, then one registered behavior delta per arm with the
    same model/profile/spec/scorer. Require the declared mechanism event and full executed-control projection;
    score hermetically; repeat outcome-bearing arms. Stop on invariant failure, fix, rebuild/reseal, and restart
    the invalid arm—never compare across binary/profile epochs.

## Minimum generated-register enforcement

The smallest acceptable permanent gate is:

1. Rust derives the complete `SwarmConfig` field set and literal production environment-reader set, then
   fails on missing, duplicate, alias-as-control, inert, or stale rows; it asserts the exact disposition/type/
   campaign-role coverage and both serialized and effective source defaults for every persisted control.
2. `goose swarm controls` exports schema, canonical build identity, registry digest, accepted aliases,
   environment-input digest, default, type, phase/mechanism-event contract, campaign role, and effective-echo
   capability for config and environment-only rows without probing a provider. The digest includes every
   runtime-affecting environment/config fallback, not only `GOOSE_SWARM_*`; a golden/schema test round-trips it.
3. Campaign planning accepts only a source-exported behavior control, materializes effective explicit values (including
   `false` for default-on ablation), and requires exactly one full-projection delta or zero for a declared
   replicate. A nullable-default fixture must prove reference/arm values equal execution. Runtime, removal,
   telemetry, env-only-unobservable, alias, implicit, and multi-delta plans fail.
4. Launch rechecks the binary, registry, environment, staged config, reference profile, candidate profile,
   spec, scorer, and queue/plan digests. Post-run verification compares the complete executed projection and
   rejects a missing or mismatched `levers_resolved` event.
5. Each behavior control has a registered mechanism-event predicate. A run with the requested value but no
   causal event is `UNFIRED`, never evidence; registry/event coverage is bidirectional. Provider lifecycle,
   tree epoch, semantic observation/intervention, split admission, and repair promotion each have replay
   fixtures. An enabled-but-hard-disabled branch fails this gate.

Existing coverage proves exact persisted names/readers, a source registry digest, staged-file locks, and a
full persisted projection. It does **not** yet satisfy the effective-default, all-runtime-input,
environment-metadata, nullable-reference, or mechanism-event parts above. In addition, 39 environment-only
values lack a run-level echo, the live launcher has not adopted schema 2, and the new semantic observation
plane has no production reviewer. Those are blockers, not paperwork.
