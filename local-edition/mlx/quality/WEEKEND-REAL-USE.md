# WEEKEND REAL-USE campaign (2026-10-31 →)

Owner 2026-10-31: "test these local models until we fix all of the egregious problems ... start using goose outside
of the benchmarks ... for now engine fixes". Director: campaign director agent (workhorse session). Handoff thread:
~/Projects/machine-sync/docs/COORDINATOR-HANDOFF.md. Q ids for this campaign: Q-522 onward, assigned here only.

## Plan
1. SHIP 3.0.137 from main (+ mimo-edit-arg-names / mimo-distributed if the merge gate is green; refreeze bench manifests).
2. REAL-USE rounds (goose-task-author style briefs, app over CDP, words read): Gabee small models now; workmule
   Qwen3.8-Flash single and the MiMo split once the Gauntlet frees the Studios.
3. Fix egregious problems at the root, ranked by how badly they block real use; test + mutation proof + independent check.

## Decisions
- 2026-10-31 10:50 mimo-distributed merge: trial `git merge-tree main mimo-edit-arg-names` = ONE conflict
  (crates/goose-sidecar/src/engine.rs); the branch already merged main 3.0.136 (2e1304d70a) and its dev build
  eca39ae893 has run the 5 h MiMo split Gauntlet 8985a86d (compaction graded B+, vision in use). Decision: merge
  into main IF the merge gate (cargo tests of touched crates + desktop typecheck/tests + unsigned package + launch)
  is green; otherwise ship 3.0.137 without it and record why.

## Problems (one line each: id · found · evidence · fixed sha · re-test)
| id | rank | problem | found (round, evidence) | fixed sha | re-test |
|----|------|---------|-------------------------|-----------|---------|

## Rounds
| round | host · config · model · build | brief | result |
|-------|-------------------------------|-------|--------|

## Handoff (written at ~300k context)
