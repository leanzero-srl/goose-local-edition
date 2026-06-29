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
| APP4 | Python | multi-module + INQUISITIVE | habit tracker (add/done/streak/list, JSON) | — | IN FLIGHT | — | run_in_background bcqqtud2a, NEW binary f5cac6468, ASK_FLOOR=75 -> validates idle-fix (PreReview events) + advertised-entry prompts + inquisitive |

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
