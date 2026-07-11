# Overnight UI-edition build pass — results

Complex apps built by the local qwopus fleet through `goose swarm run` (the same engine the desktop
swarm provider drives), using the fixed binary that captures real tool-call output + reasoning into the
run panel. Each app is verified by actually running it. CDP was environmentally dead this session, so the
builds were started from the CLI rather than clicked in the UI — but they create the identical sessions +
turn-loops the app shows, and result output is captured (no more "dummy" dots).

## 1. tracker — Python/SQLite issue tracker — STRONG PASS (CLI-naming drift)
- Built: 02:48, exit 0, **1171 LOC**, 21 files, 8 modules (db, cli, commands_project/issue/report, __main__, tests).
- Own tests: **23/23 pass** — and they cover the hard requirements:
  - `test_forward_transition_blocked_by_open_blocker` (done/close rejected while a blocker is open)
  - `test_ready_issues`, `test_blocked_becomes_ready_when_blocker_done` (dependency logic)
  - `test_issue_block_and_unblock`, `test_issue_block_self_raises`
  - `test_report_status/assignee/priority`, `test_export_import_roundtrip`, validation tests
- Real code: `_check_blockers` / `has_open_blockers` implement the rejection rule; multi-command argparse CLI.
- DEVIATION from spec: command/flag naming drifted — `issue create` (spec: `add`), `--project-id` (spec:
  `--project`), `ready`/`blocked` are `issue` subcommands (spec: top-level), no global `--db`. Functionality
  is all present and tested, but the exact CLI surface differs. This is the fidelity gap at this complexity:
  structure + logic + tests land; exact interface naming drifts.
- Run-panel observation (captured, real): db.py written (258 lines) -> "ALL SMOKE TESTS PASSED"; genuine
  errors surfaced ("cat: test file No such file", "missing field path" from a malformed model tool-call).

## 2. sheet — TypeScript spreadsheet engine — FAIL (core computation broken)
- Built: 04:49, swarm exit **1** ("1 core subtask failed"; judge flagged spec_drift/looping/over_reading on
  a subtask), **993 LOC**, 18 files. Architecture is exactly right: tokenizer/parser/evaluator/graph/types/cli.
- Compiles clean with `tsc` (dist/ produced). CLI surface matches the spec: eval/get/deps/set.
- BUT the core doesn't work: `get grid.json B1` (with B1==A1+A2, A1=5, A2=10) returns the RAW formula
  `=A1+A2` instead of `15`; `eval` prints every cell's formula uncomputed. The CLI IS wired to evaluate()
  (cli.ts:39) so the bug is inside the tokenize→parse→evaluate pipeline — it returns raw formula text
  rather than a computed value. No runnable test script (`npm test` missing) — the tests subtask is the one
  that failed.
- VERDICT: FAIL. This is the DEEP algorithmic archetype (recursive-descent parser + dependency graph +
  cycle detection) — exactly where COMPLEX-ARCHETYPES.md predicted the fleet would hit its ceiling, and it
  did: right structure + interface, broken evaluation. This is the sharpest signal of the night on the
  wall the local fleet still hits at real complexity.
## 3. vcs — Rust content-addressable version store — STRONG PASS (works end-to-end)
- Built: 06:26, exit 0, **809 LOC**, 8 files, clean modules: objects/store/commands/cli/main.
- cargo build clean; integration test (`end_to_end`) passes.
- Golden sequence ALL correct (ran the real binary):
  - init; add a.txt+b.txt; commit -> SHA-256 hash 914ff7e…; modify a.txt; commit -> f0cd0a5…
  - `log` lists BOTH commits with hash + message + timestamp (commit DAG walk works)
  - `checkout <first-hash>` RESTORES a.txt to "hello v1" (real content restoration from the tree)
  - `diff <c1> <c2>` correctly reports "a.txt modified"
- VERDICT: STRONG PASS. A genuinely working content-addressable store — SHA-256 objects, commit DAG,
  checkout, diff — the different-paradigm Rust archetype landed fully at real scale.
## 4. ledger — Python double-entry accounting — (pending)
