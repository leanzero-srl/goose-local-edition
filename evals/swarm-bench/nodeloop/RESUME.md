# RESUME — live state, rewritten 2026-08-05 ~03:20 local

The previous version led with *"THE ONE THING THAT MUST HAPPEN NEXT — CLEAR `STOP` AND RESTART"*.
**That is DONE.** It also carried an open contradiction that has since been resolved against me. A
stale resume file tells the next reader to redo finished work and hides the thing that is actually
open — so this file is rewritten, not appended to.

## ✅ There is no outstanding obligation. The sweep is running.

```bash
cd ~/Projects/goose/evals/swarm-bench/nodeloop
./loop.sh status          # expect: RUNNING pid=80288
```

Restarted 02:32:31 with **`MIN_REPS=5`** loaded, `STOP` cleared. If it is ever NOT running, start it:
`./loop.sh start` — **bare, never piped.** A pipeline's exit code is not the command's, and
`start | tail` once returned 1 while having started successfully (F298).

⚠ **A `>>>` line is written ONCE PER UNIT and a unit runs ~2 hours. A quiet `loop.log` is normal**
— treating that silence as a failed restart cost a whole tick (F299/L161).

## 🧊 The engine is still frozen (F253)

`complete()` keys on `engine_build`, so any boundary mid-curve voids every cell collected. Do not
rebuild, do not `cargo check`. **Reading source is free. Instrument fixes ship freely.**

**THREE ENGINE PATCHES REMAIN QUEUED AND UNCOMPILED.** The next boundary MUST run
`cargo clippy --all-targets -- -D warnings` BEFORE deploying (L108: `cargo build` skips
`#[cfg(test)]`).

| commit | what | status |
|---|---|---|
| `f1a20c99b` | scouts gated on `straggler_stop_degrade`, not `straggler_stop` | F256, unverified |
| `95b36748f` | `rules_delivered` also emits `rules_sections` | F259, unverified |
| `00563c6ea` | planner gets Σ device weights, not `devices.len()` | **premise now CONFIRMED on the wire** — `skeleton_drafts` carries `worker_count: 3` while the fleet has SIX slots |

F276's proposed patch at `judge.rs:459-470` is **WITHDRAWN, not deferred** — F292 showed the backstop
it would have added already exists and already fires.

## 🎯 GOAL ONE — the node curve

**Protocol frozen in `PREREGISTERED.md`. The verdict is mechanical: `python3 curve.py` with an
ABSOLUTE path. NEVER compute p by hand.**

    CELL 1  baseline-n3-r0   wall 7725.4 s · score 0.6595 · A .8333 B .3194 C .8571 D .705
                             prefix 2218.7 (redraft 1) · 2 tasks failed
    CELL 2  baseline-n3-r1   wall 8488.0 s · score 0.478  · A .8333
                             prefix 1330.0 (redraft 0) · 21 done / 1 failed (integrate-verify, 3 attempts)
    CELL 3  baseline-n3-r2   IN FLIGHT · prefix 1316.0 (redraft 0, plan_confidence 88)

`complete()` reads **True / True / False / False / False** for (3,0)/(3,1)/(3,2)/(3,3)/(1,0).
`baseline-n3-r2`'s OLD stored row was a void 2-node refusal, which is why the curve re-runs it before
reaching the n1 arm — that is correct, not a bug (F299).

## 🔴 The two things a new reader is most likely to get wrong

**1. THE F296 CONTRADICTION IS RESOLVED, AND IT WAS MY INSTRUMENT.** For several ticks this file's
predecessor implied the sweep's scores might be untrustworthy. They are fine.
**8 of 8 built apps hardcode `http://127.0.0.1:89xx`; 0 of 8 read `MERIDIAN_BASE_URL`.** Those env
vars exist in exactly one place in the bench (`score_build.py:507-508`) and **nothing reads them** —
dead code. My rescore bound a fixture on 8500/8501 and the apps never spoke to it. Every number from
it is void. **F289's bimodal tier B is NOT a scorer artifact** (F304/F305).

**2. OFFLINE RE-SCORING CONFLICTS WITH A RUNNING SWEEP, STRUCTURALLY.** `sweep.py:59` sets
`PORT_BASE = 8930` and assigns vendor ports upward per unit, and each app bakes in the port it was
built against. So an archived tree can only be re-scored on **its own** port, inside the range the
live sweep uses. **Re-score only BETWEEN units**, and check the specific port:
`lsof -nP -iTCP:8931 -sTCP:LISTEN`. `sweep.py:1025-1030` records a leftover listener that held 8931
for eighty-two minutes and failed the next unit outright.

## The findings a new reader most needs

- **F303** — the redraft is a DISCRETE branch on `plan_confidence` vs `ask_floor` 85, and the split is
  clean across **seven** cells: no-redraft `1091.3 · 1148.9 · 1316.0 · 1330.0`, redraft
  `1730.9 · 2218.7 · 2839.0`, **gap (1330.0, 1730.9) still empty**. Confidence lives in `plan_loaded`
  in `run.jsonl`, **not** in the stored `prefix` blob (every row reads `None`).
- **F300** — the sink is **deliberately exempt** from `over_reading` (`judge.rs:373-378`) and left to
  the idle `worker_timeout`, because applying the check *"GUARANTEES it is killed"*. That comment
  names a past run that *"reported FAILED though the app works"*, and cell 2 has the same signature.
- **F294** — `worker_timeout_secs` = **420** is an **IDLE-gap** timer, not a wall-clock cap
  (`swarm.rs:11321`). The knob documentation calls it a wall-clock cap with default 900; both halves
  are wrong.
- **F295** — `task_completed` has **no `ok` field**, it has `status`. Use `run_finished.report`, never
  a hand-rolled tally; `.get('ok', True)` on an absent field reads every run as clean.
- **F290** — the stall detector was disarmed by omitting a flag; state now carries forward.

## Instruments (never re-implement one — L2, violated three times this session)

`curve.py` verdict · `occupancy.py` occ-3 · `dispatch_audit.py` da-3 · `reaudit.py` in-place row
migration (**use it whenever `audit_version` goes stale — it saved ~2h15m of re-runs**) ·
`goalstate.py --tick` (carries state forward; `--self-test`) · `review.py` · `failures.py` ·
`sweep.py` · `loop.sh {status,stop,start,boundary}` · `promptbench.py` (needs the fleet — never during
a measured cell).

⚠ `occupancy.py` / `dispatch_audit.py` / `curve.py` need an **ABSOLUTE** path.
⚠ `git add` must run from the **repo root**; from `nodeloop/` it fails on a doubled path.
⚠ `cd` drifts between Bash calls — re-`cd` at the start of every command.

## Fleet

3 LM Studio nodes, `PARALLEL 2` ⇒ **6 slots**. **NEVER load, unload or re-alias anything in LM
Studio.** If the engine cannot use three identical nodes, that is a `swarm.rs` bug, not a fleet
question.
