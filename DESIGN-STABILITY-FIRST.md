# DESIGN-STABILITY-FIRST — the boring pipeline (BP-1), with two grafts

Written 2026-08-29 18:25Z by the design panel (two designs, three judges) while r2 was in RESEARCH
(r2 run.jsonl: phase open 17:43:01Z, ask 18:05:12Z, research 18:07:27Z). Every number below is traceable to
`RUN-LEDGER.md`, `EXPERIMENTS-LEDGER.md`, `TICK-NOTES.md`, a run.jsonl, or a `file:line` in the working
tree at commit `2b1e755ac`+. Anything not cited is marked OPINION. swarm.rs line numbers drifted during the
day; the ones here were re-grepped at 18:20Z.

The panel's ranking was unanimous: **BP-1 first** (judges: stability 7/8/8, speed 7/7/8, quality 5/5/4),
**SPINE + HANDS second** (4/7, 5/4, 6/7). SPINE's single 35-75 minute one-node spine call was scored as an
unbounded stall by construction (goose compacts at 80% of context and continues, `context_mgmt/mod.rs:21`,
`agent.rs:1807`; a transport drop restarts the call from zero, `scheduler.rs:561`). Two of its ideas are
grafted here because the judges named them: the finding-set-SHRANK re-dispatch terminator, and the
zero-code config arm.

---

## 0. THE STEER THAT OVERRIDES EVERY "GATE" BELOW — mild, not deterministic (Mihai, 2026-08-29 22:30)

*"Let's avoid making it overly too deterministic and gated, be very mild with this, we've done deterministic
and plumbing a lot and it didn't work because of how unpredictable these models are."* This document was
written as "a model call only produces an artifact; code makes every continue/stop decision". That reading
is now too strong. The binding reading: **code MEASURES and hands the measurement to a model call; it never
refuses, aborts, caps or hard-limits model work.** A deterministic pass is allowed only as an idempotent
safety net that is a no-op when the model already did the job (r2's one-round REVIEW cleared sharing and
owning-nothing itself; PLAN-REPAIR then had nothing to do), and it never ends a run. Terminators are
lenient and progress-based — "stop when nothing improves", never "exactly one round". Supervision that
redirects is the MILD tool for unpredictability: cut its cost, do not replace it with rules. Each step in §9
carries a **MILD:** clause saying what this changes; where the clause and the original text disagree, the
clause wins.

## 1. The priority order and what it means operationally

Mihai's order is **STABILITY > SPEED > QUALITY, lexicographic**: *"The prime reason the swarm would get
adopted is only if it's fast, if it's stable and if it happens to bring some additional quality."*
Operationally: a layer that can stall, loop, hang or exit dirty is deleted even if it might add quality; a
layer that costs minutes is deleted unless it has a **measured** positive; and quality is bought only with
mechanisms whose terminator is structural (one call) or progress-based (a file landed, a finding set shrank)
— never a wall clock, turn count or retry count on a model call. Every stability failure measured this week
lived in an LLM-in-the-loop layer that assumed convergence (REVIEW loop, repair proxy, judge steers);
every part that worked was deterministic plumbing (DAG, file ownership, transport-drop retry, process
groups, the gate). So the design rule is: **a model call only ever produces an artifact; code makes every
continue/stop decision.**

---

## 2. BEFORE / AFTER

**Today** (r0's emitted sequence, RUN-LEDGER.md:43; PILLARS sits between CONTRACTS and BUILD in the campaign skill):

| # | phase today | LLM calls (r0) | terminator today | r0 min |
|---|---|---|---|---|
| 1 | OPEN | 1 (+ resplit on r1/r2) | one call; 6.4 / 6.9 / 22.2 min for the same prompt | 6.4 |
| 2 | ASK | 1 proxy | one call | 1.1 |
| 3 | RESEARCH | 10 slice briefs + 3 coverage lanes (16 lanes, TICK-NOTES.md:32) | coverage_gap re-fans serially; judged, not measured | 39.8 |
| 4 | SYNTHESIS | 1 | one call | 5.9 |
| 5 | REVIEW | fanned reviewers × rounds | was "no new finding" (diverged on r1); one round since `5173eab67` | 12.8 (r1: 51.4) |
| 6 | CONTRACTS | 3 module stubs | one fan | 5.0 |
| 7 | BUILD | 10 tasks / 13 dispatches + 149 judge looks | agent returns; max_attempts 3 (config.yaml:160) | 49.8 |
| 8 | INTEGRATE | 1 sink | agent returns | 30.6 |
| 9 | REPAIR | proxy ask | `proxy_yes` — was false under benchmark (a1324c68e: round 0 only) | 0.5 |
| 10 | TEST | 3 testers | agents return | 29.1 |
| 11 | RATE | 1 rater | one call | 7.6 |
| 12 | exit | 1 overview agent | hung 20+ min on a grandchild pipe (44b2ad6cd) | 20+ |

Total r0: 188.6 min + ~20 min hang = ~208.6 min; 71 min before BUILD; 67.8 min after BUILD producing zero
fixes (RUN-LEDGER.md:47, EXPERIMENTS-LEDGER.md:211-245).

**Proposed** — seven steps, straight-line, no loop on any model verdict:

| # | phase proposed | LLM calls | terminator | replaces |
|---|---|---|---|---|
| 1 | OPEN — slices | 1 structured-output call (`open_slices` swarm.rs:23320) | STRUCTURAL: one call; 2 unparseable replies → single-slice plan (swarm.rs:25258-25310 region) | OPEN + resplit + coverage fan |
| 2 | SYNTHESIS — task DAG | 1 call (`synthesize_plan` swarm.rs:24512) from slices, no briefs | STRUCTURAL: one call; Err/invalid DAG → `flat_plan_from_briefs` (swarm.rs:26067) | ASK + RESEARCH + SYNTHESIS |
| 3 | PLAN-REPAIR — fix measured flags | 0 | STRUCTURAL: pure function, two passes, asserted idempotent | REVIEW |
| 4 | BUILD — fleet writes files | 1 per task, 3 nodes × 2 slots (`live_fleet_slots` swarm.rs:21770) | PROGRESS: agent returns; drop → re-dispatch; content failure → re-dispatch only while the delivery finding set SHRINKS, else Done(degraded) | CONTRACTS + BUILD + judge + pre-review + replan + speculation |
| 5 | GATE→sink, INTEGRATE | 0 + 1 sink (owns []) | PROGRESS: agent returns; relaxed through upstream failure (scheduler.rs:2979-2996) | INTEGRATE |
| 6 | GATE | 0 | STRUCTURAL: finite probe list (`run_spec_contract` swarm.rs:20546; `handle_gate` :1889) | REPAIR-verify + TEST + RATE |
| 7 | REPAIR — one shard wave, re-GATE, ship | 1 per file group with findings | STRUCTURAL: one wave by code; promote only if `shard_beats_baseline` (swarm.rs:34011) | repair rounds + twin + overview |

Worker prompt after: task description + file-layout manifest + **real excerpts of every completed
dependency** (dep_block swarm.rs:31953-31993, `dep_signatures` OFF) + **the vendor's real responses**
in `doc_facts` (swarm.rs:31531, filled by a deterministic curl) + `grep -n`/`sed -n` licensed for the rest.

---

## 3. What is deleted, and the measurement that condemns each

| deleted | the measurement | source |
|---|---|---|
| RESEARCH fan + coverage lanes | 39.8 of 188.6 min; coverage tail alone 15.6 min after all briefs were done; 3 coverage lanes = 123,968 of 356,684 durable think bytes (35%) with ~1 tool call each; briefs (median 4,789 chars) did not prevent the 5 wrong-key defects | TICK-NOTES.md:30-32; RUN-LEDGER.md:44,47; EXPERIMENTS-LEDGER.md:326-360 |
| ASK proxy + resplit | 1.1 / 2.0 min for a decision code can take by rule; resplit fired on r1 and r2 with no measured effect on the plan | RUN-LEDGER.md:47,91; TICK-NOTES.md:96 |
| REVIEW (even the one-round form) | r1: new findings 8→4→9, 51.4 min, 257,878 think bytes, product replace 7 / remove 1; round 1 rediscovered `viz-engine` owns nothing, which `plan_synthesized.tasks_owning_nothing` had already flagged before the round | EXPERIMENTS-LEDGER.md:46-69; RUN-LEDGER.md:99; swarm.rs:25090 |
| CONTRACTS | hands a SIGNATURE, withholds the BEHAVIOUR of a file already on disk; 5 of 12 TEST defects are verbatim wrong-key/wrong-shape; 2 of 3 and 3 of 6 stubs did not parse in earlier runs | EXPERIMENTS-LEDGER.md:326-360; swarm.rs:9245 |
| `dep_signatures` ON (Tier-A) | `sync.py` read `items` where the vendor sends `data`, `amount` for `amount_minor` → `sync_completeness` 0/12288 → crit ×0.6 and 7 checks vacuous | r0 verdict `verdict-hermetic-seed687ff58b-port8850-0.0568.json`; swarm.rs:1326, :26274 |
| The judge (idle-node + omni looks, steers, drift-hold, and the r3 `judge_restream` escalation `2b1e755ac`) | r0: 149 looks → 2 nudges (1.3%); r1: 122 → 6, all ignored; r2 opener: 5 steers ignored, call finished on its own at 22 min; earlier run 46% of fleet-minutes judging, 33 of 34 DRIFTING nudges changed nothing; the one documented contribution was negative (re-streams 27,297 → 2,004 chars) | RUN-LEDGER.md:50-56,100-104; TICK-NOTES.md:31,86,93-96; EXPERIMENTS-LEDGER.md:79-95 |
| Pre-review, dynamic replan, speculation twins, sink idle-fill, pillars | no positive measurement on the record; pre-review once manufactured a phantom finding on the 0.6720 run; replan is a re-emission by another name | swarm.rs:27014-27030 region; config.yaml `dynamic_replan: true`; EXPERIMENTS-LEDGER.md:26-32 |
| TEST fan | 29.1 min, 3 lanes (60/33/38 tool calls), 12 defects, none acted on; the deterministic gate had already found 37 incl. `GET / 404` and undefined DOM ids | RUN-LEDGER.md:47; EXPERIMENTS-LEDGER.md:109-141 |
| RATE + repair-continue ask (`proxy_yes`, round loop) | 7.6 min, 29 criticals / 2 minors with 6 input-validation nits rated beside "sync loads nothing"; consumer: none (`complete_fix_dispatched` 0) | EXPERIMENTS-LEDGER.md:211-245; swarm.rs:37855 |
| Twin-race and serial fix paths | "two full rounds of twins dying at this cap with the ETag findings unchanged — indicts the shape"; whole-tree #join took 115 of 138 min while 4 file shards finished in 24 | swarm.rs:23584-23585, :37757, :37832 |
| End-of-run overview agent | a model call after the tree is final whose product nothing scores | swarm.rs:18753, :39634 |
| `max_attempts` as a count | the last counter that reads as a cap; replaced by the SHRANK rule (§5) | config.yaml:160; scheduler.rs:1756-1794 |

OPINION, flagged as such by the evidence pack and all three judges: the judge's value has been asserted,
never measured positive. Deleting it also retires `2b1e755ac` (steer→re-stream escalation) before it is
ever exercised; that commit's `nudge_delivery()` test stays green as dead code until the file goes.

---

## 4. What is kept, and the measurement that earns each

| kept | the measurement | source |
|---|---|---|
| Slice decomposition OPEN → SYNTHESIS as ONE plan, corrected by patches only | r0: 10 tasks / 16 files, `tasks_sharing_a_file=0`, chain 3, startable 8, sink owns [] — first run ever to reach BUILD, 10/10 completed; the multi-draft ladder measured 84→84→70→70 | RUN-LEDGER.md:37-45; EXPERIMENTS-LEDGER.md:26-32,178-183 |
| DAG scheduler, file ownership, owns-nothing sink, relax-through-failure, degrade-on-stall | node occupancy 3/3 at BUILD; the sink invariant is the reason a boot defect is found by the run and not the scorer | scheduler.rs:559, :717, :1756-1794, :2979-2996; TICK-NOTES.md:34 |
| Transport drops excluded from real failures | app-js dropped at 11:30/11:46/11:58Z on three nodes, completed on attempt 4 at 12:08 (25,447 B vs a 4,798 B partial) | EXPERIMENTS-LEDGER.md:178-184; TICK-NOTES.md:37-38 |
| No caps on model calls | `effective_idle_budget` uncapped (swarm.rs:3767), `UNCAPPED_SECS` (:3753), inactivity `read_timeout` in the transport is the only dead-stream end (:15435, :17172); the 1800 s cap once wrote a truncated call as `status=done` | EXPERIMENTS-LEDGER.md:20-24 |
| The dependency-content channel (dep_block) and `doc_facts` | the one place the engine already puts a real file in front of a worker; line-bounded with a truncation marker (14,000 / 3,500 char budgets bound prompt size, not model work) | swarm.rs:31911-31993, :31531 |
| Deterministic plan repairs on record | `pin_sink_id` (patch.rs:508), `require_advertised_entry_files` (swarm.rs:20276, truth table :13372), `decomposition_of` (:25009) | tests in place |
| Process groups + `kill_app_tree` + phantom-free gate + `goose swarm gate` replay | old binary leaked 2 servers in under a minute; new binary 0 leaks and 4 real findings in 2.6 s on the r0 tree; 41 orphans before the fix | TICK-NOTES.md:54,65; EXPERIMENTS-LEDGER.md:211-250 |
| In-run deterministic product checks | r0 `complete_verify` 37 findings naming exactly the classes the target lost to (`GET / 404`, DOM ids no html defines); render gate rows=0 | r0 run.jsonl complete_verify; swarm.rs:37488 |
| Per-file shard repair in shadow trees, promote-only-if-better, ship-best | 4 file-attributed shards, 24 min wall, all promoted (the only measured repair shape with a promotion) | swarm.rs:23584-23585, :34011, :36962 |
| Hermetic scoring at the run's own seed/port with the playwright node | blind 0.0832 retracted; comparable 0.0568 with probe_unavailable 30 → 1; `compare_vs_cloud.py` had overstated by multiplying inner | TICK-NOTES.md:51-55,60; RUN-LEDGER.md:58; score_sb7.py:4702-4715 |

---

## 5. The stability argument — a checklist the next tick verifies field by field

Every remaining LLM decision point and its terminator. "The model has not finished speaking" is the one
residual the owner has mandated; nothing below cuts it.

| # | decision point | terminator | tick-verifiable field |
|---|---|---|---|
| 1 | OPEN, one call | model emits the schema reply; dead stream ended by transport inactivity; 2 unparseable → single-slice plan, no third ask | `phase open` then `slices_opened` then `phase synthesis` with no `coverage_*`, `open-resplit`, or `phase ask` between |
| 2 | SYNTHESIS, one call | model emits; Err/cycle/dup/dangling → flat plan; no re-emission path exists | exactly one `plan_synthesized`; `plan_patched` count 0; `plan_repaired` then `plan_loaded` |
| 3 | PLAN-REPAIR | pure; pass order (b) shared-files, (c) module/package, then (a) owns-nothing; second pass asserted no-op | `plan_repaired.after.tasks_owning_nothing == []`, `.shared_files == []`, `.module_package_collisions == []` |
| 4 | BUILD workers, N agent loops | each ends when its agent returns; a transport drop re-dispatches uncounted; a content failure re-dispatches only while `verify_owned_files` findings SHRANK vs the previous attempt; flat → `degraded_stall` Done, dependents relaxed | every `task_retry` carries `reason: transport` or `findings_before > findings_after`; no `task_retry` with equal sets; `judge_look_dispatched` count 0 |
| 5 | INTEGRATE, one agent loop, owns [] | returns; cannot cascade; relaxed through upstream failure | `task_dispatched integrate-verify` carries the GATE findings in its description; one `task_completed` for it |
| 6 | GATE | finite probe list from the spec + DOM scan + render + reboot; every spawn a process group | `complete_verify` count exactly 2 (pre-repair, post-repair); `pgrep -f 'python.*-m app'` = 0 after each |
| 7 | REPAIR shards, one agent loop each | returns; promotion by `shard_beats_baseline`; the wave count is 1 by straight-line code — no `proxy_yes`, no round loop | `complete_fix_dispatched` count == attributed file groups; no `fix_criticals`, `defects_rated`, `complete-fix::twin`; one `complete_result`, one `run_finished`, heartbeat `EXITED:` |
| 8 | exit | no overview agent; drain on process-group liveness (44b2ad6cd) | `run_finished` within 60 s of `complete_result`; orphans 0 |

Hang, loop and stall sites, named:
- Loops: no phase has a `loop {}` on a model verdict; the phase driver is straight-line (the REVIEW loop, the repair round loop, `proxy_yes`, `last_round_promoted` swarm.rs:36980-37883 are deleted).
- Hang: the exit hang was a Popen grandchild holding the pipe; fixed at six spawn sites (EXPERIMENTS-LEDGER.md:231-250) but **proven only by unit tests and a pipetest — r2 is the first run to reach the exit path on that binary (NOW.md claim 2)**; the checklist's row 8 is the live proof.
- Stall: a worker that reasons without acting ends only when the model finishes (r2 OPEN 22.2 min, TICK-NOTES.md:93-96); the structured-output schema is the only pressure, by the owner's rule.
- Inherited, not solved: a long INTEGRATE call compacts at 80% of context and continues (`agent.rs:1807`, `context_mgmt/mod.rs:21`), and a transport drop restarts it from zero (scheduler.rs:561; swarm.rs:32723-32749 region); handing it the GATE findings shortens the session but does not bound it. Residual, stated.
- The vendor probe (§9 step 4) is an HTTP fetch, not a model call, so it carries a connect + read timeout of the transport class; an accepting-but-silent vendor yields empty `doc_facts` and a `vendor_probe{ok:false}` event, never a wait.

---

## 6. Speed — per-phase budget on the 3-node fleet

Derived from r0/r1/r2 phase events; internal diagnostics, never a cross-run number against the cloud
entrant (EXPERIMENTS-LEDGER.md:329-334).

| phase | low | high | basis |
|---|---|---|---|
| OPEN | 6 | 22 | r0 6.4, r1 6.9, r2 22.2 (17:43:01→18:05:12Z); unchanged by this design; the largest single variance |
| SYNTHESIS | 5 | 7 | r0 5.9, r1 5.8; slice-only input trims prefill, unmeasured |
| PLAN-REPAIR | 0 | 0 | pure; the gate replay scale is 2.6 s (TICK-NOTES.md:65) |
| BUILD | 30 | 55 | r0 49.8; drop-free critical path 27.0 (ledgerd 24.8 + boot-wrapper 2.2, r0 task_completed); +20-30% on dependents for real-file reads (the cloud agent's 7 own-code reads); the same drop tail r0 paid |
| GATE→sink | 0 | 2 | spec-contract 2.6 s + render + reboot |
| INTEGRATE | 20 | 30 | r0 30.6 with 61 calls; the judges did not grant the designer's halving |
| GATE | 1 | 2 | |
| REPAIR wave | 15 | 25 | 4 shards / 24 min wall (swarm.rs:23584); never under benchmark |
| re-GATE + ship | 1 | 2 | |
| exit | 0 | 0 | if 44b2ad6cd holds live (r2 settles it) |
| **total** | **78** | **145** | midpoint ~110; the speed judge's independent recomputation gave 79-142, mid ~105 |

Against r0's 208.6 min including the hang: the deleted phases alone account for 115.4 min (ASK 1.1 +
RESEARCH 39.8 + REVIEW 12.8 + CONTRACTS 5.0 + TEST 29.1 + RATE 7.6 + hang 20). One-node phases
(OPEN + SYNTHESIS + INTEGRATE) are 31-59 min, ~40% of wall — the fleet idles 2/3 there; that is the next
speed target after r3, not this design's.

Cloud reference, for scale only: the ledger records the single qwen3.8-27b agent at 106 min
(EXPERIMENTS-LEDGER.md:407); the evidence pack cited 151.3 min from a build manifest the speed judge could
not find under `evals/swarm-bench/runs/sb7-cloud` — **unreconciled; cite neither as comparable**.

---

## 7. Quality — what is given up, the two gating criticals, the honest ceiling

**Given up.** (1) Coverage rests on the one OPEN call: no coverage lanes, no REVIEW; the 0.0023 run's
opener produced nine slices for twelve components (swarm.rs:25363-25372 region) and r1's coverage_gap found
3 missed slices incl. frontend-serving (TICK-NOTES.md:80). PLAN-REPAIR rule (d) covers what the spec's
endpoint table advertises; cross-cutting concerns REVIEW used to name (error envelope, webhook registration,
SSE) rest on OPEN's prompt alone. (2) No module spec: workers get the slice objective + real files + vendor
bodies; money/DST semantics (`b_buckets_dst`, the 0.8 critical the target tripped) get no pre-written spec.
(3) Repair sees only what the GATE sees: a 2xx with the wrong body shape (r0 defects 2-4: health, summary,
buckets) is invisible to `spec_contract` today. (4) One wave: a shard that fails to beat baseline ships the
defect. (5) No judge: a lane spiralling in thinking has nothing but the model finishing.

**The two criticals that gate r0 (0.0568 = 0.1577 × 0.36):**

| critical | r0 cause | attacked by | gate-visible? |
|---|---|---|---|
| `j_workflow_journey` (`GET /` 404; J+V+P+T+E = 0.52 of tier weight unreachable, RUN-LEDGER.md:47) | no task owned serving the page; `coverage_rows_not_work` dropped "web/ files" | PLAN-REPAIR rule (d): `/` appended to the entry-owning task's description; GATE→sink; shard on the entry module | YES — the r0 tree replay shows `GET / 404` as a real finding (TICK-NOTES.md:65) |
| `sync_completeness` 0/12288 (`items` vs `data`; 7 checks vacuous) | dependent built from a signature stub, 0 reads of the vendor or the dependency | real dependency excerpts with key literals; vendor bodies in `doc_facts` | NO today — proposed GATE row `sync_rows` (§9 step 12): after boot and the spec's sync call, the app's rows must be > 0 when the vendor's page 1 has rows; OPINION, medium confidence |

**Ceiling, arithmetic first** (verdict tier weights; evidence pack): with r0's multiplier 0.36 the target
needs pre-severity ≥ 0.557 (3.5× r0); with ONE 0.6 critical ≥ 0.334; with zero criticals r0's own 0.1577 is
still short. Both inner and criticals must move. Expected shape of the first BP-1 run: fewer criticals of
the served-page / entry-file / leaked-server classes (deterministic), unchanged or worse on money/DST
semantics, and a real chance the inner lands below r0's 0.1789 while the run finishes in roughly half the
time with no hang — which under STABILITY > SPEED > QUALITY is the intended order of wins. OPINION: a
served page plus a loading sync would put the score in the 0.10-0.15 band; 0.2006 is not promised by this
design and the judges scored its quality 4-5 for that reason.

---

## 8. ALL vs SINGLE — the control experiment on the same hardware

Three arms, same binary (`levers_resolved.build_sha`), same spec (`evals/swarm-bench/spec-build-sb7.md`),
same `fixture_seed` and vendor port, `benchmark: true`, same playwright render node, scored by
`score_sb7.py --tree <dir> --seed <seed> --port <port> --json-out` (score_sb7.py:4702-4715). Sequential,
never concurrent — two jobs on three nodes make both numbers meaningless (check-the-fleet rule).

- **Arm F (fleet):** BP-1 on 3 nodes × PARALLEL:2 (`lms ps` 18:20Z: gabee 135,936 ctx, mihai 262,144, workhorse 262,144).
- **Arm S (single):** BP-1 with ONE device enabled, `weight: 2`, so `live_fleet_slots` (swarm.rs:21770) gives the node's two slots; every fan collapses deterministically (`one_lane_per_host` swarm.rs:21817; the scheduler runs 2 tasks at a time). Pick workhorse by measured decode rate (r0 `fix_target_selected`: 12.7 tok/s vs 8.4 / 8.3) and say so in the row.
- **Arm R (reference):** plain `goose run -t <the cloud run's prompt>` on the same one node, no engine at all — the local analogue of the 0.2006 run and the honest falsifier of decomposition itself. Needs no code.

**How arm S is produced — RESOLVED 2026-08-29 21:35 local, by reading the engine.** The config.yaml
`devices:` block is DEAD for benchmark runs. `run_build.py:157-160` launches the engine with
`GOOSE_SWARM_MAX_NODES=<nodes>` and `GOOSE_SWARM_PLANNER_ALSO_WORKS=0`, and under that env the pool is
built from `lms ps` (resident, servability-checked) and capped by KEEPING THE FASTEST nodes per
`cfg.speed_weights` (swarm.rs:35016-35075, test :12119 `a_capped_pool_keeps_the_fastest_nodes_and_stays_deterministic`).
r2's `pool_resolved` (run.jsonl 17:43:01Z) carries the three resident `*-qwen3.8-27b-brainwaves` devices at
weight 2 with `planner_pushed: False` — while config.yaml's block lists three DISABLED `qwopus3.6` devices
(mtime 11:58, untouched since). So the trap is not a trap: **arm S = the Benchmark view with the node
selector at 1** (`bench_dispatch.mjs 9897 sb-7 1`), which resolves deterministically to `worksmacstudio`
(`speed_weights: {worksmacstudio: 3, local: 2, gabee: 1}`, config.yaml:203-206) — the same node the decode
rate would pick — with no planner push (harness forces it off) and NO config edit, NO fleet
reconfiguration. `arm_config.py --devices` (step 13) is unnecessary; `snapshot_run.py` should tag `arm` from
`pool_resolved.devices.len()` instead. Residual: `planner_model: workhorse-qwopus3.6-27b-coder-mtp`
(config.yaml:158) names a non-resident model; r0/r1/r2 planned on the pool regardless, so the fallback is
live, but the row for arm S must record which device planned (`pool_resolved` + the first `phase` event's
lane device).

**Verdict rule, pre-registered before any run:** the fan earns its keep only if `score_F ≥ score_S` AND
`wall_F < wall_S`. `score_S ≥ score_F` means decomposition is not earning and BP-1 ships as a single-node
product. Compare in this order: (1) score and the critical rows, (2) wall clock — comparable HERE because
both arms are the same hardware class, (3) work: tool calls per lane, reads per dependent, tasks completed,
code bytes, transport retries. Rows land in RUN-LEDGER.md tagged `arm=F/S/R` by `snapshot_run.py`; a fourth
column in `compare_vs_cloud.py`, reading the `score` field (never inner × crit, the 6.44% overstatement
class, TICK-NOTES.md:60).

---

## 9. Implementation order — ranked by confidence, never by effort

Batch by FILE when agents edit (one agent per file); every step has an isolation test that runs without a
fleet. "Before r3" = the engine change r3 measures; "r3 by config" = off by config in r3, code deleted in r4
once r3 proves nothing depended on it.

| rank | conf. | step | files | blast radius | isolation test | when |
|---|---|---|---|---|---|---|
| 1 | high → **DONE `ee0cbfe73` 2026-08-29 22:12** — **MILD:** the plan-boundary REFUSAL is a WARNING (a flag the tick and the panel show), never an abort; the pass stays an idempotent no-op after a REVIEW that already fixed the flags (refusal sits at the plan-JSON boundary, not `Dag::from_specs` — 45 scheduler_mock tests build file-less specs by construction) | **PLAN-REPAIR** `repair_plan_flags(plan_json, spec)`: (b) first claimant keeps a shared file; (c) `X.py` merges into the `X/` owner as `X/__init__.py`; then (a) owns-nothing tasks removed, dependents re-pointed; (d) every advertised endpoint literal (`spec_get_endpoints` :19941, per-service `spec_advertised_surface` :4013, `/` survives since 0d5ac740d) absent from all descriptions is appended to the entry-owning task's (mapping from `require_advertised_entry_files` :20276); emit `plan_repaired{before,after}`; assert the second pass is a no-op | swarm.rs beside `decomposition_of` :25009 | one pure fn, one call site, one event | truth tables on the real r1 plan (`tasks_owning_nothing=['viz-engine']`, collisions `app/ledgerd.py`/`app/notifierd.py`) and the r0 plan with the real spec asserting `GET /` lands in the ledgerd entry task; idempotence test; a Dag::from_specs test that a non-sink owns-nothing task is REFUSED (dag.rs:94-130 validates only dup ids / unknown deps / cycles today) | before r3 |
| 2 | high | **GATE→sink**: run the gate when the last producer completes; prepend findings to `integrate-verify`'s description | swarm.rs `integrate_verify_spec_inner` :18724; the sink dispatch seam | the sink's prompt; one extra gate (seconds) | a sink dispatch built from a tree with a known 404 carries that finding text; fixture from `goose swarm gate` on the r0 tree | before r3 |
| 3 | high | **attribute_gate_finding** for unassigned findings: grep the tree for the endpoint literal, else the service's entry file; still-unattributed → shipped as known bugs, never a whole-tree residue worker | swarm.rs near `extract_file_from_finding` :29951, `group_findings_by_file` :30180 | repair attribution only | r0's real strings (`GET /` 404 → ledgerd entry; `web/app.js:291 references DOM ids…` → web/app.js) against the archived r0 tree read-only; existing first-source tests stay green | before r3 |
| 4 | high | **Delete CONTRACTS**: phase block, `generate_contracts` :17363, `drop_unparseable_stubs`, `frozen_interfaces_block` :30503, the config default | swarm.rs; coherence.rs `scope_contract_bundle` dead | worker prompt loses FROZEN MODULE INTERFACES; the `contracts` event disappears (UI tolerates absence already) | prompt-assembly tests assert the section absent; `cargo test -p goose-swarm`; grep proves no reader of `self.contracts` | before r3 |
| 5 | high/med | **MILD:** REVIEW (one round, 7 min, fixed the flags itself in r2) STAYS as the semantic fixer with the measured flags as its input; only RESEARCH/coverage/ASK are on the table, and they are deletions, not gates. **Straight-line the planner** in `run_linear_plan` :25355: delete the coverage spawn (`coverage_task` :25501), ASK proxy, resplit (:23980), RESEARCH fan (:24034), `review_once` :25220 and review-added research; SYNTHESIS takes slices (id/title/objective/questions + open_decisions folded as "choose the conventional option"); call step 1 before pin/DAG | swarm.rs planner driver | events `phase ask`, `research_completed`, `review_findings`, `coverage_*` stop; tick.py / snapshot_run.py / useSwarmRun.ts read them as optional rows | fake-dispatcher test (the `review_once` closure seam) asserting the phase sequence open → synthesis → plan_repaired → plan_loaded and that a synthesis Err still yields a loadable DAG | before r3 |
| 6 | high/med | **Real dependency excerpts**: `dep_signatures` default OFF (:1326; test :26274 flips to "must ship OFF"); Tier-A branch → `shape_excerpt(source)` = signatures + every line carrying a string-key access / JSON key literal / route decorator / return-dict, within the existing budgets; wording licenses `grep -n`/`sed -n`, drops "do NOT cat it" | swarm.rs dep_block :31953-31993; coherence.rs `extract_signatures` reused | every worker prompt; size bounded by code (14,000 / 3,500) | ledgerd-shaped fixture returning `{'data': [...]}` with `amount_minor` → excerpt contains both literals within 3,500 chars; F196 truncation test still passes; first-wave task gets "NONE ON DISK YET" | before r3 |
| 7 | medium | **MILD:** the progress rule is lenient — a retry is allowed unless the output is byte-identical or the finding set did not shrink for TWO consecutive attempts; never a count. **SHRANK re-dispatch** (graft from SPINE): a content failure re-dispatches only while the `verify_owned_files` (:22777) finding set is strictly smaller than the previous attempt's; flat → `degraded_stall` Done via the existing degrade path; `max_attempts` stops being read | scheduler.rs :1756-1794 region; swarm.rs config :1309 | every BUILD retry decision | mock-dispatch DAG test: attempts with finding sets 5→3→3 end Done(degraded) after the third with dependents relaxed; a transport drop between them is not counted | before r3 |
| 8 | medium | **Vendor probe into doc_facts**: at BUILD start, if `spec_vendor` (:19761) yields base/docs, fetch the docs page and one page of each advertised vendor GET (bounded ~6k chars; connect + read timeout of the transport class), inject into every worker's `doc_facts` (:35538 assembly); emit `vendor_probe` | swarm.rs; dispatch.rs field unchanged | one HTTP fetch per vendor endpoint; inert when the spec names no vendor | local `http.server` serving `/v3/docs` and `/v3/payments` with `{'data': [...]}` → `doc_facts` contains `"data"` and `amount_minor`; a silent port yields empty doc_facts and `ok:false`, returning under the timeout | before r3 |
| 9 | medium | **MILD:** not "ONE wave" — repair fans while the tree keeps improving (`fix_converged`-style, tree-based) and stops when a wave changes nothing; the deletions are the TEST fan / RATE / twins, not the loop. **Straight-line the tail**: after INTEGRATE run GATE, attribute, fan ONE shard wave (`complete_parallel` :33798 unconditional), re-gate, `ship_best` (:36962); delete `test_app` :23424, `rate_findings` :23575, the round loop and `proxy_yes` (:36980-37883), twin (:38195) and serial paths, the overview agent (:39634) | swarm.rs tail :36863-39713 (~2,850 lines; `doc_facts` cloned at 11 dispatch sites :38154-39472 per judge 3) | events `defects_observed`/`defects_rated`/`fix_criticals`/`complete-fix::twin` disappear; tick.py "repair:" row and the desktop repair panel read `complete_verify`/`complete_fix_dispatched`/`complete_result` | new `goose swarm repair <tree> --spec <spec>` subcommand: stub dispatcher asserts shard set == attributed groups, non-improving shard not promoted, ONE wave and exactly two `complete_verify`; then live on the archived r0 tree (4 real findings, ~20 min fleet replay, not a run) | before r3 |
| 10 | medium → **MEASURED 2026-08-29 21:50** — **MILD:** switching supervision off is an EXPERIMENT arm for r3, not a decision; the judge is the mild tool and its cost, not its existence, is the target | **Supervision OFF by config** — config-reachable and truly off when false: `omni_judge` (sole guard on `judge_look_dispatched`, :15843→:16321), `dynamic_replan` (:36704), `goals` (pillars, already None), `sink_review` (already off), `supervision_pool`, `retarget`. **NOT config-reachable, ON by default, absent from `levers_resolved`** — and NO plumbing gets built for them (Mihai, 22:20: *"if something gets deleted and redone what's the point of doing it to begin with?"*): the Benchmark view's spawn env (main.ts `benchmark-run`, the block that already sets `GOOSE_SWARM_BENCHMARK`) passes them as `=0` for r3 — four lines that die with step 11 — and the tick proves the state by the ABSENCE of their events, not by a levers row: `GOOSE_SWARM_PREREVIEW` (:36724), `GOOSE_SWARM_TAIL_REVIEW` (scheduler.rs:59, the LIVE idle-fill), `GOOSE_SWARM_JUDGE` (:36712, the idle-model judge, distinct from omni), `GOOSE_SWARM_PREREVIEW_DIMS` (:27190). Speculation is env-only but defaults OFF. One command for the rest: `arm_config.py --set omni_judge=false dynamic_replan=false incremental_replan=false goals=false sink_review=false supervision_pool=false retarget=false benchmark=true` | config.yaml + main.ts env block (four lines) | none in the engine | `arm_config.py --set` dry-run shows the keys; a fixture replay asserts zero `judge_look_dispatched`/`judge_nudge`/`pillar_check_advisory` | r3 by config |
| 11 | medium | **MILD: DEFERRED** — deletion only if r3-without-supervision is not worse on the criticals; otherwise the judge stays and its nudge cost is cut (fewer looks, re-stream on ignored steers, which already landed). **Delete supervision code**: judge wiring (:36581-36587 region, :15538 omni), judge branches in the stream loop (:17040 `judge_restream` incl. 2b1e755ac), `distill_pillars` :17309, sink idle-fill, speculation twins; judge.rs (1,385 lines) consumers in lib.rs / scheduler.rs | swarm.rs, goose-swarm judge.rs + scheduler.rs judge slot, UI captions, tick.py counters | largest structural deletion; one agent per file | `cargo test -p goose-swarm` (mock-dispatch DAG completes with no judge); vitest for caption paths; verify in the RUNNING app over CDP | r4, after r3 shows nothing depended on it |
| 12 | medium | **MILD:** a FINDING row fed to repair, never a block on shipping. **GATE row `sync_rows`** (new): after boot and the spec's sync call, the app's row count must be > 0 when the vendor probe's page 1 had rows — makes the top critical gate-visible and shardable | swarm.rs `run_spec_contract` :20546 | one probe row; needs step 8's probe output | replay on the r0 tree asserts the row fires (r0 loaded 0 of 12,288); a tree whose sync works passes | r3 if step 8 lands; else r4 |
| 13 | medium | **All-vs-single harness** — DONE 2026-08-29 (loop-state): `snapshot_run.py` tags `arm` from `pool_resolved.devices` (F/S; R by hand), `compare_vs_cloud.py --single <verdict> [--wall-f/--wall-s]` prints the S line and the pre-registered rule with wall UNMEASURED unless stamped or passed. `arm_config.py --devices` is NOT needed — arm S is the Benchmark view at nodes=1 (§8) | ~/goose-builds/loop-state | harness only | `launch.sh --dry-run` for both arms asserts 3 vs 1 enabled devices and identical seed/port/levers; compare against the r0 hermetic verdict + a copied stand-in asserting the `score` field is read | after r3 (arm S and R run on the r3 binary) |
| 14 | high | **Instruments and UI follow the events**: `plan_repaired`, shard-only repair rows, `vendor_probe`; retire judge/review/coverage counters — via `digestStreamFields()` only, never a hand-copied lane field | tick.py, snapshot_run.py, useSwarmRun.ts, SwarmRunPanel.tsx | instruments and the panel; no engine behaviour | fixture run.jsonl with the BP-1 sequence through vitest and tick.py; verify live over CDP (`tick_ui.mjs`) | with r3 |

Order of landing before r3, by confidence: 1, 2, 3, 4, 14 (fixtures first), 5, 6, 7, 8, 9; then rebuild,
install, `goose swarm gate` and `goose swarm repair` replays on the archived r0 tree as the pre-run proof
(test-sooner rule), then r3 with step 10's config. Steps 11-13 follow r3.

---

## 10. Confidence, honestly, per section

| section | confidence | why |
|---|---|---|
| §1 priority order | high | the owner's words; the lexicographic reading is his |
| §2 BEFORE/AFTER | high on BEFORE (emitted phases, measured minutes); medium on AFTER — SYNTHESIS from slices without briefs has never run |
| §3 deletions | high | every deleted layer has a measured cost and no measured positive on the record; the judge's "no value" is OPINION, flagged |
| §4 keeps | high | each has a measurement with a run behind it; the shard wave's measurement is one wave, not under benchmark |
| §5 stability | high on "cannot loop" (structural, enumerable by reading a straight-line driver); **medium on "cannot hang"** — the exit fix is unit/pipetest-proven, r2 is the first live pass; INTEGRATE compaction + drop-restart is an inherited residual, not solved |
| §6 speed | medium | rests on r0's phase minutes (internal diagnostics), an unmeasured shard wave under benchmark, and an OPEN whose duration varied 3.5× on the same prompt |
| §7 quality | medium-low | the mechanism is the cloud agent's, the capacity is not (3,500-char excerpts vs a 200k context); one wave; `sync_rows` gate row is new and unmeasured; 0.2006 is not promised |
| §8 all-vs-single | **medium-low until the config trap is resolved** — the file arm S would edit is not the file r2 ran on | 
| §9 implementation | high for steps 1-4, 14; medium for 5-9 (large deletions in a 42k-line file, ~2,850-line tail, 11 clone sites); medium-low for 10 (config keys unverified to reach the engine) and 13 |

What would raise the confidence fastest, in order: r2 reaching `run_finished` with orphans 0 (§5 row 8);
the `goose swarm repair` replay on the r0 tree promoting a shard for `GET /` (§7 first critical); the
vendor-probe test producing `data`/`amount_minor` in a worker prompt (§7 second critical); and the config
r2 actually used, found and written down (§8).
