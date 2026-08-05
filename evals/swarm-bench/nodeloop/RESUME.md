# RESUME — live state, rewritten 2026-08-05 ~07:50 local

**The previous version led with the measurement campaign and told the reader to protect the engine
freeze. Mihai corrected that posture directly, and this file is rewritten rather than appended to,
because a resume file that states a superseded priority sends the next reader the wrong way.**

## 🔴 THE POSTURE CHANGED. READ THIS BEFORE ANYTHING ELSE.

> *"in the end the main goal is to make the 3 node swarm or rather swarm actually work better than a
> 1 node, which means you need to make **fixes and improvements in the engine**."* — Mihai, 07:45

I had been protecting a measurement at the cost of the goal. The 8-pair curve answers *whether*
three nodes win. The job is to **make** them win.

- **The F253 engine freeze is SUPERSEDED.** It existed so cells stay comparable. Comparability is
  worth less than a working swarm. **Finish the in-flight cell** (never waste a live 2h build),
  **then take the boundary, ship engine fixes, re-baseline.** A voided curve is an acceptable price.
- **n = 8 (F327) is SUPERSEDED, not fudged.** It was registered blind for a *verdict* experiment
  that is no longer the priority. Say that plainly; never quietly shrink a pre-registered n.
- **The boundary MUST run `cargo clippy --all-targets -- -D warnings` FIRST** (L108/L119). Three
  patches are queued and uncompiled: `f1a20c99b`, `95b36748f`, `00563c6ea`.

## 🎯 The target the measuring bought

`occupancy.py`'s plan ceiling, `max_useful_nodes = total_work / critical_path`:

| cell | critical path | total work | max useful | attainable occ | ACTUAL occ |
|---|---|---|---|---|---|
| `baseline-n3-r0` | 3827.4s | 19314.6s | **5.05** | 1.0 | 0.5645 |
| `baseline-n3-r1` | 6906.0s | 18203.9s | 2.64 | 0.8787 | 0.4737 |
| `baseline-n3-r2` | 3353.2s | 16006.3s | **4.77** | 1.0 | 0.6499 |
| `baseline-n3-r3` | 1767.8s | 8406.6s | **4.76** | 1.0 | 0.2582 |

Three of four plans afford ~4.8-5 nodes against a pool of 3. Time-at-six-concurrent is
13.8% / 6.4% / 0.8%. A **one**-node run of the same spec reaches EXECUTE occupancy **1.0**.

**⚠ THE PREMISE IS NOT YET CLEARED.** These runs also fired judge 103 / 72 / 64 / 43 and pre_review
7 / 12 / 12 / 9. `occupancy.py` counts busy node-seconds from `task_dispatched`/`task_completed`
**only**. If judge and pre_review occupy a device, the fleet was working and the number is measuring
my own blind spot (L4). **A scheduler fix built on an uncorrected occupancy figure is a fix for a bug
that may not exist.** Workflow `wf_94b83a28-e0e` runs this falsifier FIRST, before any finder.

**⚠ AND THE RATIO IS BIASED UPWARD.** `longest_path()` does `plan_deps.setdefault(tid, [])`, so any
task absent from `plan_loaded` becomes a dependency-free root. r0 4/20 · r1 2/22 · r2 4/21 · **r3
7/19, including `http-api-server`, `meridian-client`, `frontend-page` — not test tasks.** r3's 4.76
is the least trustworthy number in the table. **Instrument fix queued: read the accepted `best_plan`,
not the last `plan_loaded`.** (L197)

**What survives both caveats:** `baseline-n3-r1` has only 2 extras, is the one number *below* the
pool (2.64), and is the **worst cell** — score 0.4780, longest wall 8488.0s, zero redrafts, shortest
prefix 1330.0s. Cheapest planning, narrowest DAG, worst app. And its own attainable 0.8787 against an
actual 0.4737 is a gap plan width does not explain.

## What is running right now

```bash
cd ~/Projects/goose/evals/swarm-bench/nodeloop
./loop.sh check           # expect OK, loop pid 91810, engine pid 91813
```

`baseline-n1-r0` is in EXECUTE (prefix closed 2925.8s). It is the campaign's first 1-node cell and
closes the first matched pair — **let it finish**, then take the boundary. If the loop stops:
`rm -f STOP && ./loop.sh start` — bare, never piped (F298).

Workflow `wf_94b83a28-e0e` (`/workflows` to watch) is doing a read-only scheduler investigation:
premise falsifier → 4 finder lenses (capacity accounting, ready-task loop, forced serialisation,
artificial dependencies) → adversarial refutation of every finding → a work order ranked by
confidence-that-it-is-correct.

## Cells collected (baseline arm, frozen engine)

| cell | unit | wall | score | occ | tier B | prefix (redraft) | replan bonus |
|---|---|---|---|---|---|---|---|
| 1 | `baseline-n3-r0` | 7729.3 | 0.6595 | 0.5645 | .3194 | 2218.7 (1) | +2 TEST-ONLY |
| 2 | `baseline-n3-r1` | 8488.0 | 0.4780 | 0.4737 | .2083 | 1330.0 (0) | +2 TEST-ONLY |
| 3 | `baseline-n3-r2` | 6752.6 | 0.6030 | 0.6499 | .3194 | 1316.0 (0) | +4 TEST-ONLY |
| 4 | `baseline-n3-r3` | 7302.6 | **0.8157** | 0.2582 | **.875** | 2882.7 (2, REVERT, conf 61) | +2 APP-SIDE |
| — | `baseline-n1-r0` | running | — | exec **1.0** | — | 2925.8 (1, conf 60→100) | cannot replan |

n3 score mean **0.6641**, sd **0.1401**. These stay valid as a *baseline to beat* even though the
curve will not reach n=8 — that is the point of re-baselining after the fixes land.

## The mechanism results worth keeping (they are why the fixes have a target)

- **Planning scales with the fleet, measured twice (F343/F345).** Detailing costs 112.4 s/task on one
  node vs 60.2 on three in the discarded round, and 50.9 vs 27.2 in the accepted round — **the same
  1.87 from two independent windows in the same runs.** The skeleton vote does NOT scale and does not
  need to (232/214 vs 222/331 vs 226/296/238, flat), because a 2-3 draft vote clears in one wave on
  2 slots or 6. **The one-node arm serialises the fan, not the vote.**
- **A reuse cache is not worth building (F321/F324).** Genuine redrafts reuse 15.2% of accepted tasks.
  The measurement `swarm.rs:22967-22980` asks for by name is taken; the answer is no.
- **`retarget_discarded` means SET ASIDE, not thrown away** — `best_plan` ships it back, and 2 of 4
  redrafting runs end in a REVERT.
- **The prefix IS the plan (F323).** `plan_loaded` → first `task_dispatched` is 0.0s in 8 of 8.

## The four things a reader arriving cold is most likely to get wrong

**1. THE PARK USED TO DESTROY EVIDENCE (F338).** `cp -R "$RUNDIR"/*/ "$PARK/"` copies directory
*contents*, so twelve unit dirs collapsed into one park and `swarm-1node-r0`'s original log is gone.
Fixed to `cp -R "$RUNDIR" "$PARK"`. This was also F329's root cause — I fixed the metric that noticed
the duplicates and never asked why they existed.

**2. TWO EVENTS SPELL THE SAME FIELD DIFFERENTLY.** `plan_loaded.tasks[].files` vs
`retarget_discarded.tasks[].owned_files`. `planshape.owned()` is the only place either is read.

**3. A SPLIT PARENT NEVER COMPLETES (F334).** Any in-flight count built from `dispatched − completed`
reports every split as a permanent hang. `occupancy.py` models this correctly — use it.

**4. `run.jsonl` SILENCE IS NOT WORKER IDLENESS (F336).** A worker emitting tokens writes no events
until it finishes. Never infer a stall from event absence.

## Instruments (never re-implement one — L2, violated three times this session)

`curve.py` verdict · `power.py` feasibility · `planshape.py` plan shape + reuse · `bonusclass.py`
replan bonus class · `occupancy.py` node-seconds + plan ceiling · `dispatch_audit.py` · `reaudit.py`
· `goalstate.py --tick` · `review.py` · `failures.py` · `sweep.py` ·
`loop.sh {status,stop,start,boundary,check,selftest}` · `autorestart.sh` · `promptbench.py`.

⚠ `occupancy.py` / `dispatch_audit.py` / `curve.py` need an **ABSOLUTE** path.
⚠ `git add` must run from the **repo root**.
⚠ `cd` drifts between Bash calls — re-`cd` every command, and make every glob print how many files it
opened (L174).
⚠ `<(...)` process substitution is unreliable in this Bash tool — materialise to a file (F331).
⚠ `grep -c` exits 1 on zero matches.

## Fleet

3 LM Studio nodes, `PARALLEL 2` ⇒ **6 slots**. **NEVER load, unload or re-alias anything in LM
Studio.** If the engine cannot use three identical nodes, that is a `swarm.rs` bug, not a fleet
question — and per the pivot above, fixing that bug is now the whole job.
