# NEXT-BIG — the ranked path to "more quality, more speed, as nodes are added"
(Workflow wf_cf08c359-feb, Sunday 2026-08-16: live anatomy of the first GREEN n3 run + n1
contrast + engine lever inventory. Everything below is measured, not assumed.)

## Where the node-seconds go today (the green 0.7474 run, 116 min)
- Prologue 25.6% — now mostly 3/3-parallel (scouts, drafts, details); only 290 s fully idle.
- DAG execution 44.7% — average occupancy 2.32/3: full fleet 55% of the window, ONE node for
  the last 11.6 min (the test-task critical path into the join).
- Gates + fix waves 29.7% — the product rails working as designed, with visible overhead.
- Raw occupancy ~80%, BUT ~42% of all node-time either idled or did not convert.

## The ranked non-conversion (what to fix, in order)
1. DEAD FIRST ATTEMPTS — 2,496 node-s (22.6% of DAG busy). The judge now correctly kills
   spec-drifted work, but detection lands at 8–14 min per kill. FIX: earlier drift looks +
   the verdict text injected into the retry (guided, not a blind re-roll).
2. FIX-WAVE LATE CLOSE — both repair waves' winners verified 5–7.5 min before the wave
   closed; two 1,200-s capped twins converted nothing. FIX: first strictly-better twin WINS
   and cancels the rest (early-close).
   → 1+2 together ≈ 27 node-minutes per run, about a fifth of the n3 wall.
3. THE ONE-NODE DAG TAIL — 697 s behind the test task's critical path. FIX: per-case test
   split (S2) or the existing-but-experimental SPECULATE twin racing.
4. PROLOGUE FULL-IDLE — 290 s (merge gap + confidence/contracts stretch). Smaller.

## Why n1 loses (the contrast runs)
One node runs the same prologue serially (~34% of wall), executes independent tasks in
sequence (~600 s reclaimable in detail specs alone), and pays the same gate/fix costs with no
parallel twins — which is why 6 of 8 n1 runs hit the 150-min cap and the fleet finished green.

## Lever truth (code-read, with line evidence in the workflow journal)
- ALREADY ON: SPEC_REPAIR (best-of-N repair race), SINK_SHARD, TAIL_REVIEW.
- OFF, runtime env flips (no rebuild): COMPLETE_PARALLEL (per-file fix fan), TESTGEN,
  SPECULATE (experimental), SUPERVISION_POOL. → test as treatment arms after the verdict.
- NOT BUILT (new code): pipelined prologue / per-contract release. best_of_n_skeletons is
  config-only (no env var).

## The recommendation
Post-verdict engine batch, in order: (1) fix-wave early-close, (2) earlier drift detection +
guided retry, (3) tail speculation or test-split. Then the free lever flips as arms in the
trimmed treatment phase. This attacks the measured 42% non-conversion directly and every item
scales WITH node count — the exact axis Mihai named.
