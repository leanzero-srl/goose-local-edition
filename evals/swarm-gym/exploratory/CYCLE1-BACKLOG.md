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
