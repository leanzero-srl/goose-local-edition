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
| APP2 | TypeScript | greenfield CLI (moderate) | CSV column stats (mean/median/mode/stddev, --column, --json) | — | IN FLIGHT | — | run_in_background bkgnsb30f, CONTRACTS on |

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
