# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-28 23:3x (tick 2) (date) · heartbeat cron 90b0083a + runwatch.sh per run

## Live
- Installed: 3.0.73 on BOTH Macs; the app serves the 27B SPLIT (Thunderbolt/jaccl, 0.46 ms).
- E2E #3u RUNNING: RU-2026-09-28-3u-split-tensor, session 20260928_47, jira brief, watcher runwatch.sh. It
  exercises needs-you answering (Q-376) and compaction on 3.0.73.
  - Turn 0: 905 s, notes file OK.
  - Turn 1: 212 s. Both memories were saved, but it invented a reason for the 24/9 date conflict in the client
    file (Q-448, model behaviour, parked to the training round, forge-tuner 42b1893).
  - Turn 2 (DC end-of-support web research) is running. Watch for invented tool names; the Q-367 hint has been
    live since 3.0.71.
- Q-447 FOUND this tick (STABILITY/perf):
  - Split decode is ~2.7× slower on cache-hit chat turns since the FIRST 3.0.71 launch (18:12).
  - Before: 8.2–9.5 tok/s at 159k–211k. After: 2.8–4.1 tok/s at 41k–90k. Cold 21k demo delegates still run 6.9–12.1.
  - Launch caps, link and thermals are identical.
  - Suspect: Q-347's stable-head snapshot (4b16c7ba3) sharing KV buffers, so a copy is made per step. Unproven.
  - An mlx-backend agent is root-causing it on a tiny-model repro (no touch of the live engine).
  - #3u keeps running meanwhile: it measures needs-you and compaction, not speed.
- Owner demo done (nodes: Studio single, both Macs, deepseek-v4.1-flash · OpenRouter; strategy "Studio chat, split
  for heavy work"; screenshots ~/goose-builds/quality/DEMO-2026-09-28-strategy).

## CI
- main green through 9d15e2522; ac6be4fbb running.

## Agents (worktrees)
- Q-447 split decode regression — mlx-backend, dispatched 23:3x.
- Q-428 (+Q-430/432) the owner's "don't interrupt a node doing its thing" per-role option — agent-a626efc4490c889b7.
- Q-429/431/433..440 demo UI/instrument defects — agent-ad310ae976a00eaad.
- DONE, waiting for batch 4 (all reported, not yet merged):
  - Q-417 worktree-agent-ad33799988ff81b4d (servedModel for single/split/remote);
  - Q-407 worktree-agent-a324bae2eb0f9de65 (a dead goosed's commands ended);
  - Q-423 worktree-agent-a5f81fabce30067fc (the engine's stderr in a durable log; BOTH Macs);
  - Q-426 worktree-agent-a35cf92df2428aff6 (the PiP X closes it for the session).

## Batch 4 → 3.0.74 (next)
1. When Q-428, Q-429..440 and Q-447 report: /tmp/merge-074 = main + Q-417 + Q-407 + Q-423 + Q-426 + Q-428 + Q-429..440
   + Q-447, via ledger_resolve, with main merged in before the ff.
2. ONE full gate in its own session (scratchpad/gate074.sh, target ~/goose-targets/g074), when #3u is at a turn
   boundary or done. NO cargo on the MacBook while #3u decodes. The Q-447 agent is the one allowed job.
3. ff main, push, release 3.0.74 (/tmp/rel074.sh), install on BOTH Macs (Q-423, maybe Q-447), split-start smoke.
4. Live prove:
   - Q-447 (decode ≥ 8 tok/s on a cache-hit turn);
   - Q-428 (re-run the demo's shared-Mac case);
   - Q-426 (PiP);
   - Q-417.
5. Tell the owner Q-428's result (his ask).

## Queued / scheduled
- Q-427 QUEUED behind: Q-426 landing. Q-398 QUEUED behind: agent cap. Q-425 QUEUED behind: fork access.
- Q-424 SCHEDULED waits on: a Studio ladder (32k/64k/128k/176k peak Metal) with the engine free.
- Q-385 SCHEDULED waits on: the split free (the ' =>' logprob read).
- Ledger hygiene: Q-407 and Q-417 read 'open' but are DONE, and Q-164/181/209/210/221 have read 'cutting' since
  09-27. Reconcile them at the batch-4 merge.

## Standing rules for every tick
- Check CI, agents, disk ≥ 30 GB (126 GB now) and clean.sh. Merge via scratch plus ledger_resolve, gate, ff, push.
- The coordinator assigns Q ids; the next free id is Q-449. Agents use their own scratch folders.
- Never navigate the main window while an E2E runs. Kill pids, never killpg.
- Training: next round on the MacBook, ONLY on the owner's word (memory next-training-on-macbook).
