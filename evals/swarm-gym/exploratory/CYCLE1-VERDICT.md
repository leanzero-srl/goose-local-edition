# Cycle 1 Verdict — 15 apps, 3 archetypes, all app-dispatched on the 3-node qwopus fleet

## Scorecard
| verdict | count | apps |
|---------|-------|------|
| STRONG PASS | 2 | inventory, calc |
| GOOD (near-pass) | 4 | crm, jsonq, blobs, trie |
| PARTIAL | 1 | timesheet |
| UNRUNNABLE (tested-but-no-entry) | 1 | expense |
| FAIL | 7 | bookclub, csvql, tmpl, glob, kvstore, taskq, wal |

By archetype: DATA 1 strong/1 good/1 partial/1 unrunnable/1 fail (mostly usable). ALGO 1 strong/1 good/3 fail.
SYSTEMS 2 good/3 fail (Rust is the weakest — but blobs+trie prove it CAN work).

## The convergent story: the fleet CAN build, but 4 systemic issues sink most runs
The two STRONG passes (inventory, calc) prove the model writes correct core logic when wiring+contracts hold.
Almost every failure is NOT "the model can't code" — it's one of these, ranked by fix confidence:

1. #7 SCHEDULER salvage orphans dependents (HIGH) — expense + tmpl: a salvaged looping task is marked Done but
   never unblocks its dependents → the CLI/verify tasks never dispatch → tested-but-unrunnable apps.
2. #8 GATE is Python-only + exit-0-weak (HIGH) — kvstore (empty main) + taskq (won't compile) shipped clean;
   tmpl `render` exits 0 with empty output. No cargo build/check; --help too weak; no golden-output assertion.
3. #4 CONTRACT DRIFT — the dominant failure (bookclub ctx.obj, csvql row dict/list, tmpl parser/renderer,
   glob filter-vs-test, blobs layout, trie names). Two workers disagree on a shared contract.
4. #11 FLEET STARVATION (HIGH, user-flagged) — app provider drops GOOSE_SWARM_SPLIT → coarse near-serial plans
   → 1-2 nodes idle most of the wall-clock. Pair with #4 so more parallelism doesn't mean more drift.

Meta-pattern across ALL false-greens: the model's own tests UNDER-COVER the spec (timesheet tests bypass the
real entry, jsonq skips slice+chain, trie tests match impl not spec, csvql wrong shape). "Tests pass" is not
"works" — the gate must run the spec's OWN golden examples against the real entry.

## Full backlog (see CYCLE1-BACKLOG.md + FIX-PLAN.md for exact diffs)
HIGH value/confidence: #7 scheduler salvage, #8 language-aware+golden gate, #11 SPLIT (with #4).
MED-HIGH: #4 CONTRACTS (shared-type stubs), #6 dispatch weights.
MED: #1/#2 pool robustness, #5 durable Playwright node (immediate wrapper fix applied+verified), #9 worker
tool-call waste.
UNTESTED (user-flagged): #10 Loop creation/execution — never exercised; test planned (recipe authored).

## What "functional" looks like after the fixes (the cycle-2 hypothesis)
#7 → expense/tmpl become runnable. #8 → kvstore/taskq/wal can't ship broken. #4+#11 → parallelism without
drift, so bookclub/csvql/glob/trie converge. Target for cycle 2: the same specs move from 2 strong/4 good to
a majority runnable, and the fleet visibly stays busy (SPLIT on).
