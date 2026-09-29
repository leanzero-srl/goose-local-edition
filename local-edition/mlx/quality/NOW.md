# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-28 23:3x (tick 5, 23:59) (date) · heartbeat cron 90b0083a + runwatch.sh per run

## Live
- Installed: 3.0.74 on BOTH Macs; the split is serving.
- E2E #3v RUNNING: RU-2026-09-29-3v-split-tensor-stdlib, session 20260929_5, the stdlib-backport brief, 3.0.74,
  runwatch.
- Q-428 + Q-430/432/441/442/443 PROVEN LIVE on 3.0.74; Q-447 decode 11.1 tok/s on a cache-hit turn with a helper
  beside it, no RANK_KV_DTYPE_MIXED. The prove found Q-458..463 (cutting).
- E2E #3u COMPLETE 30/30; Q-342/Q-347 proven on its compaction.
- A brief author is writing a 3rd brief (needs-you cards, chat search/notes, compaction recall).

## CI
- a66c6ccf6: one of two runs RED on the sidecar test shutdown_releases_the_port_from_residue_of_its_own_group (a flake; the sibling run passed) → Q-449 agent.

## Agents (worktrees)
- merge-075 (/tmp/merge-075) holds Q-454, Q-455, the lint fix (via main), Q-269, Q-334/335, Q-207, Q-208.
- Cutting: Q-457 (faded colour tints, on merge-075), Q-458..462 (composer/chip lines), Q-463
  (context_window_unknown on strategies), Q-465+Q-464 (corrupt config overwritten; hermit test), Q-456
  (gooseServe.test flake).
- Q-270/373/424/205 SCHEDULED on measurements; Q-398/425 QUEUED behind fork access.

## 3.0.75 (next build) — on main now: Q-454 (docx tables), Q-455 (chat context line), the Linux lint fix
- Build after the live prove, so it can carry anything that fails there.

## Queued / scheduled
- Q-427 QUEUED behind: Q-426 landing. Q-398 QUEUED behind: agent cap. Q-425 QUEUED behind: fork access.
- Q-424 SCHEDULED waits on: a Studio ladder (32k/64k/128k/176k peak Metal) with the engine free.
- Q-385 SCHEDULED waits on: the split free (the ' =>' logprob read).
- Ledger hygiene: Q-407 and Q-417 read 'open' but are DONE, and Q-164/181/209/210/221 have read 'cutting' since
  09-27. Reconcile them at the batch-4 merge.

## Standing rules for every tick
- Check CI, agents, disk ≥ 30 GB (126 GB now) and clean.sh. Merge via scratch plus ledger_resolve, gate, ff, push.
- The coordinator assigns Q ids; the next free id is Q-466. Agents use their own scratch folders.
- Never navigate the main window while an E2E runs. Kill pids, never killpg.
- Training: next round on the MacBook, ONLY on the owner's word (memory next-training-on-macbook).
