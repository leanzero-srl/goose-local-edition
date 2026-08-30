# RUN LEDGER — one row per run, so runs are compared by numbers and not by memory

Written by `~/goose-builds/loop-state/snapshot_run.py`. Re-running replaces a run's row, so
a run in flight can be snapshotted on every tick and this always holds its latest state.

The numbers that decide whether a run could have succeeded are the PLAN ones: tasks against
files_planned (over-decomposition), collisions (two tasks writing one file), sink_owns (a
file-owning join is cascaded-Failed by any build failure), chain and startable (whether the
DAG is actually parallel). reasoning_chars against answer_chars is the waste.

## swarm-3node-r0-ENDED-29criticals-repair-never-ran-benchmark-forces-proxy-no

- **phases**: open → ask → research → synthesis → review → contracts → build → integrate → repair → test → rate
- **started**: 10:07:43
- **tasks**: 10
- **files_planned**: 16
- **collisions**: none
- **sink_owns**: []
- **chain**: 3
- **startable**: 8
- **brief_median**: 4789
- **reasoning_chars**: 298848
- **answer_chars**: 137674
- **code_files**: 20
- **code_bytes**: 110095
- **elapsed_min**: 189
- **phase_split**: open 6m · ask 1m · research 39m · synthesis 5m · review 12m · contracts 4m · build 49m · integrate 30m · repair 0m · test 29m · rate 7m
- **before_build_min**: 71 (internal diagnosis only — never a cross-run number)
- **vs_target**: ONLY THE SCORE COMPARES. Target 20.06% (qwen3.8-27b, one cloud agent, same model). Wall clock is NOT comparable — that run is on far faster hardware, so minutes measure the machine. Bytes are NOT a verdict — more code is not better code, and it had no planning phases to spend budget on. Cross-hardware-honest work numbers for THIS run: 110,095B delivered, 298,848 chars reasoned, 10 tasks completed, 3 retried.
- **python_parses**: 11 ok / 0 SYNTAX ERROR / 0 empty
- **web_files**: app.js, index.html, styles.css, viz.js
- **tasks_done**: 10/10 completed, 13 dispatched, 3 retried
- **plan_patched**: 1
- **plan_loaded**: 1
- **review_rounds**: r1:new=4 r2:new=0
- **judge_looks**: 134
- **judge_nudges**: 2
- **judge_ended**: 0
- **drift_held**: 8
- **tree_defects**: 0
- **repair**: delivery_defect_steer=2 brief_defects=1 testgen=1 fix_target_selected=1 complete_verify=1 complete_result=0
- **score**: **0.0568** hermetic — seed 687ff58bfa6b707d (the run's own), vendor port 8850, playwright node (inner 0.1789 × crit_mult 0.36). TWO criticals: `sync_completeness` 0/12288 (the `items`-vs-`data` key) and `j_workflow_journey` (the frontend, graded for the first time). vs target 0.2006: 28%. vs published local 0.0273: 2.1×. An earlier 0.0832 was BLIND (no playwright, 30/99 checks unavailable) and on a FRESH seed — retracted, kept as `verdict-BLIND-fresh-seed-NOT-COMPARABLE-0.0832.json`. Comparable verdict: `verdict-hermetic-seed687ff58b-port8850-0.0568.json` in the run dir.

## swarm-3node-r1-KILLED-review-diverged-8-4-9-new-findings-4-rounds-51min-vs-r0-12min

- **phases**: open → ask → research → synthesis → review
- **started**: 15:43:54
- **tasks**: 0
- **files_planned**: 0
- **collisions**: none
- **sink_owns**: NO SINK
- **chain**: 0
- **startable**: 0
- **brief_median**: 0
- **reasoning_chars**: 144276
- **answer_chars**: 136323
- **code_files**: 1
- **code_bytes**: 696
- **elapsed_min**: 98
- **phase_split**: open 6m · ask 2m · research 32m · synthesis 5m · review 51m
- **before_build_min**: 0 (internal diagnosis only — never a cross-run number)
- **vs_target**: ONLY THE SCORE COMPARES. Target 20.06% (qwen3.8-27b, one cloud agent, same model). Wall clock is NOT comparable — that run is on far faster hardware, so minutes measure the machine. Bytes are NOT a verdict — more code is not better code, and it had no planning phases to spend budget on. Cross-hardware-honest work numbers for THIS run: 696B delivered, 144,276 chars reasoned, 0 tasks completed, 0 retried.
- **python_parses**: 0 ok / 0 SYNTAX ERROR / 0 empty
- **web_files**: NONE — 0.56 of the scoring weight is unreachable without a served page
- **tasks_done**: 0/? completed, 0 dispatched, 0 retried
- **plan_patched**: 3
- **plan_loaded**: 0
- **review_rounds**: r1:new=8 r2:new=4 r3:new=9
- **judge_looks**: 105
- **judge_nudges**: 6
- **judge_ended**: 0
- **drift_held**: 11
- **tree_defects**: 0
- **repair**: delivery_defect_steer=0 brief_defects=1 testgen=0 fix_target_selected=0 complete_verify=0 complete_result=0

## swarm-3node-r2-KILLED-by-operator-killpg-reap-INTEGRATE-139m

- **phases**: open → ask → research → synthesis → review → contracts → build → integrate
- **started**: 17:43:01
- **tasks**: 11
- **files_planned**: 23
- **collisions**: none
- **sink_owns**: []
- **chain**: 2
- **startable**: 10
- **brief_median**: 4740
- **reasoning_chars**: 281376
- **answer_chars**: 152004
- **code_files**: 41
- **code_bytes**: 873076
- **elapsed_min**: 289
- **phase_split**: open 22m · ask 2m · research 48m · synthesis 5m · review 6m · contracts 6m · build 67m · integrate 129m
- **before_build_min**: 92 (internal diagnosis only — never a cross-run number)
- **vs_target**: ONLY THE SCORE COMPARES. Target 20.06% (qwen3.8-27b, one cloud agent, same model). Wall clock is NOT comparable — that run is on far faster hardware, so minutes measure the machine. Bytes are NOT a verdict — more code is not better code, and it had no planning phases to spend budget on. Cross-hardware-honest work numbers for THIS run: 873,076B delivered, 281,376 chars reasoned, 12 tasks completed, 2 retried.
- **python_parses**: 29 ok / 0 SYNTAX ERROR / 0 empty
- **web_files**: app.js, index.html, styles.css, test_camera.js, viz.js, viz_camera.js
- **tasks_done**: 12/11 completed, 15 dispatched, 2 retried
- **arm**: F (fleet of 3)
- **fixture_seed**: 5cd47b42e2a7c3e0
- **plan_patched**: 1
- **plan_loaded**: 1
- **review_rounds**: r1:new=9
- **judge_looks**: 110
- **judge_nudges**: 7
- **judge_ended**: 0
- **drift_held**: 14
- **tree_defects**: 0
- **repair**: delivery_defect_steer=3 brief_defects=2 testgen=3 fix_target_selected=0 complete_verify=0 complete_result=0
- **seed_provenance**: backfilled 2026-08-30 from trace-swarm-3node-r0.jsonl header (trace.jsonl now pinned in the archived tree); the row briefly read "unknown (no trace file)" after a whole-row re-snapshot

## swarm-3node-r4-KILLED-replan-r0-spliced-5-tasks-past-repair-shadow-reintroduced-sink-owned-README-build-7m

- **phases**: open → ask → synthesis → review → build
- **started**: 06:12:51
- **tasks**: 8
- **files_planned**: 12
- **collisions**: none
- **sink_owns**: ['README.md']
- **chain**: 3
- **startable**: 1
- **brief_median**: 8362
- **reasoning_chars**: 105875
- **answer_chars**: 7625
- **code_files**: 4
- **code_bytes**: 6308
- **elapsed_min**: 27
- **phase_split**: open 8m · ask 0m · synthesis 2m · review 9m · build 6m
- **before_build_min**: 20 (internal diagnosis only — never a cross-run number)
- **vs_target**: ONLY THE SCORE COMPARES. Target 20.06% (qwen3.8-27b, one cloud agent, same model). Wall clock is NOT comparable — that run is on far faster hardware, so minutes measure the machine. Bytes are NOT a verdict — more code is not better code, and it had no planning phases to spend budget on. Cross-hardware-honest work numbers for THIS run: 6,308B delivered, 105,875 chars reasoned, 0 tasks completed, 0 retried.
- **python_parses**: 4 ok / 0 SYNTAX ERROR / 0 empty
- **web_files**: NONE — 0.56 of the scoring weight is unreachable without a served page
- **tasks_done**: 0/8 completed, 2 dispatched, 0 retried
- **arm**: F (fleet of 3)
- **fixture_seed**: f41fc69916195fec
- **plan_patched**: 1
- **plan_repaired**: 5 actions, owning-nothing 0->0, unassigned endpoints 0->0
- **plan_loaded**: 1
- **review_rounds**: r1:new=1
- **judge_looks**: 22
- **judge_nudges**: 0
- **judge_ended**: 0
- **drift_held**: 6
- **tree_defects**: 0
- **repair**: delivery_defect_steer=2 brief_defects=0 testgen=0 fix_target_selected=0 complete_verify=0 complete_result=0

## swarm-3node-r4b-KILLED-reviewer-verbatim-cycle-24m-judge-read-shapes-not-words-looks-2-6-OK-at-78pct-dup

- **phases**: open → ask → synthesis → review
- **started**: 06:59:53
- **tasks**: 0
- **files_planned**: 0
- **collisions**: none
- **sink_owns**: NO SINK
- **chain**: 0
- **startable**: 0
- **brief_median**: 0
- **reasoning_chars**: 99408
- **answer_chars**: 8396
- **code_files**: 0
- **code_bytes**: 0
- **elapsed_min**: 21
- **phase_split**: open 7m · ask 0m · synthesis 6m · review 6m
- **before_build_min**: 0 (internal diagnosis only — never a cross-run number)
- **vs_target**: ONLY THE SCORE COMPARES. Target 20.06% (qwen3.8-27b, one cloud agent, same model). Wall clock is NOT comparable — that run is on far faster hardware, so minutes measure the machine. Bytes are NOT a verdict — more code is not better code, and it had no planning phases to spend budget on. Cross-hardware-honest work numbers for THIS run: 0B delivered, 99,408 chars reasoned, 0 tasks completed, 0 retried.
- **python_parses**: 0 ok / 0 SYNTAX ERROR / 0 empty
- **web_files**: NONE — 0.56 of the scoring weight is unreachable without a served page
- **tasks_done**: 0/? completed, 0 dispatched, 0 retried
- **arm**: F (fleet of 3)
- **fixture_seed**: 9fed69d18bfba590
- **plan_patched**: 0
- **plan_loaded**: 0
- **review_rounds**: none
- **judge_looks**: 22
- **judge_nudges**: 4
- **judge_ended**: 0
- **drift_held**: 5
- **tree_defects**: 0
- **repair**: delivery_defect_steer=0 brief_defects=0 testgen=0 fix_target_selected=0 complete_verify=0 complete_result=0

## swarm-3node-r0

- **phases**: open
- **started**: 08:38:47
- **tasks**: 0
- **files_planned**: 0
- **collisions**: none
- **sink_owns**: NO SINK
- **chain**: 0
- **startable**: 0
- **brief_median**: 0
- **reasoning_chars**: 15843
- **answer_chars**: 1422
- **code_files**: 0
- **code_bytes**: 0
- **elapsed_min**: 52
- **phase_split**: open 51m
- **before_build_min**: 0 (internal diagnosis only — never a cross-run number)
- **vs_target**: ONLY THE SCORE COMPARES. Target 20.06% (qwen3.8-27b, one cloud agent, same model). Wall clock is NOT comparable — that run is on far faster hardware, so minutes measure the machine. Bytes are NOT a verdict — more code is not better code, and it had no planning phases to spend budget on. Cross-hardware-honest work numbers for THIS run: 0B delivered, 15,843 chars reasoned, 0 tasks completed, 0 retried.
- **python_parses**: 0 ok / 0 SYNTAX ERROR / 0 empty
- **web_files**: NONE — 0.56 of the scoring weight is unreachable without a served page
- **tasks_done**: 0/? completed, 0 dispatched, 0 retried
- **arm**: F (fleet of 3)
- **fixture_seed**: ddcbba021ea1eba1
- **plan_patched**: 0
- **plan_loaded**: 0
- **review_rounds**: none
- **judge_looks**: 13
- **judge_nudges**: 2
- **judge_ended**: 0
- **drift_held**: 1
- **tree_defects**: 0
- **repair**: delivery_defect_steer=0 brief_defects=0 testgen=0 fix_target_selected=0 complete_verify=0 complete_result=0

