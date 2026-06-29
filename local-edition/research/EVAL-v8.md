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
| A1-3 | A1 minimal | a CLI task scheduler | SMOKE+autofix,SPLIT,PREREVIEW,DONE_GATE,JUDGE,research | A1-3-sched | RUNNING | — | — | — |
| A1-3 | A1 minimal | a CLI task scheduler | (tbd) | — | pending | — | — | — |
| A2-1 | A2 max-detail | double-entry ledger CLI (full spec) | (tbd) | — | pending | — | — | — |
| A2-2 | A2 max-detail | log-pipeline DSL (full spec) | (tbd) | — | pending | — | — | — |
| A2-3 | A2 max-detail | state-machine workflow engine (full spec) | (tbd) | — | pending | — | — | — |
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
- ROOT CAUSE: formula-parser is the hard lynchpin everything depends on. Attempt 0 (gabee) was
  judge-killed (over_reading — see the watch observation); attempts 1-2 PRODUCED formula_parser.py
  (385 valid lines, parses — DONE_GATE correctly silent) but the TASK still failed (the worker wrote
  the file yet did not reach a clean final_output — likely max-turns / test-thrash on a hard module).
  3 real failed attempts -> exhausted -> fail_descendants tanked evaluator/cli/tests/integrate-verify.
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

### A1-3 — a CLI task scheduler  (RUNNING)
On the NEW 31f671a18 binary (SMOKE+autofix + SPLIT+PREREVIEW+DONE_GATE). Watching: does it stay
cohesive (A1-3 is less lynchpin-heavy than the spreadsheet), does SMOKE pass, and if it smoke-FAILS
does the corrective AUTOFIX fire (smoke_after_fix event) + resolve it — the first chance to validate
the auto-fix end-to-end.
