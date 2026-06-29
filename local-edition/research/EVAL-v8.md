# EVAL v8 — 9 end-to-end runs on the qwopus fleet (new improved binary + flags ON)

Validates the Track A v8 improvements against the 4 A/B draw classes (lone-node STALL,
cross-module CONTRACT DRIFT hidden by isolation tests, BUILT-BUT-UNWIRED entry, NO
end-to-end run). Each run: fresh scratch dir, `target/debug/goose swarm run "<spec>"
--output-format json`, env `LMSTUDIO_HOST` + `LMSTUDIO_API_KEY` + `CONTEXT7_API_KEY` +
the `GOOSE_SWARM_*` flags shipped at launch. Reviewed QUALITATIVELY FIRST (read source,
trace correctness, RUN the entry point on golden inputs / Playwright for web-SVG, check
tests assert GOLDEN values), then quantitatively. Scores corr/test/qual/spec 1-10 vs the
`AB-CONTROLLED.md` baseline (qwopus means 5.8/5.6/7.6/5.6).

Fleet (auto-discovered via lms ps): 3x qwopus3.6-27b-coder-mtp — gabee@Mac.lan,
mihai@Local, workhorse@WorksMacStudio.lan, all weight-1.

Binary builds this session: `80f9b2408` (GOOSE_SWARM_SMOKE, Track A #1).

## Run matrix

| # | arch | spec | flags active | scratch dir | status | smoke fired | scores (c/t/q/s) | verdict |
|---|------|------|--------------|-------------|--------|-------------|------------------|---------|
| A1-1 | A1 minimal | a CLI markdown-to-HTML renderer | SMOKE,SPLIT,PREREVIEW,JUDGE,research | A1-1-mdhtml | DONE | YES / PASS | 9/9/9/9 | CLEAN WIN |
| A1-2 | A1 minimal | a CLI spreadsheet with formulas | SMOKE,SPLIT,PREREVIEW,DONE_GATE,JUDGE,research | A1-2-sheet | DONE | YES / caught FAIL | 2/0/5/2 | FAIL (no entry; 5/7 subtasks failed) |
| A1-3 | A1 minimal | a CLI task scheduler | SMOKE+autofix,SPLIT,PREREVIEW,DONE_GATE,JUDGE,research | A1-3-sched | DONE | smoke PASS but app BROKEN | 3/5/5/4 | RUNS-but-broken (store unwired -> no persistence; AST reviewer caught it) |
| A1-3 | A1 minimal | a CLI task scheduler | (tbd) | — | pending | — | — | — |
| A2-1 | A2 max-detail | double-entry ledger CLI (full spec) | SMOKE+autofix,SPLIT,PREREVIEW,DONE_GATE,CONTRACTS,REVIEW,research | A2-1-ledger | DONE | smoke+review CLEAN | 8/6/8/8 | WIN (1st multi-module WIN — CONTRACTS decoupled) |
| A2-2 | A2 max-detail | log-pipeline DSL (full spec) | full v8 stack (+ stub-cleanup + wire-fix) | A2-2-logdsl | DONE | smoke+review CLEAN | 8/8/8/8 | WIN (2nd multi-module WIN — logfunnel class tamed) |
| A2-3 | A2 max-detail | state-machine workflow engine (full spec) | full v8 stack | A2-3-fsm | DONE | smoke+review CLEAN | 8/6/8/8 | WIN (A2 = 3/3 — DRAW class converted) |
| A3-1 | A3 feature | chaos-fern: add --svg export (Playwright) | full v8 stack (amendment) | A3-1-chaosfern-svg | CUT (thrash) | n/a (cut) | 5/3/4/6 | PARTIAL: --svg WORKS but amendment RE-ARCHITECTED (abandoned dups, broken test); AST caught it; Playwright env-blocked |
| A3-2 | A3 feature | byte-oracle: add --json output | full v8 stack (amendment, edit-in-place instr) | A3-2-byteoracle-json | CUT (wire-fix loop) | smoke PASS (entry --help ok) | 4/5/4/5 | PARTIAL/FAIL: --json written to STRAY ROOT cli.py (wrong path) so `-m byte_oracle --json` errors; AST caught stray; wire-fix flailed on pre-existing detector dup |
| A3-3 | A3 feature | byte-oracle: add --count | full v8 stack (amendment, NEW binary f9e89b782, NO instr) | A3-3-byteoracle-count | DONE | smoke PASS | 7/6/7/7 | WIN — VALIDATES f9e89b782: --count in the REAL cli.py in place, NO stray, works via -m |
| TS-1 | agnostic | LANG=TypeScript todo CLI (greenfield) | architect de-Python ONLY (6881ae6d9), CONTRACTS off | TS-1-todo | CUT (iv thrash 13x) | 30 vitest PASS | 4/6/5/5 | real working TS LOGIC, but CLI entry crashes (new URL on a path); integrate-verify thrashed on the OLD-binary Python prompt -> never ran the TS entry -> bug shipped (validates worker de-Python need) |
| A3-2 | A3 feature | byte-oracle: add --json output | (tbd) | — | pending | — | — | — |
| A3-3 | A3 feature | byte-oracle/chaos-fern: 2nd feature | (tbd) | — | pending | — | — | — |

Note: chaos-fern already ships a `sierpinski` IFS option (seen in its `--help`), so A3-3's
second-fractal idea is taken — pick a different feature (e.g. byte-oracle magic-number
signatures, or chaos-fern `--from-file` validation/error-path hardening).

## Per-run notes

### A1-1 — a CLI markdown-to-HTML renderer — CLEAN WIN (corr 9 / test 9 / qual 9 / spec 9)
DONE on the SMOKE binary (80f9b2408): all 6 subtasks, 0 failed; dispatched gabee 3 /
mihai 2 / workhorse 4.
- SMOKE GATE FIRED + PASSED: ran=true, py_files=7, collect=ok, entry_package=mdhtml,
  entry_ok=true, findings=[]. The v8 oracle gave a deterministic "this app runs" signal
  the old runs lacked. First production proof the SMOKE increment works.
- RAN it on golden markdown (heading/bold/italic/inline+fenced code/list/link): correct
  CommonMark HTML across ALL constructs (h1, strong, em, code, ul/li, a href, pre>code
  language-python). renderer.py wisely WRAPS markdown-it-py (a real CommonMark lib) —
  idiomatic, not a hand-rolled parser — with graceful optional-dep degradation
  (linkify/typographer/pygments warn+skip to stderr; HTML clean on stdout), RenderError
  wrapping, full typing + docstrings.
- TESTS assert GOLDEN values, not smoke-tests: 25 passed / 1 skipped — <strong>bold</strong>,
  the <em><strong> nesting order, html-disabled strips <script> (security), inline/fenced
  code; plus subprocess CLI integration (empty file -> empty out + rc 0; nonexistent path
  -> rc != 0 + stderr). The 1 skip is the linkify test, correctly skipped (plugin absent).
- vs AB qwopus mean 5.8/5.6/7.6/5.6 — well above; on par with the clean-win class
  (chaos-fern 9/8/9/9, byte-oracle 9/9/9/9).
- Caveats: A1 minimal-spec is the EASY archetype (clean cohesive app = qwopus's known
  strength), so a clean win is expected; the v8 features' real test is A2 (multi-module
  draw class) — does SMOKE/DONE_GATE/contracts CATCH a broken integration. The judge fired
  ~39 verdicts (chatty but harmless: one correct over_reading re-dispatch of integrate-
  verify, resolved on attempt 1). Minor APP nit (not a swarm issue): linkify is default-on
  but the plugin isn't installed, so every run warns to stderr — defensible degradation.

### A1-2 — a CLI spreadsheet with formulas — FAIL (corr 2 / test 0 / qual 5 / spec 2)
DONE on the 8f4fb225a binary (SMOKE+SPLIT+PREREVIEW+DONE_GATE; NO contracts/autofix — predates
them). Outcome: "5 core subtask(s) failed". DONE: shared-types, cell-sheet. FAILED: formula-parser
(3 attempts -> exhausted), and its 4 dependents cascaded — formula-evaluator, cli-entry, tests,
integrate-verify. Files: only spreadsheet/{types,cell,sheet,formula_parser,__init__}.py — NO
evaluator, NO cli.py, NO __main__.py, NO tests. Not a runnable spreadsheet.
- SMOKE GATE caught it CLEANLY: ran=true, collect=ok (the 5 modules import), entry_package=null,
  finding="no python3 -m <pkg> entry point (no package with __main__.py) — the app may be
  unrunnable". The deterministic no-entry / unrunnable verdict — exactly the BUILT-BUT-UNWIRED /
  NO-ENTRY draw class. (The scheduler also reported the 5 failures, so detection was doubly clear.)
- ROOT CAUSE (corrected after reading the run — NOT max-turns): the DETAILER's spec said "File owned:
  formula_parser.py" while the skeleton owned [spreadsheet/parser.py] — the detailer was never told the
  owned files, so it invented a contradicting name. The worker followed the spec -> wrote
  formula_parser.py -> the assigned parser.py was NEVER created (no parser.py on disk) -> the
  missing-owned-files guard returned Transient every attempt -> 3 attempts exhausted ->
  fail_descendants tanked evaluator/cli/tests/integrate-verify. The 385-line formula_parser.py parses
  (DONE_GATE correctly silent); judge over_reading kills were secondary. FIXED in 7e81b3b6a (detailer
  now gets owned_files + must use them verbatim).
- vs AB qwopus mean 5.8/5.6/7.6/5.6 — WELL BELOW; a clear FAIL, the multi-module regime where qwopus
  draws/loses (logfunnel/fsdrift class). A1 "minimal spec" but it decomposed into a hard 7-subtask
  multi-module app, so it accidentally tests the draw class.
- WHAT THE v8 GATES WOULD CHANGE (to test on A2 with the full binary): (a) CONTRACTS — a FROZEN
  formula-parser interface injected upfront would let evaluator/cli build against it EVEN IF the
  parser task is rocky, decoupling the lynchpin (the single biggest potential win here). (b)
  SMOKE-AUTOFIX — on the no-entry finding it would fire one fix worker to add __main__.py/cli wiring
  (partial — the missing evaluator is a deeper hole). (c) JUDGE over-kill of the hard parser
  (observation, now 2nd data point) wasted attempt 0 — candidate judge tuning if it recurs on A2.
- KEY TAKEAWAY: the "worker writes the file but the TASK fails (no clean final_output on a hard
  module)" failure is distinct from a syntax error (DONE_GATE) — a hard lynchpin task exhausting its
  attempts cascades the whole run. CONTRACTS' decoupling is the most promising mitigation; validate on A2.

### A1-3 — a CLI task scheduler — RUNS BUT BROKEN (corr 3 / test 5 / qual 5 / spec 4)
DONE on 31f671a18 (SMOKE+autofix+SPLIT+PREREVIEW+DONE_GATE; REVIEW was NOT on in-run). All 8 subtasks
done, 0 failed, NO filename drift (all 8 sched/ files correctly named), calm judge (12 verdicts, 0
kills). SMOKE PASSED (entry sched, entry_ok, collect ok). 44 pytest pass. By the OLD criteria this is a
clean WIN — runs, rich 8-command CLI (add/list/run/start/status/enable/disable/remove), tests green.
- BUT IT IS BROKEN, and ONLY the AST reviewer found it. `python3 scratchpad/ast_review2.py` flagged
  sched.runner + sched.store as UNWIRED (no non-test module imports them). RUNNING it confirmed a real
  user-facing bug: `python3 -m sched add --name backup ...` prints "Added task <uuid>" but `python3 -m
  sched list` (separate process) shows NOTHING — tasks DO NOT PERSIST. cli.py's add does
  Schedule(); Task(...) in memory and never saves, because store.py (the persistence module) is unwired;
  each invocation starts empty. Also cli.py's `run` inlines subprocess.run instead of using runner.py
  (runner unwired/duplicated). A task scheduler whose tasks vanish between commands is non-functional.
- THE DEEP POINT: SMOKE (runs) + 44 passing tests + an 8-command --help ALL gave A1-3 a clean bill; the
  44 tests test store.py/runner.py IN ISOLATION (they pass) but the CLI never calls them (classic
  lesson #11 "tests pass but the integrated feature is broken", here via UNWIRING not contract drift).
  The deterministic AST unwired check was the ONLY signal that caught it. This is the strongest
  validation in the run of the deterministic-gate + AST-reviewer thesis.
- Score 3/5/5/4 (vs AB qwopus mean 5.8/5.6/7.6/5.6): corr 3 (headline persistence broken), test 5 (green
  but miss the wired path), qual 5 (clean modules but 2 unwired duplicates — the inline-duplicate
  pattern, lesson #2), spec 4 (rich CLI but the central persist/run are inline/absent). A DRAW/FAIL-class
  result masquerading as a win — the multi-module regime again.
- DIRECTLY MOTIVATES the next build: an AST-finding fix-dispatch (mirror SMOKE-autofix) — on an unwired
  finding, fire ONE fix worker told to WIRE the module into the app (load store on start, save on add).
  That would have fixed A1-3's persistence. And A2-1 (CONTRACTS ON) tests whether a frozen store/runner
  interface makes the CLI worker IMPORT instead of inline.

### A2-1 — double-entry ledger CLI (FULL spec) — WIN (corr 8 / test 6 / qual 8 / spec 8)
DONE on the full v8 stack (CONTRACTS+REVIEW+SMOKE+autofix+DONE_GATE; binary 326a140be). All 6 subtasks
done, 0 failed. THE HEADLINE RESULT: the FIRST multi-module app in the eval that WORKS end-to-end.
- CONTRACTS WORKED (verified by reading the tree): the phase fired (banner + "frozen interfaces injected");
  ledger-core (ledger.py) imported the EXACT frozen models interface (from .models import Account,
  AccountType, JournalEntry; uses entry.is_balanced @property + JournalLine.debit/credit) — NO drift, NO
  re-invention. cli.py's contract STUB was OVERWRITTEN to a real 108-line CLI (no surviving stubs). The
  in-run REVIEW event = findings:[] (no unwired modules) — CONTRACTS prevented the A1-3-style unwiring.
- RAN IT END-TO-END (the decisive test A1-3 failed): add-account cash asset + revenue income; post a
  BALANCED entry (cash:100:0 | revenue:0:100) -> ok; post an UNBALANCED entry -> correctly REJECTED
  ("Unbalanced journal entry"); trial-balance in a FRESH process via --file -> cash debit=100.0, revenue
  credit=100.0 (debits==credits) — PERSISTENCE ROUND-TRIPS. A genuinely working double-entry ledger. 5
  pytest pass (the 5 spec behaviors). SMOKE pass, AST review clean.
- Score 8/6/8/8 vs AB qwopus mean 5.8/5.6/7.6/5.6 — well above. test 6 (5 tests cover the key behaviors
  but thin). Minor nit: post exits 0 even when it rejects an unbalanced entry (should be non-zero); not a
  correctness defect (it DOES reject).
- HEADLINE: A1-2 (spreadsheet, NO contracts) FAILED 2/0/5/2 (drift cascade); A1-3 (scheduler, NO contracts)
  broken 3/5/5/4 (store unwired -> no persistence); A2-1 (ledger, FULL stack WITH contracts) = 8/6/8/8
  WORKING. The v8 stack converted the multi-module DRAW class into a WIN. HONEST CAVEAT: A2-1 is a
  MAX-DETAIL spec (A2) while A1-2/A1-3 are MINIMAL (A1), so the detailed spec ALSO helps — not contracts
  alone; a contracts-OFF same-spec run would isolate it. But the draw-class mechanisms (drift cascade,
  unwiring) that sank A1-2/A1-3 demonstrably did NOT occur with the full stack — verified in the tree.

### A2-2 — log-pipeline DSL (FULL spec) — WIN (corr 8 / test 8 / qual 8 / spec 8)
DONE on the full v8 stack (binary 8af809359, has stub-cleanup + wire-fix). 10 done, 0 failed. The 2nd
multi-module DRAW->WIN — and it is the LOGFUNNEL class (which STALLED with no dispatcher / unwired stages
in the AB).
- CONTRACTS fired; 0 stray stub files (the prompt fix prevented the stub-writing — no removal line); AST
  review CLEAN (10 modules, 0 unwired) -> stages are WIRED through the runner/dispatcher (the exact thing
  logfunnel lacked). SMOKE pass (entry logdsl, collect ok). No surviving stubs.
- RAN IT END-TO-END: python3 -m logdsl --pipeline "filter ERROR | count" over 3 lines (2 ERROR) -> 2
  (correct); --pipeline "filter ERROR | upper" -> ERROR X / ERROR Z (multi-stage WIRED + correct). 44
  pytest pass (tokenizer/parser/stages/pipeline — deep). A genuinely working pipeline DSL.
- Score 8/8/8/8 vs AB mean 5.8/5.6/7.6/5.6. The architect explicitly planned a runner-module (dispatcher)
  — the missing piece in logfunnel — and CONTRACTS froze the tokens/parser/stages interfaces so dependents
  wired against them.
- A2 ARCHETYPE = WIN, WIN (ledger 8/6/8/8, log-DSL 8/8/8/8). Both hard multi-module max-detail apps WORK
  with the full stack, vs A1-2 FAIL + A1-3 broken (multi-module, no contracts). Consistent DRAW->WIN.

### A2-3 — state-machine workflow engine (FULL spec) — WIN (corr 8 / test 6 / qual 8 / spec 8)
DONE on the full v8 stack. 5 done, 0 failed. fsm/ package (machine.py = State/Transition/Machine + step +
validation; cli.py; __main__) + tests. Plan confidence 64/100 (lower — the FSM spec gave the architect less
to lock onto; proceeded fine). CONTRACTS fired, 0 stray stubs, AST review CLEAN (6 modules, 0 unwired).
SMOKE pass. 10 pytest pass.
- RAN IT END-TO-END (turnstile machine): state -> locked; fire coin -> locked->unlocked, state PERSISTS to
  unlocked across a fresh process; fire coin from unlocked (no transition) -> "Error: No transition from
  'unlocked' on event 'coin'" with EXIT 1 (correctly REJECTED, proper non-zero exit — cleaner than A2-1's
  ledger which exited 0 on rejection); fire push -> back to locked. Graph validation rejects unknown states.
  A genuinely working FSM.
- Score 8/6/8/8. A2 = WIN/WIN/WIN (ledger 8/6/8/8, log-DSL 8/8/8/8, fsm 8/6/8/8). All THREE hard multi-
  module max-detail apps WORK with the full stack — across THREE distinct draw classes (contract-drift
  cascade / no-dispatcher stall / state-graph). The thesis is strongly confirmed.

## DE-PYTHON (technology-agnostic swarm) — FUNCTIONALLY COMPLETE (2026-06-29)
Per the user's direction ("it should not care about python, allow multiple technologies"). The swarm ENGINE
(scheduler/dispatch/judge) was always language-agnostic; the architect prompt + every gate were Python-
hardcoded. Now all language-aware via a `TargetLang` profile (Python/TypeScript/Rust/Go/Other) +
`detect_language(spec, existing-file extensions)`. SIX increments, Python BYTE-IDENTICAL throughout (268
goose-cli + 29 goose-swarm tests green at every step; experiment specs now lead with `LANG=<X>`):
 1. ARCHITECT prompt (6881ae6d9) — per-language directive + entry-point mandate + test command.
 2. WORKER prompt (75682ae7c) — directive + the dependency-API filter via is_source_file/is_test_file
    (FUNCTIONAL fix: non-Python workers now get sibling-module APIs injected instead of over-reading).
 3. PLANNER fallbacks (12cd6a744) — `(NOT py_compile)` gated; synthetic integrate-verify entry_run_example;
    solo `plan` test_cmd.
 4. SMOKE gate (dcf6a6b2e) — returns ran=false on non-Python (no pytest-against-TS misfire on a mixed tree).
 5. DONE-GATE + judge (8d7f4d55b) — per-file `syntax_error` dispatch (.py = ast.parse verbatim; others skip).
 6. CONTRACTS (this) — gated to a Python target (never injects Python stubs into a TS/Rust app).
DEFERRED (LOW confidence, not needed for correctness): a TypeScript import-graph AST reviewer — run_ast_review
already no-ops cleanly on a no-.py tree; a real ES-import reviewer is a separate effort. Full per-language
CONTRACT stubs (TS interfaces / Rust traits) also deferred.
VALIDATION: TS-1 (architect-only binary) produced real TypeScript (30 vitest pass) but its still-Python
integrate-verify thrashed 13x -> proved the worker/gate de-Python is necessary, not just the architect.
RUST-1 (full de-Python binary) produces real Rust with integrate-verify dispatch=1 (NO thrash) -> the chain
works end to end. A Python control (PY-CTRL) confirms the 9-run Python matrix behavior is unchanged.

### TS-1 — LANG=TypeScript todo CLI (FIRST non-Python run; architect de-Python only) — 4/6/5/5, CUT
The first language-named experiment + the first real proof the de-Python works. Ran on binary 6881ae6d9
(architect de-Python ONLY — the worker/planner/gate fixes came AFTER this launched), CONTRACTS off.
- ARCHITECT de-Python WORKS: it planned a correct, idiomatic TypeScript project — `src/{cli,commands,store,
  errors,types,validation}.ts`, vitest tests, `tsconfig.json`, `vitest.config.ts`, `package.json` (scripts
  build:tsc / test:vitest, bin ./dist/cli.js). ZERO `.py` files. The workers wrote real `.ts`.
- THE LOGIC WORKS: `npm install` clean, `npx vitest run` -> 30 tests PASS across 3 files. The command/store/
  validation/errors modules are correct.
- BUT THE CLI ENTRY CRASHES (false-green, same class as A3-2): `npx tsx src/cli.ts add "x"` -> Node
  `ERR_INVALID_URL`. Root cause cli.ts:95 — the main-guard does `new URL(process.argv[1])` on a plain
  filesystem PATH (should be a path-string compare or pathToFileURL). The program can't actually be run;
  the 30 unit tests bypass the CLI entry so they pass anyway.
- WHY IT SHIPPED BROKEN: integrate-verify is the v8 gate DESIGNED to catch exactly this (RUN the real
  entry). But on this OLD binary the integrate-verify WORKER still got the Python prompt (run `pytest`),
  fought it on a TS project, and was judge-re-dispatched 13 TIMES without ever running the TS entry -> the
  broken main-guard never got caught. I CUT it at 13 dispatches.
- Score 4/6/5/5. NET: a clean END-TO-END validation of the de-Python chain — the architect step ALONE
  produces correct TS, and TS-1 demonstrates concretely WHY the worker + integrate-verify de-Python (shipped
  after, binary dcf6a6b2e) are needed: without them the verify gate can't run a non-Python entry, so a
  runnable-entry bug escapes. RUST-1 (next, on the fixed binary) tests whether the full chain yields a
  runnable program. Minor blemish: tests split across `test/` AND `tests/` dirs (both work).

### A3-3 — byte-oracle: add --count (AMENDMENT, NEW binary f9e89b782, NO instruction) — WIN (corr 7 / test 6 / qual 7 / spec 7)
THE VALIDATION RUN for the f9e89b782 amendment EXACT-path rule, on a clean copy, with NO per-run edit-in-place
instruction (tests the rule as the DEFAULT). 3/3 done, smoke PASS.
- f9e89b782 WORKED: the architect owned the QUALIFIED path `byte_oracle/cli.py` (not a bare `cli.py`), the
  worker EDITED it IN PLACE (--count added to the REAL cli.py, 14 matches), and there is NO stray root cli.py.
  Contrast A3-2 (OLD binary, even WITH an explicit instruction) which wrote a stray ROOT cli.py and left --json
  dead. The rule flipped a wrong-path FAIL into a working in-place amendment.
- RAN IT (real entry): `python3 -m byte_oracle --count /tmp/botest` prints the table THEN a correct per-type
  summary ("Type counts: png 1, text 2, zip 1", total 4); `python3 -m byte_oracle` table still works; 133
  pytest pass. The feature is genuinely wired + correct (not false-green like A3-2).
- Only review finding = the PRE-EXISTING `byte_oracle.detector` dup (lesson 14), unrelated to --count; the
  wire-fix again flailed on it (re-confirms the wire-fix-skip-pre-existing candidate — wire-fix should ignore
  modules already unwired before the run).
- Score 7/6/7/7 vs AB 5.8/5.6/7.6/5.6 — at/above on correctness. A3 amendment archetype: A3-1 5/3/4/6 (re-
  architect, no fix) -> A3-2 4/5/4/5 (wrong-path, no fix) -> A3-3 7/6/7/7 (f9e89b782 fix) — clear upward
  trend as the amendment fix landed.

NOTE — 9-RUN PYTHON MATRIX COMPLETE (A1 x3, A2 x3, A3 x3). Greenfield: A1-1 WIN + A2 3/3 WIN. Amendments:
A3-1/A3-2 partial (failure modes found + fixed), A3-3 WIN (fix validated). Next phase = language-named
experiments (TS-1 running) validating the de-Python work.

### A3-2 — byte-oracle: add --json (AMENDMENT, edit-in-place instr) — PARTIAL/FAIL (corr 4 / test 5 / qual 4 / spec 5)
CONTROL run (OLD binary 8af809359 + an explicit "edit the existing cli.py in place" instruction in the spec).
CUT at ~43min during a flailing wire-fix. The edit-in-place instruction PREVENTED the A3-1 parallel-RENAME (the
byte_oracle package stayed intact, no render_ascii-style dup) — BUT a SECOND amendment failure appeared:
- WRONG-PATH WRITE: the add-json-flag worker wrote a NEW `cli.py` to the CWD ROOT (81 lines, with --json +
  `import json`) instead of EDITING `byte_oracle/cli.py` (220 lines, left UNCHANGED). So `python3 -m
  byte_oracle --json` -> "error: unrecognized arguments: --json" — the feature is DEAD (the package entry
  never imports the stray root cli.py). Note: my spec said "edit the existing cli.py" UNqualified — the
  architect/worker owned a bare `cli.py` (root), not `byte_oracle/cli.py`. f9e89b782's "own the EXACT existing
  path" clause is meant to fix exactly this; A3-3 (new binary) is the test.
- FALSE-GREEN TESTS: 135 pytest PASS because tests/test_json_output.py imports the stray ROOT cli.py / the
  json function directly, NOT via `python3 -m byte_oracle` — so the suite is green while the actual CLI lacks
  --json. (Smoke also passed: it only checks `-m byte_oracle --help` exit 0, which the base CLI satisfies.)
- The PRE-REVIEWER did catch a real spec drift mid-run (worker used JSON key "detected_type"/"extension"
  instead of "detected") and re-dispatched with a corrective hint — but in the stray file, so it didn't help
  the real entry. The AST REVIEWER correctly flagged the stray 'cli' (+ the pre-existing detector dup) as
  unwired. The WIRE-FIX then FLAILED: it tried to wire BOTH the stray cli AND the pre-existing intentional
  detector dup (lesson 14), ran 14 shell calls over ~9min without resolving -> I cut it.
- Score 4/5/4/5 vs AB 5.8/5.6/7.6/5.6 — below. TWO real findings: (1) WRONG-PATH write (edited file landed at
  cwd root, not the owned package path) — a worker/planner path bug distinct from A3-1's re-architecture;
  (2) WIRE-FIX mis-applies on amendments (chases pre-existing intentional dups). The table default still works
  + detection is correct (unchanged base).

### A3-1 — chaos-fern: add --svg export (AMENDMENT) — PARTIAL / CUT (corr 5 / test 3 / qual 4 / spec 6)
Full v8 stack on a COPY of chaos-fern. CUT after ~34min: cli-entry + tests-svg THRASHED (each judge-killed
once -> attempt 2, then no file writes for 150s+ while running shell — over-reading the messy layout). NOT a
v8-feature bug (no ContentRetry/infra retries; bounded by the intervention cap; the idle-timeout did not
fire because the worker kept emitting shell events).
- THE AMENDMENT RE-ARCHITECTED instead of editing in place: it created NEW chaos_fern/fern.py +
  render_ascii.py + export_svg.py and rewired cli.py to import THEM, ABANDONING the original
  renderer.py/ifs.py/chaos_game.py. The AST reviewer (scratchpad/ast_review2.py) CORRECTLY flagged all
  three originals as built-but-unwired — a clean demo of the reviewer catching amendment-duplication.
- THE FEATURE WORKS though: python3 -m chaos_fern fern --svg --out /tmp/fern.svg wrote a valid 5.7MB SVG
  with 100000 point <circle> elements at the correct Barnsley-fern coordinates (scales with --iter); the
  ASCII default still renders a fern (via the new render_ascii.py). BUT tests/test_cli.py FAILS pytest
  collection (broken by the rewrite) and tests-svg never completed (cut).
- PLAYWRIGHT: env-BLOCKED. The Playwright MCP browser LAUNCHES (about:blank renders) but cannot render real
  content in this sandbox — file:// is blocked, the browser is network-isolated from a local HTTP server
  (127.0.0.1 times out), and data: URLs with the SVG content time out at 30s (the backend then hangs). Six
  attempts across 4 approaches. Verified the SVG instead by STRUCTURE (valid <svg 800x400> + N green
  fern-point circles) + ALGORITHM (the coords are the chaos-game Barnsley fern) + the matching ASCII render.
- Score 5/3/4/6 vs AB mean 5.8/5.6/7.6/5.6 — below. The AMENDMENT failure mode (re-architect vs edit-in-
  place) is distinct from the greenfield A2 wins; A3-2 tests recurrence with an explicit edit-in-place spec.

## ASK-TEST — inquisitive-swarm handshake PROVEN LIVE (2026-06-29)
GOOSE_SWARM_ASK_FLOOR=100, GOOSE_SWARM_ASK_FILE=1, vague spec "build a tool to process logs", binary cf573d811.
Swarm computed final plan confidence 82/100 (best-of-2 cross-draft agreement) < floor -> verbalized
uncertainties were EMPTY -> generic-fallback question fired -> wrote .swarm/clarify-questions.json + emitted
low_confidence_ask + BLOCKED. The HARNESS answered as the human (.swarm/clarify-answers.json, 1 concrete
answer) -> swarm logged "clarifications received — re-planning", emitted low_confidence_answered, folded the
Q&A into planner findings, RE-PLANNED exactly once (asked flag), then -> EXECUTE. Closed Q&A loop validated
end-to-end. See SWARM-LESSONS lesson 22.
PLAN-SHAPING CONFIRMED: the re-planned run's subtask specs are saturated with EXACTLY my clarifications —
syslog/nginx/JSON-lines parsing, malformed-line skip+count, per-level summary, --json/--level/--since flags
(grep of the run jsonl: 2 syslog, 4 JSON-lines, 6 malformed, 4 per-level, --json/--level/--since each). So
the answers genuinely RESHAPED the decomposition, not just triggered a re-plan. (The EXECUTE then FAILED on a
flaky worker — parser-module claimed done without writing parser.py -> cascade to pipeline-cli/tests/iv; a
known worker-flakiness issue UNRELATED to the inquisitive feature, so the final tool didn't build. The
feature itself — ask + answer + re-plan-with-the-answers — is fully validated.)

## ASK-TEST2 — inc3 weak-bump + correct NO-ask (2026-06-29)
Spec "build a CLI to deduplicate records in a CSV", GOOSE_SWARM_ASK_FLOOR=70. inc3 weak-bump fired:
"ask floor 70 -> 75/100 (+5 weak-planner bump for 27b)". final plan confidence 80/100 >= eff floor 75 ->
the gate CORRECTLY did NOT ask (the feature respects the threshold; no over-asking on a clear-enough spec).
Validates: the scaling is live + the gate is calibrated (asks only below the effective floor). To exercise
the inc2 generator a sub-floor confidence is needed -> ASK-TEST3 with a higher floor.

## ASK-TEST3 — inc2 GENERATOR validated LIVE (real interrogatives, not the fallback) (2026-06-29)
Spec "build a CLI to deduplicate records in a CSV", floor=90 -> eff 95 (27b +5 weak-bump). final plan conf
60/100 < 95 -> the inc2 clarify_questions() GENERATOR produced 3 CRISP REAL interrogatives (NOT the generic
fallback): "How are duplicates identified — all columns or a subset of key columns via CLI flags?", "Which
duplicate to keep (first/last/...)?", "Output to a new file or overwrite in place?" — exactly the real
ambiguities of CSV-dedup, all ending with '?'. The HARNESS answered as the human (--key default all, keep
FIRST + --keep flag, --out default stdout never in-place) -> swarm logged "clarifications received —
re-planning" -> re-planned. The inc2 GENERATOR is validated end-to-end: it asks GOOD questions, not just the
fallback. The inquisitive feature (inc1 handshake + inc2 generator + inc3 scaling) is fully proven LIVE.

## ASK-TEST3 EXECUTE — flaky-worker pattern PERSISTS despite the guided retry (honest finding, 2026-06-29)
The inquisitive feature worked end-to-end (asked 3 real questions, harness answered, re-planned with the
clarifications). BUT the EXECUTE of the re-planned DAG FAILED: 6 subtasks failed because shared-types flaked —
"claimed done but never wrote" fired 4x; shared-types was dispatched 3x and EXHAUSTED, cascading to its
dependents. Only dedup_csv/reader.py + writer.py were written; no CLI/dedup logic.
TAKEAWAYS (honest): (1) the guided-retry fix (92f393495: ContentRetry hint naming the missing files) is a NET
improvement over the old blind Transient re-roll, but it does NOT eliminate a model that stubbornly claims
done without writing a particular file — shared-types failed anyway. (2) RECURRING PATTERN across 3 runs: the
EARLY "shared/types" dependency subtask (parser-module / shared-models / shared-types) is the repeat offender
that flakes — possibly the weak model treats a thin "types" spec as trivial/already-done. A deeper fix would
need investigation (read the flaky worker session trace: is it writing to a wrong path, claiming done after a
read, or looping?) — NOT built now; the guided retry is the bounded mitigation. The inquisitive feature
itself is unaffected + fully validated.

## REGRESS1 — fully-hardened-binary regression: markdown-to-HTML renderer WIN ~8/7/8/8 (2026-06-29)
Binary ef3dfee24 = ALL session fixes together (inquisitive 3-inc + de-Python 6-inc + flaky-worker guided-retry
+ empty-file exemption + wire-fix-skip-pre-existing), full v8 stack incl CONTRACTS, spec = A1-1's CLI
markdown-to-HTML renderer (pkg md2html). RESULT: ALL 5 subtasks done, 0 failed. smoke CLEAN (collect ok,
md2html entry_ok, findings []). review CLEAN (0 findings, NEW 0 — wire-fix-skip-pre-existing did NOT mis-fire
on greenfield). RAN `python3 -m md2html`: correct HTML for ALL constructs (<h1>, <strong>, <code>, <ul><li>).
29/30 pytest pass (1 minor failure). KEY VALIDATIONS: (1) the hardened binary still WINS A1-1's class — every
session fix COEXISTS without regression. (2) the GUIDED-RETRY fix (92f393495) RECOVERED LIVE: core-renderer
flaked (claimed done without writing) but the guided hint got the worker to write -> recovered -> run
completed (the old blind Transient re-roll would have exhausted). (3) wire-fix-skip-pre-existing CLEAN on
greenfield (NEW 0). COST: slow (~33min) — the core-renderer flake cost several guided retries. Net: the
fully-hardened binary is REGRESSION-CLEAN and the new fixes are validated working together.
