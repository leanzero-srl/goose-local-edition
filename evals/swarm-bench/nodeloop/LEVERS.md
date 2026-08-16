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
| GOOSE_SWARM_COMPLETE_CAP_SECS | raw env read [swarm.rs] | code-read | config-backed or default ON — reachable; evidence via levers_resolved |
| GOOSE_SWARM_COMPLETE_PARALLEL | raw env read [swarm.rs] | code-read | raw/gate default OFF — reachable via env; QUEUED unless an arm claims it |
| GOOSE_SWARM_COMPLETE_ROUNDS | raw env read [swarm.rs] | code-read | raw env, default 2 clamp[1,6] — the invariant test binds it to complete_cap; KEEP, tune only with the cap pair |
| GOOSE_SWARM_COMPLETE_STALL_ROUNDS | raw env read [swarm.rs] | code-read | raw env, default 2 min 6 — early-exit on stalled rounds; the G-batch progress-shaped cap's natural home; KEEP |
| GOOSE_SWARM_CONTRACTS | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_CONTRACT_RETRY | config-default gate [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_CONTRACT_VALIDATE | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_CONVERGE | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_CROSS_MODULE_CHECK | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_DEGRADE_ON_STALL | config-default gate [swarm.rs]; raw env read [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_DELEGATED_OK | raw env read [swarm.rs] | code-read | cfg-backed via straggler_stop_resolved shape (default from config; resolved=true live) — BAKE candidate with the ask family |
| GOOSE_SWARM_DELIVERY | config-default gate [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_DEP_SIGNATURES | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_DETAIL_BUDGET_SECS | raw env read [swarm.rs] |  | resolved=420 (live run, engine-emitted) |
| GOOSE_SWARM_DETAIL_MEMO | raw env read [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_DIVERSE_PLAN | config-default gate [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_DOC_EXAMPLES | assured-bundle gate (in_bundle=false) [swarm.rs] | code-read | raw/gate default OFF — reachable via env; QUEUED unless an arm claims it |
| GOOSE_SWARM_DOC_FETCH | config-default gate [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_DOC_PREFETCH | config-default gate [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_DONE_GATE | raw env read [swarm.rs] | code-read | raw env default OFF, py-syntax gate on done-claims, retry-budget bounded — QUEUED (overlaps the smoke gate; measure before baking) |
| GOOSE_SWARM_DRAFT_TEMP | raw env read [swarm.rs] |  | resolved=null (live run, engine-emitted) |
| GOOSE_SWARM_DRAFT_TIMEOUT_SECS | raw env read [swarm.rs] |  | resolved=null (live run, engine-emitted) |
| GOOSE_SWARM_FAILED_TASKS_BLOCK_GREEN | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_FAN_VERIFY | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_FIX_CAP_SECS | raw env read [swarm.rs] | code-read | raw env default 1200 clamp[120,3600] — DESIGNED per-fix budget paired with complete_cap 3000 (invariant-tested); G-batch derives it later; KEEP |
| GOOSE_SWARM_FORCE_WRITE | config-default gate [swarm.rs] | code-read | config-backed or default ON — reachable; evidence via levers_resolved |
| GOOSE_SWARM_GOALS | assured-bundle gate (in_bundle=true) [swarm.rs]; config-default gate [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_GROUNDED_RESEARCH_ONLY | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_INCREMENTAL_REPLAN | config-default gate [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_INHERIT_HINTS | raw env read [swarm.rs] | code-read | raw/gate default OFF — reachable via env; QUEUED unless an arm claims it |
| GOOSE_SWARM_JUDGE | raw env read [swarm.rs] | session-read | default ON (unwrap_or(true) at attach) — the judge does NOT scale with the fleet (F670: verdict ratio 0.9); re-aim work done via PREREVIEW_DIMS; KEEP |
| GOOSE_SWARM_KIND_PROMPT | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_MAX_NODES | raw env read [swarm.rs] | session-read | the bench's pool cap (curve instrument) — KEEP, harness-side lever |
| GOOSE_SWARM_NO_TOOLS_MEANS_ASK | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_OCCUPANCY | config-default gate [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_OMNI_JUDGE | raw env read [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_OVERVIEW | config-default gate [swarm.rs] | session-read | desktop-panel feed toggle — operational, KEEP |
| GOOSE_SWARM_OWNED_FILE_FENCE | config-default gate [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_PARALLEL_TESTS | assured-bundle gate (in_bundle=true) [swarm.rs]; config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_PERSONA | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_PLANNER_ALSO_WORKS | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_PREREVIEW | raw env read [swarm.rs] | session-read | default ON (unwrap_or(true)) — the ONLY fleet-scaling idle mechanism (5.1x/run, F670); carries the per-node quality target; BAKED in practice |
| GOOSE_SWARM_PREREVIEW_DIMS | config-default gate [swarm.rs] | ON (new) | Q2 re-aim; registered check = dimension field on findings |
| GOOSE_SWARM_PROBE_ADVERTISED_POST | assured-bundle gate (in_bundle=false) [swarm.rs] | session-read | default OFF, armed by probe_post arm — FIRED LIVE (F755/F758: rank-1 catch); BAKE-ON candidate after one more armed rep |
| GOOSE_SWARM_PROGRESS_WATCHDOG_SECS | raw env read [swarm.rs] |  | resolved=900 (live run, engine-emitted) |
| GOOSE_SWARM_READ_ON_FIX | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_RELAX_CONTRACTED_DEPS | config-default gate [swarm.rs] | DELETE | inert on 12-run corpus (0 code->code edges) AND empties ALL deps when live — C2 verdict |
| GOOSE_SWARM_REPEAT_BREAK | raw env read [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_REQUIRE_SERVABLE | raw env read [swarm.rs] | session-read | pool guard (abort when no device servable) — safety, KEEP |
| GOOSE_SWARM_REQUIRE_TESTS | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_RESEARCH_TOOLS | config-default gate [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_RESUME | assured-bundle gate (in_bundle=false) [swarm.rs] | session-read | operational (resume a stopped run) — KEEP, not a tuning lever |
| GOOSE_SWARM_RETARGET | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_RETARGET_DRAFT_STEP | raw env read [swarm.rs] | session-read | raw env, ladder growth step clamp[1,3] — B5/G1 changed the ladder's economics; re-evaluate with the next laddering cell; QUEUED |
| GOOSE_SWARM_RETARGET_ROUNDS | raw env read [swarm.rs] |  | resolved=4 (live run, engine-emitted) |
| GOOSE_SWARM_RETARGET_STALL_GUARD | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_RETARGET_STALL_TOLERANCE | raw env read [swarm.rs] | session-read | stall-guard tuning for the ladder — same family as above; QUEUED |
| GOOSE_SWARM_REVIEW | assured-bundle gate (in_bundle=true) [swarm.rs]; config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_SALVAGE_REQUIRE_CRITICAL | raw env read [scheduler.rs]; raw env read [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_SALVAGE_SPIN | raw env read [scheduler.rs]; raw env read [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_SCOPED_CONTRACTS | config-default gate [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_SCOUT_DOC_URLS | assured-bundle gate (in_bundle=false) [swarm.rs] | session-read | arm lever, one cell run (0.6236 unit family) — QUEUED with the doc-wire family readouts |
| GOOSE_SWARM_SINK_CAP_REF_BYTES | raw env read [swarm.rs] | session-read | sink-cap scaling reference (F425 verified live: 1.87x scaling) — KEEP, measured-good |
| GOOSE_SWARM_SINK_CAP_SECS | raw env read [swarm.rs] |  | resolved=1800 (live run, engine-emitted) |
| GOOSE_SWARM_SINK_LEAN_PREFILL | raw env read [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_SINK_MAX_TURNS | raw env read [swarm.rs] |  | resolved=120 (live run, engine-emitted) |
| GOOSE_SWARM_SINK_PREBUILD | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_SINK_REVIEW | raw env read [scheduler.rs]; raw env read [swarm.rs] | session-read | default OFF; switchability fixed a3fdfce02; its idle-fill is a QUALITY play (adversarial verdict) — arm queued in earlier session's sweep; QUEUED |
| GOOSE_SWARM_SKELETON_FIRST | raw env read [swarm.rs] | session-read | worker skeleton-first note gate — S3 subsumes it when skeleton-fill lands; KEEP until S3, then re-verdict |
| GOOSE_SWARM_SMOKE | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_SPECULATE | raw env read [swarm.rs] | session-read | default OFF; Speculated fired 0x in 75+ logs — S7 replaces the rung with test generation; DELETE candidate after S7 lands |
| GOOSE_SWARM_SPEC_CONTRACT | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_SPEC_REPAIR | raw env read [swarm.rs] | BAKED ON | d91fd8b96 — the one use of three nodes this bench found |
| GOOSE_SWARM_SPEC_SIZED_PLAN | config-default gate [swarm.rs] |  | resolved=true — DEFAULT ON since F853 (was an arm; the fleet-scaled ask only binds inflationary). The sweep's spec_sized_plan arm (env=1) is now identical to baseline — an informative arm needs env=0. |
| GOOSE_SWARM_SPEC_WINS | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_SPIRAL_BREAK_CHARS | raw env read [swarm.rs] |  | resolved=12000 (live run, engine-emitted) |
| GOOSE_SWARM_SPIRAL_THINKING_CHARS | raw env read [swarm.rs] |  | resolved=0 (live run, engine-emitted) |
| GOOSE_SWARM_SPLIT | raw env read [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_SPLIT_FAT | config-default gate [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_SPLIT_FAT_FILES | raw env read [swarm.rs] | session-read | reachable, config-backed — evidence via levers_resolved |
| GOOSE_SWARM_SPLIT_INHERIT_SPEC | raw env read [scheduler.rs]; raw env read [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_SPLIT_SECS | raw env read [swarm.rs] |  | resolved=null (live run, engine-emitted) |
| GOOSE_SWARM_STRAGGLER_GRACE_SECS | raw env read [swarm.rs] |  | resolved=null (live run, engine-emitted) |
| GOOSE_SWARM_STRAGGLER_STOP | raw env read [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_STRAGGLER_STOP_DEGRADE | raw env read [swarm.rs] |  | resolved=false (live run, engine-emitted) |
| GOOSE_SWARM_STREAM_RETRY | raw env read [swarm.rs] | session-read | reachable, config-backed — evidence via levers_resolved |
| GOOSE_SWARM_STRUCT_STOP | raw env read [swarm.rs] |  | resolved=80 (live run, engine-emitted) |
| GOOSE_SWARM_THINK_OFF | config-default gate [swarm.rs] | session-read | reachable, config-backed — evidence via levers_resolved |
| GOOSE_SWARM_TS_SMOKE_TESTS | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_UNWIRED_DEMOTES_VERIFIED | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_USER_NOTES | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_VERIFY_COMMANDS | config-default gate [swarm.rs] |  | resolved=true (live run, engine-emitted) |
| GOOSE_SWARM_WRITE_FIRST | config-default gate [swarm.rs] |  | resolved=false (live run, engine-emitted) |
