# Push-to-real-completion — verify → fix → re-verify until green, with research escalation (user direction, 2026-07-03)

DESIGN AFTER the current pillars rate A/B finishes (do NOT rebuild the binary mid-A/B). Research + design may start now (no fleet, no rebuild). This is the user's next top direction for the swarm.

## The user's vision (verbatim intent)
- Goose must **NOT let errors slip** — push to **REAL completion**, never deliver an app that just doesn't work.
- **Test the app it just built** — Playwright (or other browser means) for web/UI; for CLI/backend, **run the actual advertised commands with golden checks** (even simpler + better).
- **Iterate on its own until it FIXES the issues.**
- After **2 attempts**, **compact/distill the error** and **search online for solutions** (do real research), then apply.
- **Acceptance (the user's success test):** (1) is this giving a **functional + reasonable result**? (2) is it **reducing the operator's headaches**? Both yes = success.

## Why this is live right now (observed)
- Pillars rate A/B, on-1: `report budget` interface HELD (pillars worked) but **pytest FAILED** and "1 core subtask failed" — the run still FINISHED and delivered the broken app. Exactly the "errors slipped, not pushed to real completion" failure the user is calling out.

## Current state (what exists — grounds the design)
- `GOOSE_SWARM_SMOKE` — post-run smoke gate (`pytest --collect-only`, `python -m <pkg> --help`) + a ONE-SHOT `smoke_fix` re-dispatch. Not a loop; one try.
- `integrate-verify` sink — runs the test suite + builds/runs the advertised entry point + fixes, but bounded by worker_max_turns and can END red.
- `GOOSE_SWARM_REVIEW` — AST wiring reviewer + `wire-fix` (one-shot).
- `GOOSE_SWARM_GOALS` pillars (SHIPPED part 1+3+5): distilled pillars + injected + judge-on-goals. Part 4 (review-checks-pillars-and-mutate) is a SUBSET of this push-to-completion loop.
- Missing: a HARD verify→fix→re-verify LOOP that (a) refuses to report done while a deterministic check fails, (b) distills the error between attempts, (c) after 2 failed attempts escalates to WEB SEARCH (the web-search MCP worker extension) for a solution.

## Proposed feature: `GOOSE_SWARM_COMPLETE` (default OFF, gated) — the completion loop
1. **Verify** (deterministic, model-free where possible): run the app's own test suite (pytest green?), run the ADVERTISED entry-point golden checks (the pillar `check` hints when GOALS on; the spec's example invocations otherwise), and for web/UI a Playwright navigate/click/assert/screenshot. Collect concrete failures.
2. **Distill the error**: compact the failing traceback / wrong-output into a SHORT, targeted fix instruction (weak models drown in long tracebacks). One failure → one crisp fix directive naming file + symptom + expected.
3. **Fix**: re-dispatch a bounded fix subtask whose description IS the distilled error (reuse smoke_fix/wire-fix scaffold + DispatchRequest + prior_hints).
4. **Loop bound**: `GOOSE_SWARM_COMPLETE_ROUNDS` (default 2-3), no-progress/oscillation guard (same failure twice ⇒ escalate, don't re-try identically), tied to the sink wall-clock (GOOSE_SWARM_SINK_CAP_SECS).
5. **After 2 failed attempts → RESEARCH escalation**: attach the web-search MCP worker extension to the fix worker, hand it the distilled error, have it research a solution online (+ Context7 for library APIs), then apply + re-verify.
6. **Final gate — never deliver broken**: if hard checks still fail at the bound, do NOT report a clean done — surface loudly (report the exact failing checks) so the operator knows, rather than a silent broken delivery.

## Acceptance (the user's own test — bake into validation)
- FUNCTIONAL + REASONABLE result: after the loop, the delivered app passes its tests AND its advertised commands produce the golden outputs, more often than without the loop.
- REDUCES operator headaches: fewer delivered-but-broken apps; when it can't fix, it says so instead of pretending.
- Validate in exploratory: ON vs OFF on specs that currently deliver red (like on-1) — does the loop turn red→green, and does the research escalation ever rescue a stuck fix?

## Risks (honest)
- Infinite / oscillating loop on a weak fleet → mandatory round cap + no-progress guard + wall-clock tie-in (the sink-cap idle-vs-active limitation applies — needs the hard-ceiling follow-up too).
- Flaky/mis-written test failing a CORRECT app → deterministic-check-first, distinguish "app wrong" from "test wrong" (a pillar/golden check on the real entry point is more trustworthy than the model's own unit test).
- Web search returns junk / hallucinated fix → constrain to the distilled error, prefer Context7 for library APIs, re-verify after applying (never trust the fix, verify it).
- Cost/time blowup on the weak fleet → the round + wall-clock caps; log what was dropped.

Related: [[commit-every-change]]; composes with GOALS/PILLARS (part 4), SMOKE, REVIEW, integrate-verify, the sink-cap + its hard-ceiling follow-up.

---

## GROUNDED DESIGN (research workflow wquu0vz8g, 2026-07-03) — implementation map

### ROOT CAUSE (why a red app ships green today)
- `scheduler.run` -> `complete()` sets `TaskState::Done` UNCONDITIONALLY on an Ok dispatch (scheduler.rs ~556). Done == "the model stopped cleanly", NOT "the app works". No correctness predicate.
- GOOSE_SWARM_SMOKE (swarm.rs 7087-7154) + REVIEW (7156-7249) are default-OFF + ADVISORY — never folded into `report` or the exit code (7355-7372: Ok iff core_failed==0, smoke/review not consulted).
- 4 holes: integrate-verify owns no files (completion guards no-op, 5953); test-dep stripping (red suite blocks nothing, 1241-1272); salvage flips failed->Done (scheduler 1068-1110); sink-cap finalizes integrate-verify as Done on timeout (2771-2776). Net: on the default path nothing ever RUNS the app. THIS is on-1 (interface held, pytest red, still delivered).

### GOOSE_SWARM_COMPLETE (default OFF, off-path byte-identical) — insert after scheduler.run (7085) before run_finished (~7264), wrapping the smoke block (7087-7154)
- Flags: GOOSE_SWARM_COMPLETE (master, off), GOOSE_SWARM_COMPLETE_ROUNDS (u32 default 2, clamp 6), GOOSE_SWARM_COMPLETE_CAP_SECS (loop wall-clock, mirror sink-cap 2749-2776), GOOSE_SWARM_COMPLETE_WEB (Playwright sub-flag, off). Parse like GOOSE_SWARM_SINK_CAP_SECS.
- B1 VERIFY (deterministic) -> `run_complete_verify(root,lang,pillars) -> CompleteVerdict{passed,failures:Vec<Failure{kind,test_id,exception,file_line,expected,actual,raw}>}`: REUSE run_smoke_gate (3926: pytest + python3 -m pkg --help) [add `pytest -x --tb=short` in the python arm 3972-3991 so one directive/round]; + advertised-command golden checks from Pillar.check (make it load-bearing: extend distill_pillars 3155 + pillars_schema 6254 to emit a runnable check+expected) = PART 4 realized; + Playwright (browser_snapshot, role/testid) behind GOOSE_SWARM_COMPLETE_WEB only.
- B2 DISTILL -> `distill_failure(&Failure)->String` (generalize smoke_fix_description 5429): 3-slot "test X failed with EXC at file:line, expected A got B, fix FN in file" — strip site-packages frames, keep last user frame.
- B3 FIX -> reuse smoke-fix scaffold (7118-7132): DispatchRequest{task_id:"complete-fix", description:distilled, attempt:round, all_files:smoke_all_files, prior_hint:last_lesson}; smoke_fix_dispatcher.run(fix_req). Reflexion lesson via prior_hint (dispatch.rs 54-56 -> SUPERVISOR NOTE 5924-5930).
- B4 BOUND: for round in 0..rounds { verify; emit verify_round event; if passed break; sig=failure_signature(hash test+exc+file:line+msg); if sig==prev break/escalate (no-progress); if wall-clock cap break; dispatch fix }. Green-set regression DETECTION+report in v1 (defer auto git-worktree revert).
- B5 RESEARCH (round>=2 OR sig-repeat OR ImportError/ModuleNotFound class): attach context7+web-search via build_worker_extension (2379-2459, auto-OFF without secrets); research_exts field on dispatcher (~2472); merge into extensions at run_agent_in (5939 / attach 2694-2700); extend SUPERVISOR NOTE (5924-5930) to say research THIS error first. NEVER trust — re-run full VERIFY; adoption gated on green only. Prefer context7 for lib-API errors.
- B6 FINAL GATE (THE key change): after loop, if complete_on && !final.passed -> emit complete_result event with exact failing checks + return Err at the exit-code site (7360-7371) so a red app can NO LONGER exit 0. Off-path (complete_on=false) leaves 7355-7372 untouched.

### CONFIDENCE: VERIFY+exit-gate (B1/B6) HIGH (fixes on-1); DISTILL (B2) HIGH; FIX (B3) HIGH; loop-bound detection (B4) HIGH, auto-revert MED (defer); pillar-check goldens (B1.2/part4) MED (weak model may emit wrong golden -> keep advisory if no confident golden; prefer spec's literal example invocations); research (B5) MED, spec-alignment sub-check LOW (rely on hard re-verify); Playwright LOW (sub-flag, ship CLI/golden 70% first).

### BUILD ORDER (each gated build+clippy -D warnings+test+fmt, commit only my file):
1. VERIFY (reuse smoke) + DISTILL + bounded FIX loop + FINAL GATE exit-code change. <- fixes on-1, HIGH conf.
2. Pillar.check goldens (part 4 realized).
3. RESEARCH escalation (web-search/context7 MCP).
4. Playwright web oracle (sub-flag).
Validate ON-vs-OFF on red-delivering specs (on-1 class): red->green rate; does research rescue a stuck fix; "delivered done while red" -> 0 (the gate). Acceptance = functional result up AND broken-deliveries down.

---

## QUANTIFIED BASELINE (workflow wsiqec8th, 2026-07-03) — grounds the feature in numbers

Two sources: (A) deep-verify of N=7 completed runs (self-report re-joined to actually running pytest+entry+golden); (B) the 30-run benchmark CSV (checks column = external golden verdict, independent of self-report).

### The problem, measured
- **Delivered-broken rate (benchmark N=30): 5/30 = 16.7%** fail >=1 golden check. By variant: gguf 3/15 = 20%, mlx 2/15 = 13.3%. By archetype: crud 9/10, compute 9/10, **transaction 7/10 = 70% (weakest — 3 of the 5 broken deliveries)**.
- **Silent-broken (exit-0 while a real check is red): bounded 0% (deep N=5 exit-0, too small) .. 16.7% (benchmark upper bound).** Honest framing: design against the upper bound — up to 1 in 6 delivered apps is broken, and any broken app that also exits 0 is a silent lie the gate must catch.
- HONEST nuance: in the deep sample the swarm's done/failed accounting was actually correct (the 2 broken runs exited non-zero). pab-on-1 (interface held + golden flow right + pytest 1-failed) is the exact TARGET shape, but it exited non-zero — an honest near-miss, not a silent lie. So the feature's value is: (a) the exit gate STRUCTURALLY guarantees 0 silent, (b) the fix-until-green loop RECOVERS broken-at-first-verify runs (raises green rate), (c) honest red for the unrecoverable.
- Failure kinds (deep sample): pytest_red 1 (round() 10.555->10.55 vs half-up 10.56), entry_broken 1 (__main__ never calls main(), missing import, 0 tests collected), golden_wrong 0.
- 2/30 hit the 3600s wall-cap (both gguf/crud) but PASSED golden — time-failures, not broken deliveries (guardrail: the fix loop must not blow the cap for more runs).

### Quantified TARGET (re-measure ON vs OFF, same 30-run benchmark, tier medium, gguf+mlx)
Part A (functional UP): (1) delivered-actually-green 83.3% -> >=95% (>=28/30); (2) transaction 70% -> >=90%; (3) NEW fix-loop recovery count (broken-at-first-verify -> green after fix/research).
Part B (broken deliveries DOWN): (4) silent-broken count -> 0 (primary gate assertion); (5) exit-honesty (broken runs that exit !=0) -> 100%; (6) total delivered-broken 5 -> <=2.
Guardrail (7): wall-time p50/max + cap-hit count must not rise materially (OFF 2/30 capped).
Success = "0% silent + as many recovered as the loop can honestly close, with the rest loudly red" (NOT "100% green").

### Measurement caveat to fix in the ON/OFF re-run
The exit-code <-> checks per-run JOIN is the single biggest gap (the benchmark CSV lacks it; deep N=7 too small). PYTEST_EXIT capture was unreliable under zsh (PIPESTATUS vs pipestatus) — read green/red from pytest summary lines, and fix the capture so exit-code<->checks joins per run in the re-measurement.

---

## V2 ARCHITECTURE — FLEET-PARALLEL MAP-REDUCE for the tail phases (user direction, 2026-07-03)

USER INSIGHT: the tail phases (integrate-verify, smoke-fix, review, judge) run on ONE node while 2 sit idle. Split them into per-model sub-agents (fan across the fleet's models, not one model), each returning a SUMMARY (context savings — critical on weak models that degrade with big context). Do this in ANY phase EXCEPT the actual file-WRITE, which must be partitioned to avoid conflicts.

KEY: the primitive ALREADY EXISTS — `fanout_over_fleet` (swarm.rs ~4247), used by SCOUT (parallel research), parallel_plan (fleet details every subtask), and generate_contracts (one stub sub-agent per module). The read/generate phases are already fleet-parallel + reduced. The GAP is the tail verify/fix, which is single-task/single-node.

THE DIVIDING LINE (user's, correct): READ/ANALYZE (verify, judge, review) = fan across all models FREELY (no write conflict), each returns a distilled summary -> reduce. WRITE (fix) = PARTITION BY FILE (reuse EXECUTE's non-overlapping owned_files), one fix-agent per file-group across the fleet; only same-file failures serialize.

RESHAPES GOOSE_SWARM_COMPLETE into a map-reduce loop: (1) VERIFY map = per-module verification fanned across the fleet -> failure summaries; (2) REDUCE = dedup into one failure list (distill, parallel); (3) FIX map = group failures by file -> one fix-agent per file-group across nodes; (4) RE-VERIFY map. Bounded. Kills BOTH the correctness gap AND the idle-tail throughput (all 3 nodes attack independent failures instead of gabee grinding 33min while mihai/workhorse idle).

SEQUENCING (recommended): v1 = the SERIAL loop + the EXIT-CODE GATE (the load-bearing correctness fix — red cannot exit 0; high-confidence, no parallelism needed). v2 = the fleet-parallel map-reduce (this section) — bigger surface (the reduce/dedup, the by-file partition, keeping the round-bound honest across parallel fixes), own gate + A/B, MED confidence (risk: partition mis-grouping same-file failures; reduce on weak models). Reuse fanout_over_fleet + the DAG owned_files partitioning.

RELATED THROUGHPUT: the idle tail is ALSO addressable by GOOSE_SWARM_SPECULATE (race a twin of the longest in-flight task on an idle node). The map-reduce fix is the more direct win for the fix phase specifically.
