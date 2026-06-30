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
RUN 1 (SKELETON_FIRST=1, ABskelon): single cli-entrypoint task (NOT split — confound removed).
  cli metrics: over_read 0, judge 3x ok, tool_calls 23 (incremental write+fill+shell-checks), errors 0,
  cli.py 4043b, package layout correct. [quality + phase-time pending completion]
RUN 2 (SKELETON_FIRST=0, ABskeloff): [pending — launch after run 1]
TRADEOFF VISIBLE: skeleton-first -> 0 over_read (no false-kill) BUT 23 tool calls (MANY round-trips = more
wall-clock on a slow 27B). The verdict needs run 2: does skeleton-OFF trade fewer round-trips for more
over_read? Net = time + quality. HONEST: not yet decided.
