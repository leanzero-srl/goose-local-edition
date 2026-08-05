# RESUME — live state, rewritten 2026-08-05 ~05:47 local

The previous version opened with *"There is no outstanding obligation. The sweep is running."*
**Both halves are now false.** `STOP` is armed and three committed fixes are waiting on a restart. A
stale resume file sends the next reader to redo finished work and hides the thing that is actually
open, so this is rewritten rather than appended to.

## 🔴 THE ONE OUTSTANDING ACTION — RESTART THE SUPERVISOR

`STOP` was written at **05:40:02**. `sweep.py` checks it at the TOP of its while loop, so the unit in
flight (`baseline-n3-r3`, cell 4) **finishes and records first**, then the loop exits cleanly.
Nothing is discarded — `loop.sh status` says so itself: *"STOP sentinel present — it will exit after
the current unit."*

```bash
cd ~/Projects/goose/evals/swarm-bench/nodeloop
./loop.sh status                  # wait for NOT RUNNING
rm STOP && ./loop.sh start        # BARE, never piped (F298: a pipeline's exit code is not the command's)
./loop.sh status                  # expect RUNNING, a NEW pid, and NOW: baseline-n1-r0
pgrep -f 'goose swarm run' | wc -l   # must read 1 before and after
```

This is a **SWEEP restart, not an engine boundary.** The binary is untouched, no collected cell is
voided, F253 is not in play.

**Three fixes activate on restart** — all three are invisible to pid 80288, which holds the code as
it was at launch (L23):

| fix | what it does |
|---|---|
| `curve_first()` (F328) | the 1-node arm was **STARVED** — `n1-r0` sat at backlog index 1 forever |
| `CURVE_REPS = 8` (F327) | registered blind; scoped to baseline n3/n1 only |
| watchdog silence rule (F330) | the old rule would have **killed a healthy 3-discard cell** |

**After the restart the next three units are `n1-r0`, `n1-r1`, `n1-r2`.** Their n3 partners are
already complete, so **three matched pairs close in three units.** Then run
`python3 curve.py` with an ABSOLUTE path — never compute p by hand.

## 🧊 The engine is still frozen (F253)

`complete()` keys on `engine_build`, so any boundary mid-curve voids every cell collected. Do not
rebuild, do not `cargo check`. **Reading source is free. Instrument fixes ship freely.**

**THREE ENGINE PATCHES REMAIN QUEUED AND UNCOMPILED** — `f1a20c99b`, `95b36748f`, `00563c6ea`. The
next boundary MUST run `cargo clippy --all-targets -- -D warnings` BEFORE deploying (L108: `cargo
build` skips `#[cfg(test)]`).

## 🎯 GOAL ONE — the node curve

| cell | unit | wall | score | prefix (redraft) |
|---|---|---|---|---|
| 1 | `baseline-n3-r0` | 7729.3 | 0.6595 | 2218.7 (1, REVERT) |
| 2 | `baseline-n3-r1` | 8488.0 | 0.4780 | 1330.0 (0) |
| 3 | `baseline-n3-r2` | 6752.6 | 0.6030 | 1316.0 (0) |
| 4 | `baseline-n3-r3` | in flight | — | 2882.7 (2, REVERT), accepted conf **61** vs floor 85 |

**No 1-node cell has ever been scored** — `sweep.read_results()` returns zero rows with `nodes == 1`.
That is the whole reason the restart matters.

`power.py` sizes the question: the 3-node replicate spread is **mean 0.5802, sd 0.0929, range 31% of
the mean**. For 5 pairs the 1-node arm would have to score about **0.432** against 0.580 for a
coin-flip chance of significance. Wall-clock is the safe arm; **score is the binding constraint.**

## 🔴 The three things a new reader is most likely to get wrong

**1. F325's HEADLINE IS WITHDRAWN.** It claimed the engine had "decisively beaten the null" at
p = 3.7e-05 on 6-of-7 clean runs. **Four of those seven runs are the same run** — `loop.sh start`
parks the tree with `cp -R` on every start, and the fresh mtimes let the copies pass the
`binary_mtime()` scope check that correctly excluded the original. On 3 distinct runs: task-level
**p = 0.1442 (not significant)**, run-clustered **p = 0.0343 (marginal)**. The supportable claim is
*"appears to have improved, p ≈ 0.03 on three runs"* (F329).

**2. THE TWO PLAN EVENTS SPELL THE SAME FIELD DIFFERENTLY.** `plan_loaded.tasks[].files` vs
`retarget_discarded.tasks[].owned_files`. Reading `owned_files` on both returns None for every
accepted task and reports that the accepted plan owns nothing. `planshape.owned()` is the only place
either key is read (F321).

**3. `retarget_discarded` MEANS "SET ASIDE", NOT "THROWN AWAY".** `retarget_stall_guard` +
`best_plan` (`swarm.rs:22885`) ship it back, and **2 of 4 redrafting runs end in exactly that
revert**. A revert also scores the same plan twice, which double-counts it in any pairing (F324/F326).

## Findings a new reader most needs

- **F330** — the watchdog's `elapsed > 3600` rung sits at conf 0.85, ABOVE the 0.8 abandon line,
  despite its own comment claiming otherwise. It now measures **silence since the last planning
  event**, not total duration. Controls both ways.
- **F327** — the sign test's bar is a **sawtooth in n**: 6 and 7 pairs are HARDER than 5, because
  below n=8 a single crossing kills the result outright. n=8 is the first n that absorbs one loss.
- **F323** — `plan_loaded` → first `task_dispatched` is **0.0 s in 7 of 7**. The prefix IS the plan.
- **F303/F322** — the redraft/no-redraft prefix split is clean among **3-node** runs only;
  `swarm-1node-r0` reaches 2031.3 s with zero discards.
- **F304/F305** — 8 of 8 built apps hardcode `127.0.0.1:89xx` and none read `MERIDIAN_BASE_URL`.
  Re-score an archived tree only on its OWN baked-in port, only between units.

## Instruments (never re-implement one — L2)

`curve.py` verdict · `power.py` feasibility · `planshape.py` plan shape + reuse · `bonusclass.py`
replan bonus class · `occupancy.py` · `dispatch_audit.py` · `reaudit.py` (in-place row migration —
use whenever `audit_version` goes stale) · `goalstate.py --tick` · `review.py` · `failures.py` ·
`sweep.py` · `loop.sh {status,stop,start,boundary,check,selftest}` · `promptbench.py` (needs the
fleet — never during a measured cell).

Every one of them has a `--self-test`. Run it before trusting a number.

⚠ `occupancy.py` / `dispatch_audit.py` / `curve.py` need an **ABSOLUTE** path.
⚠ `git add` must run from the **repo root**.
⚠ `cd` drifts between Bash calls — re-`cd` at the start of every command, and make every glob print
how many files it opened (L174).

## Fleet

3 LM Studio nodes, `PARALLEL 2` ⇒ **6 slots**. **NEVER load, unload or re-alias anything in LM
Studio.** If the engine cannot use three identical nodes, that is a `swarm.rs` bug, not a fleet
question.
