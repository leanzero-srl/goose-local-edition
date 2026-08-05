# RESUME — live state, rewritten 2026-08-05 ~12:15 local

Fifth rewrite today. The previous one (10:30) is not merely stale — **it states things this session has
since disproved**, which is worse, so this replaces it wholesale. 38 commits since 07:00.

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
./loop.sh check                 # BAD-fails on units that never started (F349)
```

## 🔴 The posture (Mihai, 07:45)

> *"the main goal is to make the 3 node swarm actually work better than a 1 node, which means you
> need to make **fixes and improvements in the engine**."*

Measuring is subordinate to shipping. The F253 freeze and the pre-registered n = 8 are **superseded,
not fudged**.

## 🔴 READ THIS FIRST: what today disproved, including my own headlines

Seven findings landed (F366-F373) and **four of them killed an earlier one of mine**. If you carry
only one thing forward, carry this list — every entry cost a tick to establish.

| claim | verdict |
|---|---|
| "the fleet is bottlenecked (LM Link funnels every node through localhost)" | **DEAD** — 2.98× concurrency costs 1.19× per task (F366) |
| "3 nodes bought 2.18× the code" | **DEAD** — non-test source is 27087 B vs 27103 B, **equal to 0.06%** (F370) |
| "more slots ⇒ more modules" | **DEAD** — n3 has **7** file-owning tasks, n1 has **8** (F369) |
| "the tasks each wrote more" | **DEAD** — they wrote the same; three n1 tasks wrote **nothing** (F370) |
| "the test spec is thin" | **DEAD** — test specs are the **LONGEST**, median 2160 vs 1236 (F371) |
| "the retry burst is a 1-node story" | **DEAD** — n3-r0 failed 3 of its 5 test tasks (F371) |
| "`force_write_tool` is a live landmine shipped unverified" | **DEAD** — the engine measured it, documented it, and **test-pins it OFF** (F373) |
| "make the sink budget progress-shaped" | **DEAD** — a loop emits tool calls, so no progress rule separates repair from loop (F368) |

## ✅ What actually stands

1. **The parallelism WORKS.** Execute occupancy 0.86 on the 3-node arm; matched by task id (n=11) the
   median duration ratio is **1.19 at a median concurrency ratio of 2.98** ⇒ ~2.5× real throughput.
   The fleet is neither idle nor bottlenecked. **(F366)**
2. **The deficit is three whole-app readers**, and they have the *lowest* concurrency ratios, so
   contention does not explain them either: `integrate-verify` 7.85×, `verify-e2e::0` 3.65×,
   `verify-e2e::1` 4.04× — against 0.21-1.51× for every other matched task. Critical path 3827 s vs
   2036 s. **(F366)**
3. **The 3-node arm lost the tier-A integration check while its integrator was cut off.** `sync_shape`
   1.00 → 0.00 on the run whose `integrate-verify` was terminated at **1800.1 s == `sink_cap_secs`
   exactly**, having made 10 shell + 9 write + 1 edit calls with 56 messages continuous over 1705 s and
   **zero final output** — cut mid-repair, not looping. **(F367, F368)**
4. **Test-authoring tasks fail at 29% against 1% for every other kind** (n=21 vs 74), at twice the
   attempts, on BOTH fleet sizes. **(F371)**
5. **A failed task does not imply a missing file.** n3-r0's `test-core` FAILED yet left 3035 B + 2566 B;
   n1-r0's three failed owners left nothing. **(F371)**
6. **~14% of completed tasks are watchdog salvages** and the salvage is a genuine rescue — salvaged
   median 7940 B vs 4947 B clean, and **zero salvaged tasks left a file empty while one CLEAN task did**.
   **(F356, F359)**

## 🏁 The test-task failure is a CONFIRMED DEAD END for cheap fixes

Do not re-open this without new data. The whole space is enumerated and every option is spent:

- `kind_prompt` — **ON** (baked default, test-asserted), and `rules_delivered` shows `tailored: true`
  on test tasks. The 29% is what it does *with the fix working*. FIRED ≠ CORRECT.
- `act_now_nudge` — **ON**, and it is the best intervention I have measured: on the 9 shared bench
  cases it takes writes from 23.8% → **48.0%** and no-tool-call from 24% → **4%**. It shipped
  `d9394ebda` 08-03 18:17, **before both baseline runs**, so the 29% already includes it.
- `force_write_tool` — the named `tool_choice` form is **rejected by the server** (`Invalid tool_choice
  type: 'object'`), 27 of 27 samples 400'd. `"required"` is not enforced and biases to `shell`.
- The engine's own summary, at `swarm.rs:19457`: *"every alternative aimed at the same failure is
  either harmful or rejected by the server."*

## ✅ Shipped today — all clippy-green, all UNMEASURED

| commit | what |
|---|---|
| *(boundary 08:15)* | `f1a20c99b`, `95b36748f`, `00563c6ea`. **`engine_build` changed.** |
| `e26f26869` | **F350** planning fan-outs count SLOTS, not devices |
| `ec32f9e2f` | **F351** `pre_review` emits `secs` |
| `23813603e` | **F352** `judge_verdict` carries `judge_node` |
| `d09eaa39e` | **F353** `occupancy.py` occ-5 |
| `d6c8150c5` | **F357** `TaskCompleted` carries `salvaged: bool` |
| `0dd19f949` | **F358** salvage counting, two routes |
| `f7cd8d94a` | **F365** all six verified together: 534 tests, 0 failures |
| `816d2abcd` | **F369** sink ceiling scales with the tree it must integrate |

**F369 in one line:** `sink_cap_secs` was a constant while the join's work tracks the tree on disk.
Now sized **at sink dispatch** from a **frozen list of the run's own declared files** (never a
directory walk), `sink_cap_ref_bytes` = 30000, clamped to [1×, 2×]. The 43328 B tree gets 2600 s; the
2× clamp (3600 s) still cuts the 4326 s join measured to loop. ⚠ **Magnitude confidence LOW** — the
reference is fitted to n=1 and nothing says 2600 s is enough. **Falsifier: any join > 4326 s, or
`sync_shape` still failing on a 3-node run ⇒ REVERT.**

## 📌 Four predictions registered in advance. The next real run tests them.

- **`pre_review.secs`** — 7-12 events, **100-250 s**.
- **`judge_node`** — non-empty on **40-75%** of verdicts. **Concentrated on one node ⇒ the `position()`
  selection defect confirmed from the log rather than simulation.**
- **`salvaged: true`** — **1-3 per 3-node run**. Zero on *every* run ⇒ the flag isn't wired.
- **F350's three falsifiers:** detail-fan makespan **−≥20%** vs 244/204 s else **REVERT**;
  `straggler_aborted` must **not rise** else **REVERT**; reconstructed concurrency must reach **6**.

⛔ **Do NOT ship the selection fix yet** (`scheduler.rs:1099`/`:1220` use `position()` while
`pick_device` sorts by `in_flight`). Fixing it before a run carries `judge_node` destroys the only
chance to confirm it from evidence (L202).

## The baseline — ⚠ the n3/n1 wall ratio is NOT like-for-like

| cell | unit | wall | score | EXEC occ | tree |
|---|---|---|---|---|---|
| 1 | `baseline-n3-r0` | 7729.3 | 0.6595 | 0.8568 | 43328 B (27087 src + 16241 test) |
| 2 | `baseline-n3-r1` | 8488.0 | 0.4780 | 0.5746 | sink stalled, FAILED |
| 3 | `baseline-n3-r2` | 6752.6 | 0.6030 | 0.8139 | |
| 4 | `baseline-n3-r3` | 7302.6 | **0.8157** | 0.5910 | |
| 5 | `baseline-n1-r0` | 5842.9 | 0.5798 | **1.0** | 27103 B (**0 test — 3 MISSING**) |

**"3 nodes is 1.32× slower" compares a run that completed its plan against one that abandoned three
tasks in 22 seconds and shipped no tests.** n1's 5843 s is cheap partly because it did less, and its
0.5798 is low partly because it shipped no suite. Neither number measures the other arm (L215).
⚠ Pre-boundary binary (L137); every score mixes clean and salvaged tasks.

## Six things a reader arriving cold gets wrong

1. **A unit that never started is FAST, not failed** (F349) — 113 such rows entered the corpus in
   twenty minutes while `loop.sh check` printed OK, and `curve.py` published a p-value off seven
   fabricated pairs. Both now guard on `harness_ok is False` **and** a 60 s floor.
2. **`session_id` is null on ~1 in 5 tasks** (F355/F356) — never assume a transcript is reachable.
3. **A split parent never completes** (F334). Never compute in-flight as `dispatched − completed`.
4. **`worker_timeout_secs` = 420 is IDLE time, not wall-clock** (F294); different from `sink_cap_secs`.
5. **`verdict.json` checks carry `score`, not a boolean** — a bool reader returns 0/35 for every cell.
6. **Read the GATE FUNCTION, not the field doc.** They routinely say opposite things: the field doc
   sells the mechanism, `fn <lever>()` records whether it works. This cost a whole tick and a published
   retraction today (F373), and four times a comment at the site already held the answer
   (`:12665`, `:1152`, `:267`, `:19445`).

## Instruments (never re-implement one — L2)

`curve.py` · `occupancy.py` (occ-5) · `phases.py` · `power.py` · `planshape.py` · `bonusclass.py` ·
`dispatch_audit.py` · `reaudit.py` · `goalstate.py --tick` · `sweep.py` ·
`loop.sh {status,stop,start,boundary,check,selftest}` · `autorestart.sh` · `fleetsample.sh` ·
**`promptbench.py` + `bench/*.jsonl` — a real prompt-bench corpus with archived payloads. CONSULT IT.**

⚠ absolute paths for `occupancy.py`/`curve.py`. ⚠ **`git add` from the repo root — `cd
/Users/mihaiperdum/Projects/goose` first** (violated 6× today). ⚠ `grep -c` exits 1 on zero matches.
⚠ a pipe hides the exit code. ⚠ `cargo` needs `source bin/activate-hermit`. ⚠ `cargo fmt` rewrites
your edits — re-Read before the next Edit.

## Fleet

3 LM Studio nodes, `PARALLEL 2` ⇒ **6 slots**. **NEVER load, unload or re-alias anything.**
