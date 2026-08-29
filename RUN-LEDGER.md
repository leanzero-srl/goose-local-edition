# RUN LEDGER — one row per run, so runs are compared by numbers and not by memory

Written by `~/goose-builds/loop-state/snapshot_run.py`. Re-running replaces a run's row, so
a run in flight can be snapshotted on every tick and this always holds its latest state.

The numbers that decide whether a run could have succeeded are the PLAN ones: tasks against
files_planned (over-decomposition), collisions (two tasks writing one file), sink_owns (a
file-owning join is cascaded-Failed by any build failure), chain and startable (whether the
DAG is actually parallel). reasoning_chars against answer_chars is the waste.

## swarm-3node-r0

- **phases**: open → ask → research → synthesis → review → contracts → build → integrate → repair → test
- **started**: 10:07:43
- **tasks**: 10
- **files_planned**: 16
- **collisions**: none
- **sink_owns**: []
- **chain**: 3
- **startable**: 8
- **brief_median**: 4789
- **reasoning_chars**: 266614
- **answer_chars**: 123989
- **code_files**: 20
- **code_bytes**: 110095
- **elapsed_min**: 157
- **phase_split**: open 6m · ask 1m · research 39m · synthesis 5m · review 12m · contracts 4m · build 49m · integrate 30m · repair 0m · test 5m
- **before_build_min**: 71 (internal diagnosis only — never a cross-run number)
- **vs_target**: ONLY THE SCORE COMPARES. Target 20.06% (qwen3.8-27b, one cloud agent, same model). Wall clock is NOT comparable — that run is on far faster hardware, so minutes measure the machine. Bytes are NOT a verdict — more code is not better code, and it had no planning phases to spend budget on. Cross-hardware-honest work numbers for THIS run: 110,095B delivered, 266,614 chars reasoned, 10 tasks completed, 3 retried.
- **python_parses**: 11 ok / 0 SYNTAX ERROR / 0 empty
- **web_files**: app.js, index.html, styles.css, viz.js
- **tasks_done**: 10/10 completed, 13 dispatched, 3 retried
- **plan_patched**: 1
- **plan_loaded**: 1
- **review_rounds**: r1:new=4 r2:new=0
- **judge_looks**: 110
- **judge_nudges**: 2
- **judge_ended**: 0
- **drift_held**: 8
- **tree_defects**: 0
