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
- inventory (data): STRONG PASS (prior) — 808 LOC, spec-exact CLI, 22/22.
- bookclub (data): **FAIL** — 724 LOC, 16/22 tests fail, CLI crashes on init (ImportError: no module 'shelf.models';
  no __main__.py entry). Broken package wiring / cross-module imports. NOTE: the smoke gate (new fix) CAUGHT it
  (collect=errors "ModuleNotFoundError: shelf.models" + "no python3 -m <pkg> entry point — unrunnable"), but the
  build shipped broken because smoke is ADVISORY (one corrective re-dispatch that didn't fix it). Built during the
  fleet transient (workhorse as qwopus:2) with 2 retries.
  → BACKLOG #3: UI builds should use the STRONGER gate (GOOSE_SWARM_COMPLETE: verify-by-running + iterate to green)
    or smoke should hard-block "unrunnable" rather than ship. The advisory smoke detects but doesn't prevent.
  → BACKLOG #4 (recurring): package/import wiring breaks (shelf.models import, csvql row-type) — cross-module
    contract drift is the dominant failure. Candidate: a planner "shared-types/interface" contract the workers
    must import, verified before completion.

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
