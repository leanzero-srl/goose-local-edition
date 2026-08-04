# RESUME — live state, rewritten 2026-08-05 ~01:05 local

The previous version of this file described a park from 2026-08-03 and said *"seven engine changes
committed, NONE yet verified"*. **All seven are now verified on the wire (F277), the node curve is
running, and there is a live obligation below.** A stale resume file is worse than none: it tells the
next reader to redo work that is done and hides the one thing that is not.

## 🔔 THE ONE THING THAT MUST HAPPEN NEXT — CLEAR `STOP` AND RESTART

```bash
cd ~/Projects/goose/evals/swarm-bench/nodeloop
./loop.sh status                 # while it says RUNNING, wait — the current cell is being kept
rm STOP && ./loop.sh start       # the MOMENT it is no longer RUNNING
```

`./loop.sh stop` was issued deliberately (F280). `sweep.py:1421` checks the sentinel at the **top of
the unit loop**, so `baseline-n3-r1` runs to completion and its result is kept; the supervisor then
exits cleanly. The restart reloads **`MIN_REPS=5`** and the F254 watchdog fix.

**Why it cannot be skipped:** the running supervisor (pid 22764) holds `MIN_REPS=3` in memory (L23).
Backlog positions 1-6 are identical under either target, but **from position 7 a target-3 supervisor
walks off to other arms and the curve stops at n=3 — and F260 proved n=3 can never clear 0.05 (smallest
attainable p = 0.125).** ⚠ **A STOP sentinel nobody clears is a stopped campaign, which is exactly the
state the previous resume was written from.**

## 🧊 THE ENGINE IS FROZEN (F253)

`complete()` keys on `engine_build`, so **any boundary mid-curve voids every cell already collected.**
Do not rebuild, do not `cargo check` (it also steals CPU from `local-mihai`). Reading source is free.
Instrument fixes ship freely — `reaudit.py` means an `AUDIT_VERSION` bump costs **zero** unit re-runs.

**THREE ENGINE PATCHES ARE QUEUED AND UNCOMPILED.** `cargo fmt` is clean; nothing has type-checked.
**The next boundary MUST run `cargo clippy --all-targets -- -D warnings` BEFORE deploying** (L108:
`cargo build` skips `#[cfg(test)]` — that is how 45 lib tests went dark for sessions).

| commit | what | expected effect |
|---|---|---|
| `f1a20c99b` | scouts gated on `straggler_stop_degrade`, not `straggler_stop` | stop discarding 1 of 3 research lenses (F256) |
| `95b36748f` | `rules_delivered` also emits `rules_sections` | makes kind-mismatch measurable again (F259) |
| `00563c6ea` | planner gets Σ device weights, not `devices.len()` | plan width targets **6 slots**, not 3 devices (F269-F271) |

## 🎯 GOAL ONE — the node curve

**Claim:** a 3-node run beats a 1-node run on **BOTH** wall-clock **AND** shipped quality.
**Protocol is frozen in `PREREGISTERED.md`; the verdict is mechanical — run `python3 curve.py`, never
compute p by hand.**

    CELL 1 (finished, clean)  baseline-n3-r0
      wall 7725.4 s · score 0.6595 · 3/3 nodes · prefix 2218.7 s · EXECUTE 5089.6 s @ 0.8568
      mean concurrency 3.792 vs plan ceiling 5.046 · total_task_secs 19314.6 · critical path 3827.4 s
    NOW   baseline-n3-r1     NEXT  baseline-n1-r0  <- the first matched pair

**Two predictions are on record and they DISAGREE (F281). Do not widen either.**
`F261` registered `n1_wall / n3_wall ∈ [1.6, 2.4]` from a partial read that F273 later proved biased;
the finished-cell decomposition gives **1.51**. ≈1.5 ⇒ the band was wrong · 1.6-2.4 ⇒ the decomposition
is missing something · outside 1.4-2.4 ⇒ both wrong.

## The findings a new reader most needs

- **F273** — the partial read of a run is **biased, not just noisy**: unfinished said "no scheduling
  slack", finished says **24.9% below the plan ceiling**. Tails and failures land at the END.
- **F274** — the biggest task and the entire serial tail were the **same task, and it FAILED**
  (24.6% of all node-busy, 3 dispatches, nothing produced).
- **F276** — a repeat `over_reading` cannot escalate, but **the fix I proposed was banned by a comment
  at the fix site**. The real gap: "did nothing" has a deterministic backstop, "acted a lot and
  produced nothing" does not.
- **F262** — the 1-node arm's `plan_confidence` is **`null`**, so it skips a quality gate it cannot
  compute. An arm that cannot compute a check is not passing it.
- **F272** — mini-goal 2 was **revoked by its own pre-registered rule** (10 attempted / 3 failed,
  p = 1.000). **RESOLVED = ONE: F207, weights routing.**

## Instruments (never re-implement one — L2, violated this session in F266)

`curve.py` verdict · `occupancy.py` occ-3 (concurrency histogram + prefix phases) · `dispatch_audit.py`
da-3 · `reaudit.py` in-place row migration · `goalstate.py --tick` · `review.py` · `failures.py` ·
`sweep.py` · `loop.sh {status,stop,start,boundary}` · `promptbench.py` (needs the fleet — **never run it
during a measured cell**).

⚠ `occupancy.py` / `dispatch_audit.py` / `curve.py` need an **ABSOLUTE** path; a relative one throws and
`| head` swallows the exit code.

## Fleet

3 LM Studio nodes, `PARALLEL 2` ⇒ **6 slots**. **NEVER load, unload or re-alias anything in LM Studio.**
If the engine cannot use three identical nodes, that is a `swarm.rs` bug, not a fleet question.
