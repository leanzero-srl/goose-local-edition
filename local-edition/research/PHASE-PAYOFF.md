# PHASE-PAYOFF — does each swarm phase earn its minutes? (quality + time, evidence-based)

Method (memory swarm-phase-tuning-approach): A/B the SAME spec both ways, compare QUALITY (build+run+correct+
wired, JUDGE BY RUNNING) AND time. Phases stay configurable knobs with evidence-based defaults; pure bugs get
fixed. Per-phase wall-clock is in every run report (research/planning/execute) since fcaa7cb99.

## Phase wall-clock baseline (from UNIQ1-4, computed from jsonl)
| run | total | planning (pre-code) | execute | note |
|-----|-------|---------------------|---------|------|
| UNIQ1 | 105m | 30m (29%) | 75m | FAILED (unwired) |
| UNIQ2 | 58m | 22m (38%) | 36m | WIN |
| UNIQ3 | 97m | 24m (25%) | 73m | WIN |
| UNIQ4 | 85m | 27m (32%) | 58m | PARTIAL (budget bug) |
PLANNING is a persistent 22-30min / 25-38% before any code. The biggest waste suspect.

## EVIDENCE so far
- SMOKE GATE — PAYS OFF (keep, default-on). Caught UNIQ4 flat-layout unrunnable-via-`-m` + auto-fixed; would
  otherwise have shipped broken. A real defect caught for ~minutes of cost.
- ENTRY-WIRING + DB-SCHEMA-FREEZE — PAY OFF (quality). UNIQ1 (no wiring) failed; UNIQ2/UNIQ3 (wired) work;
  UNIQ3 wired with NO answer from me = the fix works default. Schema consistent across UNIQ4 modules.

## EXPERIMENT 1: SKELETON_FIRST (direction A) — clean A/B, same bookmark-manager spec
RUN 1 (SKELETON_FIRST=1, ABskelon): single cli-entrypoint task (NOT split — confound removed). COMPLETE.
  cli metrics: over_read 0, retries 0, judge 3x ok, tool_calls 23 (incremental write+fill+shell-checks), cli.py 4043b.
  TIME (phase report): research 1.7m | planning 6.6m | execute 20.5m | TOTAL 28.9m.
  QUALITY: CLEAN WIN. 4 done 0 FAILED (run-status PASSED — integrate-verify did NOT false-negative here).
  Golden all correct: list newest-first, search matches both, tags counts (tech2/news1/lang1), export valid
  JSON exit0, bad-format exit2, unknown-id exit1. NO stub left. CLI wired (all 8 commands).
RUN 2 (SKELETON_FIRST=0, ABskeloff): COMPLETE. cli SPLIT into 4 tasks (cli-entry-point 6 + 3 command tasks 1
  each = 9 tool_calls), over_read 0, retries 0. TIME: research 2.0/planning 9.7/execute 19.1/TOTAL 30.8m.
  QUALITY: CLEAN (golden all correct by running) — but run-status FAILED (integrate-verify + tests
  false-negatived a WORKING app again; tests subtask produced none, same as UNIQ4).

### VERDICT (skeleton-first) — NOT-WORSE on simple apps -> DEFAULT ON (66dac9395, per user rule)
| metric | run1 ON | run2 OFF |
|--------|---------|----------|
| over_read | 0 | 0 |
| retries | 0 | 0 |
| quality (golden by RUN) | CLEAN | CLEAN |
| cli round-trips | 23 (1 task) | 9 (4 tasks) |
| execute | 20.5m | 19.1m |
| total | 28.9m | 30.8m |
WASH: identical quality, 0 over_read both ways (the front-load problem did NOT occur on this moderate app), total
time within noise. The 23-vs-9 round-trips did NOT cost wall-clock (run2 split into 4 tasks, offsetting). So
skeleton-first is NOT-WORSE on simple apps + helps on COMPLEX entries (UNIQ3 over_read) -> default ON, opt-out
SKELETON_FIRST=0. CONFIDENCE: not-worse = SOLID (quality identical + over_read 0 both); time = LOW (N=1 +
decomposition confound); complex BENEFIT = INFERRED (UNIQ3), confirm with a complex A/B (UNIQ5). Reversible.
NOTE: run-status FALSE-FAILED both UNIQ-class working apps via integrate-verify — that phase needs the
visibility-then-fix (it is not paying off as a TRUST signal). tests-subtask-produces-nothing recurs (UNIQ4+run2).

## RUN-STATUS TRUST — BOTH false-negative causes now FIXED
1. JUDGE-KILL (6e1547b2d, CONFIRMED UNIQ6): integrate-verify owns no files -> over-read gate permanently armed
   -> guaranteed judge_killed. Fixed: over-read gate exempts no-owned tasks. UNIQ6 integrate-verify NOT killed.
2. DEPENDENCY-BLOCKED (5146fd69b, NEW): integrate-verify depended on the tests subtask; a failed tests blocked
   it (0 attempts, never ran). Fixed: strip_integrate_verify_test_deps removes test deps from integrate-verify
   so it runs the PROGRAM regardless of the tests. Test integrate_verify_does_not_block_on_tests.
NET: integrate-verify now RUNS on every app -> run-status reflects whether the APP works (honest DONE on a
working app; honest FAIL catching real bugs like UNIQ6 infer-persist). VALIDATE on UNIQ7+ (a WORKING app should
now report DONE; a buggy one should FAIL because integrate-verify actually caught it). The tests-subtask itself
still fails sometimes (owns files, genuine over-read/looping) — separate reliability item, but it no longer
poisons run-status.

## UNIQ9 (habit tracker) — MULTI-PHASE PAYOFF observed in ONE run (smoke+review+judge all fired)
- SMOKE gate PAID OFF: result {ran:true, py_files:9, collect:ok, entry_package:habits, entry_ok:TRUE, findings:[]}
  = the entry `python -m habits --help` RUNS (exit 0) + pytest collects. Confirms runnable before shipping.
- REVIEW (AST wiring) PAID OFF: found "module 'habits.models' imported by no non-test module — built-but-unwired".
  models.py (336b dataclass) is dead/unreachable. Real finding a unit-test-only pass would miss.
- SpecDrift judge PAID OFF: caught __main__.py checkin/uncheck using POSITIONAL date vs the spec --date flag
  (a bug the golden would fail on) -> re_dispatched cli-app att2 to fix. BUT att2 was finalize-spin-KILLED
  mid-fix (verdict looping->failed) so the --date fix may be INCOMPLETE (golden will verify).
- test-dep-strip CONFIRMED: BOTH tests terminally FAILED yet the run proceeded to smoke+review (not blocked).
  run-status reflects the APP (smoke), not the failing tests. The exact scenario the strip fix was built for.
- dynamic replanner correct: round 0 added [] stopped (nothing to parallelize at the tail) -> 2 idle nodes = LEGIT.
- verify-not-rewrite CONFIRMED (att1 wrote commands.py+__main__.py immediately, trace).

### finalize-spin-while-FIXING — now TWO instances (backlog B strengthened)
tests-advanced/tests-core: wrote test -> debugged failing test >420s w/o edit -> looping killed. AND cli-app att2:
was applying the SpecDrift --date fix, made progress (ok x2), then went stale >420s -> looping killed mid-fix.
So the 420s finalize-spin threshold kills workers that are LEGITIMATELY fixing/debugging, not just idle-spinning.
This is the strongest evidence yet for backlog B (threshold + salvage: on finalize-spin, if the file PARSES and
smoke/entry is ok, SALVAGE it as done rather than discarding the partial fix). Needs the existing finalize-spin
test kept green. CONFIDENCE MED — salvage risks shipping a half-fix; must gate on parse+smoke-ok.

## SHIPPED: finalize-spin SALVAGE (429a69d5a, GOOSE_SWARM_SALVAGE_SPIN default-on)
Fixes the UNIQ9 run-status honesty REGRESSION. When a NON-TEST task terminal-fails via finalize-spin (Looping)
AND >=1 owned file exists non-empty on disk, mark it Done (salvaged), skip fail_descendants, so a dependent
(integrate-verify) still runs and is the real gate. Never salvages test tasks or a Looping-with-no-file (disk
check). Scheduler terminal path. Tests: salvage helpers + judge_terminal_fails_worker_stuck_at_cap stays green
(mock writes no real file -> not salvaged). VALIDATE on UNIQ10: if an entry finalize-spins after writing, the
run should now report honestly (integrate-verify runs) instead of FAILED-by-cascade.

## CLI-STRUCTURE contract (57c892261) — VALIDATED: WORKS (UNIQ12 helpdesk)
UNIQ10 entry DRIFTED (flat group-add + per-command --db -> `--db init` rc2, spec_drift-FAILED). Shipped the
CLI-contract. UNIQ12 entry (cli-entrypoint) built COMPLIANT, VERIFIED BY RUNNING:
  `python -m helpdesk --help` => usage: helpdesk [-h] --db DB {init,agent,ticket,sla} ...  (GLOBAL --db + NESTED
  subcommands agent/ticket groups). `--db test.db init` rc0; `agent add alice` rc0 (NESTED); dup agent rc1
  (clarify honored); `ticket open ... --priority high` rc0; `ticket list` rc0 tabular w/ header (clarify honored);
  invalid priority rc2. entry judge verdicts: 14 ok, 0 spec_drift, recovered from 1 broken_code in-place.
VERDICT: CLI-contract PAYS OFF — converted the entry from UNIQ10 drift-fail to spec-compliant nested/global CLI.
The contract SHIFTED the entry failure mode from structural spec_drift (unrecoverable in 3) to an ordinary
broken_code (recovered in-place). Confidence: HIGH now (validated by running on a fresh complex app). Note: the
title positional takes ONE arg (multi-word needs quotes) — argparse-standard, a golden test-artifact not a bug.
Remaining: full golden (SLA/workload math) on run_finished + wire-vs-inline check (does the entry import+dispatch
sibling handlers or reimplement inline?).

## ASK_REPLAN A/B — VERDICT: skip (reuse plan) = equal quality + ~15min faster -> DEFAULT flipped OFF (2a195ae7f)
Same ASKING helpdesk spec, one flag flipped. UNIQ12 (ASK_REPLAN=1, re-plan): conf 69->88, FULL WIN, ~15min re-plan.
UNIQ12b (ASK_REPLAN=0, skip): conf 78 reused, FULL WIN, went answer->execute with NO re-plan (saved ~15min) AND
executed CLEANER (entry cli-parser 0 spec_drift + 0 broken_code vs UNIQ12 entry 1 broken_code). BOTH golden
all-correct (global --db + nested subcommands; SLA report t1 high BREACHED 1over; workload 1 open avg 0.0; error
exits; UNIQ12b also honored --assignee optional). So the re-plan's confidence boost did NOT yield a better app.
DECISION: flip GOOSE_SWARM_ASK_REPLAN default -> OFF (reuse). Opt in with =1. CONFIDENCE MED: N=1 + a draft-variance
CONFOUND (skip arm reused a higher-conf 78 plan vs re-plan arm's 69 start), so this is evidence-based not proven; a
2nd ASKING-spec pair (ideally same initial skeleton both arms) would strengthen. Knob stays configurable. Net: the
~15min re-plan tax (4x observed) is cut by default; anyone wanting the re-plan sets =1.

### REVIEW phase — scope confirmed on UNIQ15 (catches app-breaking, not subtle edge-validation)
UNIQ15 skeleton-stub entry (8 NotImplementedError handlers, app dead) was CAUGHT + FIXED by the idle-node pre-review
(correctness-checks completed tasks) -> handlers filled, app functional. So REVIEW PAYS OFF on the big class
(unimplemented/app-breaking). BUT the 2 subtle validation gaps (balance <unknown-account> rc0, income-statement
from>to rc0) were NOT fixed by the review (still rc0 after finalization). So the review's correctness-check catches
NON-FUNCTIONAL/unimplemented code but not spec-completeness edge cases (a handler that RUNS but skips one validation
branch looks correct to the reviewer). This is the right scope (catch the app-killers cheaply) — closing every edge
validation gap is a weak-model completeness limit, acceptable per app. NOT a fix target now; a future spec-coverage
reviewer (assert each spec-listed error path exits nonzero) could close it but is lower-value than the app-breaker catch.
