# Exploration Cycle 1 — 5 apps per archetype (15 total), UI-dispatched, isolated --dir builds

Goal: build a BACKLOG of concrete issues across the fleet's archetypes, then fix everything at the end.
Each app is verified by RUNNING it + its tests + golden spec-contract checks. Findings accrue here.

## Apps
DATA (Python/SQLite): inventory ✓(prev STRONG PASS), bookclub, expense, crm, timesheet
ALGORITHMIC (Python): csvql ✗(prev FAIL), calc, jsonq, tmpl, glob
SYSTEMS (Rust): kvstore ✗(prev FAIL), taskq, blobs, wal, trie

## Results
| app | archetype | LOC | tests | contract | verdict | key finding |
|-----|-----------|-----|-------|----------|---------|-------------|
| inventory | DATA | 808 | 22/22* | exact | STRONG PASS | correct accounting 699.30 |
| bookclub | DATA | 724 | 17/37 | drift | FAIL | ctx.obj None → every subcmd crashes |
| expense | DATA | 1422 | 65/65 | n/a | UNRUNNABLE | no CLI entry (scheduler #7) |
| crm | DATA | 976 | 31/32 | ok | GOOD | export/import no round-trip; dict-repr list |
| timesheet | DATA | 1400 | 36/36 | false-green | PARTIAL | tests bypass entry; --db broken; smoke self-healed |
| calc | ALGO | 716 | 47/47 | exact | STRONG PASS | right-assoc 512, vars, all errors exit≠0 |
| jsonq | ALGO | 843 | 10/10 | mostly | GOOD | slice+chain returns empty; test misses it |
| tmpl | ALGO | 1045 | 24/24 | drift | FAIL | render empty via real entry (parser/renderer shape drift); scheduler #7 orphaned verify |
| glob | ALGO | 472 | 0 (task died) | drift | FAIL | glob/re filter broken (match-all/none); test path works |
| csvql | ALGO | 936 | 3/14 | drift | FAIL | rows list vs cli row.values() dict |
| kvstore | SYSTEMS | 253 | 0 | n/a | FAIL | empty fn main(){}, shipped (gate #8) |
| taskq | SYSTEMS | 995 | won't compile | n/a | FAIL | 5 syntax errors in log.rs (bad escapes); judge said ok, no Rust gate |
| blobs | SYSTEMS | 1057 | 13/20 | test-wrong | GOOD | CLI works spec-correct; 7 tests assert wrong .blobs/ layout |
| wal | SYSTEMS | 997 | 2/8 | n/a | FAIL | append LSN stuck at 1; read/verify see 0 records (no persistence) |
| trie | SYSTEMS | 838 | 20/21 | spec-shape | GOOD | logic sound; insert/remove≠spec set/del, no --dir; tests match impl |

## Backlog (issues → fix at end of cycle)
(accrues as builds complete)

## BACKLOG ITEM #1 (swarm, found on bookclub) — pool includes devices whose model isn't loaded
The run pool is built from config.devices WITHOUT validating against the LIVE loaded models. bookclub's pool
was [gabee-...-mlx, mihai-...-mlx, qwopus3.6-27b-coder:2] but LM Studio only had mihai-...-mlx + qwopus:2
loaded (gabee's machine/model was down). Workers routed to gabee got "Bad request (400): Invalid model
identifier 'gabee-qwopus3.6-27b-coder-mlx'" and retried. The swarm COPES (reroutes + completes) but burns
retries + adds 400 noise on every build while a node is down. Also the config model_ids drift from live
(config 'workhorse-...-mlx' vs live 'qwopus3.6-27b-coder:2').
FIX (at cycle end): at run start, probe the live /v1/models (or /api/v0/models) and DROP any config device
whose model_id is not currently loaded (respect allow_model_load=false — never cold-load). The pool should
be the intersection of configured+enabled devices and actually-loaded models. This makes builds robust to a
node going offline mid-session instead of 400-looping on it.
Verified separately: the new writer's full_reasoning capture WORKS — it surfaced this exact 400 error in the
worker's reasoning (the detail the run panel now shows).

## BACKLOG ITEM #2 (swarm fleet detection) — transient JIT instances + prefix-derived node identity
reconcile_pool_with_fleet() -> probe_lms_http() reads /api/v0/models and turns EVERY distinct loaded
identifier into a pool node, deriving the node name from the model-id prefix (device_from_lms_id = text
before the first '-'). Two weaknesses, seen live:
1. LM Studio JIT auto-instances ("qwopus3.6-27b-coder:2", ":3", ...) are counted as separate nodes. They are
   duplicate instances of an aliased model, not distinct machines — so a box with both its alias and a JIT
   instance loaded yields 2 pool entries (oversubscription) + a phantom node named from the JIT prefix
   ("qwopus3.6"). Observed: workhorse briefly loaded as "qwopus3.6-27b-coder:2" during a reload and the pool
   used that instead of "workhorse-qwopus3.6-27b-coder-mlx".
2. Node identity from the raw id prefix is fragile (works for gabee-/mihai-/workhorse- aliases, breaks for
   JIT names). 
FIX (cycle end): filter loaded ids whose suffix is a bare ":N" JIT duplicate (keep the aliased sibling if
present); and map live model_ids back to a configured device by model_id/host rather than deriving the node
from the prefix. Detection is otherwise ACCURATE to live state — this is robustness to transient LM Studio
reload windows, not a correctness bug in the happy path.

## Results log
- inventory (data): STRONG PASS — 808 LOC, modular (inv/commands/{io,movement,product,supplier,reporting}).
  RE-VERIFIED via real round-trip: init→product add→receive 100→ship 30→report value = "Total: 699.30"
  (70 units × 9.99, correct accounting), all exit 0. Cosmetic nit only: `movements` ledger prints ship as
  "+30" not "-30" (display sign; underlying stock math is correct). This is the archetype's gold standard.
- bookclub (data): **FAIL** — 724 LOC, 20/37 tests fail. RE-VERIFIED end-to-end (my earlier "ImportError/no
  __main__" verdict was STALE — the build actually finished more): imports OK, __main__.py present, `--help`
  works. But EVERY real subcommand crashes: shelf/cli.py:18 `db_path = ctx.obj.get("db_path","shelf.db")` →
  `AttributeError: 'NoneType' object has no attribute 'get'` because the group callback never initializes
  ctx.obj (missing `ctx.ensure_object(dict)` / `ctx.obj = {...}`). 17 passing tests are pure model/db units;
  the 20 failing are the CLI integration tests. Classic cross-module CONTRACT DRIFT: the group in commands.py
  and the custom Group.invoke in cli.py disagree on who populates ctx.obj.
  → CRITICAL SMOKE-GATE INSIGHT: `python3 -m shelf --help` returns exit 0 (Click short-circuits --help BEFORE
    invoke), so a --help-only smoke probe passes on a fully-broken CLI. The smoke/complete gate MUST exercise
    a REAL subcommand round-trip (e.g. add then list) against a temp db, not just --help. Folding into #3.
  → BACKLOG #3: UI builds should use the STRONGER gate (GOOSE_SWARM_COMPLETE: verify-by-running a real command
    + iterate to green), and the smoke probe must run a real subcommand, not --help. Advisory smoke detects but
    doesn't prevent, and --help is too weak a probe.
  → BACKLOG #4 (recurring): cross-module contract drift is the dominant failure (bookclub ctx.obj, csvql
    row-type). Candidate: a planner "shared-types/interface" contract the workers must import + a done-gate that
    runs a real command.

## BACKLOG ITEM #5 (goose extensions / Playwright) — stale /usr/local/bin/node 19.8.1 breaks npx extensions
ROOT CAUSE (found): the Playwright MCP extension runs `npx -y @playwright/mcp@latest`, but the machine has a
STALE root-owned /usr/local/bin/node = v19.8.1 (from 2023) that shadows the newer nodes (/opt/homebrew node
26.3.1, nvm 22.22.0). When goose spawns npx, it resolves to /usr/local/bin/node 19.8.1; Playwright requires
>=20, so it quits with "You are running Node.js 19.8.1. Playwright requires Node.js 20 or higher." (the
"1 extension failed" seen in every screenshot).
FIRST ATTEMPT (WRONG — corrected): pointed cmd at /opt/homebrew/bin/npx. This DOES NOT WORK: that npx is a
`#!/usr/bin/env node` script, so it re-resolves `node` via PATH; and even `cmd: node <npx-cli.js>` fails
because npx spawns the @playwright/mcp bin as a CHILD which is ALSO `#!/usr/bin/env node` → re-resolves node
via PATH → stale 19.8.1 again. Proven empirically: under `PATH=/usr/local/bin:/usr/bin:/bin` (the app's Finder
PATH order), `env node` = v19.8.1 and npm/playwright reject it. Also goose FORBIDS setting PATH via extension
`envs` (extension.rs:81 DISALLOWED_KEYS includes PATH — anti-hijacking), so envs:{PATH} can't fix it either.
WORKING FIX (applied + verified end-to-end): make cmd a shell wrapper that fixes PATH BEFORE exec:
  cmd: /bin/sh
  args: ["-c", "export PATH=/opt/homebrew/bin:$PATH; exec npx -y '@playwright/mcp@latest' --browser=chrome --user-data-dir=/tmp/cw-chrome"]
Setting PATH inside the wrapper means npx AND the child playwright bin both find node 26. Verified: under the
stale `PATH=/usr/local/bin:...`, the exact wrapper command starts @playwright/mcp v0.0.78 clean, node=v26.3.1,
NO 19.8.1. config.yaml.bak2 saved. Takes effect next app launch; app-level chat-session check still PENDING.
SYSTEM-HYGIENE (user, needs sudo — the only fix that helps ALL tools, not just goose): remove/update the stale
node — `sudo rm /usr/local/bin/node /usr/local/bin/npx /usr/local/bin/npm` (or `brew link --overwrite node`).
DURABLE GOOSE FIX (backlog): when goose spawns a stdio extension, PREPEND a known-good node dir to the CHILD's
PATH in the env it builds (main.ts extension env / the Rust extension spawn) — goose sets the spawn PATH itself
so it is NOT subject to the user-facing DISALLOWED_KEYS ban. This makes every npx/node extension immune to a
stale system node without a per-extension wrapper. This is the real durable fix.

## BACKLOG ITEM #6 (swarm weights) — pool DISPATCH weight ignores speed_weights (all nodes = weight 1)
OBSERVED: run_started pool for the expense build shows every node at weight:1, e.g.
  mac-gabee-...(model gabee-qwopus3.6-27b-coder-mlx) w1 | local-mihai-... w1 | worksmacstudio-workhorse-... w1
even though config has speed_weights {local:2, gabee:2, worksmacstudio:3}.
ROOT CAUSE (read swarm.rs:1167-1183 reconcile_pool_with_fleet): the pool's dispatch `weight` is derived ONLY
from a matching cfg.devices[].weight (by model_id) OR LM Studio PARALLEL OR 1. The `speed_weights` map is
consulted ONLY in planner_rank (swarm.rs:1202) to pick the PLANNER model — it never feeds the pool dispatch
weight. So the user's "slower machine does less work" request has no effect on how tasks are load-balanced.
Also key-space is inconsistent: model NAMES are gabee/mihai/workhorse but lms-ps DEVICE (host) is
mac/local/worksmacstudio; speed_weights mixes both (gabee is a model-name, local/worksmacstudio are hosts).
FIX (task #58): in reconcile_pool_with_fleet, when no explicit cfg.devices[].weight override exists, fall
back to the speed_weight matched by pattern against (host + identifier) — same haystack planner_rank uses —
before defaulting to 1. Keep the explicit-override-wins precedence. Then worksmacstudio gets w3, gabee/local
w2, and the scheduler's weighted round-robin actually sends the M3-Ultra more tasks. Add a unit test:
speed_weights pattern → pool weight, explicit device weight still wins. This is the standing weights ask
(memory: swarm-model-weights-request) — deliver it here.

## BACKLOG ITEM #7 (goose-swarm SCHEDULER — HIGHEST VALUE) — salvaged task never relaxes its dependents
SEVERITY: high, confidence: HIGH (read the two code paths side by side; root cause is unambiguous).
OBSERVED on expense: 65/65 tests pass, 6 clean modules — but NO entry point (spend/__main__.py, spend/cli.py
both absent) so it is UNRUNNABLE, and the spec explicitly requires `python -m spend`. Run ended
`scheduler_stuck {remaining: 2}`. The 2 stuck tasks were `cli-entry` (deps: the 4 module tasks) and
`integrate-verify` (deps: cli-entry). Three module workers (transactions, import-export, balance-reports)
went `looping` then `salvaged_spin`.
ROOT CAUSE (crates/goose-swarm/src/scheduler.rs): the SUCCESS completion path (lines 579-590) decrements
each dependent's `indegree_remaining` and promotes it to Ready when it hits 0. The SALVAGE path
(complete(), ~lines 1117-1162) sets `self.dag.tasks.get_mut(tid).unwrap().state = TaskState::Done` (line
1159) for `salvaged_spin` but DOES NOT run the dependent-relaxing loop. So a salvaged task is marked Done yet
its dependents' indegree is never decremented → they stay Pending forever → no ready work but tasks remain →
`scheduler_stuck`. The salvage was ADDED precisely so a spun-but-written task wouldn't fail its dependents
(comment at scheduler.rs:30-34, "UNIQ9: entry spun on its final fix -> integrate-verify blocked"), but it
only fixed the state, not the indegree relaxation — so dependents are still orphaned, just Pending instead of
Failed. This is the dominant "BUILT-BUT-UNWIRED entry" failure class in disguise.
FIX (task #58): factor the dependent-relaxing loop (579-590) into a helper `fn relax_dependents(&mut self,
tid: &str)` and call it from BOTH the success path AND the salvage branch (after line 1159). Add a
scheduler_mock test: a task with a dependent, salvaged via Looping, must leave the dependent Ready (indegree
0), and the scheduler must then dispatch it — NOT emit scheduler_stuck. HIGH confidence this converts
expense-class runs from unrunnable-library to runnable-app.
NOTE: deliberately NOT fixed mid-cycle — keeping cycle-1 on one unchanged binary for a clean cycle-2 A/B and
to measure how many of the 15 apps this bug bites.

## Results log (cont.)
- expense (data): **PARTIAL/FAIL (unrunnable)** — 1422 LOC, 6 modules + 6 test files, 65/65 pytest GREEN,
  imports clean, correct module design. BUT no CLI entry point (no `python -m spend`), which the spec requires
  → cannot be run as an app. Caused by BACKLOG #7 (scheduler salvage orphans dependents → cli-entry +
  integrate-verify never dispatched → scheduler_stuck remaining:2). Also `shared-types` had spec_drift (the
  conftest `sample_data` fixture does db.commit() with no yield of inserted IDs). The library quality is
  actually STRONG; the harness bug (#7) is what makes it fail. Best evidence yet that #7 is the priority fix.
- csvql (algorithmic): **FAIL** — 936 LOC, has entry point (python -m csvql, query/columns subcommands),
  imports OK, `columns` subcommand WORKS (prints name/age). 11/14 tests fail. RE-VERIFIED — TWO independent
  contract-drift bugs:
  (1) ENGINE drift (dominant): csvql/cli.py:50 `writer.writerow(row.values())` assumes each result row is a
      DICT, but evaluator.py yields rows as LISTS → `AttributeError: 'list' object has no attribute 'values'`
      on EVERY query (even the spec-correct `csvql query FILE "SELECT * FROM data"` crashes). The cli worker
      and evaluator worker disagreed on the row data type. Same class as bookclub's ctx.obj.
  (2) TEST-HARNESS drift: the spec mandates `csvql query FILE "QUERY"` (FILE + QUERY separate); the `columns`
      test correctly uses the subcommand, but the `query` tests invoke bare `csvql "SELECT * FROM \"<file>\""`
      (file embedded in the SQL, no `query` subcommand) → argparse "invalid choice" exit 2. The test worker
      used two different invocation styles and contradicted the spec for queries.
  → REINFORCES BACKLOG #4: cross-module data-type/interface drift is THE dominant failure across archetypes
    (bookclub ctx.obj, csvql row dict-vs-list + CLI-shape). The planner-emitted shared-types/interface
    contract (GOOSE_SWARM_CONTRACTS) that every worker MUST import, plus a done-gate that runs one real
    spec-exact command, would catch both. Note: the CLI here was actually spec-correct on shape; the ENGINE
    row-type is the killer — a contract stub for "evaluator.run() -> list[dict] | list[list]" resolves it.

## BACKLOG ITEM #8 (goose-swarm/cli GATE — HIGH VALUE) — smoke/complete gate is PYTHON-ONLY; Rust gets a free pass
SEVERITY: high, confidence: HIGH (the smoke event payload proves it).
OBSERVED on kvstore (systems/Rust): src/main.rs is literally `fn main() {}` (empty), src/index.rs (95 LOC)
and src/log.rs (157 LOC) are real but ORPHANED (main.rs has no `mod`/CLI wiring), src/store.rs and
src/commands.rs never created, 0 tests. Binary runs but does NOTHING (--help empty). Run report:
done=[index-module, shared-types-and-log], FAILED=[cli-and-commands, integrate-verify, integration-tests,
store-module] — 4 of 6 tasks failed INCLUDING integrate-verify, yet the app shipped.
ROOT CAUSE: the smoke gate emitted `{ran:true, py_files:0, collect:null, tests:{kind:pass}, entry_package:null,
entry_ok:true, findings:[]}`. It only knows Python (pytest --collect-only + `python -m <pkg>`); with py_files=0
it reports entry_ok:true + tests pass + NO findings. So EVERY systems-archetype (Rust) build escapes the gate,
and a completely empty `fn main(){}` is graded "entry ok". store-module terminal-failed via `over_reading` ×3
(not salvaged — only Looping is salvaged) → fail_descendants killed cli-and-commands → main.rs stayed the empty
scaffold stub.
FIX (task #58): make the gate language-aware. If Cargo.toml exists: `cargo build` (compile), `cargo test`
(assert not 0-tests when a tests/ target was planned), run the built binary `--help` AND one real spec
subcommand asserting exit 0 AND non-empty output; treat an empty `fn main(){}` / no-op binary as FAIL. Also:
a run whose integrate-verify (or the CLI/entry task) FAILED must NOT be reported shippable — hard-block, not
advisory. This is the systems-archetype counterpart to bookclub's "--help too weak" (BACKLOG #3): the gate
must run a REAL command in the REAL language.
→ REINFORCES #3 (gate must verify-by-running a real command, per language) and the "unwired entry" class.

- kvstore (systems/Rust): **FAIL (unrunnable, empty main)** — 253 LOC across index.rs/log.rs (real) but
  main.rs=`fn main(){}`, orphaned modules, 0 tests. store-module over_read ×3 → terminal fail → fail_descendants
  killed cli-and-commands + integration-tests + integrate-verify. Shipped anyway because the Python-only smoke
  gate gave Rust a free pass (BACKLOG #8). Compiles clean (cargo build ok) which is why "it built" but is a no-op.
- crm (data): **GOOD (near-pass)** — 976 LOC, runnable (python -m deals), 31/32 tests pass. Best DATA result
  after inventory. Real round-trip works: init→contact add→list→opp add all exit 0. Two issues:
  (1) export/import does NOT round-trip (the 1 failing test): export writes a sectioned CSV
      (`SECTION,contacts\n<rows>\nSECTION,opportunities\n<rows>`) but `import` silently no-ops on that exact
      format (exit 0, but `contact list` on the fresh db is empty). Self-contained contract drift inside
      export_import.py — the writer and reader disagree. Family #4, milder (one module, not cross-worker).
  (2) QUALITY nit: `contact list` prints the raw Python dict repr `{'id':1,'email':...}` instead of a
      formatted table/line. Functional but ugly. Weak-model output-formatting shortcut.
  No scheduler_stuck; entry built fine. Shows the fleet CAN produce a mostly-working modular app.
- timesheet (data): **PARTIAL (false-green tests)** — 1400 LOC, 9 modules, 36/36 pytest PASS, but the real
  entry is spec-broken and the tests don't catch it. TWO findings:
  (1) SMOKE GATE WIN (my GOOSE_SWARM_SMOKE fix, first observed working): a task produced broken_code
      (IndentationError commands_io.py:72) → `smoke` reported collect:errors → a corrective fix fired →
      `smoke_after_fix` collect:ok tests:pass. The gate self-healed a broken build. This is the intended
      behavior and it worked.
  (2) FALSE-GREEN / DUAL-FRAMEWORK drift: the entry (clock/cli.py) is ARGPARSE, but the tests use Click's
      CliRunner driving the command GROUPS in-process with `obj={"db": path}` — a different framework AND a
      different code path. Consequence: the real `python -m clock` entry has NO working `--db` (spec requires
      "global --db"): `clock --db X init` → "invalid choice X"; `clock init --db X` → "unrecognized arguments".
      You cannot point the CLI at a db on the command line at all. 36/36 tests pass anyway because they never
      invoke the real entry. STRONGEST evidence for BACKLOG #3: the done-gate MUST run the REAL entry with a
      REAL spec command incl. --db; unit tests can bypass the entry entirely and give false confidence.
  → REFINES #8/#3: my smoke `entry_ok` check is too shallow (reported entry_ok:true here). It must run a real
    spec subcommand with --db against a temp db, not just import/basic-entry.
- calc (algorithmic): **STRONG PASS** — 716 LOC, clean modules (tokenizer/parser/ast/evaluator/cli), 47/47
  tests. RE-VERIFIED spec-exact via real entry `calc eval "EXPR"` / `calc run FILE`: precedence 2+3*4=14,
  parens 20, right-assoc 2^3^2=512, unary minus, variadic max(1,9,4)=9, variables across statements
  (run FILE → 5/10/11), and ALL error cases (div0, unknown var, wrong argc, unbalanced) exit non-zero.
  2nd gold-standard app after inventory. Tests are in-process (import calc.*) but the real entry ALSO works,
  so no false-green. Proof the fleet CAN build excellent apps when wiring/contract are right.
- jsonq (algorithmic): **GOOD (near-pass)** — 843 LOC, clean modules, 10/10 tests. Engine mostly spec-correct
  via real `jsonq get FILE "PATH"`: child, index, wildcard `[*]`, numeric filter `[?(@.price>10)]`, string
  filter `[?(@.name='Alice')]`, `keys`, and errors (missing file/bad path) all exit non-zero — all correct.
  ONE bug: array SLICE doesn't STREAM. `$.items[1:3]` alone works (returns the 2-item array), but
  `$.items[1:3].id` returns EMPTY — the slice yields a single array value instead of a stream of matched
  elements, so a child access chained after a slice can't descend. `[*]` and filters stream correctly, which
  is why only slice+chain breaks. TEST GAP: the suite tests `$.users[0:1]` alone, never slice+child, so it's
  false-green on the spec's explicit chaining requirement.

## META-PATTERN (recurring across apps) — model's own tests UNDER-COVER the spec → false green
Seen on bookclub (CLI tests skip ctx.obj path via unit-only), timesheet (tests drive Click in-process, never
the real argparse entry / --db), jsonq (slice tested alone, never slice+chain), csvql (query tests use wrong
invocation shape). The weak model writes tests that pass but avoid the spec's HARDER cases (chaining, real
entry, error paths). Implication for the done-gate (#3): do NOT trust "tests pass" — derive a handful of
GOLDEN spec-contract checks from the spec's own examples (e.g. the spec literally lists `$.items[?(@.price >
10)].id` and `2^3^2==512`) and run THOSE against the real entry. Spec examples are the ground truth the tests
keep dodging.
- tmpl (algorithmic): **FAIL (renders empty) — CAPSTONE, validates the whole backlog** — 1045 LOC, entry +
  modules, 24/24 tests, `tmpl check` works. But `tmpl render FILE DATA` produces EMPTY output for EVERYTHING
  (even a no-tag "just text" template), exit 0. FOUR failure classes compounded in one app:
  (a) CONTRACT DRIFT (#4): the 24 tests call `Renderer().render([Variable(...), Text(...)], ctx)` with a
      hand-built LIST of AST nodes; the CLI calls `Renderer().render(_parse_template(source), data)` where the
      parser returns a DIFFERENT AST shape (a wrapper/root, not a bare list) → render() iterates nothing →
      empty. Parser-output shape ≠ renderer-input shape. Same family as csvql (list vs dict).
  (b) SCHEDULER #7 (2nd occurrence): `cli` looped ×2 → salvaged_spin → the salvage marked cli Done but did
      NOT relax its dependents → integrate-verify + tests orphaned → scheduler_stuck remaining=2. The broken
      render was never validated because its verify sink was orphaned by exactly bug #7.
  (c) FALSE-GREEN (meta-pattern): tests exercise render() in-process with pre-built node lists, never the real
      parse→render pipeline, so 24 green while the real entry is fully broken.
  (d) GATE GAP (#3 refinement): `tmpl render` EXITS 0 on empty output. An exit-code-only gate passes it. The
      done-gate MUST assert NON-EMPTY / expected output for a known input, not just exit 0.
  This single app is the strongest argument for fixing #7 + #4 + #3 together.
- glob/gmatch (algorithmic): **FAIL** — 472 LOC (thin), entry + glob_matcher/regex_matcher, no tests (the
  tests task died via over_reading; smoke+smoke_after_fix ran but can't catch a semantic bug). The `test`
  subcommand matches CORRECTLY (`test glob "*.txt" file.txt`→MATCH exit0; file.md→NO exit1), but the `glob`
  and `re` FILTER subcommands are broken: `*`/`.*` patterns match EVERYTHING (b.md matches `*.txt`; dog
  matches `[abc]*`; ba matches `a.*b`) while `?`/`+`/class-only patterns match NOTHING (`foo?`, `[0-9]+`
  return empty). Same drift family: the filter path and the test path apply matching differently — the correct
  matcher exists (test proves it) but the filter loop doesn't use it right. Reinforces: golden-output gate
  (the spec's own examples) would catch this; exit-0 + smoke does not.
- taskq (systems/Rust): **FAIL (won't compile)** — 995 LOC, 5 real modules + integration test, main.rs
  properly wired (mod decls). But `cargo build` FAILS with 5 errors in src/log.rs (E0765 unterminated string;
  `unknown start of token: \`; `prefix 'n'/'tmp' unknown`) — the model wrote stray backslashes / `\n` outside
  string literals. ALL 8 LLM judge verdicts were "ok" and NO smoke ran (Python-only gate, py_files:0). So
  non-compiling Rust shipped with a clean bill of health.
  → SHARPENS #8: the pre-done gate has `py_syntax_error` (ast.parse) for Python but NO equivalent for Rust.
    Add a deterministic `cargo build`/`cargo check` gate for any Cargo.toml project, run BEFORE a Rust task is
    accepted (mirror py_syntax_error's placement) so a compile error is a content-retry with the compiler
    error as the hint — not an "ok". The LLM judge cannot be trusted to compile-check.

## BACKLOG ITEM #9 (worker efficiency) — 15% of worker tool calls fail; two distinct causes
DATA: across the 12 builds, 86 of 589 tool calls (14.6%) came back ok:false. Breakdown:
  (a) MOST are `shell` calls running pytest that report FAILING tests — these are SYMPTOMS of the app bugs
      already in the backlog (contract drift #4 etc.), i.e. the worker's own test-fix loop seeing red. Fixing
      the root causes (#4/#7/#8) removes these. Not a new bug, but the high rate tracks app quality.
  (b) AVOIDABLE weak-model operational noise (a new class, ~a dozen calls):
      - `bash: python: command not found (code 127)` — worker calls `python`, not `python3`.
      - shell-quoting blowups in inline debug snippets: `bash: syntax error near unexpected token )` from
        `python3 -c "tokens = lex(\"{{ 'hello world' }}\")"` (nested quotes).
      - `write` tool called with `missing field 'path'`; `edit` "No match found"; `read_image` on a DIRECTORY
        ("Is a directory (os error 21)").
FIX CANDIDATES (throughput = the master goal): a light harness guard/hint layer — normalize `python`→`python3`
in the shell tool (or PATH-shim), validate write/edit args with a corrective retry hint instead of a hard
fail, and a system-prompt nudge to avoid nested-quote inline snippets (write a temp file instead). Low risk,
recovers wasted turns. Confidence MED — needs care not to mask real errors.

## BACKLOG ITEM #10 (UNTESTED FEATURE — user-flagged) — Loop creation + execution never validated
Mihai flagged: the exploration has NO loop test → likely I never created one. Correct — every run so far was a
one-shot swarm build, never a Loop. A goose LOOP (ui/desktop/src/components/loop/{LoopModal,LoopView}.tsx) =
"runs a recipe repeatedly on a SCHEDULE until a STOP CHECK command passes OR the iteration cap is reached."
Created from: a name, a recipe (YAML file or goose://recipe deep link), a schedule, max-iterations, and a
stop-check shell command. Backed by the scheduler (acpListSchedules). This is EXACTLY Mihai's nightly-evolve
use case, so it MUST work on the LOCAL fleet.
TEST PLAN (do in cycle 2 / dedicated step, fleet-free so recipe runs don't contend with builds):
  1. Author a tiny recipe YAML (a trivial task the local model can do in 1 turn) + a stop-check that passes
     after N iterations (e.g. a file-counter).
  2. Create a Loop via the desktop LoopModal (drive with CDP) AND/OR the underlying schedule API — capture
     which path works.
  3. Verify: it RUNS the recipe repeatedly, RESPECTS the stop-check (halts when it passes), RESPECTS the
     max-iterations cap, and LoopView shows live iteration progress.
  4. Verify it uses the LOCAL provider (not cloud) and survives a model hiccup.
  5. Screenshot LoopView; check for the same visual/logging gaps the swarm panel had.
EXPECTATION: unknown — this is genuinely untested; treat a first-run failure as expected signal, not a
setback. Confidence LOW that it works first try on the local fleet (never exercised) — flagged honestly.
- blobs (systems/Rust): **GOOD (near-pass) — best Rust result** — 1057 LOC, 5 modules + integration test,
  COMPILES (5 warnings), main.rs properly wired. Real CLI works spec-correct end-to-end: init → put --name
  greeting → cat greeting = "hello blob world" → names → ls (hash+size) all exit 0 and correct. 13/20 tests
  pass. The 7 failures are TEST-vs-IMPL drift on store LAYOUT: the tests assert `<dir>/.blobs/objects` but the
  spec says "global --dir, default .blobs" (i.e. --dir IS the store), so the impl correctly makes
  `<dir>/objects` + `<dir>/log`. Here the IMPL is spec-correct and the TEST is wrong (mirror of csvql). Still
  contract drift between two workers — reinforces #4. Rust CAN produce a working app (contra kvstore/taskq);
  the difference is this one compiled AND wired the entry.

## BACKLOG #10 — CONCRETE HEADLESS TEST MECHANISM (found, ready to run when fleet is free)
The bundled binary HAS `goose schedule {add,list,run-now,remove}` + a `--local` flag (swarm-model edition).
A desktop Loop = a schedule + stop-check command + max-iterations (LoopModal builds a cron payload, default
`0 0 14 * * *`, and calls acpCreateSchedule). Headless test path (no cron wait): author a minimal recipe YAML
(version/title/instructions + a trivial 1-turn task), `goose schedule add … --recipe <file>`, then
`goose schedule run-now <id>` and inspect the created session. For the LOOP-specific stop-check + iteration-cap
semantics, drive the desktop LoopModal via CDP (that logic lives in LoopView, not necessarily the CLI schedule).
Recipe schema confirmed from goose-self-test.yaml: version, title, description, author, activities, parameters
(+ instructions/prompt). Will run this after cycle-1 builds finish so recipe iterations don't contend the fleet.
- wal (systems/Rust): **FAIL (no persistence)** — 997 LOC, lib.rs + 6 modules + 2 test files, COMPILES, entry
  wired. But the core WAL is broken across process invocations: `append first` → LSN 1, `append second` → LSN
  1 AGAIN (never advances), and `read`/`tail 1`/`verify` all see 0 records ("OK 0") despite 2 appends. Each CLI
  command is a fresh process and append doesn't read the existing log's last LSN nor persist readably — so
  nothing accumulates. 2/8 tests pass (init + verify-empty). Not contract drift — a genuine functional bug in
  the writer/reader persistence. The single-invocation illusion (append prints an LSN) hides that nothing is
  durable. A golden round-trip gate (append twice → read → expect 2 records) catches it instantly; exit-0 does not.

## BACKLOG ITEM #11 (throughput / fleet starvation — user-flagged, HIGH value) — app provider drops GOOSE_SWARM_SPLIT
SYMPTOM (Mihai's LM Studio screenshot): 1 node PROCESSING, 2 nodes READY/idle — fleet starved.
MEASURED (from run jsonl, distinct-task-id concurrency, retries not double-counted): true_max_concurrent peaks
at 2–3 (all 3 CAN be used — expense/blobs hit 3), but a large share of dispatches happen at concurrency=1
(trie 5 serial vs 4 parallel; blobs 7 vs 5). So the scheduler + fleet detection are NOT broken — the fleet is
just under-fed most of the wall-clock.
ROOT CAUSE: the plans are coarse near-serial chains. trie's plan = 4 tasks: shared-types-and-store (root, runs
ALONE) → commands-and-cli + integration-tests (width 2) → integrate-verify (ALONE). Two of three phases are
single-task. The runtime lever that FIXES this — GOOSE_SWARM_SPLIT (propose_split partitions an over-long
subtask's owned files into 2–4 INDEPENDENT children so several workers run in parallel; swarm.rs:6601,6718–6726
"split-enable is OFF in the default; GOOSE_SWARM_SPLIT=1 turns task-splitting on at runtime") — is OFF for app
builds. PROOF: zero split/child events across all 15 builds; only `scouts_planned`. The app PROVIDER
(crates/goose/src/providers/swarm.rs:229–241) passes ONLY `--output-format json` + `GOOSE_SWARM_SMOKE=1` — it
never sets GOOSE_SWARM_SPLIT. My earlier direct-CLI A/B runs DID pass `GOOSE_SWARM_SPLIT=1`. So the regression
is real and it is a DROPPED FLAG on the app path, NOT anything CDP/scheduler/fleet-detection.
FIX (task #58): in the provider, set GOOSE_SWARM_SPLIT=1 (and consider GOOSE_SWARM_SPLIT_SECS to tune the
too-long threshold) the same way SMOKE is set — one line. Then fat serial tasks decompose and the fleet fills.
HONEST CAVEAT (quality risk): more parallel children = more cross-task interfaces = MORE surface for the
dominant contract-drift failure (#4). SPLIT should land TOGETHER WITH the CONTRACTS fix (#4, shared-type stubs
every child imports) so we get the throughput WITHOUT the drift cost — this is the "throughput, same quality"
invariant. Splitting alone on a weak model could trade starvation for drift. MED-HIGH confidence on the util
win; the pairing with #4 is what keeps quality flat.
DEEPER (separate): the planner itself emits narrow DAGs (lumps types+store into one big serial root). A
wider-fan-out planner prompt would help beyond SPLIT, but SPLIT is the immediate designed lever.
- trie (systems/Rust): **GOOD (spec-shape drift) — 2nd-best Rust** — 838 LOC, 4 modules, COMPILES, 20/21 tests.
  Trie logic is SOUND (insert/get/prefix/remove all correct via real CLI). But the CLI DEVIATES from the spec
  contract: implemented `insert`/`remove` (spec said `set`/`del`), and takes DIR POSITIONALLY (`trie init DIR`)
  instead of the spec's "global --dir". The tests were written to match the IMPL's names, not the spec, so
  20/21 green while a spec-exact contract check would fail. Same false-green family as timesheet (tests follow
  impl, both drift from spec). 1 real failure: range_empty edge case. Reinforces the golden-spec-contract gate:
  the exact command names in the spec must be verified, not the model's own tests.

## BACKLOG ITEM #12 (process leak) — app orphans goosed backends + swarm runs on quit
FOUND while checking why the fleet looked busy after cycle 1: 2 orphaned `goose swarm run` (crm 11.5h old, wal
2.5h old — both COMPLETED builds) + 6 orphaned `goose serve` (goosed) backends, ALL re-parented to launchd
(pid 1), i.e. their app instances died but the children survived. Cause: `osascript quit` (and window close)
SIGKILLs goosed before its kill_on_drop / shutdown can reap the child swarm run; each relaunch leaks one
backend. Over a relaunch-heavy session (15 builds → ~15 relaunches) this piled up 6+ leaked backends + 2 leaked
runs. They were at 0% CPU (hung, not actively churning) so likely not the PRIMARY starvation cause (#11 is),
but leaked runs CAN hold fleet connections and are pure waste. Cleaned up by exact-pid kill.
FIX (MED): goosed should install a shutdown/signal handler that reaps its child `goose swarm run` (and the app
should reap goosed cleanly on quit). Partly amplified by my testing pattern (many relaunches) — for cycle 2,
reduce relaunches (dispatch more via one instance or CLI) to avoid re-introducing the leak + CDP degradation.

## BACKLOG #10 RESULT — Loop base mechanism VALIDATED on the local fleet (partial)
Tested the headless path on 1.41.24: `goose schedule add --schedule-id looptest --cron ... --recipe-source
<recipe> --local` succeeded, and `goose schedule run-now --schedule-id looptest` RAN the recipe on the LOCAL
model — it used the shell tool and appended to /tmp/loop_test/log.txt. So the base "run a recipe on the local
fleet" mechanism WORKS (this is the core of Mihai's nightly-evolve use case). Confidence now HIGH on the base.
NOTES:
- The recipe ran CHATTY: it appended 9 lines ("iteration 1..9", some dupes/out-of-order) from ONE run-now,
  i.e. the local model looped the shell command internally instead of the single append the recipe asked for.
  That's a recipe-prompt/model-behavior nit (tighten the recipe instruction / add a done signal), NOT a loop
  mechanism failure. run-now also runs SYNC and long (didn't return in 2 min) because the model kept going.
- STILL UNTESTED: the FULL Loop semantics (stop-check command + iteration cap) live in the desktop
  LoopView/LoopModal, not the CLI schedule. Need a CDP drive of the Loops UI to verify the stop-check halts and
  the cap bounds. Follow-on. The base scheduler + local-provider recipe execution is proven.

## BACKLOG #6 CORRECTION (from live cycle-2 observation) — reverted; wrong lever
Mihai's cycle-2 LM Studio screenshot: mihai 1+1 QUEUED, workhorse 1+2 QUEUED, gabee READY (idle). My #6 set
the pool concurrency WEIGHT = speed_weight (3/2/1). LM Studio serves ONE request per model at a time, so
weight>PARALLEL just QUEUES on a node while an idle node starves — worse concurrency. REVERTED: concurrency
weight = node's real capacity (explicit override, else LM Studio PARALLEL, else 1) = 1 slot/node, no queuing.
KEY: the scheduler ALREADY does "faster host does more work" via a SEPARATE speed_weight field consumed by
pick_device's ROUTING (speed-weighted load share + least-loaded-wins + work-stealing) — untouched and correct.
So the user's ask was already satisfied by routing; concurrency should never have been touched. SPLIT (#11)
remains the real starvation fix (enough parallel tasks to fill 3 nodes). Lesson: `weight` = concurrency (cap
at PARALLEL), `speed_weight` = routing share — never conflate them.
