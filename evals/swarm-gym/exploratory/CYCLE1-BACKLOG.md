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
