# APP-BENCH — overnight functional-app benchmark (2026-06-29 → )

GOAL (user directive): the local-model swarm must deliver a FUNCTIONAL app in 15–25 min — one that
BUILDS + RUNS + is CORRECT, not a finished-but-broken shell that costs hours of debugging. Study the
journey, find why apps are/aren't functional, improve the swarm (esp. the adversarial review must verify
FUNCTIONALITY: catch hallucinations / fake / stub / unfinished impls), re-test. Report: how many apps, functional?

## Per-app assessment framework (answer ALL — be deep, not superficial)
For every app:
1. **Time-to-deliver** (run_started → run_finished, wall-clock). Target 15–25 min.
2. **FUNCTIONAL?** — BUILD (compiles/imports) + RUN (the real primary command) + CORRECT (right output on real input). NOT "did it finish".
3. **Failure mode** (if not functional): fake/stub impl? unfinished module? wrong logic? flaky worker (claim-done-no-write)? unwired? crash?
4. **Was my prompt pointing at a complex-enough app?** (don't only test trivial CLIs).
5. **Did I answer its questions?** (ask-floor runs — answered as the human, concrete?).
6. **Did the local model follow the PHASES correctly?** (research → plan → contracts → execute → smoke → review; any phase skipped/looped/stalled?).
7. **Did the REVIEW push toward something FUNCTIONAL?** (did smoke/AST-review/integrate-verify catch the real defects, or rubber-stamp a broken app?).
8. **Reasoning vs output** — read the worker/planner traces: where did the reasoning diverge from a working deliverable? What ELSE could improve it?

## Batch (diverse technologies × archetypes)
| id | tech | archetype | spec | time | FUNCTIONAL? | failure mode | notes |
|----|------|-----------|------|------|-------------|--------------|-------|
| APP1 | Python | greenfield CLI (moderate) | unit converter (length/weight/temp, --precision, list-units) | ~43.7min | **NO** | flaky hallucinated-completion (5/7 subtasks failed) + CLI spec-drift | files built + runs, but `convert 100 km mi` (the spec's own example) ERRORS — built bare-VALUE CLI, no `convert` subcommand; integrate-verify FAILED but didn't recover |
| APP2 | TypeScript | greenfield CLI (moderate) | CSV column stats (mean/median/mode/stddev, --column, --json) | 66.9min | **PARTIAL** | missing tsconfig.json -> broken build (no dist); very slow | LOGIC CORRECT (mean/median/mode/pop-stddev right, --json works, missing-col -> exit 1, 12/12 vitest pass, runs via tsx) BUT `npm run build` (tsc) emits nothing -> advertised `node dist/cli.js`/bin fails. 7/7 subtasks "done" = false-green on the build |
| APP3 | Rust | greenfield CLI (moderate) | word-frequency counter (--top, --min-length, ties alpha) | 28.1min | **YES (WIN)** | (none — 1 cosmetic warning) | FUNCTIONAL + CORRECT: `wordfreq f --top 3` -> the:3 cat:2 dog:1 (counts + alpha tie-break right), --min-length filters right, --help clean, builds (1 harmless unused-deref warning main.rs:25), 8/8 cargo test. 4/4 subtasks done. Rust delivered where Python failed + TS was partial |
| APP4 | Python | multi-module + INQUISITIVE | habit tracker (add/done/streak/list, JSON) | 46.8min | **YES (functional)** | run REPORTED fail (tests+integrate-verify) but app WORKS — false-NEGATIVE; very slow | FUNCTIONAL + CORRECT: add/done/streak/list work; streak gym=1 after done; REAL persistence ~/.habit.json {"gym":["2026-06-30"]} (ISO dates, survives across processes); store WIRED by the AST-review wire-fix (caught it built-but-unwired — review WIN). Did NOT ask (confidence>=75, ISO default sensible). Run "failed" on tests/integrate-verify despite the app being functional |

## Improvement log (empirical — build only what the failures justify, then re-test)
(pending the first apps' data)

## Running tally
apps attempted: 1 | functional: TBD | avg time-to-deliver: TBD

## Finding (2026-06-29) — the "7s claim-done" = HALLUCINATED COMPLETION
Read-the-logs on APP1 test-converter flakes: ~7s sessions, ZERO write/text_editor tool calls, message text
claims "I produced the file". So the weak 27B sometimes HALLUCINATES completion — emits "done, wrote X" +
calls final_output WITHOUT calling the write tool. The claimed-done guard catches it (file missing/empty) +
the guided retry (92f393495) sometimes recovers, sometimes exhausts. (Worker sessions persist with 0 output
tokens in sessions.db -> study the JOURNEY via the .swarm jsonl + activity digests, not raw worker reasoning.)
TWO failure classes for "functional apps", both targets of the planned review upgrade:
  (A) HALLUCINATED COMPLETION (claim done, no write) — partly handled (guard + guided retry); the deeper
      prevention (force a write before final_output) is in crates/goose core = OUT OF SCOPE; mitigation only.
  (B) FAKE/STUB/UNFINISHED impl (worker WRITES but the body is pass / ... / raise NotImplementedError / TODO /
      a trivial hardcoded return) — NOT currently detected. THIS is the high-value new build: a model-free
      stub/fake detector (extend the AST reviewer) + strengthen integrate-verify to RUN the primary command
      and assert REAL output, so the adversarial review verifies FUNCTIONALITY (per the user's ask).
PLAN: build the diverse app batch, confirm A vs B frequency empirically, then build the stub/fake detector +
functional-verification upgrade, adversarially review, RE-TEST, report functional-app count + time-to-deliver.

## APP1 DEEP ANALYSIS (2026-06-30) — core REAL, INTERFACE drifted; what to change
FAILED: ~43.7min (target 15-25), NOT functional. run_finished done[converter-core, test-edge-cases]
FAILED[cli-entrypoint, integrate-verify, test-cli, test-converter, test-error-handling] = 5/7.
ROOT CAUSES (two, distinct):
1. **CLI INTERFACE SPEC-DRIFT (the functional killer).** The CORE (converter.py) is REAL + correct — convert()
   does real factor math, _convert_temperature() real Kelvin conversion, is_valid_unit/list_units/categories
   all genuine. So the model CAN write correct logic. BUT cli.py built `@click.command()` + `@click.argument(value,
   float)` + `-f/--from-unit -t/--to-unit` — a FLAG-based SINGLE command. The spec said "e.g. convert 100 km mi"
   + "a list-units subcommand". So: `convert 100 km mi` ERRORS ("convert is not a valid float" — click reads
   'convert' as the value), and `list-units` DOESN'T EXIST. The app fails its OWN advertised usage. The smoke gate
   PASSED FALSELY because it only runs `--help`, never the spec's example command.
2. **FLAKY HALLUCINATED-COMPLETION** sank 5/7 subtasks (test-converter claimed-done-no-write 4x; cli-entrypoint +
   integrate-verify + 3 tests failed). Mitigated by guard+guided-retry (not eliminated); deeper fix is core=OOS.

WHAT TO CHANGE — instructions + goose guidance (REPRIORITIZED by this data):
A. **GOOSE GUIDANCE — run the SPEC'S EXAMPLE COMMANDS in smoke + integrate-verify (HIGHEST VALUE).** Extract the
   advertised invocations from the spec ("convert 100 km mi", "list-units") and RUN them verbatim in the smoke
   gate + integrate-verify; FAIL (and re-dispatch a fix) if any errors. Currently smoke only does `--help`, which
   passes while the real interface is wrong. This deterministically catches the exact drift that sank APP1 (and
   echoes A3-2's wrong-path false-green). This is "the review verifies FUNCTIONALITY" made concrete.
B. **ARCHITECT/CONTRACT INSTRUCTION — pin the CLI surface from the spec's examples.** The cli-entrypoint task
   (and the CONTRACTS stub) must state the REQUIRED invocations explicitly ("the CLI MUST expose: `convert <value>
   <from> <to>` and `list-units`"), so the worker can't silently redesign to flags.
C. **WORKER INSTRUCTION — match the spec's EXACT CLI shape** (subcommands + positional args as the examples show;
   do not "improve" it into a flag interface).
REPRIORITIZATION: APP1's core was NOT a stub, so the spec-example verification (A) + CLI-surface pinning (B) are
HIGHER value than the stub/fake detector for THIS failure mode. Build order now: (1) idle-node fix [user top
priority, design ready], (2) spec-example verification A+B [grounded by APP1], (3) stub/fake detector [still
useful for the hallucinated/fake class]. Confirm A's value against APP2/3 before over-investing.

## APP2 DEEP ANALYSIS (2026-06-30) — logic CORRECT, BUILD broken (missing tsconfig); slow
7/7 subtasks "done", 0 failed, but FALSE-GREEN on the build. 66.9min (2.7x over the 15-25min target).
FUNCTIONAL?: PARTIAL. WORKS: src/*.ts is real + correct — `npx tsx src/cli.ts -f f.csv --column x` prints
mean 2.5 / median 2.5 / mode / pop-stddev 1.118 (all numerically correct for [1,2,3,4]); --json emits clean
JSON; a missing column exits 1; 12/12 vitest pass. BROKEN: there is NO tsconfig.json, so `npm run build`
(script = `tsc`) just prints tsc help and emits nothing -> no dist/ -> the advertised `node dist/cli.js`
(package.json start + bin `tsstats`) FAILS with "Cannot find module dist/cli.js". So a user following the
project's own build+run instructions hits a broken build, even though the logic is sound.
ROOT CAUSE: the swarm produced package.json (with a `build: tsc` script + a dist-based bin) but never
generated the tsconfig.json that `tsc` needs -> an INCOMPLETE BUILD CONFIG. The TS smoke/verify did not
catch it (no TS build gate that runs `npm run build` + checks the entry exists).
WHAT TO CHANGE — instructions + goose guidance:
A. **GOOSE GUIDANCE (highest value, generalizes APP1+APP2): smoke + integrate-verify must BUILD the project
   to its ADVERTISED entry and RUN it.** For TS: `npm ci/install` + `npm run build` (or tsc) + assert the
   built entry (package.json bin/main/start target, e.g. dist/cli.js) EXISTS, then run it on a real input.
   For Python: run the spec's example commands (APP1). This single gate catches BOTH APP1 (wrong CLI) and
   APP2 (no dist) — both are "the advertised entry doesn't work" false-greens that --help-only smoke misses.
B. **ARCHITECT/CONTRACT: a TS project MUST include tsconfig.json** (and any config its build script needs)
   as an owned file — list it like package.json, so `tsc` has a config. Generalize: if package.json has a
   `build` script or a dist-based bin/main, the matching build config + a verified build are REQUIRED.
C. **TIME: TS on the 27B is SLOW** (66.9min; one test task alone 18.5min). Data point for the 15-25min goal
   — the TS test phase is a long pole; a future improvement may need to cap/split heavy test tasks or speed
   the build-verify. Note, don't build yet.
This REINFORCES the spec-example/advertised-entry verification as THE top improvement (now grounded by 2 apps).

## APP3 — FUNCTIONAL WIN (Rust, 28.1min) — the first deliverable app
4/4 subtasks done, 0 failed, 28.1min (just over the 25min target but the CLOSEST + the only WIN so far).
BUILD: cargo build OK (1 cosmetic warning: an unused leading deref `*freq.entry(..).or_insert(1)` at
main.rs:25 — harmless, the and_modify/or_insert still does the count). RUN+CORRECT: `wordfreq f --top 3`
-> `the: 3 / cat: 2 / dog: 1` (counts right; ties at count 1 broken ALPHABETICALLY -> dog before mat/on/sat
-- correct); `--min-length 4` correctly prints nothing (no word >=4 chars); `--help` clean (clap). 8/8 cargo
test. WHY RUST WON where Python (APP1) FAILED + TS (APP2) was PARTIAL: (1) Rust COMPILES — a broken build
can't ship green (vs APP2's tsx-bypass + APP1's flaky no-build); cargo build IS the entry verification.
(2) clap derives a correct, conventional CLI from the arg struct -> no CLI-interface drift (vs APP1's
hand-rolled click flags diverging from the spec). (3) Strong typing + the CONTRACTS feature kept the 3
modules coherent. KEY TAKEAWAY: the languages with a COMPILE + a derive-based CLI (Rust/clap) are far more
likely to ship a functional, spec-matching app on a weak 27B than Python (hand-rolled CLI, no compile). This
strengthens the advertised-entry/build-verify improvement (37cfd95fc): forcing a real BUILD + advertised-entry
run is exactly what Python/TS lack and Rust gets for free.

## IDLE-FIX LIVE VALIDATION (APP4, new binary f5cac6468, 2026-06-30) — works + finds defects, but partial
The idle fix (concurrent judge+pre-review, idle_jobs<=idle_capacity) + the PreReview event let me observe it LIVE.
EVENT NAME GOTCHA: serializes as "pre_review" (snake_case), not "PreReview".
WHAT WORKED: pre_review FIRED on cli-entry-point (device mihai) with had_findings=TRUE — the idle node caught a
REAL defect, persisted to .swarm/prereview/ and fed to integrate-verify. The judge ran 22x concurrently
(store-module 10x + tests 12x, all "observed"). So idle nodes do VALUABLE correctness work, not just busywork —
this serves BOTH goals (utilization + functionality).
HONEST LIMIT: 2 of 3 nodes still showed IDLE across repeated lms ps snapshots while `tests` ran on gabee. Root
cause is NOT the concurrency fix (that works) — it is that idle WORK is BOUNDED: (1) each completed task is
pre-reviewed exactly once (2 done tasks -> at most 2 pre-reviews, quickly exhausted); (2) the judge re-judges the
SAME in-flight task every ~15s tick (12x on `tests` = repetitive) and its "observed" verdicts intentionally do
NOT re-notify the loop, so between ticks the idle nodes sleep ~15s; (3) the replanner (bonus tasks) fired once and
is capped at max_replans=2. So at a SPARSE DAG tail (few tasks, 1 in-flight) there is genuinely little continuous
idle work. The fix fills idle nodes IN BURSTS (judge tick + the finite pre-reviews), not continuously.
TO FULLY ELIMINATE TAIL IDLE (flag for the user — each has real costs, don't rush): (a) SPECULATIVE EXECUTION — an
idle node runs a PARALLEL attempt of the in-flight task, first-to-finish wins (risk: file-conflict, wasted work);
(b) raise max_replans / lower the replan idle threshold (cost: ~900s planner calls, tail-padding — the idle-node
workflow flagged these); (c) more pre-review passes per task / deeper review (diminishing value). RECOMMEND: keep
the current fix (real value: caught a defect + observable), and treat full tail-idle-elimination as a separate,
user-steered decision given the costs. NET: idle fix is a genuine improvement (concurrent + observable + caught a
real bug), honestly PARTIAL on pure utilization.

## APP4 DEEP ANALYSIS (2026-06-30) — FUNCTIONAL but run-failed (false-NEGATIVE) + slow; AST review WIN
46.8min (way over target). run_finished done[cli-entry-point, store-module] FAILED[tests, integrate-verify].
BUT the APP IS FUNCTIONAL + CORRECT: `habit add gym; habit done gym; habit streak gym` -> 1; `habit list` ->
`gym: 1`; persists to ~/.habit.json as {"gym":["2026-06-30"]} (REAL persistence — survives a fresh process;
ISO dates). All 4 subcommands work. So the swarm REPORTED FAILURE on a FUNCTIONAL app — a false-NEGATIVE
(opposite of APP2's false-green). The tests/integrate-verify subtask failed (likely a unit-test assertion or
integrate-verify flake) while the actual program works end-to-end.
REVIEW WIN: the AST reviewer caught habit.store BUILT-BUT-UNWIRED (store.py written but cli.py never imported
it -> no persistence) and the wire-fix CORRECTLY wired it (cli.py now `from habit import store` + calls
store.cmd_add/done/streak/list). Without that, the app would have been a non-persisting shell. This is the
model-free review + wire-fix doing EXACTLY the user's ask (catch a built-but-unwired functional defect).
INQUISITIVE: did NOT ask (confidence>=75) — and chose ISO dates + a sensible ~/.habit.json by default, so the
no-ask was correct (the spec was clear enough).
TAKEAWAYS: (1) the run-status is NOT the same as functionality — a FUNCTIONAL app can be reported "failed"
(tests/integrate-verify flake). Judging by RUNNING the app (not trusting run_finished) is essential, as the
user insisted. (2) Python is SLOW + over target again (46.8min) — Python apps (APP1 43.7, APP4 46.8) run ~1.6x
the Rust win (28.1) and far over 25min. (3) The AST review + wire-fix is a proven functional-defect catcher.
BATCH SCORECARD so far: APP3 Rust = clean WIN (28min); APP4 Python = functional but run-failed + slow (47min);
APP1 Python = FAIL (CLI drift); APP2 TS = PARTIAL (broken build). => 2 of 4 functional; ALL over 25min; Rust
fastest+cleanest; Python functional-but-slow; the swarm's review caught real defects in 2 (cli-entry pre-review,
store wire-fix). TIME is the systemic gap vs the 15-25min goal.

## RE-TEST RESULTS (new binary f5cac6468 — validating the advertised-entry prompts 37cfd95fc)
### APP2-RETEST (TS CSV-stats) — FUNCTIONAL WIN, 25.9min — PROMPT FIX VALIDATED
The cleanest possible validation: SAME spec, NEW binary. Original APP2 = PARTIAL (no tsconfig -> `npm run build`
emitted nothing -> broken advertised entry; 66.9min). RE-TEST = FUNCTIONAL WIN:
- It GENERATED tsconfig.json (the exact missing file) — the architect now includes the build config.
- `npm run build` (tsc) SUCCEEDS -> dist/ produced (cli.js + modules). The advertised `node dist/cli.js` RUNS.
- `node dist/cli.js f.csv --column x` -> Mean 2.5 / Median 2.5 / StdDev 1.2910 (sample) — CORRECT; missing
  column -> exit 1; clean commander --help (Usage: csv-stats [options] <file>).
- 6/6 subtasks done, 0 failed. 25.9min — AT the 15-25min target (vs original 66.9min).
- Minor gap: no `npm test` script (test/ files exist but unscripted). Mode N/A for all-unique input (reasonable).
VERDICT: the advertised-entry prompt fix (force integrate-verify to BUILD + run the advertised entry + add the
missing build config) CONVERTED the broken-build class PARTIAL -> FUNCTIONAL WIN. Validated by re-test (1 run).
Confidence: the fix WORKS but is LLM-dependent — 1 success is not proof of reliability; the deterministic build
gate remains the belt-and-suspenders IF future runs regress. Not built now (prompt worked; don't over-build).
### APP1-RETEST (Python unit-converter) — IN FLIGHT (bpr1gy7wy)
Tests whether the advertised-entry prompts ALSO fix the CLI-spec-drift class (original APP1 built `value -f -t`
flags, not the spec's `convert X Y Z` subcommand + list-units). The prompt now says run the spec's EXACT
commands + match the interface. Judge: does `python3 -m <pkg> convert 100 km mi` (~62.137) + `list-units` WORK?

### APP1-RETEST (Python unit-converter) — FUNCTIONAL WIN, 39.9min — CLI-DRIFT FIX VALIDATED
Original APP1 = FAIL (built `value -f -t` flag CLI, not the spec; `convert 100 km mi` errored; 43.7min).
RE-TEST (new binary) = FUNCTIONAL WIN: cli.py now `add_subparsers` with REAL `convert` + `list-units`
subcommands (`subparsers.add_parser("convert")` / `("list-units")`). `convert 100 km mi` -> 62.14 (the EXACT
example original APP1 errored on); `convert 0 C F` -> 32; `list-units` lists units; unknown unit -> exit 1;
7/7 subtasks done; 77 pytest pass. 39.9min (still over target). VERDICT: the advertised-entry prompt fix
("run the spec's EXACT commands, match the interface, don't redesign into flags") CONVERTED the CLI-spec-drift
class FAIL -> FUNCTIONAL WIN. Now BOTH failure modes the fix targeted are validated by re-test (APP2 build +
APP1 CLI-shape).

## ★ MORNING SUMMARY (for the user — overnight session, 2026-06-29 -> 06-30)
GOAL was: get the local 27B swarm to deliver FUNCTIONAL apps (15-25min); make the adversarial review verify
functionality; keep idle nodes used; analyze every app + improve + re-test.

APP RESULTS (judged by RUNNING the app, not run-status):
| app | tech | binary | FUNCTIONAL | time |
|-----|------|--------|-----------|------|
| APP3 word-freq | Rust | new | YES (clean win) | 28.1min |
| APP4 habit-tracker | Python | new | YES (run-status falsely "failed") | 46.8min |
| APP2 CSV-stats | TS | OLD | PARTIAL (broken build) | 66.9min |
| APP2-RETEST | TS | new | YES (build fixed) | 25.9min |
| APP1 unit-conv | Python | OLD | NO (CLI drift + flaky) | 43.7min |
| APP1-RETEST | Python | new | YES (CLI fixed) | 39.9min |
=> On the NEW (fully-improved) binary: 4/4 apps FUNCTIONAL (APP3, APP4, APP2-retest, APP1-retest). The 2
original failures were on the OLD binary and BOTH were fixed by the improvements + validated by re-test.

IMPROVEMENTS SHIPPED + VALIDATED (all committed, 31 swarm + 269 cli tests green):
1. IDLE-NODE FIX (the user's "goose should keep idle nodes used"): judge + pre-review now run CONCURRENTLY
   on separate idle nodes (idle_jobs<=idle_capacity; was a single shared slot -> a 2nd node slept). Adversarial
   review caught + I fixed a double-decrement over-count. Added a PreReview jsonl event for observability.
   LIVE: pre-review fired on idle nodes and CAUGHT A REAL DEFECT (cli-entry-point, had_findings) -> fed to
   integrate-verify. HONEST LIMIT: PARTIAL — fills idle in bursts; idle persists at the sparse DAG tail (idle
   work is bounded). Full elimination needs speculative-exec/aggressive-replan (costs flagged) — your call.
2. ADVERTISED-ENTRY PROMPTS: integrate-verify must BUILD to the ADVERTISED entry (TS: npm run build then run
   node dist/..., NOT tsx source which hid APP2's broken build) and run the SPEC'S EXACT commands (matching
   the interface, no flag redesign which sank APP1) + add any missing build config (tsconfig). VALIDATED by
   BOTH re-tests (APP2 build + APP1 CLI). Confidence: works but LLM-dependent (each 1 success); a DETERMINISTIC
   build gate is the scoped fallback if future runs regress (not built — prompt worked, didn't over-build).
3. (Earlier) The AST reviewer + wire-fix CAUGHT habit.store BUILT-BUT-UNWIRED on APP4 + wired it (else a
   non-persisting shell). The review machinery demonstrably catches real functional defects.

SYSTEMIC FINDINGS:
- TIME is the gap: the 27B is slow. Only APP2-retest hit target (25.9min); Python apps run ~40-47min, ~1.6x
  the Rust win. This is the #1 unsolved issue vs the 15-25min goal — it is fleet SPEED, not correctness.
- RUN-STATUS != FUNCTIONALITY: APP2 false-green (7/7 done but broken build), APP4 false-negative (run "failed"
  but app works). Judging by RUNNING the app — as you insisted — was essential every time.
- Rust >> Python/TS on this fleet for shipping FUNCTIONAL apps: it COMPILES (broken build can't ship green),
  clap derives a correct conventional CLI (no drift). Python ships correct logic but slowly + drift-prone; TS
  needed the build fix (now works).

OPEN ITEMS / NEXT (your call): (a) TIME — needs a fleet-speed lever (split/cap heavy test tasks? faster model?
fewer phases?), the hardest + most impactful; (b) idle-fix is partial — speculative exec or aggressive replan
to fully fill the tail (costs flagged); (c) the advertised-entry prompt is LLM-dependent — promote to the
deterministic build gate if it regresses; (d) the false-negative run-status (a functional app reported failed)
deserves a look (integrate-verify/tests flake shouldn't fail a working app).

## TIME ANALYSIS (where the wall-clock goes — the #1 open item)
PHASE SPLIT (all 4 new-binary apps): research ~2min (FAST — parallel scouts work), plan ~6-8min (parallel
planning, moderate), EXECUTE+REVIEW ~17-39min (THE BULK). research is NOT the problem; the bulk is execute.
DOMINANT SINK = slow INDIVIDUAL TASKS on the 27B (5-13min EACH), especially:
- TEST tasks: APP1-retest had THREE (tests-cli 545s=9min, tests-edge-cases 349s, tests-conversion 233s) =
  ~19min of test-writing alone. APP2 tests 234s. Test tasks are 30-50% of wall-clock.
- cli-entry-point: a CHOKEPOINT everything depends on, and slow (APP3 769s=12.8min!, APP1 361s).
- integrate-verify: 280-418s, now SLOWER because the advertised-entry prompt makes it BUILD + run the spec's
  commands (a correctness/speed tradeoff — worth it, it catches real defects, but it adds ~3-5min).
CONCRETE TIME LEVERS (user to weigh — each is a real tradeoff, NOT built overnight):
1. CAP / CONSOLIDATE TEST TASKS (highest value): the architect over-decomposes tests into 2-3 separate slow
   tasks, and the 27B writes exhaustive suites (9min each). Cap test scope (a focused golden-value suite, not
   exhaustive) OR merge the test tasks. Could cut 10-20min. Tradeoff: less test coverage.
2. KEEP cli-entry-point TINY (it's the chokepoint): the architect rule already says keep shared deps small;
   the entry task balloons. A stricter "entry = wiring only, <40 lines" could shave the 769s outlier.
3. integrate-verify speed: it got more thorough (build+commands). Could time-box it or run the spec commands
   in the deterministic smoke gate instead (faster, model-free) — ties to the deterministic-build-gate candidate.
4. FASTER MODEL for the easy/test tasks (the planner labels hard tasks for the 27B; a faster small model for
   test-writing could parallelize cheaper) — a fleet/config lever, not code.
BOTTOM LINE: TIME is fleet SPEED (27B per-task latency x 5-7 tasks), not correctness. The parallelism + idle
fix already help; the biggest code lever is reducing the TEST-task burden. This is the systemic gap vs 15-25min
and the clearest next-improvement target — but it trades against test coverage, so it is a USER decision.

## APP4 FALSE-NEGATIVE — ROOT CAUSE (diagnosed, fix candidate for the user)
Why APP4's run reported "failed" on a FUNCTIONAL app: the `tests` subtask was marked FAILED (3 attempts
exhausted, empty error), BUT its test files (test_cli.py, test_store.py) ARE present and `python3 -m pytest`
-> 20 PASSED. So the worker WROTE valid, passing tests, yet every attempt was marked failed -> integrate-verify
(downstream) cascaded -> run "failed". This is a FLAKY-WORKER FALSE-NEGATIVE: the INVERSE of "claimed done
without writing" (a false-positive done) — here the worker DID the work (files written, tests pass) but its
completion/attempt flaked (final_output flake / transient / a done-gate mis-fire), so good work was reported as
failure.
FIX CANDIDATE (for the user — NOT built; it touches the completion/retry path which is delicate): on a FAILED
worker attempt, RE-CHECK the owned files — if they EXIST + are non-empty + VALID for the language (e.g. Python
test files that `pytest --collect-only` accepts, or a module that imports), treat the task as DONE rather than
failed (verify-by-ARTIFACT, not by the worker's completion signal). This is the mirror of the existing
claimed-done guard (which catches false-POSITIVE done by checking files are missing); this would catch
false-NEGATIVE fail by checking files are actually GOOD. Confidence: MED — the completion/retry path is the
riskiest surface (an earlier idle_jobs double-decrement bug lived nearby); it needs careful design + adversarial
review + a scheduler_mock test asserting "wrote-good-files-but-attempt-failed -> Done". Worth doing IF the
false-negative recurs across runs; otherwise the practical takeaway stands: JUDGE BY RUNNING THE APP, the
run-status lies in BOTH directions (APP2 false-green, APP4 false-negative).

## APP5 — HARD ARCHETYPE FUNCTIONAL WIN (Python double-entry ledger, 37.5min)
The missing hard/multi-module/correctness-critical archetype (vs the 4 moderate CLIs). NEW binary. RESULT:
FUNCTIONAL WIN. 5/5 subtasks done, 0 failed, 37.5min. Real multi-module pkg: ledger_cli/{models,store,
commands,cli,__main__}.py + tests. CORRECTNESS-CRITICAL CORE WORKS:
- Double-entry BALANCING RULE: `post ... --debit cash 100 --credit sales 100` -> exit 0 (balanced OK);
  `post ... --debit cash 50 --credit sales 30` -> exit 1 + "Error: Debits must equal credits" (REJECTED with
  non-zero exit, exactly as the spec demanded). This is the hard correctness requirement — it WORKS.
- trial-balance sums to zero (Cash -1200 + Rent_Expense +1200 = 0 across the book) = real double-entry.
- account add validates --type against [asset|liability|equity|income|expense] (built TYPE as a -t/--type
  choice OPTION, not the spec's positional — a DEFENSIBLE design with enum validation, not a real defect).
- Persists to data/ledger.json; 21 pytest pass.
IDLE-FIX on the harder run: pre_review fired 2x (at idle moments), judge_verdict 12x; at full fan-out all 3
nodes were busy (idle_capacity 0 -> correctly no idle jobs). Idle-fix behaves correctly across the run.
VERDICT: the swarm + the improvements (contracts/advertised-entry/idle) DELIVER on a HARD app — hard
multi-module correctness-critical work is NOT the ceiling on this fleet. (NB the buggy first read: my exit
check captured `head`'s exit via a pipe, not python's — the corrected no-pipe test shows exit 1. Always
capture the real process exit.) 37.5min — still over the 15-25 target (TIME remains the systemic gap).

## ★ UPDATED SCORECARD (new binary): 5/5 FUNCTIONAL across archetypes
| app | tech | archetype | FUNCTIONAL | time |
|-----|------|-----------|-----------|------|
| APP3 word-freq | Rust | moderate CLI | YES (clean) | 28.1min |
| APP2-retest CSV-stats | TS | moderate CLI | YES (build fixed) | 25.9min |
| APP1-retest unit-conv | Python | moderate CLI | YES (CLI fixed) | 39.9min |
| APP4 habit-tracker | Python | multi-module | YES (run-status false-neg) | 46.8min |
| APP5 ledger | Python | HARD multi-module (correctness-critical) | YES | 37.5min |
=> 5/5 FUNCTIONAL on the improved binary, spanning moderate CLIs (3 techs) AND a hard correctness-critical
multi-module app. The improvements hold across archetypes. The ONLY systemic miss is TIME (all >25min except
APP2-retest 25.9) — fleet SPEED, not correctness.

## APP6 — complex TS expression evaluator: FAILED (runtime crash), 73.5min — CEILING data point
Studied against the user's 7 points:
1. FUNCTIONAL? **NO.** It BUILDS (tsconfig present — advertised-entry fix held; tsc -> dist/ clean) and the
   parser is REAL (no eval() — a genuine recursive-descent tokenizer/parser/evaluator, exactly as spec'd), BUT
   `calc "2 + 3 * (4 - 1)"` CRASHES at runtime: "Invalid array length", exit 1 — on EVERY expression. A real
   bug (an array allocated with a bad length, likely in the tokenizer). So: compiles, doesn't run.
2. TIME: **73.5min** — 3x over the 15-25 target; the WORST of the batch. Complex TS on the 27B is very slow.
3. Prompt complex enough? **YES** — a recursive-descent evaluator with precedence + variables + a no-eval
   constraint is genuinely hard (the user wanted complex; this WAS complex).
4. Answered its questions? No ask floor -> it did not ask.
5. PHASES followed? **YES** — research -> plan -> contracts -> execute -> smoke/review all fired (4 pre_review,
   1 AST review, 1 smoke). But cli-entry-point + tests-core + integrate-verify FAILED in execute (3/7).
6. Did the REVIEW push toward FUNCTIONAL? **Partially, but it FAILED to fix it.** integrate-verify (with the
   advertised-entry prompt) DID try to build+run the entry — that is WHY it failed (the entry crashes, and the
   27B could not fix the "Invalid array length" bug across its attempts -> exhausted -> the run correctly
   reported FAILED). The smoke gate is PYTHON-ONLY (returns ran:false for TS) so there was no DETERMINISTIC
   TS build+run check; the only functional check was the (LLM) integrate-verify, which tried but the model
   could not repair a hard runtime bug.
7. Working in 15-25min? **NO** — 73.5min + broken.
CEILING FINDING: on a COMPLEX TS app, the 27B produced a real runtime bug it could NOT self-fix even with the
review trying. This is a MODEL-CAPABILITY ceiling, not a false-green (the run was honestly FAILED). vs APP5
(hard Python ledger, FUNCTIONAL 37.5min): Python hard worked, complex-TS-parser did not. So the ceiling is
roughly: hard-but-conventional (ledger) PASSES; algorithmically-tricky + a weaker-for-the-27B language (a TS
parser) FAILS + is slow.
WHAT ELSE COULD IMPROVE THIS DELIVERABLE: (a) a DETERMINISTIC language-aware build+RUN gate (TS: npm run build
+ run the bin on a trivial input + assert no crash) would catch the "Invalid array length" reliably (here the
run WAS marked failed, so the user knew — but the deterministic gate makes the functional check not depend on
the flaky LLM integrate-verify). (b) the 27B can't fix hard runtime bugs -> simpler decomposition (split the
tokenizer/parser into smaller verified pieces) or a stronger model for the fix step. (c) TS is slow -> the
TIME gap is worst on complex TS.
BATCH UPDATE: APP1-retest/APP2-retest/APP3/APP4/APP5 functional (5), APP6 FAILED (complex-TS ceiling). So
6 of 7 distinct apps functional on the new binary; the 1 failure is a complex TS algorithm at the 27B's ceiling.

## APP7 — hard Python cron parser: PARTIAL (subtle next bug), 29.7min — tests-pass-but-wrong-output
Studied against the 7 points:
1. FUNCTIONAL? PARTIAL. Builds + runs. matches "*/15 * * * *" at T10:00:00 -> True (correct, minute 0 div 15).
   Malformed expr -> exit 2 (correct). BUT the next command has a SUBTLE CORRECTNESS BUG: next "0 9 * * 1-5"
   from T08:00:00 --count 2 returns 09:00:00 AND 09:00:01 — it steps by SECONDS, so the 2nd "next run" is one
   second later (still minute 0 / hour 9 -> technically matches) instead of the next DISTINCT cron occurrence
   (the next weekday 09:00). So next --count N returns sub-minute duplicates — broken for its purpose.
2. TIME: 29.7min — CLOSE to target (best Python time; slightly over 25).
3. Prompt complex enough? YES — cron field parsing (star, step, ranges, lists) + next-run datetime math is hard.
4. Questions? No ask floor -> did not ask.
5. PHASES followed? YES — 6/6 subtasks done, 0 failed; run SUCCEEDED.
6. Did the REVIEW push toward FUNCTIONAL? It verified RUNS but NOT CORRECT. smoke/AST/pre-review/integrate-
   verify all passed (6/6 done) but NONE caught the next seconds-granularity bug (it is subtle — 09:00:01 does
   satisfy minute=0/hour=9). 21 pytest PASSED but never asserted next returns DISTINCT minute occurrences.
   EXACTLY the user deepest concern: tests pass + it runs, but the output is subtly WRONG. A golden-value test
   (next 2 of 0 9 * * 1-5 == [today 9:00, tomorrow 9:00]) would have caught it; the swarm tests asserted shape.
7. Working in 15-25min? Close on time (29.7) but the next bug = not fully usable.
STUB-DETECTOR LIVE: 0 findings (no stub in the produced code) -> the detector correctly did NOT false-positive
on a real implementation. A live CATCH of a real stub still awaits a run where the 27B actually stubs.
TAKEAWAY: the review reliably catches "does not BUILD/RUN" (APP1/APP2/APP6) but NOT subtle wrong-output when the
code runs + shape-tests pass. Grounded improvement: integrate-verify / a golden-value check must assert the
OUTPUT matches what the spec IMPLIES on a known input, not just no-crash (recurring EVAL-v8 lesson 6).
BATCH (new binary): fully-functional = APP1-retest, APP2-retest, APP3, APP4, APP5 (5); APP6 FAILED (TS parser,
27B ceiling); APP7 PARTIAL (subtle next bug). APP7 29.7min best Python; avg ~37min still over target.

## APP8 — hard Python JSON-schema validator: HARD FAILURE (wrong language + recursive ceiling), 60.9min
Studied against the 7 points + a root-cause dig (the user: analyse the failed app carefully):
1. FUNCTIONAL? NO. TWO compounding failures: (a) LANGUAGE MIS-DETECTION — the spec said LANG=Python but the
   swarm built it in TYPESCRIPT (dist/cli.js, dist/validator.js, node_modules with vitest; ZERO .py files).
   Root cause found: detect_language checked s.contains(".js") BEFORE the python cue, and "SCHEMA.json"/
   "DATA.json" contain ".js" -> the TypeScript branch fired and the explicit LANG=Python was never reached.
   (b) RECURSIVE CEILING — even as TS, core-validator (recursive nested schema validation) FAILED; 5/7
   subtasks failed (cli-input-validation, core-validator, edge-case-tests, test-core-validator, integrate-
   verify). Same ceiling class as APP6 (parser) — the 27B cannot complete a hard recursive algorithm.
2. TIME: 60.9min — way over (the recursive-app ceiling; cf APP6 73min). Hard recursive apps blow the budget.
3. Prompt complex enough? YES — recursive nested validation (types/required/min-max/minLength-maxLength) is hard.
4. Questions? No ask floor.
5. PHASES followed? YES, but EXECUTE collapsed (5/7 failed). JUDGE WAS HEALTHY: 183 observed / 8 re_dispatch
   / 4 failed -> mostly OBSERVING, occasional re_dispatch, correctly terminal-failing genuinely-stuck tasks.
   NOT thrashing (the 133-then-183 verdict count looked alarming but the action breakdown is healthy). This
   is a POSITIVE validation of the idle-node/judge work under a hard pinned chokepoint.
6. Did the REVIEW push functional? The run failed before a working integrate-verify; the smoke gate logged
   py_files:0 (the tree was TS, not Python). Not reached.
7. Working in 15-25min? NO — 60.9min + failed.
ROOT-CAUSE FIX SHIPPED: detect_language now checks EXPLICIT language names FIRST (LANG wins) + matches
extension cues at a WORD BOUNDARY (".js" != ".json") (commit 4a9c2e30e, regression-tested). This was a real
HIGH-confidence bug that would mis-build ANY JSON-related Python app as TS.
SECOND LESSON (open): the recursive-algorithm TIME+capability ceiling recurs (APP6 73min TS parser FAIL,
APP8 60.9min recursive validator FAIL). Candidate mitigations to think about: decompose the recursive core
into smaller verified pieces (one validator-per-type subtask) so no single subtask is the whole hard
algorithm; OR a hard per-subtask wall-clock that fails fast instead of grinding 60min; OR accept the ceiling
and surface EARLY (the judge already terminal-failed at 8 re_dispatch — but only after 60min).
BATCH: 5 functional, APP6 FAIL (TS parser), APP7 PARTIAL (next bug), APP8 FAIL (lang mis-detect + recursive).
Two of the failures (APP6 TS, APP8 recursive) are 27B-capability ceilings; APP8 ALSO exposed a real
swarm bug (language detection) now fixed. avg time on hard apps ~50-70min = well over the 15-25 target.

## APP8 recursive-ceiling DIAGNOSIS (read-the-logs before tuning — no knob built, MED-LOW confidence any would help)
Traced WHY APP8 ground 60.9min before failing. core-validator timeline: dispatched attempt 0 -> judge
re_dispatch -> attempt 1 -> judge re_dispatch -> attempt 2 -> task_completed status=failed (terminal, max
attempts). NO worker_timeout/re-route markers in stderr. So it is NOT a single-attempt grind a wall-clock cap
would shorten — it is the 27B FAILING the recursive core on ALL 3 attempts (a capability limit), with the
judge behaving CORRECTLY (re_dispatch x2 then terminal-fail). The 60min is CUMULATIVE: many hard subtasks
(core-validator, cli-input-validation, edge-case-tests, test-core-validator) each retried to their attempt
cap, plus the judge 15s cycles, plus research/plan. Conclusion: the recursive-algorithm ceiling (APP6 parser,
APP8 validator) is largely a MODEL-CAPABILITY limit, not a swarm-fixable knob.
Why the obvious mitigations are MED-LOW confidence (flagged, NOT built): (a) decompose the recursive core by
constraint-type (validate-types / validate-required / validate-numeric / validate-nested) does NOT remove the
RECURSION — each piece still must recurse into nested objects, so the irreducible hard part remains; the
architect ALREADY decomposed (core-validator was already one subtask) and the recursion was still too hard.
(b) a fail-fast wall-clock would not help — it is failing across attempts, not grinding one. (c) it already
surfaces (terminal-fail + dependent cascade + run reports FAILED honestly). The judge already gives the right
behavior. So there is no high-confidence swarm change here; the honest answer is the 27B cannot do a hard
recursive algorithm, and the run correctly FAILS rather than shipping a fake. Recorded, not knob-tuned.
ONE lower-risk idea kept for later (MED): for a CRITICAL CHOKEPOINT subtask that everything depends on, once
it terminal-fails, the run is doomed — could fail the WHOLE run faster instead of also burning attempts on its
now-orphaned siblings. Needs careful trace work + a control-flow change; deferred, not rushed.

## APP9 — Python date utility (gate-validation): WIN, 17.9min — FIRST app inside the 15-25 target
The end-to-end validation of this session gates. Built as PYTHON (detect_language fix confirmed). 4/4 done, 0 failed.
1. FUNCTIONAL? YES, fully correct. series 2026-01-01 --count 3 --step 7 -> 2026-01-01 / 2026-01-08 / 2026-01-15
   (correct + distinct, exit 0). weekdays 2026-01-01 --count 3 -> 2026-01-02 Fri / 2026-01-05 Mon / 2026-01-06
   Tue (correctly skips Sat 01-03 + Sun 01-04). malformed date -> exit 2. 14 pytest pass.
2. TIME: 17.9min — INSIDE the 15-25 target (the FIRST app to do so). Moderate Python (date arithmetic, not a
   recursive ceiling) -> tractable for the 27B + fast.
3. Prompt complex enough? Moderate — 2 subcommands, multi-output with --count N, weekday skipping, validation.
4. Questions? No ask floor.
5. PHASES followed? YES — 4/4 done.
6. GATE PROOF (the point of this run): SMOKE GATE FIRED CORRECTLY -> ran=True, py_files=5, entry_ok=True,
   findings=0 (it ran the Python entry, passed, NO false-positive). The MULTI-OUTPUT (series/weekdays --count
   N — exactly APP7s wrong-output class) is CORRECT + DISTINCT. So the gates behave correctly on a passing app.
7. Working in 15-25min? YES — 17.9min + correct. The deliverable a user would actually want.
HONEST CAVEAT: APP9 had NO bug, so it PROVES the gates fire without false-positives + a correct multi-output,
but does NOT prove the golden-value check CATCHES a wrong multi-output (there was nothing to catch). The
golden-value catch-power remains unproven until a run where the 27B produces a wrong --count output and the
check catches+fixes it. The smoke gate firing cleanly (ran the Python entry, passed) IS proven here.
TAKEAWAY: the moderate-Python sweet spot (APP9 17.9min, APP4/APP5 habit/ledger, APP1-retest/APP7 ~30min) works
well; the failures cluster on RECURSIVE algorithms (APP6 parser, APP8 validator) = the 27B capability ceiling.
BATCH: functional now = APP1-retest, APP2-retest, APP3, APP4, APP5, APP9 (6); APP7 PARTIAL; APP6 + APP8 FAIL
(recursive ceiling). The gates + detect_language fix are validated working on a clean passing app.

## APP10 — TS sequence generator (multi-output): FUNCTIONAL (run-status LIED), ~34min — integrate-verify false-fail
Judged by RUNNING (the run was marked FAILED on integrate-verify; the app WORKS):
1. FUNCTIONAL? YES. Built as TypeScript (detect_language TS path correct). `node dist/index.js fib --count 8`
   -> 0 1 1 2 3 5 8 13 (CORRECT); primes --count 5 -> 2 3 5 7 11; triangular --count 4 -> 1 3 6 10; fib
   --count -1 -> exit 1 (validation). 16 vitest pass. The multi-output (the golden-value class) is CORRECT +
   distinct. NOTE: my first test mis-invoked `node dist/index.js seq fib` -> "unknown command seq"; `seq` is
   the BIN NAME (the program), `fib` is the subcommand, so the real invocation is `node dist/index.js fib`.
2. TIME: ~34min — over target (TS slower than the Python sweet-spot APP9 17.9min).
3. Prompt complex enough? Moderate (3 sequences, multi-output, a subcommand group).
4. Questions? No ask floor.
5. PHASES followed? YES — 4/4 work subtasks done; only integrate-verify "failed".
6. Did the REVIEW push functional? MIXED. The TS SMOKE GATE FIRED CORRECTLY (ran=True, entry_ok=True, 0
   findings) — the FIRST live TS smoke-gate test, NO false-positive on a healthy TS app -> validates the
   MF1/MF3 review fixes. BUT integrate-verify FALSE-FAILED: told to run "the EXACT commands the spec
   advertises", it ran the spec literal "seq fib --count N" but the built artifact is invoked as
   `node dist/index.js fib` (the leading "seq" is the BIN name, dropped when running via node <entry>) ->
   "unknown command seq" -> it concluded broken + failed the run. A FALSE NEGATIVE on a working app.
7. Working in 15-25min? ~34min, but FUNCTIONAL.
GROUNDED IMPROVEMENT (the bin-name vs subcommand invocation bug): integrate-verify + the worker entry-run
guidance must state that when invoking the BUILT entry directly (node <dist-entry> / python3 -m <pkg>), the
spec's LEADING program/bin name is NOT repeated as an argument — pass only the SUBCOMMAND + args (spec
"myapp build --x" -> `node dist/cli.js build --x`, NOT `... myapp build --x`). This false-failed APP10 (an
actually-working app marked FAILED) and is a recurring confusion. Run-status lies BOTH ways — JUDGE BY RUNNING.
BATCH: functional (by RUNNING) = APP1-retest, APP2-retest, APP3, APP4, APP5, APP9, APP10 (7); APP7 PARTIAL;
APP6 + APP8 FAIL (recursive ceiling + APP8 also the now-fixed lang bug). 7 of 10 functional. The TS smoke
gate is validated (fired correctly twice the criteria: no false-positive + entry runs).

## APP11 — Python monthly recurrence: FUNCTIONAL on golden, REAL edge-case HANG, run correctly FAILED
1. FUNCTIONAL? PARTIAL-YES. occurrences 2026-01-15 31 --count 3 -> 2026-01-31 / 2026-03-31 / 2026-05-31
   (CORRECT — Feb+Apr SKIPPED, the tricky month-skip the 27B usually botches is RIGHT, exit 0); count
   2026-01-01 2026-12-31 31 -> 7 (CORRECT); bad date -> exit 1. So correct on conventional inputs. BUT
   core.py has UNBOUNDED loops (while len(result)<count + while True, no max-iteration cap) -> a degenerate
   input (a reversed range / out-of-range day not validated at the core layer) loops FOREVER. pytest HANGS
   (2min timeout) -> tests-core + integrate-verify FAILED -> run correctly marked FAILED.
2. TIME: ~elapsed not cleanly parsed (~30-40min).
3. Prompt complex enough? YES — recurrence + month-skip + range-count.
4. Questions? ASK not enabled on this run (pre-handshake).
5. PHASES followed? YES — 3 done (cli/core/tests-cli), 2 failed (tests-core, integrate-verify on the hang).
6. Did the REVIEW push functional? The tests CAUGHT the hang (good test depth — an edge the golden cases
   missed); the worker-timeout correctly failed the hanging tests-core rather than hanging the whole run; the
   run honestly reported FAILED. The smoke gate passed (it runs pytest --collect-only, NOT the full suite, so
   it does not execute the looping test — by design, to avoid hanging). The 27B could NOT fix the unbounded
   loop (same fix-capability ceiling as APP6/APP8).
7. Working 15-25min? Correct on common cases but the edge-hang = not robust.
GOLDEN-VALUE CATCH TEST RESULT: the multi-output (the month-skip) is CORRECT, so there was NO wrong output to
catch -> the golden-value catch-power STILL unproven (the 27B got the tricky logic right; whether the
strengthened prompt helped is unknowable without the trace). What the tests DID catch was a different class
(an infinite loop), via test-execution timeout, not the golden-value check.
BATCH: APP11 -> functional-on-golden but a real edge-hang (run correctly FAILED). Pattern holds: conventional
correctness OK, a subtle bug (here an unbounded loop) the 27B cannot self-fix.

## UNIQ1 — league manager (FIRST complex hands-on app): 925 LOC / 13 modules, FAILED at INTEGRATION, ~105min
The user raised the bar to ~1000-LOC feature-dense apps; this is the first. ALSO the first ASK-handshake run
(I answered 4 clarify questions as the user) + the first hands-on monitor+backlog cycle.
1. FUNCTIONAL? NO — but the failure is INTEGRATION, not logic. 925 LOC across 13 coherent modules (db, schema,
   league, team, scheduler, fixtures, results, standings, form, bracket, cli, __main__). The schema is sound,
   the round-robin uses the correct circle method (bounded, BYE-handled), etc. BUT cli.py (the entry) defines
   an empty click group with --db and NEVER registers any command (no add_command, no imports of the command
   modules) -> `schedule` / `standings` / `bracket` / `league create` are ALL "No such command". So the whole
   app is unusable: the pieces exist, the ENTRY WIRING does not. The classic BUILT-BUT-UNWIRED ENTRY failure
   (one of the original 4 classes) — recurring AT SCALE. Run correctly FAILED (cli-entry + integrate-verify +
   tests). PLUS an earlier-caught cross-module CONTRACT DRIFT (scheduler.py fixtures(league,round,home,away)
   vs schema.py fixtures(league_id,...)). Two integration failures, zero module-logic failures.
2. TIME: ~105min — severe blowup (the ASK re-plan re-drafts the whole skeleton + a long 13-module execute).
3. Prompt complex enough? YES — 925 LOC / 13 modules, the bar is met.
4. Answered its questions? YES — the ASK handshake worked: it asked 4 GOOD clarify questions (head-to-head
   tiebreak, output format, schedule idempotency, bracket numbering), I answered as the user, it re-planned.
5. PHASES followed? YES — research -> ASK -> plan -> execute (6/9 subtasks done); cli-entry + integrate-verify
   + tests FAILED. Judge HEALTHY all run (mostly observed). 1-per-node violated mid-run (the bug the user
   caught — now FIXED in the binary, not this run).
6. Did the REVIEW push functional? PARTIALLY. The smoke gate DETECTED the entry problem (1 finding,
   entry_ok=None); integrate-verify TRIED to fix the unwired CLI but FAILED — the 27B could not wire 13
   modules into the group. So the review correctly marked it FAILED (no false-green) but could not REPAIR the
   integration. Same fix-capability ceiling, now at the INTEGRATION layer.
7. Working 15-25min? NO — 105min + unusable.
THE WALL AT ~1000 LOC (the user question): it is INTEGRATION, not module logic. The swarm builds many coherent
modules but fails to (a) WIRE them into the entry point and (b) agree on shared cross-module contracts (the DB
schema). The 27B builds the pieces; assembling them at 13-module scale is the ceiling.

## UNIQ2 — graph analysis tool: WIN (FUNCTIONAL), 1179 LOC / 8 modules, ~58.3min — integration HELD at scale
The FIRST complex (~1000+ LOC) app that actually WORKS. Judged by RUNNING (the run reported FAILED but the
run-status LIES — the app is functional).
1. FUNCTIONAL? YES. 1179 LOC, 8 modules (loader, path_algo, structural, node_queries, centrality, cli + main).
   CLI FULLY WIRED: --help lists all 8 subcommands (path/components/topo/cycles/neighbors/degree/reachable/
   centrality) — the OPPOSITE of UNIQ1's empty group. Graph algorithms all CORRECT: topo on a directed cycle
   -> error + exit 1; cycles -> A->C->B->A; path A C -> A->B->C (hops 2); degree B -> in 1 out 1. 47 pytest pass.
2. TIME: ~58.3min — complex-app range (over the 15-25 target, expected at this size).
3. Prompt complex enough? YES — 8 graph algorithms (BFS/Dijkstra/topo/cycle/components/centrality), 1179 LOC.
4. Answered its questions? YES — the ASK handshake: I answered 4 clarify questions; the SUBCOMMANDS answer
   ("every command a registered subcommand reachable from --help") is WHY the CLI wired correctly.
5. PHASES followed? YES — 9 done, 2 (cli-entry-point + integrate-verify) reported FAILED but FALSE-failed (the
   CLI works when run). 1-PER-NODE VERIFIED in execute (lms ps: 3 nodes, 1 task each), judge calls ~5 (cooldown).
6. Did the REVIEW push functional? MIXED + a FALSE-NEGATIVE: the app WORKS but integrate-verify marked it
   FAILED (same false-negative class as APP10 — run-status lies BOTH ways; JUDGE BY RUNNING caught it).
7. Working 15-25min? No (~58.3min) but FUNCTIONAL + CORRECT — the deliverable a user wants.
HEADLINE (the user where-is-the-wall question): the ~1000-LOC INTEGRATION wall is ADDRESSABLE. UNIQ1 (925
LOC, UNWIRED cli) failed; UNIQ2 (1179 LOC, WIRED cli) WORKS — the difference is EXPLICIT entry-wiring (via my
ASK answer). This VALIDATES the entry-wiring fix (52715d760) that now makes the instruction default for ALL
apps -> UNIQ3 (no wiring ASK-answer from me) should also wire. So the swarm CAN build a functional ~1200-LOC
multi-module app; the levers are (a) explicit entry wiring (DONE), (b) shared-contract freezing (the DB-schema
drift, still open), (c) integrate-verify false-negatives (it failed a working app — worth a look).
BATCH: complex apps — UNIQ1 FAILED (unwired), UNIQ2 WIN (wired+correct, 1179 LOC). The wall is integration,
and it is now being knocked down.

## UNIQ3 — data-pipeline ETL: WIN (FUNCTIONAL), 1578 LOC, ~85min — entry-wiring VALIDATED w/o my help
2nd consecutive complex-app WIN (judged by RUNNING; run-status FAILED but lies — integrate-verify false-failed).
1. FUNCTIONAL? YES. 1578 LOC, etl_pipeline/{core,io,schema,stages,errors,cli,__main__} + tests. CLI FULLY WIRED:
   --help lists all 5 commands (run/head/schema/stats/join) — and UNIQ3 hit confidence 85 so it NEVER ASKED:
   the ENTRY-WIRING FIX (52715d760) did this with NO wiring answer from me = VALIDATED on a fresh non-asking app.
2. CORRECT (golden): filter age gt 30 + select -> right 3 rows; groupby dept agg avg:salary -> eng 105.0 /
   sales 95.0 (exact); stats salary -> count4/min80/max120/mean100; bad stage -> exit 1. 87 pytest pass.
3. Prompt complex enough? YES — 7 stage types + 5 commands + SQLite-free tabular engine, 1578 LOC.
4. Answered questions? N/A — conf 85, NO ASK (tested entry-wiring without my help, the point).
5. PHASES: 9 done, integrate-verify FALSE-FAILED. 1-per-node + 3-draft VERIFIED. commands-cli over-read (judge
   caught it) but resolved CLEAN (clean cli.py, all 5 wired).
6. REVIEW false-negative AGAIN: app WORKS but integrate-verify marked FAILED (3rd: UNIQ2, APP10, UNIQ3).
7. ~85min (the integrate-verify tail is SLOW — note). Over the 15-25 target but FUNCTIONAL+CORRECT.
HEADLINE: 2 consecutive complex-app WINS (UNIQ2 graph 1179 LOC, UNIQ3 ETL 1578 LOC). The ~1000-LOC integration
wall is BROKEN — both fixes (entry-wiring + schema-freeze) working; entry-wiring now VALIDATED without my help.

### [IMPROVEMENT ITEM — integrate-verify FALSE-NEGATIVE, now 3rd occurrence, MED, the next backlog target]
integrate-verify marks a WORKING app FAILED (UNIQ2, APP10, UNIQ3). A working app reported FAILED costs the user
confidence in run-status (the whole point is trusting the result). RESEARCH: read the integrate-verify task
session trace (jsonl session_id -> sessions.db messages) — WHY does it fail? a flaky end-to-end check? a
build/timeout? the bin-name invocation? Does its FAIL block the run report from saying done even when the app
runs? Fix so a genuinely-working app is reported DONE (and a real failure still fails). HIGH-VALUE for trust.

## UNIQ4 — SQLite budget tracker: PARTIAL (functional+wired, but a budget-status bug + no tests), 709 LOC, SKELETON_FIRST=1
Phases: total 85.0min | research 2.0min | planning(start->plan) 27.0min | execute 58.0min (planning ~35min = the WASTE; 1:22 total — LONG).
1. FUNCTIONAL? MOSTLY. Runs, CLI WIRED (--help lists all 7: init/account/category/tx/transfer/budget/report —
   entry-wiring works), report balances CORRECT (checking 950 = 1000 income - 50 expense), validation works
   (unknown account -> exit 1). BUG: budget status sums RAW signed amounts as spent (salary income shows
   spent 1000; food expense spent -50 remaining 250) instead of expense magnitudes (food should be spent 50
   remaining 150) — a sign/semantics error in commands_budget_report.py.
2. SKELETON-FIRST (direction A) quality: NO STUB LEFT (grep found zero pass/todo/NotImplementedError) = the
   skeleton-first hazard did NOT bite; cli.py fully implemented + wired. over_read 1 (vs UNIQ3 5, CONFOUNDED).
3. tests subtask FAILED (no test files produced) — a real miss.
4. SMOKE GATE PAID OFF: caught the flat-layout (root __main__.py) unrunnable-via- + auto-fixed (added
   __init__.py + relative import). EVIDENCE the gate earns its time.
5. integrate-verify FAILED — and UNIQ4 ACTUALLY HAS a bug (budget status) -> this may be a TRUE negative, NOT a
   false one. So integrate-verify is not purely broken: it false-negatived UNIQ2/UNIQ3 (working) but may have
   correctly failed UNIQ4 (buggy). The fix must separate true from false — read the traces of ALL THREE.
6. Schema mostly consistent (account/category/amount/date dominant; minor 'acct' abbrev). DB-schema-freeze held.
VERDICT: a PARTIAL, not a clean win (UNIQ2/UNIQ3 were cleaner). Skeleton-first did NOT hurt quality (no stub).
The budget bug is a worker logic error integrate-verify should ideally have repaired (it flagged, did not fix).

## UNIQ5 — SQLite task tracker w/ dependency DAG: PARTIAL (complex, topo/cycle CORRECT, refuse-done bug), 1115 LOC, 114min, skeleton-first default-on
The most COMPLEX app yet (SQLite + dependency DAG + topo sort + cycle detect + 8 commands). Judged by RUNNING.
1. FUNCTIONAL? MOSTLY. CLI WIRED (--help all 8: init/project/task/dep/status/schedule/ready/report). CORRECT:
   schedule = topological order (C,B,A respecting deps), cycle detection (exit 1 on a cycle), ready set, report
   counts. The HARD algorithmic part (DAG/topo/cycle) WORKS. BUG: status set done while a direct dep is not done
   was ALLOWED (should REFUSE nonzero) — the refuse-when-deps-undone check is broken (a secondary business rule).
2. SKELETON-FIRST on the COMPLEX entry (the missing data): entry-point judge verdict 'ok', over_read 0 — skeleton-
   first HELD over_read at 0 on a big multi-command CLI entry. No stubs. So on a complex entry it does its job
   (the simple-app A/B was a wash; here it is at least neutral + clean). Default-on stands.
3. integrate-verify = the BUG CONTROL: failed [judge_killed, judge_killed, judge_failed] = over_reading kill loop
   (no-owned -> permanently armed). This is the EXACT bug fixed in 6e1547b2d. UNIQ6 (new binary) is the treatment.
4. TIME: 114min (slowest) — planning 29.2 (re-plan after ASK = the waste), execute 82.7 (test-suite 23.5min +
   the integrate-verify kill loop + slow modules). The performance gap is real on complex apps.
IMPROVEMENT vs UNIQ4: higher complexity (1115 vs 709 LOC, a real DAG/topo engine that is CORRECT) at the same
PARTIAL grade (one logic bug each). Wiring + schema-freeze + skeleton-first all held. The two open gaps are
run-status trust (integrate-verify, FIXED -> UNIQ6 validates) and the secondary-business-rule correctness.

## UNIQ6 — forward-chaining rules engine: FAIL (genuinely broken), 813 LOC, 52.7min — but the JUDGE-KILL FIX CONFIRMED
HONEST + nuanced. Judged by RUNNING.
1. FUNCTIONAL? NO. CLI WIRED (all 7: init/fact/rule/infer/query/explain/reset). infer LOGIC correct (derives c
   from a,b then d from c — forward-chaining works). BUT the app is BROKEN: infer does NOT PERSIST the derived
   facts -> query d returns no/exit1 (should be yes), fact list shows only base facts (c,d missing), explain d
   CRASHES (traceback). The core feature (derive + query/explain) does not work end-to-end. A TRUE failure.
2. JUDGE-KILL FIX (6e1547b2d) CONFIRMED: integrate-verify attempt_history = [] (0 attempts, NOT judge_killed
   over_reading) vs UNIQ5 [judge_killed,judge_killed,judge_failed]. The over-read exemption HELD. The fix works.
3. BUT run-status FAILED [integrate-verify, tests] is CORRECT here (UNIQ6 IS broken) — so UNIQ6 is a TRUE
   negative, NOT the false-negative demo I wanted. Need a WORKING complex app on the fixed binary to SHOW
   run-status honesty. tests FAILED genuinely (judge_killed over_reading+looping x3 — tests OWNS files so the
   over-read gate CORRECTLY applies; my exemption is scoped to no-owned tasks only = precise). tests failing
   then BLOCKED integrate-verify (0 attempts = the dependency-blocked false-negative cause, CONFIRMED real).
4. FASTER: no-ASK -> planning 9.4min (vs UNIQ5 29min with ASK+re-plan), total 52.7min (vs UNIQ5 114). The
   re-plan-after-ASK waste is the difference. skeleton-first default-on, no stubs.
IMPROVEMENT vs UNIQ5: swarm-side fixes all held (wiring, schema, skeleton-first, judge-kill); FASTER (no ASK).
REGRESSION on app quality (broken persistence) = the local 27B variance, not a swarm regression. The infer-
persist bug is EXACTLY what integrate-verify should catch — but tests blocked it -> STRONG motivation for the
dependency-blocked fix (let integrate-verify run even if tests fails) + the tests-subtask reliability.

## UNIQ7 — SQLite inventory tracker: WORKS PERFECTLY (best complex app) but run-status FALSE-FAILED, 66min
JUDGE BY RUNNING = the cleanest complex app yet. Golden ALL correct: --help wired (7 cmds), item list on-hand
70 (100 recv - 30 ship), ship-overflow REFUSED exit1, valuation 70*$5=$350 + Grand Total, lowstock empty,
movements dated (recv 100 / ship -30), unknown SKU exit1. Nicely formatted tables. CLI WIRED, no stubs.
BUT run-status FAILED [entry-point, integrate-verify] = FALSE-NEGATIVE (the app WORKS):
- entry-point marked FAILED (3 attempts, 1 broken_code + 1 looping earlier) but its FINAL files RUN PERFECTLY.
  A THIRD run-status facet: a real module that exhausts retries but whose final-attempt files actually work.
- integrate-verify BLOCKED by the failed entry-point (0 attempts). My test-dep-strip fix only covers TESTS; a
  failed real MODULE still blocks it (correctly in principle — but here the module false-failed).
RUN-STATUS PROGRESS: judge-kill cause FIXED (no judge_killed here), test-blocked cause FIXED (tests passed +
did not block). REMAINING: a module that exhausts retries with WORKING final files -> false-fail + blocks
integrate-verify. Harder: the scheduler marks it failed on the attempt cap though the files are usable.
TIME: 66min (planning 23.8 = re-plan waste; execute 40.2 = entry-point 3-attempt struggle). The complex CLI
entry is the recurring slow/risky spot (UNIQ7 broken_code, took 3 attempts).
IMPROVEMENT: QUALITY is the BEST complex app yet (works fully, clean output) — wiring/schema/skeleton-first all
held. The gap is purely run-status honesty (3rd facet) + the slow complex-entry. JUDGE BY RUNNING remains essential.

## UNIQ8 — SQLite snippet manager: CLEAN WIN + HONEST run-status (the MILESTONE), 743 LOC, 47min
*** THE RUN-STATUS-HONESTY MILESTONE: done 7, FAILED [] (zero) — integrate-verify RAN (1 attempt, done['ok'],
11 judge-ok verdicts, NOT killed/blocked). The FIRST clean honest DONE on a multi-module app since the fixes. ***
1. FUNCTIONAL? YES, fully. CLI WIRED (all 9: init/add/list/search/show/edit/delete/tags/export). Golden ALL
   correct: list (id title lang tags), search demo -> 2 matches, tags (demo2/greet1/iter1), export valid JSON 2
   records, bad-format exit2, unknown-id exit1. No stubs.
2. RUN-STATUS HONEST (the point): vs UNIQ2/UNIQ3 (WINS but run-status FALSE-FAILED, integrate-verify judge_killed)
   -> UNIQ8 same class of working app now reports DONE honestly. judge-kill exemption + test-dep-strip both held;
   integrate-verify ran end-to-end (11 ok) + passed. JUDGE BY RUNNING agrees with run-status for once.
3. FASTER + cleaner: planning 18.4 (some ASK), execute 26.6, total 47min. Fast entry -> no finalize-spin (entry
   finished under 7min) -> no re-dispatch loop. So the finalize-spin issue is a COMPLEX-entry problem, not moderate.
IMPROVEMENT vs prior: this is the app-after-app proof the user wanted — run-status went from LYING (UNIQ2/3/6/7
false or blocked) to HONEST DONE on a working app. Both run-status causes fixed + validated end-to-end.

## UNIQ9 — habit tracker (bm13d1izs, 501 LOC habits/*.py) — WORKING app, verify-not-rewrite + SpecDrift + smoke + review + test-dep-strip ALL fired
GOLDEN (JUDGE BY RUNNING, exit codes captured CORRECTLY as `python ...; rc=$?`):
  init OK | habit add OK | checkin --date 2026-06-29/-30 OK (the --date FLAG WORKS — SpecDrift fix LANDED even
  though cli-app att2 was finalize-spin-killed mid-fix; the file it wrote persisted + functions) | streak gym = 2
  CORRECT | stats = 2 check-ins, 2/3 week CORRECT | history --weeks 2 = correct weekly grid [##.] | report gym
  --week 2026-W27 AND --week 2026-06-29 (date form) both OK rc0 (clarify #1 satisfied: both forms -> same week).
  ERROR SIGNALING CORRECT: dup checkin rc1, unknown habit rc1, before-init rc1, argparse missing-arg rc2, happy rc0.
DEVIATIONS (minor): (1) report requires a POSITIONAL habit name — golden implied cross-habit `report --week X`;
  app made it per-habit (works WITH a name). (2) habit list omits the streak column (clarify #3 wanted it). (3)
  models.py (336b dataclass) built-but-UNWIRED — review phase CAUGHT it. (4) BOTH test suites FAILED to be
  produced (flail + finalize-spin, verified from traces).
VERDICT: FUNCTIONAL app, comparable to UNIQ8's working core but slightly less complete (report/list deviations,
  NO tests). The --date SpecDrift catch+fix is a clear judge payoff. vs UNIQ8 (743 LOC clean honest DONE): UNIQ9
  is a WIN on functionality (works end-to-end) but a step down on completeness (tests absent, 2 minor spec misses).

### MEASUREMENT-ERROR LESSON (2nd real-exit slip — do not repeat)
First golden pass reported "systematic exit-0 bug on ALL error cases" — WRONG. Cause: I ran `echo "EXIT:"; echo $?`
so `$?` captured the EXIT of `echo "EXIT:"` (always 0), NOT python. Caught it by READING __main__.py source
(main() does sys.exit(1) on error) which CONTRADICTED the bogus measurement -> redid with `python ...; rc=$?`
(NOTHING between) -> errors correctly rc1/rc2. LESSON: capture `rc=$?` on the SAME line right after the process,
never put any command (not even echo) between the process and the $? read. READ-THE-CODE caught the bad metric
(memory assess-qualitatively-not-just-metrics paid off again).

## UNIQ10 — expense-splitter (bipj1vx40) — PARTIAL: CORRECT ENGINE, DRIFTED INTERFACE
The golden run via the app's DRIFTED interface (flat `expense-add DB_PATH GROUP DESC AMOUNT_CENTS PAID_BY MODE`,
--exact NAME:CENTS / --percent NAME:PCT) gives EXACTLY the by-hand values:
  balances: alice +15500c (+$155), bob -7500c (-$75), carol -8000c (-$80) — EXACT.
  settle: carol->alice 8000c ($80), bob->alice 7500c ($75) — EXACT minimal settle.
So ALL THREE split modes (equal/exact/percent), balance math, and greedy settle-up minimization are CORRECT — the
weak swarm built genuinely complex logic right (split-logic 4283b + commands 6433b + db 3473b, all modules done).
BUT the ENTRY drifted the CLI interface from the spec: flat commands (group-add) not nested (group add);
positional DB_PATH not a GLOBAL --db before the subcommand; amounts/display in raw CENTS not dollars-2dp; shares
NAME:CENTS not name=amount. cli-entry-point terminal-FAILED via spec_drift (the judge caught the interface
violation, honest). VERDICT: PARTIAL — correct engine, non-compliant interface. Better logic than UNIQ6 (fail),
but a step down from UNIQ8/UNIQ9 (compliant working entries). 
KEY: this is the cleanest evidence yet that the ENTRY/interface is the weak spot while MODULE LOGIC is strong ->
CLI-contract-freeze (constrain the entry interface to the spec) should convert this PARTIAL to a WIN, since the
engine already computes correctly. High-confidence fix direction (the hard part — the math — already works).

### UNIQ10 FINAL (run_finished): done=[commands, shared-db, split-logic, test-suite], failed=[cli-entry-point, integrate-verify]
test-suite PASSED (25min, slow). cli-entry FAILED via spec_drift (real interface drift) -> integrate-verify
cascade-blocked. run-status HONEST. The AST-review wire-fix wired commands.py WITHOUT breaking the engine
(post-fix balances still EXACT: alice +15500c, bob -7500c, carol -8000c). VERDICT stands: PARTIAL — correct engine,
drifted interface. This is the baseline UNIQ11 (with the CLI-contract) must beat: entry should NOT spec_drift-fail.

## UNIQ11 — library lending (b595ch1c9) — FAIL (db-layer syntax) — CLI-contract INCONCLUSIVE
db-layer FAILED via broken_code x3 (SyntaxError: unterminated string literal line 21) — the weak model could not
self-repair the syntax in 3 attempts even with the compile-error hint. db-layer is a shared dependency, so its
failure cascade-BLOCKED models-layer/commands/integrate-verify AND the cli-parsing ENTRY (which needs db-layers
API) — the entry NEVER wrote its file (only __init__.py + broken db.py on disk). So the CLI-contract validation is
INCONCLUSIVE (no entry artifact to inspect, no complete app to run). Only positive fragment: cli-parsing had 5 ok
judge verdicts / NO spec_drift before it was blocked. CUT via TaskStop (stalled: db-layer dead, 2/3 nodes idle, no
path to success). broken_code verdict WORKED (caught the syntax) — this is a weak-model self-repair limitation, not
a swarm bug. UNIQ12 (helpdesk) is the clean CLI-contract re-validation.

## UNIQ12 — helpdesk ticketing (b4zhx5r0p, 451 LOC) — FULL WIN (best result of the campaign)
CLI-contract WIN (global --db + NESTED subcommands: helpdesk [-h] --db DB {init,agent,ticket,sla}). Golden ALL
CORRECT by running (REAL exit): init rc0; agent add rc0; dup agent rc1; ticket open (nested) rc0; close 3 rc0;
close-already-closed rc1; comment-on-closed rc1; ticket list --status/--priority/--assignee all correct tabular;
agent workload alice --today 2026-07-01 = "1 open ticket(s), avg age 0.0 days" EXACT; sla report --today
2026-07-03 = ticket1 high Age2 SLA1 DaysOver1 BREACHED, t2 low excluded, t3 closed excluded EXACT; unknown
ticket/agent rc1. smoke PASS (entry_ok), integrate-verify DONE, all 3 TEST tasks DONE (tests PASSED — better than
UNIQ9/10 where tests failed). entry WIRED cli via `from helpdesk import cli`. Only nit: empty --today '' tracebacks
(edge case; valid dates work). VERDICT: FULL WIN — matches UNIQ8s honest-DONE milestone AND adds a validated
CLI-contract + passing tests on a COMPLEX nested-CLI app. This is the payoff: CLI-contract converted the entry
class (UNIQ10 drift-fail) into a compliant full win. NOTE: the run itself was still spinning a SPURIOUS review
wire-fix (the from-pkg-import false-positive, now FIXED in fa71ed041) on the OLD binary — the app was done+correct
before that.

## UNIQ12b — helpdesk (b0xe8a1df, 594 LOC) — FULL WIN (ASK_REPLAN=0 A/B arm)
Same spec as UNIQ12 but skip-re-plan. Golden ALL CORRECT by running: --help = helpdesk [-h] --db DB {init,agent,
ticket,sla} (GLOBAL --db + NESTED); init/dup-agent(rc1)/close-already-closed(rc1)/comment-on-closed(rc1); ticket
open WITH and WITHOUT --assignee both rc0 (optional honored, list shows "unassigned"); ticket list --status/
--priority filters correct; workload alice = 1 open avg 0.00 EXACT; sla report = t1 high BREACHED 1over EXACT;
unknown ticket/agent rc1. entry cli-parser 0 spec_drift + 0 broken_code (cleaner than UNIQ12). Modular split
(agents/tickets/sla/db/constants/cli/__main__). VERDICT: FULL WIN — equals UNIQ12 quality while skipping the
re-plan -> the ASK_REPLAN default-off evidence. Also validated AST-import fix (no spurious cli-unwired wire-fix
expected — new binary).

## UNIQ14 — recipe/meal-planner (bwgzd3rtp) — PARTIAL/FAIL (entry SpecDrift loop did not converge; CUT)
Plan merged all commands + shopping aggregation into cli-entry (an ENTRY task) rather than a non-entry multi-file
module -> so the multi-file stub-first fix was NOT tested on its hard case here. cli-entry DRIFTED despite the
CLI-contract note (spec_drift x2: --db not GLOBAL, positional args instead of --flags), SpecDrift caught + re-
dispatched (4 dispatches) and the interface converged to compliant (--help = recipes [-h] [--db DB] {init,recipe,
ingredient,pantry,plan,shopping}) BUT a mid-loop golden showed recipe add NOT persisting (broken transient); the
4th attempt then ran 11+min (run.out stale 650s) without converging -> CUT (stall). Modules (models.py, db.py) done
cleanly. VERDICT: PARTIAL — engine/modules OK, entry stuck in a slow SpecDrift correction loop that did not finish
on this slow fleet. Honest: CLI-contract reduced but did NOT eliminate the entry drift; SpecDrift is the backstop
but a 4-dispatch loop on a slow 27B did not converge in time. The multi-file stub-first HARD case is still untested
-> UNIQ15 (ledger, 2 complex domains) targets it.

## UNIQ15 — double-entry accounting ledger (bl1ow7jsx) — WORKING WIN + 2 minor validation gaps; MULTI-FILE FIX + REVIEW PHASE both VALIDATED
Headline results (JUDGED BY RUNNING, real exit rc=$?):
- MULTI-FILE STUB-FIRST fix VALIDATED on the hard case: balance-reports (balance.py+reports.py, dep-heavy reporting
  engine) trace-confirmed WROTE both owned files FIRST then checked (session 20260701_349), 0 over_read — vs UNIQ13
  plan-shopping flail. shared-types-db (3-file) also clean.
- REVIEW PHASE VALIDATED on the skeleton-first hazard: cli-entry-point was marked DONE with ALL 8 handlers as `raise
  NotImplementedError` (app 100% non-functional at first golden). The idle-node pre-review (correctness-checks on
  completed tasks) CAUGHT it + re-dispatched a fix that FILLED every handler -> NotImplementedError count 8 -> 0, app
  now works. The skeleton-stub hazard IS backstopped by the review, exactly as the skeleton_note code comment claims.
  PHASES PAY OFF (do NOT need a completion-guard stub-detector — the review already recovers it).
- ACCOUNTING MATH CORRECT: balance Cash --as-of = 1300 (1000+500-200); trial-balance total debit 1500 = credit 1500;
  income-statement Income 500 - Expenses 200 = Net 300; MULTI-LINE txn (Cash:60+Rent:40 credit Sales:100) rc0;
  unbalanced rejected (debits=100 credits=90 nonzero); bad --type rc2; 19/19 pytest pass; 825 LOC.
- 2 MINOR VALIDATION GAPS (honest): `balance <UnknownAccount>` -> rc0 (should nonzero — unknown-account not checked
  on balance); `income-statement --from 2026-07-10 --to 2026-07-01` -> rc0 (from>to not rejected on report). 3/5
  error cases enforced (unbalanced, bad-type, and others), 2/5 missed. Weak-model gap on exhaustive validation.
- TEST-writers over_read on att0 (both test tasks) but gate+re-dispatch RECOVERED (19 tests pass). CLI-contract note:
  entry parser structure was correct (nested + global --db, --help clean) — no spec_drift on THIS entry.
VERDICT: WORKING WIN (core double-entry engine correct incl multi-line + trial-balance equality + tests) with 2 minor
validation gaps. Two phase-payoff validations in one run: multi-file stub-first (hard case) + review-catches-skeleton-
stub. Comparable to UNIQ8/12 (win + honest gaps). The strongest end-to-end result of the multi-file arc.

## UNIQ16 — inventory/warehouse (b989fz8w2) — WORKING WIN (math + validation correct) + systematic CLI-CONTRACT DRIFT
JUDGED BY RUNNING (real exit rc=$?), with the app's ACTUAL contract (see measurement note):
- MATH ALL CORRECT: stock level W1 = A 50, B 20, Total 70 (100-30 ship -20 transfer); valuation --warehouse A = $125.00
  (50*2.50); insufficient ship -> "Error: insufficient stock" rc1; transfer A->B moves qty; 8/8 pytest pass; 650 LOC.
- VALIDATION COMPLETE (app logic, not argparse): insufficient-ship, same-warehouse-transfer, negative-price,
  unknown-SKU, duplicate-SKU all rc1. Better than UNIQ15 (which missed 2/5) — the architect split out a dedicated
  hardening-cli-errors task + dynamic-replan ADDED test-validation-edge-cases (coverage expansion PAYS OFF).
- MULTI-FILE STUB-FIRST 2nd data point CONFIRMED: 4-file non-entry module (cmd-init-product-warehouse: commands/
  __init__+init_+product+warehouse) DONE 0 over_read; shared-core 2-file clean; cli-framework 3-file entry clean.
- ENTRY NOT STUBBED (0 NotImplementedError) — no skeleton-stub recurrence. test-stock finalize-spin RECOVERED via
  re-dispatch (94.9s). Reasoning was clean overall (no over_read; 2 finalize-spins both recovered).
- THE FLAW — systematic CLI-CONTRACT DRIFT: the app converted EVERY spec positional to a --flag (product add --sku
  SKU not positional SKU; warehouse add --name NAME not positional NAME; stock level --sku), and renamed options
  (--from/--to -> --source/--dest; --reorder -> --reorder-level). Internally consistent + tests pass, but does NOT
  match the spec CLI surface. The CLI-contract note did NOT prevent positional-vs-flag drift; SpecDrift did not flag
  it. A spec-following user hits argparse errors.
VERDICT: WORKING WIN (correct engine + full validation + multi-file fix 2nd data point) with a CLI-surface drift.
Comparable-or-better than UNIQ15 on correctness/validation; the CLI-contract drift is the one real miss.

### MEASUREMENT LESSON (3rd real-exit slip caught) — a spec-contract golden gave FALSE nonzeros
My first UNIQ16 golden used the SPEC contract (positional SKU, --from/--to). The app drifted to --sku/--source/--dest,
so EVERY command hit argparse rc=2 (missing-required-arg) BEFORE app logic. I nearly concluded "7 validation cases
nonzero = validation works" — a FALSE PASS (the nonzeros were argparse rejecting MY wrong syntax, not the app
validating). Caught it by reading the app's ACTUAL add_argument contract, then re-goldened with --sku/--source/--dest
-> revealed the math+validation are actually correct AND surfaced the CLI-drift. LESSON: when a golden shows uniform
rc=2 with "usage:" output, it is argparse rejecting YOUR syntax (contract mismatch), NOT the app erroring — read the
app's real argparse before scoring. rc=2 (argparse) vs rc=1 (app sys.exit) distinguishes them.

## UNIQ17 — library lending (bbemc0qyu) — CLI-contract POSITIONAL fix VALIDATED (headline); app left broken by fix-loop regression
- HEADLINE WIN: the positional-vs-flag strengthening (a9466d898) VALIDATED. Original cli.py kept EVERY spec positional
  (member add NAME -> add_argument('name'); book add ISBN; loan out ISBN MEMBER -> 2 positionals; report member MEMBER)
  and --from/--to unrenamed; confirmed by RUNNING member add Alice -> rc0 (positional accepted, not rc2 argparse).
  Direct contrast to UNIQ16 total drift. The fix WORKED. SpecDrift also caught a cli-entry drift (cli-entry x2).
- multi-file stub-first 3rd data point: db-layer (2-file) clean 0 over_read.
- BUG 1 (schema drift): report overdue crashed no-such-column-isbn on first golden; review/fix ADDED loans.isbn
  (review DID catch the runtime SQL error class). BUG 2 (REGRESSION from the fix): a cli-entry re-dispatch then broke
  cli.py with dest-supplied-twice-for-positional -> EVERY command crashes; the fix was NOT re-verified (a trivial
  --help catches it). So the app ended BROKEN by the fix loop despite the correct original entry. Run thrashed
  (cli-entry x2, tests-core x3 over_read) -> CUT.
VERDICT: positional fix VALIDATED (the campaign goal for this app) but app NOT shippable (fix-loop regression). The
review is NET positive across apps (UNIQ15 skeleton-stub fixed cleanly) — UNIQ17 is the 1st regression seen ->
review-wire-fix-spin-protection (gate corrective re-dispatch on post-fix --help smoke) is now a well-evidenced N=1
backlog item; build on a 2nd instance or after assessing the phase order. tests over_read N=3 (recovers).

## UNIQ18 — gradebook (b0gl5m7o9) — FULL WORKING WIN (cleanest of the arc); positional fix 2nd data point + weighted-avg correct
JUDGED BY RUNNING (real exit rc=$?), read ACTUAL contract first:
- CLI-CONTRACT POSITIONAL fix VALIDATED (2nd data point, hardest case): cli.py kept EVERY spec positional — course
  add name; student add name; assignment add COURSE NAME (2 positionals); grade set COURSE ASSIGNMENT STUDENT (3
  POSITIONALS); report course/student/gradebook positional. All commands rc0/rc1 (app logic), NOT rc2 (argparse).
  So positional-vs-flag strengthening (a9466d898) now VALIDATED on 2 apps (UNIQ17 lending + UNIQ18 gradebook 3-pos).
- WEIGHTED-AVERAGE MATH CORRECT: report student Alice = 86.0% (= (40*8/10 + 60*90/100)/(40+60)); Bob = 50.0% (=
  (40*5/10)/40); report course sorts high-to-low. Exact match to hand computation — the normalized-by-graded-weight
  formula is right.
- VALIDATION COMPLETE (app logic rc1): unknown-student, duplicate-course, zero-weight, score>max, total-weights>100
  all rejected. 5/5 error cases enforced. 5/5 pytest pass. 715 LOC.
- CLEAN ENTRY: cli-entry 8 ok verdicts, NO spec_drift, NO stub, NO fix-loop regression (contrast UNIQ17 dest crash)
  -> review-fix-regression stays N=1 (not N=2, do NOT build review-wire-fix-spin yet).
- MULTI-FILE stub-first: 3 multi-file modules (shared-models 3-file, course-student + assignment-grade 2-file) all
  clean 0 over_read = 5th app confirming the fix; report-cmds single-file dep-heavy over_read then RECOVERED.
VERDICT: FULL WORKING WIN — correct engine + full validation + correct weighted-avg + clean entry + tests pass. The
cleanest end-to-end result of UNIQ15-18. Demonstrates the campaign compounding: shipped fixes (positional, multi-file)
validated while the app is fully correct. app-after-app improvement realized.

## UNIQ19 — build-order resolver / topo sort (bp1thxa24) — FULL WORKING WIN (graph algorithm correct); 2nd consecutive full win
JUDGED BY RUNNING (real exit rc=$?), read ACTUAL contract first, topo VERIFIED programmatically:
- GRAPH ALGORITHM CORRECT: order = A,B,C,D and PROGRAMMATICALLY VERIFIED a VALID topological sort (index(req) <
  index(dependent) for every edge A->B, A->C, B->D, C->D; alphabetical tiebreak deterministic). Module uses Kahn's
  algorithm with a min-heap + DFS three-state coloring for cycle detection — correct, sophisticated, FIRST TRY, no
  broken_code. dependents A = B,C,D (transitive closure correct).
- CYCLE DETECTION CORRECT: dep add A D (would close A->D->B->A) REJECTED "error: cycle" rc1; graph intact after (order
  still A,B,C,D). check rc0 acyclic.
- VALIDATION COMPLETE: unknown-package, duplicate, self-dependency, cycle-closing-add all rc1 (4/4). 19/19 pytest. 415 LOC.
- CLI-CONTRACT POSITIONAL fix 3rd data point: package add NAME; dep add PACKAGE REQUIRES (2 positionals); dependents
  PACKAGE — all positional, dep add B A -> rc0 not rc2. Positional-vs-flag strengthening now VALIDATED on 3 apps.
- CLEAN RUN: 0 over_read, 0 broken_code, 0 spec_drift, NO fix-loop regression -> review-fix-regression stays N=1.
  Multi-file: db-layer + graph.py single-file dep-heavy clean; cli-app 3-file entry clean.
FINDING (payoff campaign): the weak model CAN produce a CORRECT sophisticated algorithm (Kahn topo + DFS-coloring
cycle detection) end-to-end when scoped to a dedicated module — contrast UNIQ11 db-layer broken_code-couldnt-self-fix.
So the recurring broken_code limit is task/scoping-specific, not a blanket algorithmic ceiling.
VERDICT: FULL WORKING WIN. 2 consecutive clean full wins (UNIQ18 gradebook + UNIQ19 buildgraph) — the shipped fixes
(positional, multi-file) hold while apps are fully correct across CRUD-aggregate AND graph-algorithm archetypes.

## UNIQ20 — FSM workflow engine (bctbpb6rd) — FULL WORKING WIN (3rd consecutive); FSM correct + positional 4th data point
JUDGED BY RUNNING (real exit rc=$?), read ACTUAL contract first:
- FSM CORRECT: fire sequence Draft -(submit)-> Review -(approve)-> Published, each status confirms; history = exact
  ordered path Draft,Review,Published; firing an event with NO transition from current state -> rc1 "No valid
  transition" AND instance STAYS Published (correct semantics). shared-db-engine (3-file, engine.py transition logic)
  built clean 0 broken_code.
- VALIDATION COMPLETE: no-transition, dup-instance, unknown-state, start-unknown-state, unknown-instance all rc1 (5/5).
- CLI-CONTRACT POSITIONAL fix 4th data point: state add NAME; transition add FROM_STATE TO_STATE (2 pos) + --on flag;
  fire ID EVENT (2 pos); start ID --at; status/history ID. All positional. NO dest-on-positional regression (dest= is
  only on --on/--at FLAGS, legal; fire's event positional is clean). Positional fix now VALIDATED on 4 apps.
- BLEMISH: tests-engine broken_code (a TEST module compile error) -> thinner suite (3 tests ran + pass) but the APP is
  fully correct. NOT an app defect; a test-file quality miss (weak model). No fix action (isolated).
- CLEAN otherwise: 0 spec_drift, no entry stub, no fix-loop regression -> review-fix-regression STAYS N=1.
VERDICT: FULL WORKING WIN. 3 CONSECUTIVE clean full wins (UNIQ18 gradebook CRUD-aggregate, UNIQ19 buildgraph graph-algo,
UNIQ20 flow FSM) across 3 distinct archetypes -> the nested-CLI+SQLite+validation app CLASS is now solidly handled by
the swarm with the shipped fixes. Next (UNIQ21): RAISE difficulty to a NEW dimension (JSON export/import round-trip +
multi-format output) to surface the next real finding rather than re-confirm a solved class.

## UNIQ21 — contacts (bh0i3eg3i) — PARTIAL at RAISED difficulty (broke the 3-win streak — the difficulty ramp found the ceiling)
JUDGED BY RUNNING (real exit rc=$?), read ACTUAL contract first (2 golden passes — spec-contract failed on structure drift, re-goldened with app contract):
- WORKS: contact CRUD (top-level add/show/list/delete); MULTI-FORMAT for CONTACTS list --format json = VALID JSON 2
  objs, --format csv = proper header+rows; validation dup/no-@/unknown-group all nonzero; 21 pytest pass.
- BROKEN 1 — member-format cross-module bug: member list crashes TypeError string-indices in utils.py:58
  format_members does m["contact_name"] but cli.py passes STRINGS not dicts. So multi-format works for contacts but
  is INCONSISTENT/broken for members (a cross-module contract mismatch — the exact CONTRACT-DRIFT failure class, now
  at raised difficulty on a 2nd formatter the model wrote differently from the 1st).
- BROKEN 2 — JSON export/import ROUND-TRIP broken: export --file -> argparse rc2 (file handling wrong) -> no file
  written -> import finds nothing. The headline new dimension (serialization round-trip) FAILED.
- CLI-STRUCTURE DRIFT: spec nested contact add NAME / group addmember GROUP CONTACT -> app FLATTENED to top-level add
  + a separate member{add,remove,list} group. The CLI-contract note (nested stays nested) did NOT prevent
  reorganization at the larger 10-command surface; SpecDrift did not flag it (cli-entrypoint over_read then recovered,
  no spec_drift verdict). Positionals themselves kept (add NAME; member add GROUP CONTACT 2 pos) = 5th data point OK.
- TESTS THIN: 21 pass but miss the member-crash AND the export bug -> test coverage does not exercise the new
  dimensions (multi-format-for-all-entities, round-trip). Weak-model test-quality gap at raised difficulty.
- MINOR: --db defaults to :memory: (spec wanted a file) -> no-persistence without --db.
VERDICT: PARTIAL. The difficulty ramp WORKED as intended — 3 clean full wins on the standard class, then raised
difficulty (multi-format-everywhere + JSON round-trip + 10-cmd surface) EXPOSED genuine weak-model ceilings:
cross-module format-CONSISTENCY (2nd formatter written wrong), file-IO round-trip, structure reorg on large surface,
and thin tests on new dimensions. These are the NEXT real findings. WATCHING if the review/smoke catches the runtime
crashes (member list + export) before run_finished.

## UNIQ22 — notes / ISOLATED JSON round-trip (bht7v4r4g) — FULL WORKING WIN (calibration: round-trip ALONE works)
JUDGED BY RUNNING (real exit rc=$?), read ACTUAL contract first (2 golden passes — 1st used flat `add`, app is nested
`note add`; SpecDrift caught+FIXED a flatten draft -> app correctly nested, my 1st golden was wrong not the app):
- JSON EXPORT/IMPORT ROUND-TRIP WORKS end-to-end: export -> valid JSON 2 notes; import into fresh t2.db -> both notes
  present; note show Meeting -> Body "agenda" + Tags "work, urgent" PRESERVED (bodies AND tags survive round-trip);
  re-import IDEMPOTENT -> still 2 notes (merge skips existing). This is the exact dimension UNIQ21 FAILED — here it WORKS.
- multi-tag (--tag action=append repeatable) works; search --tag correct; note list shows title+tags; nested note
  add/show/list/delete + top-level search/export/import (matches spec); validation dup/unknown-show/unknown-delete/
  missing-file all rc1 (4/4); 3 pytest pass; 346 LOC; positionals kept (note add TITLE) = 6th data point.
- CLEAN: no broken_code; SpecDrift PAID OFF (fixed the note-flatten drift to nested — contrast UNIQ21 where the
  10-cmd flatten was not caught). export --file / import --file both correct (contrast UNIQ21 export --file broken).
CALIBRATION VERDICT (resolves the UNIQ21 question): the JSON round-trip ALONE = FULL WIN. So UNIQ21's failure was
COMBINATION-OVERLOAD (multi-format-for-ALL-entities + round-trip + 10-cmd surface + 2 entities simultaneously), NOT
the round-trip dimension itself. The swarm handles each hard dimension INDIVIDUALLY (round-trip UNIQ22, graph UNIQ19,
FSM UNIQ20, weighted-avg UNIQ18, multi-format-for-contacts UNIQ21-partial) but hits a CUMULATIVE-complexity ceiling
when MANY are combined at once = weak-model capacity limit, NOT a single-dimension bug -> NO clean single-dimension
fix. This is the honest map of the ceiling. UNIQ23 tests the 2-dimension boundary (round-trip + multi-format on 1
entity) to locate where cumulative overload begins.

## UNIQ23 — task tracker / 2-DIM boundary (bhji5olht) — FULL WORKING WIN (round-trip + multi-format together on 1 entity)
JUDGED BY RUNNING (real exit rc=$?), nested task tree (task add/done/show/list + top-level export/import/report):
- MULTI-FORMAT (dim 1) ALL VALID: list --format json = valid JSON 2 objs; --format csv = header id,title,priority,due,
  status + 2 rows; report --format json = valid nested {done:..,open:..}. table = aligned columns.
- JSON ROUND-TRIP (dim 2) PRESERVES ALL FIELDS: export valid JSON 2 tasks; import into fresh t2 -> 2 tasks; show ->
  priority=high due=2026-07-10 status=done ALL preserved (incl the done status set before export); re-import IDEMPOTENT
  (still 2). done + --status done filter works.
- VALIDATION 5/5 nonzero: dup(rc1), bad-priority(rc2 argparse), bad-format(rc2 argparse), unknown-id(rc1), missing-file
  (rc1). 301 LOC. positionals kept (task add TITLE; done/show ID) = 7th data point. no broken_code; no entry stub.
- The confidence-ASK gate scored this 2-dim app 65 (vs 88 single-dim) -> ASKed 4 good Qs -> discriminates by complexity.
2-DIM BOUNDARY VERDICT: round-trip + multi-format TOGETHER on 1 entity = FULL WIN. So the ceiling is NOT 2 dimensions.
REFINES UNIQ21: its failure was NOT the round-trip nor multi-format per se, but CUMULATIVE overload from MULTIPLE
ENTITIES (its member-format crash was the 2nd entity's formatter written INCONSISTENTLY with the cli data shape -
dicts vs strings) PLUS a 10-command surface. With ONE entity -> ONE formatter -> consistent -> works. So the specific
ceiling is MULTI-ENTITY format/serialization CONSISTENCY (writing N mutually-consistent formatters/handlers) + large
surface = a weak-model cross-module-agreement limit. Capability MAP: 1-dim works (UNIQ18/19/20/22); 2-dim/1-entity
works (UNIQ23); 4-dim/2-entity/10-cmd fails (UNIQ21). UNIQ24 tests the multi-entity-format hypothesis directly (2
entities, multi-format on both, small surface, no round-trip) - if the 2nd formatter drifts again = N=2 confirmed.

## UNIQ24 — CRM / 2-entity multi-format (bpndnemip) — PARTIAL: format-hypothesis REFUTED, order-add validation GAP
JUDGED BY RUNNING (real exit rc=$?), nested tree (customer add/list/orders + order add/list + report; positionals name/
customer_name kept = 8th data point):
- MULTI-ENTITY FORMAT HYPOTHESIS *REFUTED*: ALL 3 formatters VALID + CONSISTENT — customer list (json 2, csv name,email),
  order list (json 3, csv id,customer_name,amount,date), customer orders (json 2, csv id,amount,date). The 2nd/3rd
  formatter did NOT drift/crash (contrast UNIQ21 member-format). report agg CORRECT + sorted DESC (Globex 200, Acme 150).
  So the swarm handles 2 entities x 3 formatters cleanly -> UNIQ21's member-format drift required the FULL 4-dim
  combination (multi-format-everywhere + round-trip + 10-cmd + 2-entity), NOT just 2 entities. Ceiling = CUMULATIVE.
- NEW BUG (order-module validation GAP): order add Nope (unknown customer) -> rc0 (should be nonzero); order add Acme
  --amount -5 (negative) -> rc0 (should be nonzero). The order module wrote the happy-path + formatter but SKIPPED 2 of
  its spec-required guards (customer-existence FK check + amount-positivity). Customer-side validations WORK (dup rc1,
  no-@ rc1, bad-format rc2, unknown-cust-orders rc1) = 4/6. 406 LOC. tests over_reading (N=4, recovering) so tests did
  not even run. AST clean (0 NotImplementedError). no format broken_code.
VERDICT: PARTIAL. The hypothesis test cleanly REFUTED multi-entity-format-consistency as the ceiling (formatters are
fine). But it surfaced a RECURRING finding at N=2: SMOKE (--help + collect-only) + THIN/OVER-READ TESTS miss runtime
CORRECTNESS/VALIDATION bugs (UNIQ21 member-crash+export N=1; UNIQ24 order-add 2 missing validations N=2). The app
"passes" smoke + ships with real validation holes. This — not format-consistency — is the real recurring swarm-quality
gap. ASSESS a fix next: read the smoke code; a spec-derived VALIDATION SMOKE (extract the spec's 'reject with nonzero'
clauses, RUN each, assert nonzero) would catch the validation-gap class; a run-commands+detect-traceback smoke would
catch the UNIQ21 crash class. Both MED conf — READ smoke_fix_description ~5184 + SMOKE flow first, build only if sound.

## UNIQ25 — secrets vault / VALIDATION-STRESS (brbp3453c) — FULL WIN, 11/11 validations (N=3 REFUTES the systemic-gap hypothesis)
JUDGED BY RUNNING (real exit rc=$?), nested tree (secret set/get/list/delete/rename/tag + policy set + audit; positionals
key + rename OLD NEW). NO ASK (planned >=80 — single-entity clear-validation app is easy to plan).
- ALL 11 SPEC VALIDATIONS ENFORCED (+ bad-format = 12/12), every one a proper nonzero: empty-key rc1, empty-value rc1,
  get-unknown rc1, delete-unknown rc1, rename-old-missing rc1, rename-new-collision rc1, tag-unknown rc1, policy-unknown
  rc1, max-reads-0 rc1, max-reads-neg rc1, audit-unknown rc1, bad-format rc2. ZERO rc0 bugs.
- HAPPY PATH works: set/get (V1)/list json(2)+csv(header+rows)/tag/rename/policy/audit; 12 pytest pass; 250 LOC;
  dedicated exceptions.py module (clean validation via custom exceptions). test-foundation spec_drift (a test task, not
  app-breaking; SpecDrift flagged it).
VALIDATION-COMPLETENESS VERDICT (N=3): 11/11 = the weak model implements validations WELL when clearly enumerated.
UNIQ24's 2 missing order-add guards were an OUTLIER (correlated with 2-entity complexity + the order module), NOT a
systemic validation-completeness gap. So the "model skips validations" hypothesis is REFUTED at N=3 -> NO validation
fix warranted (the highest-payoff validation-smoke was LOWEST-confidence anyway; not building it was correct). The
"smoke is shallow (collect-only + --help, skips test execution)" observation stands but is a MINOR gap since validations
are mostly correct -> a fragile validation gate would HURT more than help. Honest close: the swarm + weak model handle
enumerated validation well; UNIQ24 was situational. CAMPAIGN CEILING MAP holds: single-dim + 2-dim/1-entity + 2-entity-
multi-format + validation-stress ALL WORK; only UNIQ21's full 4-dim-2-entity-10cmd combination overloads (cumulative).

## CORRECTION — UNIQ24 was goldened TOO EARLY (mid-run); its final state is a (near-)FULL WIN
After UNIQ24's background run FINISHED (run_finished: all 7 tasks done, 0 failed), I re-checked its 2 "validation bugs":
order add unknown-customer -> rc1 (was rc0 mid-run); order add neg-amount -> rc1 (was rc0 mid-run). BOTH FIXED. I had
goldened UNIQ24 when cli-entry-point was done but INTEGRATE-VERIFY had NOT yet run; the integrate-verify (or a
review-fix it triggered) then FIXED both order-add validation guards. So:
- UNIQ24 CORRECTED VERDICT: (near-)FULL WIN — all 3 formatters consistent+valid (already confirmed) AND all validations
  work in the FINAL state. The "validation-gap" I recorded was a MID-RUN measurement artifact, not a shipped bug.
- INTEGRATE-VERIFY / REVIEW PAYS OFF ON VALIDATION: it caught + fixed the 2 order-add guards UNIQ24's happy-path modules
  had skipped. (Contrast UNIQ21, whose run FAILED — marked failed, did not ship a false success.)
- LESSON (discipline): DO NOT golden before run_finished / integrate-verify completes. A mid-run "complete-but-stale"
  golden can catch TRANSIENT bugs that the run's OWN later phases fix -> premature/wrong verdict. My quick-test-when-
  complete shortcut caused this. Golden AFTER run_finished, or explicitly mark mid-run goldens PROVISIONAL + re-verify on
  completion. (UNIQ23 + UNIQ25 mid-run goldens were already all-green, so those verdicts stand; only UNIQ24 was affected.)

## UNIQ26 — inventory+orders store / UNIQ21-SCALE CAPSTONE (bjcy8it0n) — near-FULL WIN, CEILING MOVED (PROVISIONAL, integrate-verify still running)
JUDGED BY RUNNING (real exit rc=$?), nested tree (product add/list/show/orders + order add/list + export/import_/report;
positionals sku/name kept). The FULL 4-dim combination that OVERLOADED UNIQ21 (2 entities + multi-format 3 cmds +
JSON round-trip + revenue-agg report + ~9 cmds + 7 validations):
- BUILT CLEANLY: all 5 build tasks (db-layer/cli-parsing/command-handlers/entry-point/tests) done with ZERO non-ok
  verdicts — NO cli-entry over_read, NO broken_code, NO spec_drift, NO cross-module mismatch. CONTRAST UNIQ21 which
  FAILED (cli-entrypoint x3 over_read + member-format crash + export --file broken). The plan SPLIT the entry
  (cli-parsing separate from command-handlers) which absorbed the big surface.
- WORKS across all 4 dims: 3 formatters consistent+valid (product list json2/csv, order list json3, product orders
  json2); report agg EXACTLY correct (SKU1 5u=$50, SKU2 1u=$20) sorted revenue DESC; export valid nested {products:2,
  orders:3}; ROUND-TRIP LOGIC works (import_ preserves products+orders into fresh db); 7/7 validations nonzero
  (dup-sku, neg-price, unknown-sku-order, zero-qty, bad-format, unknown-show, missing-file); 7 pytest pass; 378 LOC.
- ONE BUG: the import subcommand is registered as `import_` (trailing underscore — Python-KEYWORD avoidance leaked into
  the argparse subcommand STRING), so the spec's `store import --file` -> argparse invalid-choice. The round-trip is
  inaccessible via the spec name (works via `import_`). A specific, mechanical CLI-CONTRACT DRIFT (import->import_).
CAPSTONE VERDICT (PROVISIONAL — integrate-verify still running, may rename import_->import like it fixed UNIQ24): the
cumulative-overload CEILING has MOVED ABOVE UNIQ21-scale. The full 4-dim app that FAILED as UNIQ21 now BUILDS CLEANLY +
WORKS almost entirely. What carried it: (1) the planner SPLIT the entry (cli-parsing / command-handlers) so no big-entry
over_read; (2) DB-schema-CONTRACTS + multi-file stub-first kept modules consistent (no member-format-class mismatch);
(3) accumulated CLI-contract + skeleton fixes. The ONE remaining defect is a NEW, narrow, HIGH-confidence-fixable class:
Python-keyword subcommand names get a spurious trailing underscore. RE-VERIFY after run_finished (does integrate-verify
rename it?). If NOT -> add a cli_contract_note line: argparse subcommand names are STRINGS, use the EXACT spec name even
if a Python keyword (add_parser('import') not 'import_'); keyword-avoidance is for Python identifiers not CLI strings.
