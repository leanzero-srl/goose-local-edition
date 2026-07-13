# Loop test (backlog #10) — validate goose Loop creation/execution on the local fleet

Recipe: append-line.yaml (trivial 1-turn: append a line to /tmp/loop_test/log.txt, report count).
Stop-check command (loop halts when this exits 0):  test "$(wc -l < /tmp/loop_test/log.txt)" -ge 3
Max iterations: 5 (cap should never be hit if stop-check works at 3).

## Two paths to test (run when the fleet is FREE, after cycle-1 builds):
A) Headless base (scheduled recipe run, no loop wrapper):
   rm -rf /tmp/loop_test
   goose schedule add --schedule-id looptest --cron '*/1 * * * *' --recipe-source evals/.../append-line.yaml --local
   goose schedule run-now looptest        # runs the recipe once immediately
   goose schedule sessions looptest        # confirm a session ran + used the LOCAL provider
   # verify /tmp/loop_test/log.txt gained a line; then: goose schedule remove --id looptest
B) Full Loop semantics (stop-check + iteration cap) via the desktop LoopModal, driven by CDP:
   open the Loops view → Create Loop → recipe=append-line.yaml, stop-check as above, max-iter=5 → run.
   Verify: runs repeatedly, HALTS at 3 lines (stop-check), never exceeds 5, LoopView shows live progress.

## What to check
- Loop/schedule actually RUNS the recipe on the local swarm model (not cloud).
- Stop-check is honored (halts at 3, not 5).
- Iteration cap is honored (would stop at 5 if stop-check never passed).
- LoopView renders progress without the visual/logging gaps the swarm panel had.
- Failure on first run = expected signal (feature never exercised); root-cause it.
