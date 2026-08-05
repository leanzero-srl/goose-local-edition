# RESUME — live state, rewritten 2026-08-05 ~09:20 local

Third rewrite today, and each one was because the target moved: 07:50 named F346 as the engine
target, 08:02 replaced it after F348 killed F346, and this one records what actually shipped. A
resume file that describes a superseded target sends the next reader to build the wrong thing.

## 🛑 THE ONE BLOCKER: the fleet is empty

At **08:03:59** all three LM Studio nodes went from GENERATING to **no models loaded**
(`fleet-samples.tsv`, independent of the event log). LM Link still shows both remote devices
**connected**. Every unit launched after that returns in ~0.2 s with score 0.0, `actual_pool: None`
and no run log.

**Do not touch LM Studio** — that is a standing rule from Mihai. One `lms load` per node restores it,
and it is his call. The sweep is **stopped** (`STOP` armed 08:07). Restart only once `lms ps` shows
models:

```bash
cd ~/Projects/goose/evals/swarm-bench/nodeloop
~/.lmstudio/bin/lms ps          # must list models before anything below
rm -f STOP && ./loop.sh start   # BARE, never piped (F298)
./loop.sh check                 # now BAD-fails on units that never started (F349)
```

## 🔴 The posture (Mihai, 07:45)

> *"the main goal is to make the 3 node swarm actually work better than a 1 node, which means you
> need to make **fixes and improvements in the engine**."*

Measuring is subordinate to shipping engine fixes. The F253 freeze is superseded, and the
pre-registered n = 8 (F327) is **superseded, not fudged** — it was registered for a verdict
experiment that is no longer the priority.

## ✅ What shipped today. All clippy-green. All unmeasured.

| commit | what |
|---|---|
| *(boundary 08:15)* | `f1a20c99b`, `95b36748f`, `00563c6ea` finally compiled. clippy 0, release 0. **`engine_build` changed — intended.** |
| `e26f26869` | **F350** — planning fan-outs count SLOTS, not devices |
| `ec32f9e2f` | **F351** — `pre_review` emits `secs` |
| `23813603e` | **F352** — `judge_verdict` carries `judge_node` |
| `d09eaa39e` | **F353** — `occupancy.py` occ-5 reads both new fields |

**F350.** `fanout_over_fleet` sized its permits `Semaphore::new(devices.len())` while all five callers
did `.map(|d| d.model_id.clone())`, discarding `weight`. EXECUTE admits while `in_flight < weight`
(baked 2), so the fleet ran 6 concurrent in EXECUTE and 3 in every planning fan. The docstring
already claimed the bound was *"the per-device capacity the EXECUTE scheduler already honors"* — the
comment was right and the code never implemented it. Measured: the detail fan sits at concurrency
{1: 34.3s, 2: 95.7s, 3: 112.4s} and **never sustains 4** in any 3-node cell, and `swarm-1node-r0`
detailed 17 items **strictly serially — 1743.1 s of a 5842.9 s run — on a weight-2 device**.

⚠ **It will not move EXECUTE occupancy.** Those events only exist after planning. The payoff is
pre-execute wall-clock, 16-39% of a run, and F179 measured 301 s/call vs 63 s/call when two jobs
share a node, so the real gain is well under 2×.

Two scope calls made from the code rather than the report: the **skeleton draft vote is untouched**
(`swarm.rs:12665` records duplicates measured dead — *"6 slots requested, EXACTLY 3 survived… Dedup
is the fix; a length cap can never be"* — and its `HashSet` makes the vote immune to expansion anyway;
a test pins the width at 4), and **`fleet_models` stays distinct** because it also sizes
`spec_repair`'s attempt list, where expansion would double the repair race from 3 to 6.

**F351/F352** exist because the two idle-node jobs that hold real fleet slots were unmeasurable:
`pre_review` emitted no start or duration while a single call can hold a slot for **900 s**, and
`judge_verdict.device` reported the **judged worker's** node rather than the judging one, so judge
load could not be attributed at all.

## ⛔ Do NOT ship the selection fix yet

`scheduler.rs:1099` and `:1220` select the idle-job device with
`position(|d| d.cfg.enabled && d.in_flight < d.cfg.weight)` — the **first** device with any free slot
— while `pick_device` at `:592-600` deliberately sorts by `in_flight`. **The mechanism built to fill
idle nodes does not target idle nodes.** Simulated cost: judge work adds +0.062/+0.064/+0.069/+0.110
to device occupancy today against +0.143/+0.156/+0.186/+0.198 with correct placement.

**Fixing it before a run carries `judge_node` destroys the only chance to confirm it from evidence**
— the first post-fix run would show an even spread and the defect would stay a simulation of mine
forever (L91/L202). **Measure the skew first, then fix.**

## 📌 The pre-registered predictions, which the next run settles

- **F351** — 7-12 `pre_review` events with `secs` in **100-250 s**. Under ~20 s means F348's ~0.30
  idle-slot estimate is too high, and that figure is what killed F346.
- **F352** — `judge_node` non-empty on **40-75%** of verdicts. All-empty means the field is not being
  set; all-non-empty means the deterministic-only path is not being distinguished. **If the non-empty
  values concentrate on one node, the `position()` defect is confirmed from the log.**
- `occupancy.py` prints all of this itself — it states a verdict in **both** directions, including
  "the defect does not appear in this run".

## 🎯 The largest untouched target

**Sink serialisation.** `baseline-n3-r1` carries a **2566.6 s solo `integrate-verify` tail — 30% of
its wall** — which survives every correction applied today, and it matches the long-standing
observation that integrate-verify takes 36-47% of node-busy time. It needs no fleet to investigate.

## The baseline to beat (5 real cells)

| cell | unit | wall | score | EXEC occ | prefix (redraft) |
|---|---|---|---|---|---|
| 1 | `baseline-n3-r0` | 7729.3 | 0.6595 | 0.8568 | 2218.7 (1) |
| 2 | `baseline-n3-r1` | 8488.0 | 0.4780 | 0.5746 | 1330.0 (0) ← **solo sink tail** |
| 3 | `baseline-n3-r2` | 6752.6 | 0.6030 | 0.8139 | 1316.0 (0) |
| 4 | `baseline-n3-r3` | 7302.6 | **0.8157** | 0.5910 | 2882.7 (2, REVERT, conf 61) |
| 5 | `baseline-n1-r0` | 5842.9 | 0.5798 | **1.0** | 2925.8 (1, conf 60→100) |

n3 mean **0.6641**, sd **0.1401**. **The single closed matched pair: n3 7729 s / 0.6595 against n1
5843 s / 0.5798 — three nodes 1.32× SLOWER and 0.080 BETTER**, with the score edge confounded because
n3 received +2 replan tasks the 1-node arm cannot structurally get (F312). `curve.py` states outright
that p can never beat 0.5 at n = 1, so there is no verdict.

⚠ **These ran on the PRE-BOUNDARY binary.** After the rebuild they are a historical baseline, not
matched cells (L137). Everything from here must be re-baselined.

## Always use the EXECUTE occupancy column

The whole-run column (0.5645 / 0.4737 / 0.6499 / 0.2582) includes a planning prefix of 16-39% of wall
that credits zero busy **by construction**, and reading it as idleness is what produced F346. The
scheduler-owned window is **0.8568 / 0.5746 / 0.8139 / 0.5910**. The independent `lms ps` sampler
agrees: 0.753 / 0.857 / 0.909 / 0.716 (⚠ a sanity check, not a decomposition — a task span includes
local tool execution while the GPU idles, so it is not a strict superset).

## Four things a reader arriving cold gets wrong

1. **A unit that never started is FAST, not failed** (F349). It sets neither `failed` nor
   `timed_out`, and 113 such rows entered the corpus in twenty minutes while `loop.sh check` printed
   OK. Both `is_real_unit()` and `health.py` now guard on `harness_ok is False` **and** a 60 s floor.
2. **The park used to destroy evidence** (F338) — `cp -R "$RUNDIR"/*/` copies directory *contents*.
   `swarm-1node-r0`'s original log is gone.
3. **A split parent never completes** (F334). Never compute in-flight as `dispatched − completed`.
4. **Two events spell the same field differently** — `plan_loaded.tasks[].files` vs
   `retarget_discarded.tasks[].owned_files`.

## Instruments (never re-implement one — L2, violated again today)

`curve.py` verdict · `occupancy.py` (occ-5) node-seconds, plan ceiling, idle-slot accounting ·
`power.py` · `planshape.py` · `bonusclass.py` · `dispatch_audit.py` · `reaudit.py` ·
`goalstate.py --tick` · `sweep.py` · `loop.sh {status,stop,start,boundary,check,selftest}` ·
`autorestart.sh` · **`fleetsample.sh` — the independent `lms ps` sampler; READ IT (L198)**.

⚠ absolute paths for `occupancy.py`/`curve.py`. ⚠ `git add` from the **repo root**. ⚠ `grep -c` exits
1 on zero matches. ⚠ a pipe hides the exit code. ⚠ `cargo` needs `source bin/activate-hermit` or
cmake is missing.

## Fleet

3 LM Studio nodes, `PARALLEL 2` ⇒ **6 slots**. **NEVER load, unload or re-alias anything.**
