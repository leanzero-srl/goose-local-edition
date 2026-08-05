# RESUME — live state, rewritten 2026-08-05 ~13:50 local

Sixth rewrite today. The 12:15 version predates **eight findings (F374-F381)** and still states a
number F381 has since narrowed, so it is replaced rather than patched. 48 commits since 07:00.

## 🛑 THE ONE BLOCKER: the fleet is empty

At **08:03:59** all three LM Studio nodes went from GENERATING to **no models loaded**
(`fleet-samples.tsv`, independent of the event log). LM Link still shows both remote devices
**connected**. Every unit launched after that returns in ~0.2 s with score 0.0 and no run log.

**Do not touch LM Studio** — standing rule from Mihai. One `lms load` per node restores it, and it is
his call. The sweep is stopped (`STOP` armed 08:07).

```bash
cd ~/Projects/goose/evals/swarm-bench/nodeloop
~/.lmstudio/bin/lms ps                       # must list models before anything below
ls -la ~/Projects/goose/target/release/goose  # MUST be >= 13:17 — see "the binary" below
rm -f STOP && ./loop.sh start                # BARE, never piped (F298)
```

## ⚠️ THE BINARY IS PART OF THE ARM (F378 — the catch that would have cost a run)

`target/release/goose` was still the **08:56** build while F352 (09:06), F357 (09:49), F369 (11:28)
and F375 (12:54) had all landed. **The sweep runs the RELEASE binary**, so a restart would have spent
a full ~2 h cell on an engine carrying none of them, and `review.py` would have reported every
prediction unsettleable *after* the fleet time was gone.

Rebuilt **13:17** and verified the way `review.py` verifies — `strings`, not cargo's exit code:
`judge_node`, `salvaged`, `-byte tree`, `for this fleet`, `PARALLEL WORKER SLOTS` all present, and the
**negative control** passes: the stale `"for a 3-device fleet"` literal is **absent**, which is the
only half that distinguishes "rebuilt" from "was already fine".

> **AFTER ANY FURTHER ENGINE COMMIT: `cargo build --release -p goose-cli` before starting the sweep.**

## 📌 Seven predictions are registered in `PREDICTIONS`, not just in prose

`review.py:236-273` settles predictions by `strings`-matching the running binary, and today's were
living only in FINDINGS.md — prose, not implementation. Now seven lines: F350, F351, F352, F357, F369,
F374, F375. F350/F351 added struct fields and no literal, so they take `judge_node` (09:06, strictly
after both) as a **build-boundary proxy**, and that reasoning is written into the file rather than
silently conflated.

## 🔴 What today disproved — including four of my own headlines

| claim | verdict |
|---|---|
| "the fleet is bottlenecked (LM Link)" | **DEAD** — 2.98× concurrency costs 1.19× per task (F366) |
| "3 nodes bought 2.18× the code" | **DEAD** — non-test source 27087 B vs 27103 B, **0.06%** (F370) |
| "more slots ⇒ more modules" | **DEAD** — n3 has 7 file-owning tasks, n1 has 8 (F369) |
| "the tasks each wrote more" | **DEAD** — same volume; three n1 tasks wrote nothing (F370) |
| "the test spec is thin" | **DEAD** — test specs are the LONGEST, 2160 vs 1236 (F371) |
| "`force_write_tool` is an unverified landmine" | **DEAD** — measured, documented, test-pinned OFF (F373) |
| "the verify layer costs 1718 s" | **DEAD** — survivor bias; true gap ~58 s (F380) |
| "there is a ready→dispatched starvation gap" | **DEAD** — every wait is **0.0 s** (F380) |
| "test tasks fail ~29× more" | **NARROWED** — the honest range is **8× to 21×** (F381) |

## ✅ What stands

1. **The parallelism WORKS.** Execute occupancy 0.86 on the 3-node arm; matched by task id (n=11) the
   median duration ratio is **1.19 at a median concurrency ratio of 2.98** ⇒ ~2.5× real throughput.
2. **The deficit is three whole-app readers**, and they have the *lowest* concurrency ratios, so
   contention does not explain them: `integrate-verify` 7.85×, `verify-e2e::0` 3.65×, `verify-e2e::1`
   4.04× against 0.21-1.51× for every other matched task. Critical path 3827 s vs 2036 s.
3. **The 3-node arm lost the tier-A integration check while its integrator was cut off** — `sync_shape`
   1.00 → 0.00 on the run whose `integrate-verify` was terminated at **1800.1 s == `sink_cap_secs`
   exactly**, mid-repair (10 shell + 9 write + 1 edit, 56 messages over 1705 s, zero final output).
4. **Test-authoring tasks fail 8-21× more than any other kind**, on BOTH fleet sizes, with
   `kind_prompt` ON and `tailored: true` — FIRED ≠ CORRECT.
5. **A failed task does not imply a missing file** — n3-r0's `test-core` FAILED yet left 3035 B + 2566 B.
6. **~14% of completed tasks are watchdog salvages**, and the salvage is a genuine rescue (median
   7940 B vs 4947 B; zero salvaged left a file empty, one CLEAN task did).
7. **The scheduler dispatches the instant a task is ready** — every measured wait is 0.0 s (F380).

## 🏁 Three closed dead ends — do not re-open without new data

- **`force_write_tool`.** Gate fn `swarm.rs:19445-19456` records the whole argument: the named
  `tool_choice` form is rejected outright by the server, `"required"` is not enforced and biases to
  `shell`. A test pins it OFF; `levers_resolved` emits it *because* it is off, citing "27 of 27".
- **The test-task failure.** `kind_prompt` ON and tailored, `act_now_nudge` ON since 08-03 18:17
  (my bench's best arm: writes 23.8→48.0%, no-tool-call 24→4%), named `tool_choice` rejected,
  `"required"` harmful. The engine's own summary: *"every alternative aimed at the same failure is
  either harmful or rejected by the server."*
- **The `verify::`→`verify-e2e::` edge.** 0.0 s dispatch latency in all four cells; the edge is
  positively justified, not merely tolerated.

## ✅ Shipped today — all clippy-green, all UNMEASURED

`e26f26869` F350 planning fans count SLOTS · `ec32f9e2f` F351 `pre_review.secs` · `23813603e` F352
`judge_node` · `d09eaa39e` F353 occ-5 · `d6c8150c5` F357 `salvaged` · `0dd19f949` F358 salvage
counting · `f7cd8d94a` F365 joint tests · `816d2abcd` **F369 sink ceiling scales with the tree** ·
`ab7bfabc1` **F375 architect prompt no longer contradicts itself** · `da93ac762` **F376 the
slot-concurrency contract now has a test** · `430ab9393` **F377 `slot_count()` sums real weights**.

⚠ **F374 is REGISTERED, NOT FIXED, deliberately.** `worker_count` also feeds `fan_e2e_split`, so a
3-node run now emits **four** e2e shards where the archive shows three, and each shard builds and runs
the whole app. Sign unknown. **Falsifiers: e2e node-seconds > 3037 s, or any shard reporting clean
having enumerated zero commands ⇒ move the shard count to the oracle length.**

⛔ **Do NOT ship the selection fix** (`scheduler.rs:1099`/`:1220` use `position()` while `pick_device`
sorts by `in_flight`). F380 exonerated it on *dispatch latency* — that is **not** the same as
exonerating its *placement skew*, which is what the simulation is about. Measure the skew from
`judge_node` first (L202).

## The baseline — ⚠ the n3/n1 wall ratio is NOT like-for-like

| cell | wall | score | EXEC occ | notes |
|---|---|---|---|---|
| `baseline-n3-r0` | 7729.3 | 0.6595 | 0.8568 | tree 43328 B; split: `test-api-web` |
| `baseline-n3-r1` | 8488.0 | 0.4780 | 0.5746 | sink stalled, FAILED |
| `baseline-n3-r2` | 6752.6 | 0.6030 | 0.8139 | no splits |
| `baseline-n3-r3` | 7302.6 | **0.8157** | 0.5910 | splits: `api`, `meridian` |
| `baseline-n1-r0` | 5842.9 | 0.5798 | **1.0** | tree 27103 B — **0 test files, 3 MISSING** |

**"3 nodes is 1.32× slower" compares a run that completed its plan against one that abandoned three
tasks in 22 seconds and shipped no tests.** Neither number measures the other arm (L215).
⚠ Pre-boundary binary (L137); every score mixes clean and salvaged tasks.

## Seven things a reader arriving cold gets wrong

1. **A unit that never started is FAST, not failed** (F349) — 113 such rows entered the corpus while
   `loop.sh check` printed OK, and `curve.py` published a p-value off seven fabricated pairs.
2. **`session_id` is null on ~1 in 5 tasks** (F356) — never assume a transcript is reachable.
3. **A split parent never completes** (F334) — and this is a POPULATION defect, not one formula:
   **any max/last/mean over `task_completed` silently excludes split parents** (L221). It cost three
   passes on one quantity today (−161.4 s filter bug → 1718.8 s survivor bias → 0.0 s truth).
4. **`worker_timeout_secs` = 420 is IDLE time, not wall-clock** (F294); different from `sink_cap_secs`.
5. **`verdict.json` checks carry `score`, not a boolean** — a bool reader returns 0/35 for every cell.
6. **Read the GATE FUNCTION (`fn <lever>()`), not the field doc.** They routinely say opposite things:
   the field doc sells the mechanism, the gate function records whether it works. This cost a published
   retraction today (F373).
7. **The artefact a run executes is not the source you committed** (F378, L220).

## Instruments (never re-implement one — L2)

`curve.py` · `occupancy.py` (occ-5, now with `slot_count()`) · `phases.py` · `power.py` ·
`planshape.py` · `bonusclass.py` · `dispatch_audit.py` · `reaudit.py` · `goalstate.py --tick` ·
`sweep.py` · `review.py` (reads `PREDICTIONS`) · `armcheck.py` · `failures.py` ·
`loop.sh {status,stop,start,boundary,check,selftest}` · `autorestart.sh` · `fleetsample.sh` ·
**`promptbench.py` + `bench/*.jsonl` — a real prompt-bench corpus with archived payloads. CONSULT IT.**

⚠ absolute paths for `occupancy.py`/`curve.py`. ⚠ **`git add` from the repo root — `cd
/Users/mihaiperdum/Projects/goose` first** (violated 9× today). ⚠ `grep -c` exits 1 on zero matches.
⚠ a pipe hides the exit code. ⚠ `cargo` needs `source bin/activate-hermit`. ⚠ `cargo fmt` rewrites
your edits and shifts line numbers — re-Read before the next Edit. ⚠ inserting a Rust test: place it
ABOVE the previous test's `#[tokio::test]`, never between an attribute and its `fn`.

## Fleet

3 LM Studio nodes, `PARALLEL 2` ⇒ **6 slots**. **NEVER load, unload or re-alias anything.**
