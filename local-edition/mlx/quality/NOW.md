# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-28 23:3x (tick 5, 23:59) (date) · heartbeat cron 90b0083a + runwatch.sh per run

## Live
- Installed: 3.0.76 on BOTH Macs 05:4x. The split is serving ("OK.").
- E2E #3w RUNNING: RU-2026-09-29-3w-split-tensor-cafe, session 20260929_12, the café brief (37 turns): needs-you
  cards at turns 4 / 7 (two at once) / 11, chat search 19/21/32, a note to another chat at 20, frontend-design
  skill at 9, compaction recall.
- E2E #3v stopped after 9 turns for the install (E2E-RUNS). Q-367 proven live; the repeat guard broke a loop.
- To live-prove on 3.0.76, all at once while #3w runs, from its own surfaces:
  - Q-455 (context line) and Q-469 (no false correction);
  - Q-467 (counter);
  - Q-458..462 (composer lines);
  - Q-468 (banner, needs a copy config);
  - Q-334/335/457 (solid colours: live walk).

## CI
- a66c6ccf6: one of two runs RED on the sidecar test shutdown_releases_the_port_from_residue_of_its_own_group (a flake; the sibling run passed) → Q-449 agent.

## Agents (worktrees)
- none running.

## 3.0.77 = main 5e03efbc0 — RELEASE BUILDING (~/goose-builds/release-3.0.77.log)
- Gates g077 + g077b + g077c all green.
- Holds:
  - Q-470 (secret echo), Q-471/472/473/478 (banner row, no native dialogs/selects anywhere, object config
    read-only, onboarding 401);
  - Q-457 reopen + Q-474..477 (highlight behind text, solid red, focus everywhere, no rails);
  - Q-479 (contrast 199 → 0);
  - Q-480 (several ask_user in one message; agent.rs kept only the 1st on per-chunk providers).
- Install at an #3w turn boundary. Then walk the update/quit dialogs (now app-drawn) and the colours live.

## Queued / scheduled
- Q-427 QUEUED behind: Q-426 landing. Q-398 QUEUED behind: agent cap. Q-425 QUEUED behind: fork access.
- Q-424 SCHEDULED waits on: a Studio ladder (32k/64k/128k/176k peak Metal) with the engine free.
- Q-385 SCHEDULED waits on: the split free (the ' =>' logprob read).
- Ledger hygiene: Q-407 and Q-417 read 'open' but are DONE, and Q-164/181/209/210/221 have read 'cutting' since
  09-27. Reconcile them at the batch-4 merge.

## Standing rules for every tick
- Check CI, agents, disk ≥ 30 GB (126 GB now) and clean.sh. Merge via scratch plus ledger_resolve, gate, ff, push.
- The coordinator assigns Q ids; the next free id is Q-481. Agents use their own scratch folders.
- Never navigate the main window while an E2E runs. Kill pids, never killpg.
- Training: next round on the MacBook, ONLY on the owner's word (memory next-training-on-macbook).
