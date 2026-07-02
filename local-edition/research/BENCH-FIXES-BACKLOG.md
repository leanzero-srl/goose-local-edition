# Benchmark fixes backlog — APPLY ONLY AFTER BOTH MLX + GGUF RUNS FINISH

**Why deferred:** MLX and GGUF must run the benchmark on the IDENTICAL, unchanged binary +
config for an apples-to-apples comparison. Any swarm code fix or knob change between the runs
would rebuild/alter the binary and confound GGUF's results. So: run MLX → record findings here
(don't apply) → run GGUF on the same binary → record GGUF findings here → THEN apply everything
at once, gated + committed. The two-mode harness upgrade is also deferred to this batch (it edits
the bench path cosmetically; keep the bench path frozen across both runs to be rigorous).

Do NOT touch `target/debug/goose`, `crates/**`, `evals/swarm-gym/harness/bench.py`, the SUITE, or
the flags until GGUF has finished.

---

## Proposed CODE fixes (swarm.rs / goose-swarm) — from the MLX run

1. **Deadlock-recovery guard on `scheduler_stuck`** — confidence MED.
   - Evidence: MLX compute run 2 (`rdcalc-mlx-01`) hit a `scheduler_stuck` deadlock — no
     `run_finished`, only 4 of 6 planned tasks completed, the 2 stuck included the
     `rdcalc/__main__.py` entry owner → entry never written → app unrunnable → the post-run
     smoke gate never ran (no smoke event). 30 judge_verdict events before the deadlock.
   - One-off, NOT systemic (compute runs 5/8/11/14 all passed) → downgraded from HIGH to MED.
   - Proposed fix: when the scheduler detects `scheduler_stuck`, (a) mark the un-runnable
     descendants failed so the run terminates cleanly with a report, and (b) still run the
     smoke/entry gate on what WAS built so a partial app at least gets its entry wired + a
     corrective re-dispatch. ROOT CAUSE still to confirm: read the killed-task session traces —
     likely a judge-killed task whose dependents can't run and aren't marked failed → DAG can't
     progress. GATE: cargo build + clippy -D warnings + test -p goose-cli/goose-swarm.

2. **Diagnostics dim over-eager warn** — confidence MED (needs a look at the trigger).
   - Evidence: EVERY MLX run reported `overall=partial` even when `checks=pass` and the app is
     functionally correct (verified by running: crud value, compute 2^3^2=512, txn rollback all
     correct). Cause: the `diagnostics` dim returns `warn` on otherwise-clean apps, dragging
     `overall` down. Makes `overall`/`clean-rate` misleading; the trustworthy signal is
     `checks-pass-rate` (judged-by-running).
   - Proposed fix: investigate the diagnostics trigger (SMELLS regex / zero-tool-call) in
     `harness/verifier/diagnostics.py`; scope or downgrade it so a clean app is not warned, OR
     ensure `overall` doesn't drop to partial on a soft diagnostics-only warn. (Harness fix, not
     swarm.rs — still batch it here.)

## Proposed KNOB TUNING — from the MLX run

- **Idle-node auto-tune** (from the two-mode design's `monitor.py`): starved/underused nodes ->
  re-enable + `pool weight` bump via the tweaker. Two caveats to verify first: (a) does
  `cluster.assess`'s `dispatched` map include zero-count entries for enabled-but-idle nodes?
  (depends on `run_started` carrying the full pool); (b) `BUMP_WEIGHT=3` is a blind absolute set
  — read current pool weights first so it's a real bump, not a no-op/down-tune.
- (Watch for idle-node patterns in the MLX per_device data + the GGUF run before committing a
  default weight scheme.)

## Two-mode harness upgrade (deferred to this batch)
- Apply from the design in the workflow output (`w8z90qz5l`): mode tags, frozen-SUITE banner,
  README modes, `harness/monitor.py`, `harness/brain/operator_brain.py`, brain provider branch,
  cli `explore` subcommand. Then backfill `mode=benchmark` onto MLX + GGUF ledger records.

## From the GGUF run — (to be recorded after GGUF finishes)
- (pending)

---
_Once both runs are done: study both, finalize this list, then apply all at once (gated + one commit per change)._

## MLX final-study additions
- r3 txn functional one-off (exec GET emits nothing; tests pass but golden fails): weak-model coding limit, NOT a swarm-mechanism fix. But it re-confirms the value of running representative commands vs trusting generated tests — the swarm's smoke runs the model's OWN tests (which passed here), so it stays blind to a broken feature the tests don't cover. POSSIBLE (low-conf) swarm idea: have integrate-verify/smoke run a spec-derived representative command, not only the generated tests. Defer + weigh after GGUF (does GGUF show the same class?).
- MLX medium quality: checks-pass 87% (crud 100/compute 80/txn 80), 100% task success, median ~32m/run. Strong. Only fix-worthy MECHANISM item remains the scheduler_stuck deadlock-recovery guard (#1).

## GGUF-run finding: post-build tail churn hits the cap (efficiency)
- GGUF crud r4 (invtrack-gguf-03): app BUILT + CORRECT (8/8 tasks, checks=pass, golden value 18.0) but the run hit the 3600s cap (exit 124, NO run_finished) after 46 judge_verdict + 8 pre_review + 2 re-dispatch events — the swarm churned in post-build phases (judge/pre-review/integrate-verify/re-dispatch) long after the app was done.
- Candidate fix (MED, needs the full trace at end-of-run): short-circuit the post-build tail once the app already passes (integrate-verify + checks green) instead of continued judge/pre-review churn; and/or bound the judge re-judging (connects to the earlier integrate-verify-thrash finding). This is a SPEED/efficiency fix, not correctness.
- Methodology note for the verdict: the 3600s cap MASKS true times for slow runs (both variants). A capped run = "would have been slower + didn't self-terminate". Report capped runs explicitly; do not treat 3600s as the true time.
