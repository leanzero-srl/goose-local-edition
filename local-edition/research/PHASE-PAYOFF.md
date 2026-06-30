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
RUN 2 (SKELETON_FIRST=0, ABskeloff): RUNNING (b0lp2t2tq, same spec, same flags but skeleton OFF).
TRADEOFF VISIBLE: skeleton-first -> 0 over_read + 0 retries (no false-kill) BUT 23 tool calls (MANY round-trips
= more wall-clock on a slow 27B). On THIS moderate app it prevented NOTHING (0 retries with the flag on), so the
23 round-trips look like pure overhead. VERDICT pending run 2: if run 2 (OFF) is also ~0 over_read / 0 retries
with SIMILAR quality + FEWER round-trips + similar-or-less time -> skeleton-first is OVERHEAD on simple apps ->
keep default-OFF, complexity-gate it. If run 2 has over_read kills + retries run 1 avoided -> it earned its keep.
