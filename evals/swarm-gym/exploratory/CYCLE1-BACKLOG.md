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
IMMEDIATE FIX (applied): pointed the playwright extension config at /opt/homebrew/bin/npx (node 26.3.1) —
verified @playwright/mcp starts cleanly under node 26 and node 22. config.yaml.bak saved.
SYSTEM-HYGIENE (user, needs sudo): remove/update the stale node — `sudo rm /usr/local/bin/node
/usr/local/bin/npx` (or `brew link --overwrite node`) so ALL tools stop picking up 19.8.1, not just goose.
DURABLE GOOSE FIX (backlog): when spawning a stdio extension whose cmd is `npx`/`node`, goose should resolve
a node that satisfies the requirement (or prepend the modern node dir ahead of /usr/local/bin in the
extension PATH) rather than trusting whatever `node` PATH-resolves — so a stale system node can't silently
break every npx-based extension. main.ts builds the extension env; this is where it'd go.

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
