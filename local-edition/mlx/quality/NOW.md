# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-28 23:3x (tick 5, 23:59) (date) · heartbeat cron 90b0083a + runwatch.sh per run

## Live
- Installed: 3.0.73 on BOTH Macs; the app serves the 27B SPLIT (Thunderbolt/jaccl, 0.46 ms).
- E2E #3u RUNNING: RU-2026-09-28-3u-split-tensor, session 20260928_47, jira brief, watcher runwatch.sh. It
  exercises needs-you answering (Q-376) and compaction on 3.0.73.
  - Turn 0: 905 s, notes file OK.
  - Turn 1: 212 s. Both memories were saved, but it invented a reason for the 24/9 date conflict in the client
    file (Q-448, model behaviour, parked to the training round, forge-tuner 42b1893).
  - Turn 2: 346 s, 9 tools, a correct sourced answer (EOL 2029-03-28, end of sale to existing customers
    2028-03-30, JCMA floor 7.6+). Zero invented tool names (#3r: 13 of 30). It guessed 2 URLs (404), then searched.
    Q-367's error text was NOT exercised, so it stays "awaiting live prove".
  - Turn 3: 815 s, 12 tools. It spiralled through self-contradicting checks and closed on "Re-running … now: 128
    groups" with no tool run, and the reply check was silent → Q-450 (agent: claim_check extension + replay).
    - Turn 4: 619 s, 11 tools. It read the bytes, fixed the twin block and proved two runs identical (MD5).
      Still wrong: the "case-only duplicate emails" are case-only USERNAMES — the emails are lowercased by
      construction, and it said so itself at 772533. It re-used a restated paragraph (772527 = 772511) and
      rationalised the invented 128. Model behaviour, covered by the Q-448/Q-450 addenda; no goose defect.
    - Turn 5: 348 s. projects.csv saved; all 12 leads added to users.csv, 4 of them inactive; two runs MD5-identical.
      Model slip: it said dropping inactive users would orphan FRT, but FRT's lead mkowalski is active by its own
      output. No goose defect.
    - Turn 6 (identity-resolution PLAN script; loaded atlassian-migration-scripts-skill) is running. Decode is 9.5–11.9 tok/s since the 23:30 flip.
- Q-447 ROOT-CAUSED + FIXED (b49982c26, merged into merge-074):
  - mlx_lm's BatchKVCache.extend fills a KV-less row with a float32 array. A cold helper joining the chat's row
    mid-prefill turned the batch KV float32, and every cache entry restored from it too: decode ran at 4 instead
    of 11 tok/s.
  - Q-347 and buffer sharing are refuted. The new tag means BOTH Macs need 3.0.74.
- Owner demo done (nodes: Studio single, both Macs, deepseek-v4.1-flash · OpenRouter; strategy "Studio chat, split
  for heavy work"; screenshots ~/goose-builds/quality/DEMO-2026-09-28-strategy).

## CI
- a66c6ccf6: one of two runs RED on the sidecar test shutdown_releases_the_port_from_residue_of_its_own_group (a flake; the sibling run passed) → Q-449 agent.

## Agents (worktrees)
- Q-428 (+Q-430/432) the owner's "don't interrupt a node doing its thing" per-role option — agent-a626efc4490c889b7.
- MERGED into /tmp/merge-074 (branch merge-074, 00:0x; no ledger row lost, gains Q-424/425):
  - Q-417 (servedModel), Q-407 (dead goosed's commands), Q-423 (engine stderr; BOTH Macs), Q-426 (PiP X);
  - Q-450 (claim_check: an announced action with no tool, a count no output holds; 0 false in 9,615 replies, 89b9d3c79);
  - Q-449 (Linux pgrep counted a zombie; shutdown now waits for the exit it caused, 2b0473e56);
  - Q-447 (float32 KV fix; BOTH Macs); Q-429/431/433..440 (demo fixes, ea2f02977). Medium confidence, live-only: Q-431's banner under share,
    Q-434's delegate card, Q-436's remote load phases.

## Batch 4 → 3.0.74 (next)
1. When Q-428 reports (it has committed 111443cbf/93c0be18b/00e442205 plus its ledger, and is finishing), merge them into /tmp/merge-074 via ledger_resolve, with main merged in before the ff.
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
- The coordinator assigns Q ids; the next free id is Q-451. Agents use their own scratch folders.
- Never navigate the main window while an E2E runs. Kill pids, never killpg.
- Training: next round on the MacBook, ONLY on the owner's word (memory next-training-on-macbook).
