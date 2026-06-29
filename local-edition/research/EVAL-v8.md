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
| A1-2 | A1 minimal | a CLI spreadsheet with formulas | SMOKE,SPLIT,PREREVIEW,DONE_GATE,JUDGE,research | A1-2-sheet | RUNNING | — | — | — |
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

### A1-2 — a CLI spreadsheet with formulas  (RUNNING)
First run with GOOSE_SWARM_DONE_GATE=1 (on the 8f4fb225a binary) + SMOKE+SPLIT+PREREVIEW.
Spreadsheet-with-formulas is harder than A1-1 (a formula parser/evaluator + cell refs +
recalc) so it leans toward the multi-module regime — a better test of the v8 gates than
A1-1. Watching: does DONE_GATE fire a ContentRetry on any syntax-broken file, does SMOKE
catch a cross-module import error, does SPLIT trigger on the formula-engine task.
