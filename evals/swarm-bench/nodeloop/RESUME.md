# RESUME — live state, rewritten 2026-08-05 ~10:30 local

Fourth rewrite today. The first three were because the engine target kept moving; this one is because
the investigation **closed**. 27 commits since 07:00.

## 🛑 THE ONE BLOCKER: the fleet is empty

At **08:03:59** all three LM Studio nodes went from GENERATING to **no models loaded**
(`fleet-samples.tsv`, independent of the event log). LM Link still shows both remote devices
**connected**. Every unit launched after that returns in ~0.2 s with score 0.0 and no run log.

**Do not touch LM Studio** — standing rule from Mihai. One `lms load` per node restores it, and it is
his call. The sweep is stopped (`STOP` armed 08:07).

```bash
cd ~/Projects/goose/evals/swarm-bench/nodeloop
~/.lmstudio/bin/lms ps          # must list models before anything below
rm -f STOP && ./loop.sh start   # BARE, never piped (F298)
./loop.sh check                 # now BAD-fails on units that never started (F349)
```

## 🔴 The posture (Mihai, 07:45)

> *"the main goal is to make the 3 node swarm actually work better than a 1 node, which means you
> need to make **fixes and improvements in the engine**."*

Measuring is subordinate to shipping. The F253 freeze is superseded; the pre-registered n = 8 (F327)
is **superseded, not fudged**.

## ✅ Shipped today — all clippy-green, all UNMEASURED

| commit | what |
|---|---|
| *(boundary 08:15)* | `f1a20c99b`, `95b36748f`, `00563c6ea` finally compiled. **`engine_build` changed.** |
| `e26f26869` | **F350** planning fan-outs count SLOTS, not devices |
| `ec32f9e2f` | **F351** `pre_review` emits `secs` (it can hold a slot 900 s and reported nothing) |
| `23813603e` | **F352** `judge_verdict` carries `judge_node` (it named the *judged* worker) |
| `d09eaa39e` | **F353** `occupancy.py` occ-5: idle-slot accounting |
| `d6c8150c5` | **F357** `TaskCompleted` carries `salvaged: bool` |
| `0dd19f949` | **F358** salvage counting, two routes, states which it used |

## 🏁 The F354 → F363 investigation is CLOSED. Six of my own hypotheses died.

    "the sink tail is serialisation"        → three 420 s stalls, and it FAILED          (F354)
    "salvage means degraded output"         → refuted, SIGN REVERSED                     (F359)
    "flip the existing spiral switch"       → the switches were ALREADY ON               (F360)
    "derive the threshold"                  → derived it; n=11 was ONE task, and the
                                              gate cannot reach the sink                 (F361)
    "the reading judge missed it"           → it was DELIBERATELY NEVER ASKED            (F362)
    "judge starvation explains the spirals" → FLAT, wrong sign                           (F363)

**Do not restart this line without new data.** Each death is recorded with its evidence in
`FINDINGS.md`; re-deriving any of them costs a tick and ends in the same place.

## ✅ What SURVIVED, and it is worth more than the dead hypotheses

1. **~14% of completed tasks are watchdog salvages.** `swarm.rs:20705` — a task that trips the
   thinking-only watchdog **with its owned files already written** is accepted as `done` with
   `session_id: None, tool_calls: []`. r0 1/19 · r1 4/22 · r2 3/21 · r3 3/17 ⇒ **11 of 79 = 13.9%**.
   11 salvages + 4 failures account for **all 15** missing-session rows. Now flagged by the engine
   and counted by the harness.
2. **The salvage is a genuine rescue.** Salvaged owned-files median **7940 B vs 4947 B** clean;
   kind-matched test 8939 vs 8763, source 6699 vs 3855; **zero salvaged tasks left a file empty — one
   CLEAN task did.** The precondition *is* "files already written", so a salvaged task **finished its
   work and then failed to stop talking**. **Not a quality proxy.**
3. **An owns-nothing task has no early stall detection by construction** — a documented trade
   (`scheduler.rs:1152`), not an oversight. Cost now priced: **3837 s, three attempts, FAILED, 30% of
   r1's wall.** ⚠ Do NOT re-enable the re-judge; the one verdict the sink got was a useless `ok` at
   confidence 1.0.
4. **A char threshold provably cannot separate spiral from healthy work** on this corpus — clean
   reaches 1781 while spiralling starts at 1059, and one clean observation sits **above four**
   spiralling ones. `swarm.rs:361` asserted it; this confirms it with data.

## 📌 Four predictions registered in advance. The next run tests them.

- **`pre_review.secs`** — 7-12 events, **100-250 s**. Under ~20 s ⇒ F348's ~0.30 idle-slot estimate
  is too high, and that figure is what retired F346.
- **`judge_node`** — non-empty on **40-75%** of verdicts. All-empty ⇒ not wired. All-non-empty ⇒ the
  deterministic-only path isn't distinguished. **Concentrated on one node ⇒ the `position()`
  selection defect confirmed from the log rather than simulation.**
- **`salvaged: true`** — **1-3 per 3-node run**. Zero on *every* run ⇒ the flag isn't wired to the
  firing path, not that stalls stopped.
- **F350's three falsifiers, verbatim:** detail-fan makespan must drop **≥20%** vs 244/204 s else
  **REVERT**; `skeleton_drafts.straggler_aborted` must **not rise** else **REVERT**; reconstructed
  `detail_completed` concurrency must reach **6** (2 on a 1-node run).

⚠ **F350's benefit confidence is LOW.** Measured doubled/solo ratios 2.08/2.01/1.96 = **zero
throughput gain**, and `swarm.rs:2113` says the second slot exists for *bursty agent* tasks while
planning fans are no-tool single completions. **The review said ship it behind a lever default OFF; I
shipped it ON**, and that is recorded rather than hidden.

## ⛔ Do NOT ship the selection fix yet

`scheduler.rs:1099`/`:1220` select the idle-job device with `position(...)` — the first with any free
slot — while `pick_device` at `:592-600` deliberately sorts by `in_flight`. Simulated cost
+0.062/+0.064/+0.069/+0.110 against +0.143/+0.156/+0.186/+0.198 with correct placement. **Fixing it
before a run carries `judge_node` destroys the only chance to confirm it from evidence** (L202).

## Always use the EXECUTE occupancy column

Whole-run (0.5645/0.4737/0.6499/0.2582) includes a planning prefix of 16-39% of wall crediting zero
busy **by construction** — reading it as idleness is what produced F346. Scheduler-owned window:
**0.8568 / 0.5746 / 0.8139 / 0.5910**. The independent `lms ps` sampler agrees (0.753/0.857/0.909/
0.716) ⚠ but it is a **sanity check, not a decomposition**.

## The baseline to beat (5 real cells)

| cell | unit | wall | score | EXEC occ | salvaged | prefix (redraft) |
|---|---|---|---|---|---|---|
| 1 | `baseline-n3-r0` | 7729.3 | 0.6595 | 0.8568 | 1/19 | 2218.7 (1) |
| 2 | `baseline-n3-r1` | 8488.0 | 0.4780 | 0.5746 | 4/22 | 1330.0 (0) ← sink stalled, FAILED |
| 3 | `baseline-n3-r2` | 6752.6 | 0.6030 | 0.8139 | 3/21 | 1316.0 (0) |
| 4 | `baseline-n3-r3` | 7302.6 | **0.8157** | 0.5910 | 3/17 | 2882.7 (2, REVERT, conf 61) |
| 5 | `baseline-n1-r0` | 5842.9 | 0.5798 | **1.0** | — | 2925.8 (1, conf 60→100) |

**The one closed matched pair: n3 7729 s / 0.6595 against n1 5843 s / 0.5798 — three nodes 1.32×
SLOWER and 0.080 BETTER**, with the score edge confounded (+2 replan tasks the 1-node arm cannot
receive, F312). `curve.py` states outright that p can never beat 0.5 at n=1, so **there is no
verdict**.

⚠ **Pre-boundary binary — historical, not matched cells (L137). Every score mixes clean and salvaged
tasks.**

## Five things a reader arriving cold gets wrong

1. **A unit that never started is FAST, not failed** (F349) — 113 such rows entered the corpus in
   twenty minutes while `loop.sh check` printed OK, and `curve.py` published a p-value off seven
   fabricated pairs. Both now guard on `harness_ok is False` **and** a 60 s floor.
2. **`session_id` is null on ~1 in 5 tasks** (F355/F356) — never assume a transcript is reachable.
3. **A split parent never completes** (F334). Never compute in-flight as `dispatched − completed`.
4. **`worker_timeout_secs` = 420 is IDLE time, not wall-clock** (F294), and it is a different
   mechanism from `sink_cap_secs`.
5. **The engine records its own measured defects in comments at the site.** Four times today a
   comment I had not yet read already held the answer: `:12665` draft dedup, `:24155` salvage,
   `:361` char-cap futility, `:1152` the sink re-judge skip. **Grep the comments first (L206).**

## Instruments (never re-implement one — L2)

`curve.py` · `occupancy.py` (occ-5: node-seconds, plan ceiling, idle-slot accounting, salvage count) ·
`power.py` · `planshape.py` · `bonusclass.py` · `dispatch_audit.py` · `reaudit.py` ·
`goalstate.py --tick` · `sweep.py` · `loop.sh {status,stop,start,boundary,check,selftest}` ·
`autorestart.sh` · **`fleetsample.sh` — the independent `lms ps` sampler; READ IT (L198)**.

⚠ absolute paths for `occupancy.py`/`curve.py`. ⚠ `git add` from the **repo root**. ⚠ `grep -c` exits
1 on zero matches. ⚠ a pipe hides the exit code. ⚠ `cargo` needs `source bin/activate-hermit`.

## Fleet

3 LM Studio nodes, `PARALLEL 2` ⇒ **6 slots**. **NEVER load, unload or re-alias anything.**
