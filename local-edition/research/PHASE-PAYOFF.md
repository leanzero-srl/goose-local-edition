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
