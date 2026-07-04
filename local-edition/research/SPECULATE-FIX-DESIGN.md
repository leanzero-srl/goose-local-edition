# SPECULATE full fix — research + design (workflow wu88jvzkw, round 1, 2026-07-04)
Goal (user): design the full GOOSE_SWARM_SPECULATE fix to HIGH confidence via research + adversarial review; iterate until sound.

## Research (grounded, web + code)
- tokio abort() is a SIGNAL, not a stop; awaiting the JoinHandle after abort guarantees an ASYNC task's future+Drop are done. BUT spawn_blocking / tokio::fs writes CANNOT be aborted — they detach + finish later. Worse: goose worker shell subprocesses (developer/shell.rs:508-556) have NO kill_on_drop / no process-group, so aborting the primary ORPHANS the live OS process; its writes still land. => you CANNOT reliably stop a losing worker's writes.
- The reliable pattern: ISOLATION + ATOMIC RENAME. Confine every speculative write to a per-worker scratch dir on the SAME filesystem; publish only the verified winner via atomic rename (fsync file, rename, fsync dir for durability). A loser's detached late write lands in a doomed scratch dir -> harmless. "Route the write where it can't matter" instead of "stop the write (impossible)."

## Candidate verdicts (round 1, both adversarially broken)
- B (primary in real tree + abort-join): DROP — abort can't stop the orphaned shell subprocess -> loser corrupts winner. Fundamentally unsound.
- A (BOTH primary+twin in shadows, verify-before-commit, atomic-promote-owned-files): RIGHT ARCHITECTURE (matches the research) but REDESIGN — 2 holes:
  1. PROMOTE-ORDERING RACE: A promotes OUTSIDE the lock (to dodge lock-across-await), but complete() marks Done + relaxes dependents UNDER the lock -> a dependent can snapshot the real tree BEFORE the winner's files are promoted (torn READ). FIX: a structural barrier — promote the winner's owned files BEFORE the Done/relax transition (or gate readiness on promote-complete).
  2. VERIFY-BYPASS: run_smoke_gate.passed()=ran && findings.is_empty(), but ran=true is set unconditionally if any .py exists; a missing pytest or a smoke-subprocess TIMEOUT (routine under fleet load) passes the gate spuriously. FIX: require the tests ACTUALLY ran + passed (not just ran=true); treat timeout/missing-oracle as NOT-verified (fail-closed).

## KEEP (sound primitives): shadow-both-racers isolation; tmp-in-dst-dir + fsync + atomic rename per owned file (disjoint partition => whole-tree consistency).
## NEXT: round 2 refines A to close both holes + re-adversarial-review to HIGH confidence before any implementation.
