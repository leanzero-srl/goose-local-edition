# LEVERS — the audit ledger (Batch H). Generated skeleton 

Golden-formula rule: every lever ends BAKED (measured-good -> always on, lever deleted),
DELETED (inert/dangerous), or QUEUED with the arm that will decide it. A lever left
undecided is P1 debt. Verdict column filled per lever as evidence lands; source of truth
for defaults is the code, never this file.

| lever | resolution site(s) | verdict | evidence |
|---|---|---|---|
| GOOSE_SWARM_ACT_NOW | config-default gate [swarm.rs] | code-read | reachable, default ON via cfg-bundle; measured writes 23.8→48% — BAKE candidate |
| GOOSE_SWARM_ANSWERS_WIN_FLOOR | raw env read [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_ASK_AWAY | config-default gate [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_ASK_FILE | raw env read [swarm.rs] | code-read | reachable operational toggle (file-handshake forcing) — KEEP as env-only, not a tuning lever |
| GOOSE_SWARM_ASK_FLOOR | raw env read [swarm.rs] |  | resolved=85 (live run, engine-emitted) |
| GOOSE_SWARM_ASK_MAXQ | raw env read [swarm.rs] | code-read | reachable, env>config>3 — tuning lever, QUEUED behind ask-family arm |
| GOOSE_SWARM_ASK_REPLAN | raw env read [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_ASK_ROUNDS | raw env read [swarm.rs] | code-read | inert unless ask_away (resolved=false on live run) — decide with ask_away |
| GOOSE_SWARM_ASK_SCALE | raw env read [swarm.rs] | code-read | reachable only when a floor is set; heuristic bump capped 100 — KEEP |
| GOOSE_SWARM_ASK_WAIT_SECS | raw env read [swarm.rs] | code-read | raw env, default 1800 — G-batch: derive-or-KEEP decision pending |
| GOOSE_SWARM_ASSURED | raw env read [swarm.rs] | code-read | the profile switch itself — KEEP (meta-lever) |
| GOOSE_SWARM_AUTHOR_PITFALLS | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_BACKBONE | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_BACKBONE_SKIP_CONFIDENT | raw env read [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_BOUNDARY_PROBE | raw env read [swarm.rs] | code-read | default ON (off-values opt out) — reachable; evidence via probe events |
| GOOSE_SWARM_CLARIFY_SPEC_BOUND | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_CLARITY_FAIL_CLOSED | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_CLARITY_PROBE_SECS | raw env read [swarm.rs] |  | resolved=null (live run, engine-emitted) |
| GOOSE_SWARM_CLI_CONTRACT | raw env read [swarm.rs] | code-read | default ON, entry-worker injection — reachable; BAKE candidate with C3 family |
| GOOSE_SWARM_COMPILE_GATE | raw env read [swarm.rs] | code-read | raw env default OFF — reachability confirmed; QUEUED (Rust/TS beds only) |
| GOOSE_SWARM_COMPLETE | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_COMPLETE_CAP_SECS | raw env read [swarm.rs] |  |  |
| GOOSE_SWARM_COMPLETE_PARALLEL | raw env read [swarm.rs] |  |  |
| GOOSE_SWARM_COMPLETE_ROUNDS | raw env read [swarm.rs] |  |  |
| GOOSE_SWARM_COMPLETE_STALL_ROUNDS | raw env read [swarm.rs] |  |  |
| GOOSE_SWARM_CONTRACTS | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_CONTRACT_RETRY | config-default gate [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_CONTRACT_VALIDATE | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_CONVERGE | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_CROSS_MODULE_CHECK | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_DEGRADE_ON_STALL | config-default gate [swarm.rs]; raw env read [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_DELEGATED_OK | raw env read [swarm.rs] |  |  |
| GOOSE_SWARM_DELIVERY | config-default gate [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_DEP_SIGNATURES | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_DETAIL_BUDGET_SECS | raw env read [swarm.rs] |  | resolved=420 (live run, engine-emitted) |
| GOOSE_SWARM_DETAIL_MEMO | raw env read [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_DIVERSE_PLAN | config-default gate [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_DOC_EXAMPLES | assured-bundle gate (in_bundle=false) [swarm.rs] |  |  |
| GOOSE_SWARM_DOC_FETCH | config-default gate [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_DOC_PREFETCH | config-default gate [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_DONE_GATE | raw env read [swarm.rs] |  |  |
| GOOSE_SWARM_DRAFT_TEMP | raw env read [swarm.rs] |  | resolved=null (live run, engine-emitted) |
| GOOSE_SWARM_DRAFT_TIMEOUT_SECS | raw env read [swarm.rs] |  | resolved=null (live run, engine-emitted) |
| GOOSE_SWARM_FAILED_TASKS_BLOCK_GREEN | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_FAN_VERIFY | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_FIX_CAP_SECS | raw env read [swarm.rs] |  |  |
| GOOSE_SWARM_FORCE_WRITE | config-default gate [swarm.rs] |  |  |
| GOOSE_SWARM_GOALS | assured-bundle gate (in_bundle=true) [swarm.rs]; config-default gate [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_GROUNDED_RESEARCH_ONLY | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_INCREMENTAL_REPLAN | config-default gate [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_INHERIT_HINTS | raw env read [swarm.rs] |  |  |
| GOOSE_SWARM_JUDGE | raw env read [swarm.rs] |  |  |
| GOOSE_SWARM_KIND_PROMPT | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_MAX_NODES | raw env read [swarm.rs] |  |  |
| GOOSE_SWARM_NO_TOOLS_MEANS_ASK | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_OCCUPANCY | config-default gate [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_OMNI_JUDGE | raw env read [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_OVERVIEW | config-default gate [swarm.rs] |  |  |
| GOOSE_SWARM_OWNED_FILE_FENCE | config-default gate [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_PARALLEL_TESTS | assured-bundle gate (in_bundle=true) [swarm.rs]; config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_PERSONA | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_PLANNER_ALSO_WORKS | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_PREREVIEW | raw env read [swarm.rs] |  |  |
| GOOSE_SWARM_PREREVIEW_DIMS | config-default gate [swarm.rs] | ON (new) | Q2 re-aim; registered check = dimension field on findings |
| GOOSE_SWARM_PROBE_ADVERTISED_POST | assured-bundle gate (in_bundle=false) [swarm.rs] |  |  |
| GOOSE_SWARM_PROGRESS_WATCHDOG_SECS | raw env read [swarm.rs] |  | resolved=900 (live run, engine-emitted) |
| GOOSE_SWARM_READ_ON_FIX | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_RELAX_CONTRACTED_DEPS | config-default gate [swarm.rs] | DELETE | inert on 12-run corpus (0 code->code edges) AND empties ALL deps when live — C2 verdict |
| GOOSE_SWARM_REPEAT_BREAK | raw env read [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_REQUIRE_SERVABLE | raw env read [swarm.rs] |  |  |
| GOOSE_SWARM_REQUIRE_TESTS | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_RESEARCH_TOOLS | config-default gate [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_RESUME | assured-bundle gate (in_bundle=false) [swarm.rs] |  |  |
| GOOSE_SWARM_RETARGET | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_RETARGET_DRAFT_STEP | raw env read [swarm.rs] |  |  |
| GOOSE_SWARM_RETARGET_ROUNDS | raw env read [swarm.rs] |  | resolved=4 (live run, engine-emitted) |
| GOOSE_SWARM_RETARGET_STALL_GUARD | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_RETARGET_STALL_TOLERANCE | raw env read [swarm.rs] |  |  |
| GOOSE_SWARM_REVIEW | assured-bundle gate (in_bundle=true) [swarm.rs]; config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_SALVAGE_REQUIRE_CRITICAL | raw env read [scheduler.rs]; raw env read [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_SALVAGE_SPIN | raw env read [scheduler.rs]; raw env read [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_SCOPED_CONTRACTS | config-default gate [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_SCOUT_DOC_URLS | assured-bundle gate (in_bundle=false) [swarm.rs] |  |  |
| GOOSE_SWARM_SINK_CAP_REF_BYTES | raw env read [swarm.rs] |  |  |
| GOOSE_SWARM_SINK_CAP_SECS | raw env read [swarm.rs] |  | resolved=1800 (live run, engine-emitted) |
| GOOSE_SWARM_SINK_LEAN_PREFILL | raw env read [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_SINK_MAX_TURNS | raw env read [swarm.rs] |  | resolved=120 (live run, engine-emitted) |
| GOOSE_SWARM_SINK_PREBUILD | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_SINK_REVIEW | raw env read [scheduler.rs]; raw env read [swarm.rs] |  |  |
| GOOSE_SWARM_SKELETON_FIRST | raw env read [swarm.rs] |  |  |
| GOOSE_SWARM_SMOKE | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_SPECULATE | raw env read [swarm.rs] |  |  |
| GOOSE_SWARM_SPEC_CONTRACT | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_SPEC_REPAIR | raw env read [swarm.rs] | BAKED ON | d91fd8b96 — the one use of three nodes this bench found |
| GOOSE_SWARM_SPEC_SIZED_PLAN | config-default gate [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_SPEC_WINS | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_SPIRAL_BREAK_CHARS | raw env read [swarm.rs] |  | resolved=12000 (live run, engine-emitted) |
| GOOSE_SWARM_SPIRAL_THINKING_CHARS | raw env read [swarm.rs] |  | resolved=0 (live run, engine-emitted) |
| GOOSE_SWARM_SPLIT | raw env read [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_SPLIT_FAT | config-default gate [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_SPLIT_FAT_FILES | raw env read [swarm.rs] |  |  |
| GOOSE_SWARM_SPLIT_INHERIT_SPEC | raw env read [scheduler.rs]; raw env read [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_SPLIT_SECS | raw env read [swarm.rs] |  | resolved=null (live run, engine-emitted) |
| GOOSE_SWARM_STRAGGLER_GRACE_SECS | raw env read [swarm.rs] |  | resolved=null (live run, engine-emitted) |
| GOOSE_SWARM_STRAGGLER_STOP | raw env read [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_STRAGGLER_STOP_DEGRADE | raw env read [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_STREAM_RETRY | raw env read [swarm.rs] |  |  |
| GOOSE_SWARM_STRUCT_STOP | raw env read [swarm.rs] |  | resolved=80 (live run, engine-emitted) |
| GOOSE_SWARM_THINK_OFF | config-default gate [swarm.rs] |  |  |
| GOOSE_SWARM_TS_SMOKE_TESTS | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_UNWIRED_DEMOTES_VERIFIED | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_USER_NOTES | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_VERIFY_COMMANDS | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_WRITE_FIRST | config-default gate [swarm.rs] |  | resolved=false (live run, engine-emitted) |
