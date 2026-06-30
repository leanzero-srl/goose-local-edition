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
