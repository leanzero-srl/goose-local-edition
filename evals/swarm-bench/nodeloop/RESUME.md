# RESUME — live state, rewritten 2026-08-05 ~06:42 local

The previous version led with *"THE ONE OUTSTANDING ACTION — restart the supervisor"*. **That is
done, and it was done by a process, not by hand.** This file is rewritten rather than appended to,
because a resume file that describes a finished obligation sends the next reader to redo it.

## ✅ There is no outstanding obligation. The n1 arm is finally running.

```bash
cd ~/Projects/goose/evals/swarm-bench/nodeloop
./loop.sh status          # expect: RUNNING pid=91810
```

`autorestart.sh` fired unattended at **06:27:01** — *"supervisor down, 0 engines — restarting"* — and
the sweep came back as **pid 91810 (ppid 1)** running **`baseline-n1-r0`**, with `NEXT:
baseline-n1-r1`. It has since exited; its job is done. If the loop ever stops again, restart by hand:
`rm -f STOP && ./loop.sh start` — **bare, never piped** (F298).

**Three fixes activated on that restart** and are now live: `curve_first()` (F328), `CURVE_REPS = 8`
(F327), and the watchdog's silence rule (F330).

## 🎯 GOAL ONE — four n3 cells down, the n1 arm live

| cell | unit | wall | score | occupancy | tier B | prefix (redraft) | replan bonus |
|---|---|---|---|---|---|---|---|
| 1 | `baseline-n3-r0` | 7729.3 | 0.6595 | 0.5645 | .3194 | 2218.7 (1) | +2 TEST-ONLY |
| 2 | `baseline-n3-r1` | 8488.0 | 0.4780 | 0.4737 | .2083 | 1330.0 (0) | +2 TEST-ONLY |
| 3 | `baseline-n3-r2` | 6752.6 | 0.6030 | 0.6499 | .3194 | 1316.0 (0) | +4 TEST-ONLY |
| 4 | `baseline-n3-r3` | 7302.6 | **0.8157** | 0.2582 | **.875** | 2882.7 (2, REVERT, conf 61 vs floor 85) | +2 APP-SIDE |

n3 score mean **0.6641**, sd **0.1401**. **Cell 4 is the best cell by a wide margin — and it came
from the run with the longest prefix, two discards, a revert, and the lowest accepted plan
confidence of any cell.**

**THE MOMENT THE FIRST n1 CELL LANDS:**

```bash
python3 /Users/mihaiperdum/Projects/goose/evals/swarm-bench/nodeloop/curve.py   # ABSOLUTE path
```

Never compute p by hand. `curve.py` now prints the replan-bonus confound beside the verdict (F332),
because the 1-node arm **cannot replan by construction** (F312) and a bare score win would read as
"3 nodes build better apps" when part of the gap is "3 nodes were allowed to build more of the app".

Also run, per landed cell: `bonusclass.py` · `planshape.py` · `power.py` · `occupancy.py`.
**All six instruments pass `--self-test`.**

## 🧊 The engine is still frozen (F253)

`complete()` keys on `engine_build`, so any boundary mid-curve voids every cell collected. Do not
rebuild, do not `cargo check`. **Reading source is free. Instrument fixes ship freely.**

**THREE ENGINE PATCHES REMAIN QUEUED AND UNCOMPILED** — `f1a20c99b`, `95b36748f`, `00563c6ea`. The
next boundary MUST run `cargo clippy --all-targets -- -D warnings` FIRST (L108).

## 🔴 The four things a reader arriving cold is most likely to get wrong

**1. THE PARK USED TO DESTROY EVIDENCE, AND IT ATE THE OLD 1-NODE LOG (F338).** `loop.sh start`
parked with `cp -R "$RUNDIR"/*/ "$PARK/"` — the trailing `/*/` copies directory *contents*, so twelve
unit dirs collapsed into one park. `swarm-1node-r0`'s original log (`run_id
swarm-20260803-100147948`) is **gone**. Fixed to `cp -R "$RUNDIR" "$PARK"`, which now also prints
`(N of M run logs)` and shouts on a mismatch. **This was also F329's root cause** — I fixed the
metric that noticed the duplicates and never asked why they existed.

**2. TWO EVENTS SPELL THE SAME FIELD DIFFERENTLY.** `plan_loaded.tasks[].files` vs
`retarget_discarded.tasks[].owned_files`. Reading `owned_files` on both reports that every accepted
plan owns nothing. `planshape.owned()` is the only place either key is read (F321).

**3. A SPLIT PARENT NEVER COMPLETES (F334).** `task_split` children emit `task_completed`; the parent
never does. Any in-flight count built from `dispatched − completed` reports every split as a
permanent hang. **`occupancy.py` models this correctly — use it rather than hand-rolling.**

**4. `run.jsonl` SILENCE IS NOT WORKER IDLENESS (F336).** A worker emitting tokens writes no events
until it finishes. Never infer a stall from event absence.

## What the four n3 cells actually say

- **The fleet delivers ~1.7 of its 3 nodes.** `occupancy.py` mean 0.563 against a perfect 1.0 and a
  one-node floor of 0.333; six concurrent slots exist and time-at-six is 13.8% / 6.4% / 0.8%.
  ⚠ The overall figure divides by the whole wall while the prefix emits no task events, so it
  understates — the EXECUTE column (0.8568 / 0.5746 / 0.8139 / 0.5910) is the honest one.
- **Occupancy does NOT govern wall-clock.** F333 reported a perfect inverse ordering on three cells
  and labelled it a direction at P(luck) = 0.167; **cell 4 broke it** — lowest occupancy, second
  shortest wall, best score (F337).
- **The redraft ladder is not waste.** It gained in 2 of 4 runs (79→100, 41→68→88) and reverted in 2.
  `retarget_discarded` means *set aside*, not thrown away — `best_plan` ships it back (F324).
- **A reuse cache is not worth building.** Genuine redrafts reuse 15.2% of accepted tasks; the
  measurement `swarm.rs:22967-22980` asks for by name is taken and the answer is no (F321/F324).
- **2 of 8 cells ship below `ask_floor`.** The floor is advisory at the end of the ladder, not a gate
  that can refuse (F326).

## Instruments (never re-implement one — L2, violated three times this session)

`curve.py` verdict · `power.py` feasibility · `planshape.py` plan shape + reuse · `bonusclass.py`
replan bonus class · `occupancy.py` node-seconds · `dispatch_audit.py` · `reaudit.py` (in-place row
migration — use whenever `audit_version` goes stale) · `goalstate.py --tick` · `review.py` ·
`failures.py` · `sweep.py` · `loop.sh {status,stop,start,boundary,check,selftest}` ·
`autorestart.sh` · `promptbench.py` (needs the fleet — never during a measured cell).

⚠ `occupancy.py` / `dispatch_audit.py` / `curve.py` need an **ABSOLUTE** path.
⚠ `git add` must run from the **repo root**.
⚠ `cd` drifts between Bash calls — re-`cd` every command, and make every glob print how many files it
opened (L174).
⚠ `<(...)` process substitution is unreliable in this Bash tool — materialise to a file (F331).

## Fleet

3 LM Studio nodes, `PARALLEL 2` ⇒ **6 slots**. **NEVER load, unload or re-alias anything in LM
Studio.** If the engine cannot use three identical nodes, that is a `swarm.rs` bug, not a fleet
question.
