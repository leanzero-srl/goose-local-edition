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

## Cross-variant finding: txn exec-GET breakage on BOTH variants
- MLX txn r3 AND GGUF txn r6 both built a txkvbench whose `exec` GET prints NOTHING (golden empty) while --help + the generated pytests pass. It appears on both runtimes -> a WEAK-MODEL coding limit on that spec's multi-command exec path, NOT variant-specific. GGUF r6 additionally had a 3-of-4 task failure cascade (exit=1).
- This strengthens the low-conf swarm idea (record, weigh after): the smoke/integrate-verify gate runs the model's OWN generated tests (which pass here) so it stays blind to a broken feature the tests don't cover; running a SPEC-DERIVED representative command would catch it. The harness golden caught it both times, so the benchmark grading is doing its job.

## Harness finding: golden-check false-partials (judge by running matters)
- GGUF crud r10 recorded checks=partial but the app is fully correct when run (value 25, json valid, pytest pass). A golden check failed transiently at grade time (likely a shared bench.db state collision across the sequential command_succeeds checks) though the app is fine.
- FUTURE-SUITE fix (do NOT change now — would break the paired MLX/GGUF comparison): isolate each golden check's db (unique/temp file per check, or rm between checks) so check-state can't collide. For NEXT benchmark suites only.
- Verdict implication: raw checks-pass-rate UNDERSTATES true functional quality (esp. GGUF: 2 of 3 non-passes are artifacts — capped-but-correct + false-partial). Report BOTH raw checks-rate AND a judged-by-running "app actually works" rate.

## Strengthened: tests-green-but-feature-broken on txn (now 3 instances)
- MLX txn r3 (exec-GET emits nothing) + GGUF txn r6 (exec-GET broken + task cascade) + GGUF txn r12 (COUNT command missing) — all 3 had PASSING generated pytests but a broken/missing REQUIRED feature the tests didn't cover. Pattern is clear + cross-variant = weak-model completeness limit on the multi-command txn spec.
- RAISES the confidence on the swarm fix (was low): the smoke/integrate-verify gate runs the model's OWN generated tests, so it is structurally blind to a required feature the model forgot to test. FIX (now MED): have the gate ALSO run a spec-derived representative command (or check each documented subcommand exists/exits-0), not just the generated tests. Weigh + apply after GGUF finishes with the rest of the batch. GATE any swarm.rs change.

## tail-churn confidence raised (2 GGUF crud caps: r4 + r13)
- Post-build tail-churn now confirmed RECURRING: GGUF crud r4 AND r13 both built a correct app then hit the 3600s cap in judge/pre-review/re-dispatch churn (46+ judge_verdict events). 2/5 GGUF crud runs capped; MLX crud never capped. Raise the deadlock-recovery + tail-churn fix to a clear MED-HIGH: the swarm must short-circuit the post-build phases once integrate-verify + checks are green, and/or bound the judge re-judging, so a completed app is not churned past the cap. This is the single most impactful GGUF-side efficiency fix. Apply after GGUF finishes (with the batch).

---
# FIX BATCH DISPOSITION (both benchmarks done — applied vs presented)

## Tail-churn ROOT CAUSE (deep investigation, HIGH confidence)
Not a re-judge/re-dispatch loop. The `integrate-verify` SINK is a genuinely heavy critical-path
worker (~1400s even when healthy) and the run's ONLY normal exit is `all_terminal()` (every task
Done/Failed, scheduler.rs:1621-1625). The sink is the sole non-terminal task; the judge re-observes
it ~every 60s emitting "ok" — but "ok" (action=observed) is a NO-OP (apply_judge_outcome
1037-1039/1127-1128): there is NO arm that force-completes a healthy-but-slow task. The watchdog is
IDLE-based (900s no-progress); the sink emits an event every turn so it never idles → never trips.
`scheduler.run()` has no run-level wall-clock. So a slow sink runs until the external 3600s cap.
gguf-12 = pathological sink (2124s vs healthy 1414s); gguf-03 = slow detailing phase (1335s) started
the sink too late (2216s). NOTE: the `conf=1.0` on every "ok" is a hard-coded constant (judge.rs
113-114), NOT strength — the model self-rated those OKs as LOW. So the judge's "ok" is a WEAK signal.

## APPLIED (shipped, gated + committed)
- **Sink wall-clock cap (Option B)** — swarm.rs run_agent_in: `GOOSE_SWARM_SINK_CAP_SECS>0` gives the
  `integrate-verify` sink a graceful wall-clock deadline; on expiry it finalizes as DONE (not error/
  re-route) so the run terminates + the smoke gate backstops. Default unset/0 = OFF (byte-identical).
  Confidence no-regression HIGH; bounds the pathological sink (gguf-12). PARTIAL: won't rescue the
  plan-slow case (gguf-03, sink starts at 2216s). Needs a live A/B (run the bench with the flag ON)
  to confirm efficacy + no quality regression.
- **Diagnostics over-eager warn** — verifier/diagnostics.py: low-severity findings (leftover TODO,
  transient failed-then-recovered tool call) no longer warn the dim → correct apps stop reading
  overall=partial (clean-rate was stuck 0%). HIGH confidence.
- **Two-mode harness upgrade** + **mode backfill** — shipped (monitor.py, operator_brain.py, explore,
  frozen banner, mode tags on 30 records).

## PRESENTED — NOT shipped (MEDIUM/lower confidence; need review or A/B first)
- **Sink force-finalize (Option A)** — scheduler.rs apply_judge_outcome: finalize the sink after N
  sustained non-problem verdicts when it's the SOLE non-terminal task. Solves BOTH capped cases
  (incl. plan-slow gguf-03). BUT relies on the WEAK judge "ok" signal; changes load-bearing sink
  terminal semantics; smoke catches crashes/build but NOT subtle logic bugs → a premature finalize
  could ship a green run with a real integration bug the sink was mid-fixing; N=2 evidence. Ship
  env-gated default-OFF + A/B-measure (golden-pass-rate + wall-time) BEFORE any default flip.
- **Deadlock-recovery guard** (MLX compute r2 scheduler_stuck) — a DIFFERENT mechanism (real deadlock,
  stuck tasks block the DAG), one-off. Needs its own trace investigation before a fix; don't ship
  speculative.
- **Gate-runs-spec-command** (3 txn tests-green-feature-broken) — the smoke gate runs the model's OWN
  generated tests + --help, so it's blind to a required feature the model forgot to test. Real, but
  the swarm has no access to the harness golden commands; the tractable version is "assert each
  documented subcommand from --help exists/exits-0". MED, needs design.
- **Golden-check isolation** — future benchmark suites only (isolate each check's db); NOT for the
  frozen suite.
- **Independent lever:** the slow DETAILING phase (1335s gguf-03 vs 426s gguf-06) can doom a run
  before the sink even starts — a tail fix won't rescue plan-slow runs. Separate parallel-planning
  investigation.

## Sink-cap Option B is idle-only (found 06:31 during pillars rate A/B)
Option B caps IDLE-past-deadline but NOT a continuously-active integrate-verify (the tokio::select activity branch keeps winning over the fixed-deadline branch). Add a HARD wall-clock ceiling checked at the TOP of the sink loop (break before the select when now>deadline), gated by the same GOOSE_SWARM_SINK_CAP_SECS. Verified: off-2 ran integrate-verify >40min with 0 cap fires while actively editing tests.
