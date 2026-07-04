# Full-stack campaign verdict (2 specs x ON/OFF, 2026-07-04)
All mature flags ON (GOALS+COMPLETE+COMPLETE_PARALLEL+REVIEW+REVIEW_FANOUT+REVIEW_VERIFY+SMOKE+SPLIT+PREREVIEW+CONTRACTS) vs an OFF baseline. Verify-by-running each app + judged survivors against the code.

| spec | arm | exit | pytest | total | research | planning | execute | gates | verify c/s/r |
|---|---|---|---|---|---|---|---|---|---|
| invmate | ON | 0 | green | 48.2 | 2.0 | 18.9 | 16.2 | 11.1 | 1/1/0 |
| invmate | OFF | 0 | green | 48.2 | 2.0 | 9.3 | 36.9 | 0 | - |
| habiteer | ON | 0 | green | 50.3 | 2.0 | 17.8 | 19.2 | 11.4 | 1/0/1 |
| habiteer | OFF | 0 | green | 66.9 | 2.0 | 20.2 | 44.7 | 0 | - |

## 1. COMPOSE — PASS. All 4 runs delivered GREEN (exit 0, pytest pass). Turning the full feature stack ON regressed NOTHING vs OFF.

## 2. WALL-CLOCK — ON is NEUTRAL-to-FASTER (not a cost).
- invmate: ON 48.2 == OFF 48.2 (tie). habiteer: ON 50.3 vs OFF 66.9 => ON 16.6 min FASTER.
- MECHANISM (consistent across both specs): the ON structure (CONTRACTS freeze signatures + PILLARS anchor the interface) makes EXECUTE ~2-2.4x FASTER — invmate exec 16.2 (ON) vs 36.9 (OFF); habiteer 19.2 (ON) vs 44.7 (OFF). That speedup OFFSETS the extra planning (+~9m) + the COMPLETE/REVIEW gates (+~11m). So the "overhead" features PAY FOR THEMSELVES by making weak workers converge faster instead of thrashing. n=2 specs, execute stochastic, but the direction is clear + repeated.

## 3. VERIFY (adversarial verify-before-accept) — PROVEN BALANCED BOTH DIRECTIONS (judged by RUNNING).
- invmate-on 1/1/0: CONFIRMED a REAL defect the tests missed — load_store crashes on a non-dict JSON db (reproduced: AttributeError 'list' has no attribute 'items', storage.py:14). Correct CONFIRM.
- habiteer-on 1/0/1: REFUTED a FALSE finding — claimed `report rate --days 0` div-by-zero, but the app returns 0.0% (no crash). Correct REFUTE.
=> The recalibrated verifier (135c7ee71) confirms real, locatable defects AND refutes false positives. Only trustworthy findings survive. This is Claude Code's adversarial-review pattern (one writes, another refutes; survivors trusted), working end-to-end on the weak local fleet.

## 4. PER-PHASE UTILIZATION — parallel phases saturate; the SERIAL TAIL is single-node (user's 'one node crunching', confirmed).
- Every run PEAKED at 3 nodes; the parallel phases (research scouts, plan-detailing, contracts, review-fanout, verify) genuinely use the whole fleet.
- The ON runs carry MORE active=1 time than OFF (invmate-on 135 vs off 69 single-node samples) — that extra single-node time IS the added serial gates (COMPLETE fix loop + the serial parts of REVIEW). i.e. the feature stack's cost shows up as single-node TAIL phases.
- The integrate-verify SINK + the COMPLETE fix are inherently single-node (one integrator, one fix). One fix went pathologically slow (~35m, actively generating, no writes) and the IDLE-based timeout never caught it.

## 5. NEXT (the real throughput frontier, from the data + the user's observation)
- (a) HARD wall-clock cap on serial fix agents (idle timeout misses active over-generation). Small/safe.
- (b) SINK/FIX IDLE-FILL: put the 2 idle nodes to useful work during the serial sink/fix (the only large single-node windows left). Design + adversarial-review first.
- (c) REVIEW Phase 2b: the confirmed survivors (invmate load_store crash) are REAL — route survivors through a re-verified shadow-isolated fix.
