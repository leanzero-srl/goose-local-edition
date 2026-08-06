# RESUME — live state, rewritten 2026-08-06 ~09:45 local

Seventh rewrite. The 13:50 version is REPLACED rather than patched because **its headline is now
false**: it said "3 nodes is NOT better", and two independent pre-registered measurements since then
say otherwise. Full history is `FINDINGS.md` (F386-F421); this file is only what is TRUE NOW.

---

## 🥇 THE HEADLINE FLIPPED — AND BOTH HALVES WERE PRE-REGISTERED

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
2. **The stall mechanism** — long-brief test tasks fail by stalling MID-RUN (only 3 of 16 near the 420s
   first idle window; the rest 507-1592s). Needs a stalled task with a resolvable `session_id` (~1 in 5
   have none). **Do NOT build a corrector before this — F421.**
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
