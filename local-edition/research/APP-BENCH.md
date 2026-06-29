# APP-BENCH — overnight functional-app benchmark (2026-06-29 → )

GOAL (user directive): the local-model swarm must deliver a FUNCTIONAL app in 15–25 min — one that
BUILDS + RUNS + is CORRECT, not a finished-but-broken shell that costs hours of debugging. Study the
journey, find why apps are/aren't functional, improve the swarm (esp. the adversarial review must verify
FUNCTIONALITY: catch hallucinations / fake / stub / unfinished impls), re-test. Report: how many apps, functional?

## Per-app assessment framework (answer ALL — be deep, not superficial)
For every app:
1. **Time-to-deliver** (run_started → run_finished, wall-clock). Target 15–25 min.
2. **FUNCTIONAL?** — BUILD (compiles/imports) + RUN (the real primary command) + CORRECT (right output on real input). NOT "did it finish".
3. **Failure mode** (if not functional): fake/stub impl? unfinished module? wrong logic? flaky worker (claim-done-no-write)? unwired? crash?
4. **Was my prompt pointing at a complex-enough app?** (don't only test trivial CLIs).
5. **Did I answer its questions?** (ask-floor runs — answered as the human, concrete?).
6. **Did the local model follow the PHASES correctly?** (research → plan → contracts → execute → smoke → review; any phase skipped/looped/stalled?).
7. **Did the REVIEW push toward something FUNCTIONAL?** (did smoke/AST-review/integrate-verify catch the real defects, or rubber-stamp a broken app?).
8. **Reasoning vs output** — read the worker/planner traces: where did the reasoning diverge from a working deliverable? What ELSE could improve it?

## Batch (diverse technologies × archetypes)
| id | tech | archetype | spec | time | FUNCTIONAL? | failure mode | notes |
|----|------|-----------|------|------|-------------|--------------|-------|
| APP1 | Python | greenfield CLI (moderate) | unit converter (length/weight/temp, --precision, list-units) | — | IN FLIGHT | — | run_in_background bfaqzx6sx, CONTRACTS on, no ask floor |

## Improvement log (empirical — build only what the failures justify, then re-test)
(pending the first apps' data)

## Running tally
apps attempted: 1 | functional: TBD | avg time-to-deliver: TBD
