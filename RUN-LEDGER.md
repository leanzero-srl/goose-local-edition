# RUN LEDGER — one row per run, so runs are compared by numbers and not by memory

Written by `~/goose-builds/loop-state/snapshot_run.py`. Re-running replaces a run's row, so
a run in flight can be snapshotted on every tick and this always holds its latest state.

The numbers that decide whether a run could have succeeded are the PLAN ones: tasks against
files_planned (over-decomposition), collisions (two tasks writing one file), sink_owns (a
file-owning join is cascaded-Failed by any build failure), chain and startable (whether the
DAG is actually parallel). reasoning_chars against answer_chars is the waste.

## swarm-3node-r0

- **phases**: open → ask → research → synthesis → review → contracts → build
- **started**: 10:07:43
- **tasks**: 10
- **files_planned**: 16
- **collisions**: none
- **sink_owns**: []
- **chain**: 3
- **startable**: 8
- **brief_median**: 4789
- **reasoning_chars**: 258566
- **answer_chars**: 113759
- **code_files**: 15
- **code_bytes**: 74963
- **python_parses**: 8 ok / 0 SYNTAX ERROR / 0 empty
- **web_files**: app.js, index.html, styles.css, viz.js
- **plan_patched**: 1
- **plan_loaded**: 1
- **review_rounds**: r1:new=4 r2:new=0
- **judge_looks**: 91
- **judge_nudges**: 2
- **judge_ended**: 0
- **drift_held**: 7
- **tree_defects**: 0
