# S1 — shard the sink's REPAIR along the file partition (design, pre-implementation)

The measured target: the sink runs at concurrency ≤2 for 54% of the dispatch span (median
1,412s solo, p90 3,345s), and F454 showed its cost SCALES with tree size — the serial fraction
grows with node count, which is why wall-clock cannot win until the join shards.

## Mechanism (reuses three proven pieces, no new safety argument)

1. The sink's verify pass stays ONE task (unchanged): run the suite, collect findings.
2. Its FIX work — today an in-place serial loop of read-edit-build-test cycles — becomes
   `group_findings_by_file` (already a test-pinned NORMALIZED disjoint partition) fanned under
   `complete_parallel`'s shadow discipline (already promote-by-owned-file, already raced in
   spec_repair): one fix shard per file-group across the 6 slots, cross-file findings routed to
   a thin serial join pass (the K5 single-owned-file repair hook applies per shard for free).
3. Re-verify at the loop head exactly as today (the deterministic gate stays the only judge).

## Why this is safe where CooperBench failed
Disjoint write ownership by construction (the partition), interfaces frozen (contracts), merge
by promote-per-file (no negotiation), selection by execution (the re-verify). All four clauses
of the splitting design law hold.

## Registered checks (write before the arm runs)
- MECHANISM, n=1: sink-phase `complete_fix_wave{shards>=2}` on a multi-finding round; wall of
  the fix phase vs the r1-r4 serial fix median.
- SAFETY: `winner/round findings` never rise after a promote (the spec_repair guarantee).
- QUALITY GATE: stable-24 score not below the pre-S1 mean − spread.

## Cost/risk stated
Shadow copies per shard (cp -r of the tree; measured cheap at this tree size ~60KB); the thin
cross-file join remains serial (bounded by the finding mix — measured 10 survivors ≈ 4-5
defects, mostly single-file).

## Increment 3 — the thin cross-file join loses its last unverified write path (designed 2026-08-11, not yet implemented)

What increment 2 left: findings that name NO file get one serial dispatch AFTER the wave with
`speculative: false` — the last place in the whole repair tail where an agent writes the REAL
tree directly, ungraded. Every other path (twins, shards) now goes shadow → gate → promote-only-
strictly-better; the join twin should be no different.

Mechanism (one rule, already proven twice):
1. After the wave's barrier, IF unassigned findings exist: run the smoke gate ONCE on the real
   (post-promote) tree → `post_wave` count. This is the join's honest baseline — the round's
   opening count is stale the moment a shard promotes.
2. Dispatch the join twin `complete-fix::cross-file` with `speculative: true`, owned_files []
   (whole-tree twin semantics — which file the cross-file fix needs is exactly what is unknown,
   the race branch's argument verbatim). Its shadow cp -r roots at the post-promote tree by
   construction (dispatch happens after the barrier).
3. Grade the tree, not the agent's exit: `run_smoke_gate` on its shadow; promote iff
   `shard_beats_baseline(verified, post_wave)`. Emit complete_fix_dispatched/completed with
   `shard: "(cross-file)"` so the join stops being the tail's one dark dispatch.

Cost stated: one extra gate run per round that has unassigned findings (measured gate cost:
seconds — pytest on a ~60KB tree), plus one cp -r. Nothing else changes; rounds without
unassigned findings pay zero.

Registered checks (write before the arm runs):
- MECHANISM, n=1: `complete_fix_completed{shard:"(cross-file)"}` with a `promoted` verdict on a
  round whose findings include a file-less one (the http_timeout class reliably produces these).
- SAFETY: grep the round — no dispatch in the repair tail carries `speculative: false` anymore;
  findings count never rises across a join promote.
- QUALITY GATE: unchanged (stable-24 vs pre-S1 mean − spread).
