# RESUME — live state, rewritten 2026-08-05 ~13:50 local

Sixth rewrite today. The 12:15 version predates **eight findings (F374-F381)** and still states a
number F381 has since narrowed, so it is replaced rather than patched. 48 commits since 07:00.

> **PATCHED IN PLACE for F382-F385** (the body below is 13:50 text). A rewrite is the wrong tool when
> only three claims moved — but a summary is a claim and decays like one (L195), so read the two
> boxes below BEFORE the body.

## 🚨 F385 — THE PRE-REGISTERED DESIGN CANNOT SETTLE GOAL ONE

`PREREGISTERED.md` budgets **5 matched pairs**. At the observed gap (**+0.0593**: n3 mean 0.6391 over
four cells vs the single n1 cell 0.5798) a sign test needs **51 pairs** for a coin-flip chance of
clearing p<0.05, and **115** for 80% — about **204 h of fleet**. Extended from `power.py`'s own
`q_for_power`/`gap_for_q`, not re-derived (L2).

Not a sign-test artefact: ARE 2/π leaves the best available test at ~33 pairs, still **6.6×** the
registration. It does **not** say 3 nodes is no better — it says **this design cannot tell**.

🎯 **Read the table upward and it validates the standing directive:** a gap of **0.30 settles at
exactly the 5 pairs already registered.** Collecting 102 cells or widening the gap are the only two
routes, and **only engine work is affordable.** *"MEASURING IS SUBORDINATE TO SHIPPING"* is, here, the
arithmetic — not a preference.

## ⚠️ F387 — TWO SINK FAILURE MODES, AND F386 BLAMED THE WRONG ONE

| cell | integrate-verify | capped | outcome | score |
|---|---|---|---|---|
| n3-**r0** | **1800.1 s** | **yes** | finalized `done` | **0.6595 — ABOVE the arm mean** |
| n3-**r1** | 781.1 s | no | **FAILED: "no progress for 420 s"** (IDLE watchdog) | **0.4780 — worst** |
| n3-r2 | 1656.4 s | no | clean | 0.6030 |
| n3-r3 | 499.3 s | no | clean | 0.8157 |

**The capped run scored ABOVE the mean.** F369 (cap scaling) fixes the mode that cost the arm nothing;
the worst cell died of a **420 s total-silence stall**, a different mechanism entirely. ⇒ **L226: "the
worst cell" and "the cell with the dramatic failure event" are different rows until joined.**

🎯 **THE ONE CHECK THAT SEPARATES THE ARMS:** `client_timeouts` — n1 `1.00 "timeout set"`, and
`0.00 "no request timeout"` in **4 of 4** three-node runs. It is the ONLY check of 35 where every
3-node cell loses to the 1-node cell. Closing it moves the gap +0.0593 → +0.0793 and the design
**51 → 30 pairs**. ⚠️ **NOT instruction dilution** — the spec never mentions timeouts (0 hits in 3943
chars), so nothing was lost in the split; one agent added it as craft and four split runs did not.
⚠️ "n1 passes" rests on ONE cell; "n3 fails 4/4" is the solid half.

📌 The tier model (A .25/6, B .30/12, C .25/7, D .20/10) **reproduces all five published scores
exactly** — so those counterfactuals run through the real scorer, not a reconstruction of it.

## 🛑 THE ONE BLOCKER: the fleet is empty

At **08:03:59** all three LM Studio nodes went from GENERATING to **no models loaded**
(`fleet-samples.tsv`, independent of the event log). LM Link still shows both remote devices
**connected**. Every unit launched after that returns in ~0.2 s with score 0.0 and no run log.

**Do not touch LM Studio** — standing rule from Mihai. One `lms load` per node restores it, and it is
his call. The sweep is stopped (`STOP` armed 08:07).

```bash
cd ~/Projects/goose/evals/swarm-bench/nodeloop
~/.lmstudio/bin/lms ps                       # must list models before anything below
ls -la ~/Projects/goose/target/release/goose  # MUST be >= 16:32 — see "the binary" below
rm -f STOP && ./loop.sh start                # BARE, never piped (F298)
```

## ⚠️ THE BINARY IS PART OF THE ARM (F378 — the catch that would have cost a run)

`target/release/goose` was still the **08:56** build while F352 (09:06), F357 (09:49), F369 (11:28)
and F375 (12:54) had all landed. **The sweep runs the RELEASE binary**, so a restart would have spent
a full ~2 h cell on an engine carrying none of them, and `review.py` would have reported every
prediction unsettleable *after* the fleet time was gone.

🔁 **REBUILT AGAIN 16:32** for F395 (the `spec_contract` vacuous pass). Verified present:
`not one advertised endpoint returned a 2xx`, `CHECKED NOTHING`, `do NOT make this a pass`,
`inconclusive reasons are recorded separately` — all ×1.

⚠️ **A VERIFICATION TRAP THAT NEARLY CONDEMNED A GOOD BUILD.** My first probe searched the literal
*with its em-dash* (`CHECKED NOTHING — not one advertised…`) and `grep -F` returned **0**, while the
same literal minus the em-dash returns **1**. A non-ASCII character in the SEARCH string can fail to
match a binary that genuinely contains it. **Probe with an ASCII-only fragment**, or a `strings` check
will report a fresh build as stale. Note too that `every advertised check that bound was satisfied`
is still present and SHOULD be — F395 kept it as the genuine-pass branch, so its presence is not
evidence of a stale binary either.

🔁 **REBUILT 15:58** after F391 (spec_contract discloses unprobed non-GET endpoints) and
F392-F394 (43 baked-ON levers declare themselves; the build now fails if one does not). Verified with
controls BOTH ways: `cap_base_secs` x5, the HTTP-timeout pitfall, `advertised endpoint(s) were NOT`,
`judge_node`, `urlopen` all present; `for a 3-device fleet` and a nonsense literal both **0**.
⚠️ `BAKED ON` is **0** in the binary and that is CORRECT — it is a doc comment, and `strings` can only
ever verify literals that reach code. A doc-only change is unverifiable this way by construction.

🔁 **REBUILT 15:07** after the F386 (`sink_capped` effective ceiling) and F388 (HTTP-timeout
pitfall) engine commits. Verified `strings`-wise with controls BOTH ways: `cap_base_secs` ×5,
`outbound HTTP call needs an EXPLICIT timeout` ×1, `urlopen` ×2, `judge_node`, `salvaged` all present;
negative controls `for a 3-device fleet` and a deliberate nonsense literal both return **0**, which is
what proves the grep can detect absence rather than always saying yes.

The 13:17 build below is superseded; its reasoning is kept because it is the general rule:

Rebuilt **13:17** and verified the way `review.py` verifies — `strings`, not cargo's exit code:
`judge_node`, `salvaged`, `-byte tree`, `for this fleet`, `PARALLEL WORKER SLOTS` all present, and the
**negative control** passes: the stale `"for a 3-device fleet"` literal is **absent**, which is the
only half that distinguishes "rebuilt" from "was already fine".

> **AFTER ANY FURTHER ENGINE COMMIT: `cargo build --release -p goose-cli` before starting the sweep.**

## 📌 22 predictions are registered in `PREDICTIONS`, not just in prose

⚠️ **This section said "seven" and was written when there were seven.** The file now holds **22**
well-formed lines; today added **F386** (`sink_capped` reports the EFFECTIVE ceiling — a WITHIN-ROW
identity, immune to replicate spread) and **F388** (the HTTP-timeout pitfall, with DELIVERY and
OUTCOME registered as separate halves because FIRED ≠ CORRECT).

⚠️ **THREE OF THE SEVEN BANDS WERE UNSETTLEABLE AS WRITTEN AND HAVE BEEN CORRECTED (F383/F384). ALL
THREE WERE MINE, ALL FROM TODAY, ALL DERIVED FROM A SINGLE CELL BEFORE F382 QUANTIFIED THE SPREAD.**
- **F374** ">3037 s" — **WITHDRAWN** (e2e is 2277.5/425.7/1391.6 across identical cells).
- **F350** "detail-fan makespan drops ≥20%" — **WITHDRAWN**: measured 146.7/240.0/1112.9/1859.8 s, a
  **12.68× spread**, and the detail item count itself varies 8-21, so the fan is not even measuring
  the same work. Surviving half is within-run: concurrency must reach 6, `straggler_aborted` must not rise.
- **F357** "1-3 salvaged per run" — **REPLACED**: the archive it came from shows 1/19, 4/22, 3/21,
  3/17, so r1 already sat outside my own band. Now a within-run **identity** — the count of
  `task_completed` with `salvaged: true` must EQUAL the count matching F356's signature (`status done`
  + null `session_id` + zero `tool_calls`) in the same run. ⇒ **L224: an identity beats a band,
  because a band fights the variance and an identity ignores it.**

`review.py:236-273` settles predictions by `strings`-matching the running binary, and today's were
living only in FINDINGS.md — prose, not implementation. Now seven lines: F350, F351, F352, F357, F369,
F374, F375. F350/F351 added struct fields and no literal, so they take `judge_node` (09:06, strictly
after both) as a **build-boundary proxy**, and that reasoning is written into the file rather than
silently conflated.

## ✅ SHIPPED AFTER THE 13:50 BASE TEXT (F386-F394)

| # | change | verified by |
|---|---|---|
| F386 | `sink_capped` reports the EFFECTIVE tree-scaled ceiling, not the base env string — F369 was unreadable from any log | clippy RC=0 |
| F388 | the pitfalls library had **no network fact at all**; added HTTP timeouts + a trigger row MEASURED against the archive (`vendor` hits 11/16 specs, the library-name row 4/16 incl. the client module) | new test, both directions |
| F390 | `doc_fetch` wiring audited end-to-end (no `sink_review`-class bug), the doc MEASURED at 4769 B vs a 24000 B cap; **re-promoted reps 1→3**, readout repointed off a signal that would fire with the lever doing nothing | — |
| F391 | `spec_contract` silently dropped every non-GET advertised endpoint — incl. `POST /api/sync`, the one 4 of 5 cells break. Now disclosed via `inconclusive` | new test, 4 directions |
| F392-F394 | **43 baked-ON levers now declare themselves and the BUILD FAILS if one does not** — falsified both ways, incl. an anti-vacuity floor that turns parser breakage into a loud failure | clippy RC=0, controls |

⇒ **L227** after a bake, prose about defaults is a claim about the PAST — only the `Default` impl
knows. ⇒ **L228** a test built on a parser is only as good as the parser. ⇒ **L229** when a check needs
a parser you cannot trust, **ask a weaker question**.

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
| "the split loses the documented API contract" | **DEAD** — the `api` task receives all three keys in EVERY cell of BOTH arms (F389) |
| "the split loses the timeout requirement" | **DEAD** — the spec never mentions timeouts at all; it is craft, not dilution (F387) |
| "`/v1` is the coin flip" | **DEAD on this archive** — all five plans carry it 6-14× and every built client uses it (F389) |
| "the scorer passes vacuously on empty data" | **DEAD** — it returns 0.0 on `no rows` / `too few rows` (F389) |
| "test tasks fail ~29× more" | **NARROWED** — the honest range is **8× to 21×** (F381) |

## ✅ What stands

1. **The parallelism WORKS.** Execute occupancy 0.86 on the 3-node arm; matched by task id (n=11) the
   median duration ratio is **1.19 at a median concurrency ratio of 2.98** ⇒ ~2.5× real throughput.
2. 🔴 **THERE IS NO LOCATED DEFICIT (F382 — this line used to claim one).** The "three whole-app
   readers" figures (`integrate-verify` 7.85×, `verify-e2e::0` 3.65×, `verify-e2e::1` 4.04×) come from
   **r0 alone**, and `e2e+sink` across three cells of the IDENTICAL config is **1890.9 / 2082.1 /
   4077.6 — a 2.2× spread with nothing varied**. r0 is the worst of the three; against r2 the 3-node
   arm is FASTER on e2e (425.7 vs the 1-node arm's 437.8). **One pair cannot locate a deficit inside
   that spread (L223).**
3. 🔴 **RETRACTED BY F387 — the sink's fate does NOT predict `sync_shape`.** This line used to read
   "the arm lost the tier-A check while its integrator was cut off". Measured across the four cells:
   `sync_shape` is **0.00 in r0 (capped at 1800.1 s), r1 (idle-stalled) AND r3** — and **r3's sink
   finished cleanly in 499.3 s with zero failures and posted the arm's BEST score, 0.8157.** r2 ran
   1656.4 s, nearly to the cap, and **kept** it. The correlation was never there.
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
the whole app. Sign unknown. **Settleable at n=1: the shard COUNT (deterministic from the plan) and
any shard reporting clean having enumerated zero commands.** ⚠ **The cost half is NOT settleable at
n=1 and its ">3037 s" band is WITHDRAWN** — F382 measured e2e at 2277.5/425.7/1391.6 across three
cells of the identical config, so 3037 sits inside the replicate spread. **Cost needs ≥3 replicates
per arm, compared as medians.**

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
