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

## Round 2 (workflow w411ey0tj) — refined A still NOT sound; 2 deeper holes
Refined A: 4-phase pipeline VERIFY(no lock) -> GUARD/first-wins(lock) -> PROMOTE atomic-rename(no lock) -> COMMIT Done+relax(lock) + spec_committing guard + happens-before proof (promote completes before relax => dependents see files) + hardened verified() (fail-closed on timeout/missing-pytest/zero-tests — CONFIRMED genuinely fail-closed).
Skeptics broke it:
1. JUDGE INTERFERENCE (fixable): splitting the atomic complete() into G->P->C leaves the task Claimed during Phase P (promote, no lock) -> pick_judge_target (724) can re-dispatch it mid-promote (attempts+=1). FIX: spec_committing must also make pick_judge_target SKIP the task (and dynamic-replan). A committing task is off-limits to every other mutator.
2. FUNDAMENTAL (high-confidence REDESIGN): verify_shadow runs the WHOLE-TREE smoke oracle on a twin's PARTIAL shadow. A twin races a mid-run chokepoint while sibling file-owning tasks still run (in their own shadows), so their files are absent from the real tree + the twin's cp -r shadow -> pytest --collect-only ImportError -> verified()==false ALMOST ALWAYS mid-run. So verify-before-commit makes speculation a NEAR NO-OP (twins can't verify a partial tree -> never win). The only task that sees a whole tree (the sink) is excluded (owns-nothing). => whole-tree verify-before-commit is STRUCTURALLY INCOMPATIBLE with mid-run speculation.
KEY: the escape is a SCOPED verify — verify the twin's OWNED FILE only (compiles / has expected symbols, e.g. py_syntax_error / DONE_GATE style), which is partial-tree-safe AND the correct bar for a chokepoint twin (its deliverable is the owned file, not the whole tree). Round 3 tries scoped-verify + judge-block; if it still doesn't converge, SPECULATE may be fundamentally low-value (honest conclusion).
