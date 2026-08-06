# RESUME — live state, rewritten 2026-08-06 ~14:45

> ## 🥇 THE ANSWER TO THE GOAL IS NO LONGER A SCORE — IT IS TWO MEASURED BOTTLENECKS
>
> Mihai asked (07:45) to make 3 nodes beat 1 with **fixes in the engine**. As of 14:45 the honest
> position is: **no score comparison distinguishes 3 nodes from 1**, and chasing one is the wrong
> instrument — the replicate spread is 0.325, wider than every gap ever measured here. What DID
> land is a mechanical explanation, and it is actionable.
>
> **1. THE SINK IS AMDAHL (F436).** `integrate-verify` holds **53.1 / 60.6 / 30.7 / 15.6 %** of the
> dispatch window across the four 3-node cells. In the best cell ever recorded (0.9343), **100 % of
> that 56.2 minutes runs at ≤2 of 6 slots.** `dynamic_replan` is gated on `!sink_in_flight()` —
> **defensible, not a bug**: the sink owns no files and verifies-by-running, so a bonus task landing
> mid-join could ship unverified code after its PASS.
>
> **2. THE PREFIX HANDS BACK THE FAN'S GAIN (F438).** The detail fan scales beautifully — 148 s per
> detail on 1 node vs **13–74 s on 3**. Then:
>
> | cell | drafts | agreement | redraft ladder |
> |---|---|---|---|
> | n1-r0 | 2 | 88 | **0 s** |
> | n3-r0 | 3 | 54 | 821 s |
> | n3-r1 | 3 | 50 | 786 s |
> | n3-r3 | 3 | 52 | **1657 s** |
>
> More nodes ⇒ more skeleton drafts ⇒ structurally lower agreement ⇒ a ladder the small fleet never
> pays (40–57 % of the 3-node prefix). `plan_agreement` is max−min spread + mean pairwise Jaccard,
> and `best_subset_agreement`'s own doc says both *"only worsen (or hold) as the pool grows"*. The
> `diverse_plan` comment already calls this *"the bug that makes the SEQUENTIAL retarget ladder run
> every time"*.
>
> ⚠️ **DO NOT ASSUME THE TAX IS WASTE.** Retargeting cells scored 0.9343 / 0.7147 / 0.8157 against
> 0.6030 / 0.6695 for the two that did not. **The ladder may be buying the quality.** If
> `diverse_plan` ENFORCE drops the score below spread, the right fix is **pool-size-invariant
> agreement**, not skipping the redraft — a different change, worth far more than the wall-clock.
>
> ## 📐 STRATEGY CORRECTION — WHAT TODAY ACTUALLY PROVED ABOUT METHOD
>
> Every finding worth having today (F432, F434, F436, F437, F438, F439) came from a **deterministic
> mechanism readout or a code fact**, valid at n=1. Not one came from a score comparison. Meanwhile
> the score work produced two retractions (F430, F431) and a falsification that landed *exactly* on
> its threshold.
>
> There is a standing tension: each engine fix forces a rebuild, `engine_build()` changes, and the
> whole backlog re-runs, so score replicates never accumulate. **That tension resolves in favour of
> shipping** — mechanism readouts survive rebuilds untouched, and they are where all the value was.
> Treat the score as a guardrail (did anything break?) rather than as the primary instrument.
>
> ## 🔨 SEVEN CHANGES BATCHED, AWAITING THE REBUILD AT THE NEXT UNIT BOUNDARY
>
> | commit | what |
> |---|---|
> | `59352f571` | `http_timeout_scan` was blind to `http.client` and affirmed a clean tree over a client that blocks forever. Only the JSON **parser** was ever tested, never the **detector** ⇒ **L242** |
> | `f37b92bf5` | **F432** — the repair loop has NEVER completed a fix: 4/4 at `secs:1200, agent_ok:false`, because `complete_cap_secs` == `fix_cap_secs`. 1200→3000 + invariant test with negative control ⇒ **L244**. ⚠️ unmeasured ship |
> | `086981caf` | the `diverse_plan` shadow was never readable (eprintln only). New `plan_convergence` event with `would_skip_ladder` from one shared predicate — every run now answers the counterfactual free |
> | `ab24ab31c` | **F411 unblocked** — `desc_sha` was on one side only; the "needs a rare 2-retarget cell" requirement was an instrument artifact |
> | `c7674275e` `2ae70657f` | sweep re-prioritised: `sink_review` → cell 1, `diverse_plan` → cell 4 (new arm, armcheck-gated) |
> | `e3949653e` `5065dd0d8` | **F439** — 68 orphaned processes over 4 days holding ports the reaper never watched ⇒ **L246** |
> | `6ab5b157a` | `engine_build` stamped at **dispatch**, not result-write — a mid-cell rebuild used to mislabel a finished cell, and a mis-stamped `failed` row is skipped FOREVER |
>
> **REBUILD PROCEDURE:** `touch nodeloop/STOP` (checked at the top of the unit loop → clean exit
> after the current unit) → `cargo build --release` → `engine_build()` changes from
> `1785993228-235858864` → all units re-run → restart the sweep, which also picks up the new
> QUESTIONS order, the orphan reaper and the dispatch-time stamp.
>
> ## ⚠️ TRAPS THAT BIT TODAY — ALL THREE ARE THE SAME TRAP
>
> **A zero from a wrong query is not a zero.** Three times: a missing `secs` field read as 0.0 %
> idle-fill; `desc_sha` checked at the wrong nesting level (it lives inside `tasks[]`); and
> `import sweep` silently picking up `bench/sweep.py` instead of `nodeloop/sweep.py`.
>
> **A control that passes for the wrong reason is not a control.** The orphan reaper's ancestry
> guard "passed" only because the live engine happened to be younger than the age floor. With the
> age guard disabled it failed. `protect_root` is a parameter specifically so that guard can be
> exercised at all.
>
> ---


> ## ⛔ READ THIS FIRST — THE SECTION BELOW IS NO LONGER TRUE (13:45, F427/F430/F431)
>
> `baseline-n1-r0` re-ran on the new binary at 13:15 and delivered **both** falsifiers this file
> registered against itself, in the same cell.
>
> **1. The stable-set gap collapsed.** F413's `+0.1473` became **`+0.0500`** — *exactly* the
> falsifier threshold, not one hundredth of margin. Only 6 of 24 checks moved, 2 of them AGAINST
> 3 nodes. Remove the top mover and it is `+0.0087`; remove the top two and **the sign flips to
> `-0.0364`**. And the top mover is now fully explained as a **library coin-flip**: n3 wrote
> `urlopen(req, timeout=30)`, n1 wrote `http.client.HTTPSConnection(host, port)`.
>
> **2. The thin briefs are gone — F417 IS RETRACTED.** Same unit, same dir name, new binary:
> min brief **166 → 1031**, THIN **2 → 0**. Every 3-node cell is 1039–1062; the 1-node cell is
> 1031. **Indistinguishable.** The 166-char briefs were on `store` and `meridian`, the two repeat
> `detail_fallback` victims of this whole campaign, and the new cell records `detail_fallbacks: []`.
> A thin brief is **a detail call that timed out**, which tracks fleet load, not pool size.
>
> **What is true now:** two pairs lean toward 3 nodes (+0.1473, +0.0500), neither survives removing
> its top mover, and the identical-config spread is **0.325 — wider than both gaps combined.**
> There is still no measurement that distinguishes 3 nodes from 1.
>
> **The durable win from this cell is an ENGINE FIX, not a number** — see `59352f571`: the run's own
> `http_timeout_scan` affirmed a clean tree over a client that blocks forever, because it knew
> `requests`/`urlopen` but not `http.client`, and the only test covered the JSON *parser* rather than
> the detector. Fixed and verified in both directions on the live trees. **⇒ L242: a check is tested
> when its DETECTOR runs, never when its parser does.**

---

## 🥇 ~~THE HEADLINE FLIPPED — AND BOTH HALVES WERE PRE-REGISTERED~~ (RETRACTED — see above)

**1. On the pre-registered stable check set, three nodes beats one by +0.1473 (F413).**

F409 declared the 24-check stable set and the falsifier `abs(gap) < 0.05` **while the 1-node cell was
still executing**, precisely so the result could not be fitted afterwards. Measured: n3 **0.5871** vs
n1 **0.4399**. **My own prediction was falsified, three times past its threshold, toward the goal.**

n3 wins **8** stable checks and loses **4** — and **all four losses are downstream of ONE defect**: an
app whose `serve()` starts `serve_forever` on a daemon thread and returns, so the process exits and
nothing binds. **Three nodes built a richer app that does not start.**

**2. Three nodes never degrades a task brief; one node does (F412 -> F417).**

| | min instruction | THIN (<300 chars) |
|---|---|---|
| all four 3-node cells (retargets 0-3) | 1039-1062 | **0** |
| the one 1-node cell | **166** | **2** — `store`, `meridian` |

F412 found this and REFUSED to claim it, because that 3-node cell had retargeted twice and the
advantage might have come from re-detailing. **F417 settled it**: `baseline-n3-r2` retargeted **zero**
times, took the 1-node cell's exact planning path, and still had **zero** thin briefs. **It is the node
count, not the redraft.**

⚠️ **The 1-node side of both results is n=1.** A second 1-node cell without thin briefs, or a stable-set
gap that collapses, weakens this badly. The sweep will produce them and they must be believed.

---

## 🔗 THE CHAIN — every link measured, the composition not

1. **3 nodes builds the richer app** — +0.1473 stable-set, pre-registered (F413).
2. **One startup bug masks it** — all four n3 losses trace to the daemon-thread `serve()`.
3. **F408 detects that bug — 3/3 against the scorer, no false positives** (F418).
4. **F398 proved this fix loop repairs what it is handed** — live: `http_timeout_scan` findings 2 ->
   `complete_verify` passed:false -> fix loop -> round 1 findings **0**; the scorer independently
   agrees (`client_timeouts` 1.00, a check **all four** archived 3-node cells scored **0.00**).

**The composition is untested. The running sweep is the test.**

---

## 🏆 THE NEW BINARY'S FIRST CELL: 0.9343 — THE BEST EVER RECORDED HERE (F426)

Previous best 0.8157. `server_runs` **1.00** · `sync_shape` **1.00** · `total_field` **1.00** (all 247
payments) · `client_timeouts` **1.00**. **The first cell where the app both STARTS and SYNCS** — the
sync family F390 sized at 51->5 pairs and F405 showed is the only place the arms differ.

- ✅ **F400 confirmed deterministically**: `spec_contract verified: 2` in both rounds, against
  `verified: 0` in **9 of 9** archived events. Independent of the score.
- ✅ **The engine refused a false green on its own best result**: `complete_result passed: false,
  verified: false` on a 93% app. That is F395/F408/F419 working.
- ✅ **F425**: one live `sink_capped` settled F386 AND F369 — `cap_secs 3371, cap_base_secs 1800,
  tree_bytes 56188`, the within-row identity exact, scaling live at 1.87x. That sink was CAPPED and
  the cell still scored 0.9343 (second time a capped sink cost nothing).
- ⚠️ **n=1, and the identical-config spread here is 0.325 — larger than the gap to the previous best.**
  This shows the CEILING is higher, not that the binary caused it. The deterministic halves need no
  replicates; the score half needs the sweep.

## 🟢 RIGHT NOW

- **Sweep pid 23655**, `ppid 1` (detached, survives the session). 73 units, ~2 days.
- **`target/release/goose` = 08:13**, carrying **F400 · F408 · F411 · F415**.
- Analysed cells parked in **`runs/nodeloop-parked-1785993855/`**; `runs/nodeloop/` is the live sweep.
- **Fleet: 3 distinct identifiers = 3 devices x weight 2 = 6 slots.** `gabee` (Mac.lan) is at **65536**
  ctx, the other two at 200192 — 200192 FAILED on Mac.lan (LM Studio estimated 57 GB).

```bash
cd ~/Projects/goose/evals/swarm-bench/nodeloop
python3 review.py                 # THE TICK — not ad-hoc status queries (L233)
~/.lmstudio/bin/lms ps            # STATUS is column 3. Shows MODEL state, not agent state.
rm -f STOP && ./loop.sh start     # BARE, never piped
```

| cell | score | server_runs | client_timeouts |
|---|---|---|---|
| n3-r0 | 0.3895 | 0.00 | 1.00 |
| n1-r0 | 0.3313 | 1.00 | 1.00 |
| n3-r1 | 0.7147 | 1.00 | 1.00 |
| n3-r2 | 0.6030 | 1.00 | 1.00 |
| n3-r3 | 0.8157 | 1.00 | 1.00 |

⚠️ **Identical-config spread is 0.325** (r0 vs r1). Two cells is never a result here.

---

## ✅ SHIPPED AND VALIDATED

| # | change | evidence |
|---|---|---|
| **F398** | deterministic no-timeout AST detector | 5/5 offline vs the scorer, then **worked end to end live** |
| **F400** | `spec_contract` spawns the spec's OWN advertised invocation on a port we choose | inconclusive 9/9 became `verified:3` on 5/5 parked apps |
| **F408** | an app that will not bind under its own documented command is a **FINDING**, not `inconclusive` | **3/3**, no false positives (F418) |
| **F395** | a round that verified NOTHING can no longer read as a pass | confirmed live |
| **F419** | the `passed:true` over-claim | closed by F408, verified by code read rather than assumed |
| **F386 F391 F392-394 F403 F411** | instrumentation: effective sink ceiling, unprobed-endpoint disclosure, 43 baked-ON levers that fail the build if they do not declare themselves, `build_sha` unfrozen, `desc_sha` | each falsified in both directions |

## ❌ SHIPPED AND FALSIFIED

- **F415** (architect prompt: split fat test subtasks) — **did not land.** New-binary plan test briefs
  1955 / 2036 / 2321 / 1659, no siblings. **My mistake**: I put "keep each test subtask small"
  immediately before the existing "make each per-module test THOROUGH". Left in place — inert, not
  harmful.

---

## 🔴 WHERE THE FLEET'S TIME GOES (F410, from `occupancy.py`)

- **occupancy 0.4265** against **0.9936 achievable on that same plan**
- **3412.2s = 33% of wall BEFORE the first dispatch**, of which **1991.6s is REDRAFT**
- **2637.8s = 26% of wall with only ONE node working** — `verify-e2e::0` 1480.6s + sink 1015.6s +
  `verify::web` only **141.7s**
- MAX USEFUL NODES **2.98** on a pool of 3 — the plan is wide enough; the loss is execution

⚠️ **F406 called the verify barrier the ceiling and F410 CORRECTED IT** — integrated over the run it is
141.7s, 5% of solo time. ⇒ **L234: "I saw the fleet idle" is an ANECDOTE until integrated.**

---

## 🚧 OPEN, WITH THE EXACT TEST EACH NEEDS

1. **F411's purpose** — needs a cell with **>=2 retarget rounds** to compare `desc_sha` across rounds.
   That settles whether the 1991.6s re-detail is pure rework. One retarget is not enough.
2. **The stall mechanism is FOUND and it is MODEL BEHAVIOUR (F423).** Across all 15 stalled tasks with
   activity digests, **13 end their reasoning CLEANLY** — *"let me summarize my findings."*, *"I'll write
   the complete test_api.py now."* — at exactly the point the next thing should be a TOOL CALL,
   `malformed: 0`, after 1-25 real calls. **The model finishes thinking, says what it will do, and never
   acts.** This UNIFIES two failure modes: *"finished WITHOUT writing your owned file(s)"* is the turn
   ending and goose seeing it; *"stalled 420s"* is the same non-action with nothing signalling turn-end.
   ⚠️ **F422 claimed this was a mid-token STREAM DROP and is RETRACTED** — that signature is 2 of 15, and
   I generalised from a single digest (L240). The target is MODEL MOLDING, same class as `act_now_nudge`
   (already ON, measured writes 23.8->48.0%), and it is not solved. **Look at what the worker is told at
   that turn before shipping anything.**
3. **The 08:03 fleet death is unexplained** — graceful `srv cleaning up before exit`, "unloaded by user
   or API request", no crash/OOM/sleep/TTL, nothing in the repo unloads. It can recur.
4. **`lms link set-preferred-device` is still pointed at Mac.lan** — I set it to load `gabee`.

---

## ⚠️ TRAPS THAT COST REAL TIME (all measured today)

- **L236 — `strings` proves PRESENCE, never ABSENCE.** `desc_sha` was absent by `strings` AND by raw
  byte grep, and the live event carries it. Every negative control rests on this asymmetry.
- **L238 — a corrector needs a MECHANISM, not a correlation.** Four hypotheses died on measurement
  today (F399, F402, F420b, F421). Measuring first is the deliverable.
- **L237** — an instruction that contradicts the sentence after it is a coin flip, not a weak
  instruction. Check what SURROUNDS a prompt edit.
- **L233** — `review.py` IS the tick. I hand-rolled six ticks of dispatch-counting beside it.
- **L230** — a "nothing happened" guard must key only on the affirmative signal; diagnostics are
  signals, and a guard that counts them is switched off by the act of explaining itself.
- `loop.sh stop` did **not** stop after the current unit (it ran two more). Re-check before relying.
- A bare pipe character inside a PREDICTIONS line breaks the pipe-delimited format.
- Backticks in a double-quoted `git commit -m` run as command substitution — use `-F -` + heredoc.
- `nohup ... & disown` and python `Popen(start_new_session=True)` BOTH die here. Run long commands in
  the FOREGROUND and let the harness background them.
- Absolute paths always; `git add` from the repo root.
