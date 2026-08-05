# RESUME — live state, rewritten 2026-08-05 ~08:02 local

**The previous version (07:50) named F346 — "the scheduler leaves plan-available work unscheduled" —
as the engine target. That finding is DEAD (F348).** Rewritten rather than appended to, because a
resume file carrying a retracted target sends the next reader to build the wrong thing. This is the
second rewrite today and the reason is L195: a summary is a claim and decays like one.

## 🔴 THE POSTURE (unchanged — Mihai, 07:45)

> *"the main goal is to make the 3 node swarm actually work better than a 1 node, which means you
> need to make **fixes and improvements in the engine**."*

Measuring is subordinate to shipping engine fixes. **The F253 freeze is superseded**: finish the
in-flight cell, take the boundary, ship, re-baseline. **n = 8 (F327) is superseded, not fudged.**
The boundary MUST run `cargo clippy --all-targets -- -D warnings` first (L108/L119). Three patches
are queued and uncompiled: `f1a20c99b`, `95b36748f`, `00563c6ea`.

## ☠️ F348 — the fleet was never idle. Read this before trusting any occupancy number.

The pre-registered falsifier returned **`yes_they_consume_a_slot` / `premise_survives: FALSE`**, and
I verified its three load-bearing claims myself before accepting it.

1. **Judge and pre_review acquire the same slot a task dispatch does.** `scheduler.rs:1205-1207`
   (`self.devices[i].in_flight += 1`), `:1238` for pre_review, released by `IdleSlotGuard::drop` at
   `:330-335`. While a judge runs, a real task cannot enter that slot. Judge slot-seconds
   3510 / 3263 / 4183 / 1896 = **14-31% of each cell's idle slot-time**.
2. **Unit mismatch, mine.** `occupancy.py` divides by `n = len(pool)` = **3 DEVICES**, and `busy` is
   the per-device union of spans, so a device running two tasks scores 1. I compared that against the
   six-SLOT concurrency histogram and called the difference waste.
3. **Wrong denominator, mine — and I had already quoted the right one.** The 0.5645 / 0.4737 /
   0.6499 / 0.2582 figures are whole-run, including a planning prefix of 16-39% of wall that credits
   zero busy by construction. `occupancy.py` prints that caveat itself and already publishes
   **EXECUTE OCCUPANCY 0.8568 / 0.5746 / 0.8139 / 0.5910** (~0.92/0.64/0.88/0.70 with judge at device
   level). **Always use the execute column.**

**And the part worth remembering:** `nodeloop/fleetsample.sh` — written 2026-08-02, polling `lms ps`
every 30 s, 4451 rows, still sampling — says the fleet was busy **0.753 / 0.857 / 0.909 / 0.716** of
node-time. A 19-46 point gap against what I reasoned from, on disk, unread for three days (**L198**).

⚠ `lms` is a device-level **sanity check, not a decomposition**: in n3-r0's execute window the
event-log task busy (13082.6 device-s) EXCEEDS lms busy (~11422), because a task span includes local
tool execution while the GPU idles.

## 🎯 The re-aimed target

**Sink serialisation.** Counting judge and pre_review at slot level, r1 and r3 still sit near
0.55-0.61 of six slots, and **r1 carries a 2566.6 s solo `integrate-verify` tail — 30% of its wall**
— which no measurement artifact explains. This lines up with the long-standing 36-47%-of-node-busy
observation for integrate-verify.

**Plus one real defect the falsifier surfaced on its own:** `scheduler.rs:1099` and `:1220` select the
idle-job device with `position(|d| d.cfg.enabled && d.in_flight < d.cfg.weight)` — the FIRST device
with any free slot, in pool order — while `pick_device` at `:592-600` deliberately sorts by
`in_flight`. **The mechanism built to fill idle nodes does not target idle nodes.** Judge work
currently adds +0.062/+0.064/+0.069/+0.110 to device occupancy; landing on genuinely idle devices it
would add +0.143/+0.156/+0.186/+0.198 — roughly double.

**Queued engine fixes, observability FIRST** (fixing selection before measurement is how the F346
detour started):

1. `judge_verdict.device` reports the **judged worker's** device (`task_final_device`,
   `scheduler.rs:1439-1442`), not the node that ran the judge ⇒ judge load is unattributable.
2. `pre_review` emits only a completion event — no start, no duration (`scheduler.rs:2459-2468`) —
   while one call can hold a slot for up to 900 s ⇒ pre-review slot time is unmeasurable.
3. Then `position()` → least-loaded selection for idle jobs.

## What is running right now

```bash
cd ~/Projects/goose/evals/swarm-bench/nodeloop
./loop.sh check           # expect OK, loop pid 91810, engine pid 91813
```

`baseline-n1-r0` is in EXECUTE and has reached `integrate-verify` — 14 dispatched, 12 done of a
16-task plan. **Let it finish**, then take the boundary. If the loop stops:
`rm -f STOP && ./loop.sh start` — bare, never piped (F298).

Workflow `wf_94b83a28-e0e`: falsifier DONE (above); four finder lenses still running. ⚠ **They were
briefed on the now-dead premise** — read every finding against F348, and anything motivated by "the
scheduler leaves slots unfilled" is already answered. A workflow result is a claim; read the
`file:line` yourself (L130).

## Cells collected — the baseline to beat

| cell | unit | wall | score | EXEC occ | tier B | prefix (redraft) | replan |
|---|---|---|---|---|---|---|---|
| 1 | `baseline-n3-r0` | 7729.3 | 0.6595 | 0.8568 | .3194 | 2218.7 (1) | +2 TEST-ONLY |
| 2 | `baseline-n3-r1` | 8488.0 | 0.4780 | 0.5746 | .2083 | 1330.0 (0) | +2 TEST-ONLY |
| 3 | `baseline-n3-r2` | 6752.6 | 0.6030 | 0.8139 | .3194 | 1316.0 (0) | +4 TEST-ONLY |
| 4 | `baseline-n3-r3` | 7302.6 | **0.8157** | 0.5910 | **.875** | 2882.7 (2, REVERT, conf 61) | +2 APP-SIDE |
| — | `baseline-n1-r0` | running | — | **1.0** | — | 2925.8 (1, conf 60→100) | cannot replan |

n3 score mean **0.6641**, sd **0.1401**.

## Mechanism results that still stand

- **Planning scales with the fleet (F343/F345).** Detailing costs 112.4 s/task on one node vs 60.2 on
  three in the discarded round, and 50.9 vs 27.2 in the accepted round — **the same 1.87 from two
  independent windows in the same runs**. The skeleton vote does NOT scale and does not need to
  (232/214, 222/331, 226/296/238, flat) because a 2-3 draft vote clears in one wave on 2 slots or 6.
  **The one-node arm serialises the fan, not the vote.** Honest n for the 1.87 is 2 — n3-r0 does not fit.
- **The DAG bias is fixed (F347, occ-4).** Split children inherit their parent's deps and replace it;
  replan additions stay rooted because the replanner injects them for being independent. Zero
  blind-rooted tasks remain in the corpus. `max_useful_nodes` is a **plan-shape** number and never
  showed the fleet was idle — F348 kills the conclusion, not the ratio.
- **A reuse cache is not worth building (F321/F324).** Genuine redrafts reuse 15.2% of accepted tasks.
- **The prefix IS the plan (F323).** `plan_loaded` → first `task_dispatched` is 0.0 s in 8 of 8.

## Four things a reader arriving cold gets wrong

1. **The park used to destroy evidence (F338).** `cp -R "$RUNDIR"/*/ "$PARK/"` copies directory
   *contents*; twelve unit dirs collapsed into one and `swarm-1node-r0`'s original log is gone. Fixed.
2. **Two events spell the same field differently.** `plan_loaded.tasks[].files` vs
   `retarget_discarded.tasks[].owned_files`.
3. **A split parent never completes (F334).** Never compute in-flight as `dispatched − completed`.
4. **`run.jsonl` silence is not worker idleness (F336).**

## Instruments (never re-implement one — L2)

`curve.py` · `power.py` · `planshape.py` · `bonusclass.py` · `occupancy.py` (occ-4) ·
`dispatch_audit.py` · `reaudit.py` · `goalstate.py --tick` · `review.py` · `failures.py` · `sweep.py`
· `loop.sh {status,stop,start,boundary,check,selftest}` · `autorestart.sh` · `promptbench.py` ·
**`fleetsample.sh` — the independent `lms ps` sampler. READ IT (L198).**

⚠ absolute paths for `occupancy.py`/`dispatch_audit.py`/`curve.py`. ⚠ `git add` from the **repo root**
— violated again at 07:56. ⚠ `grep -c` exits 1 on zero matches. ⚠ `<(...)` is unreliable here (F331).

## Fleet

3 LM Studio nodes, `PARALLEL 2` ⇒ **6 slots**. **NEVER load, unload or re-alias anything in LM
Studio.**
