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
    - Turn 6: 852 s, 14 tools. identity-plan.js 309/55/36/30, every lead migrates, byte-identical on a re-run.
      But "resumable" was never exercised ("state: undefined rows checkpointed") and it was called verified,
      after an assert-then-retract about its own code → Q-451 (model behaviour, parked; forge-tuner c0cf851).
    - Turns 7–8: 340 s, then 105 s. 13/13 node:test pass. The totals and the 4 leads the rule saved are correct.
    - Turns 9–11: it could not interrupt an 82 ms run, said so plainly, and proved the resume path with a test
      instead (15/15). It found the official JCMA doc ("one account is always chosen as the main one (randomly)")
      and put it above the merge rule. Turn 12 (plan-projects.mjs) is running. No goose defect. Decode is 9.5–11.9 tok/s since the 23:30 flip.
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
- none running.

## Batch 4 → 3.0.74 — merge-074 (/tmp/merge-074, d750166f1) COMPLETE, GATE RUNNING
- Holds:
  - Q-417, Q-407, Q-423 (BOTH Macs), Q-426;
  - Q-429/431/433..440;
  - Q-447 (float32 KV; BOTH Macs), Q-449, Q-450;
  - Q-428/430/432 (b/o demo; break pass fixed 4).
  No ledger row lost; Q-444..446 unused.
- g074 full gate + g074b re-gate: ALL GREEN at da6f4628d.
  - Fixed on the branch: tsc b068352cf, and the seam da6f4628d (which is why clippy failed and lib tests were
    skipped).
  - The sidecar stray-listener test failed once under load and passed 5/5 alone plus the full suite → Q-452,
    scheduled on a second red.
- Q-441/442/443 MERGED (1bedb7b8f). Delta gate g074c → /tmp/g074c.out (own session): UI full, lib, acp_*,
  server, cli, clippy, dev gates, schema, wincheck.
- Green → ff main, push → release 3.0.74 (/tmp/rel074.sh) → install BOTH Macs at an #3u turn boundary →
  split-start smoke.
- Then live-prove:
  - Q-447 (decode ≥ 8 tok/s on cache-hit turns; entry bytes 32,768/token);
  - Q-428 (the demo strategy with its Chat role on "Use the next node" and on "Wait");
  - Q-426, Q-417, Q-450.
- Then tell the owner about Q-428.

## Queued / scheduled
- Q-427 QUEUED behind: Q-426 landing. Q-398 QUEUED behind: agent cap. Q-425 QUEUED behind: fork access.
- Q-424 SCHEDULED waits on: a Studio ladder (32k/64k/128k/176k peak Metal) with the engine free.
- Q-385 SCHEDULED waits on: the split free (the ' =>' logprob read).
- Ledger hygiene: Q-407 and Q-417 read 'open' but are DONE, and Q-164/181/209/210/221 have read 'cutting' since
  09-27. Reconcile them at the batch-4 merge.

## Standing rules for every tick
- Check CI, agents, disk ≥ 30 GB (126 GB now) and clean.sh. Merge via scratch plus ledger_resolve, gate, ff, push.
- The coordinator assigns Q ids; the next free id is Q-453. Agents use their own scratch folders.
- Never navigate the main window while an E2E runs. Kill pids, never killpg.
- Training: next round on the MacBook, ONLY on the owner's word (memory next-training-on-macbook).
