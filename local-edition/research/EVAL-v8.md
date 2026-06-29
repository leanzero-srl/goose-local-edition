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
| A2-3 | A2 max-detail | state-machine workflow engine (full spec) | full v8 stack | A2-3-fsm | RUNNING | — | — | — |
| A3-1 | A3 feature | chaos-fern: add SVG/HTML export (Playwright) | (tbd) | — | pending | — | — | — |
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

### A2-3 — state-machine workflow engine (FULL spec)  (RUNNING)
Full v8 stack. The AB state-machine class. Watch: contracts decoupling of states/transitions/machine, AST
review (machine wired into cli), RUN it (load a spec, fire a valid event -> transitions, fire an invalid
event -> rejected/InvalidTransition, guard blocks), smoke. Does A2 go 3-for-3?
