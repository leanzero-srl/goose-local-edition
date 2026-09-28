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
      and put it above the merge rule. Turns 12–13: plan-projects 13/13, refresh.sh 28/28.
      Turn 14 ("just the command" for CPU use) got NO command → Q-453, model behaviour; the stream and rank were
      checked intact. Turn 15 (client .docx): the leanzero-documents
      create-doc tool wrote every markdown TABLE as prose (0 `<w:tbl>`) while reporting tables:true → Q-454, agent
      dispatched. The model caught it itself by unzipping the file.
    - Turns 16–17: it rebuilt the docx with real tables and renamed the owners. Q-451-class self-contradiction
      again ("Zero occurrences of 'Mihai' is not what I asked for"), then it corrected itself. Context is
      173k/262k, so compaction is near: watch that Q-357 keeps the work folder. Turns 18–19: PDF made (3 pages); its own cross-check
      caught a made-up number in the report (3,728 vs 4,328) and fixed both files. Turn 20 (AUDIT): 1625 s, 24 tools, self-inflicted rewrites. Turn 21: the audit caught 6 of 7 planted defects
      (its reply said "Four of the six"). Turns 22–25: README written and followed from clean; 37/37 pass.
      Q-455 FOUND: goose's `<compaction>~Nk tokens remaining` makes the model cut work ("Compaction is at about
      1k, so I'll leave the table as it stands"). It is VA-107's twin for chats; agent dispatched.
      COMPACTION at 02:26–02:31 (209,739/262,144):
      - Q-342 PROVEN: the summary request read 209,120 of 217,026 from cache.
      - Q-347 PROVEN: the next request read the 41,780-token head from cache.
      - Right after, decode was 4.0 tok/s again (the Q-447 float32 promotion; fixed in 3.0.74), back to 11 by 02:35.
      Turn 26 (Standard vs Premium): 1,853 s, 37 tools; section 6 added and the PDF regenerated. Turn 27 (email)
      is running; then 28 (recall) and 29 (status note) → #3u ENDS → install 3.0.74.
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
- none. Q-455 ON MAIN 8a7f8a27a (gate green) for 3.0.75, with Q-454 and the Linux lint fix.

## 3.0.74 = main edd009c6b — DMG BUILT 01:5x (notarized; ~/goose-builds/dmg + the Studio's ~/Downloads)
- Install on BOTH Macs AFTER #3u ends (10 turns left at 02:00, incl. the turn-28 recall check). Installing
  mid-run would kill the split's rank 1. Then split-start smoke → E2E #3v on 3.0.74.
- CI Lint went RED after landing: a Linux-only string slice in Q-407's process_groups.rs:168 (the Mac gate
  never lints Linux cfg). Fixed on main 7f3368173 (CI green again) for 3.0.75; the macOS build is unaffected.
- Contents:
  - Q-417, Q-407, Q-423 (BOTH Macs), Q-426;
  - Q-428/430/432 and Q-441/442/443 (don't-interrupt option + follow-ups);
  - Q-429/431/433..440 (demo fixes);
  - Q-447 (float32 KV — BOTH Macs, new tag);
  - Q-449, Q-450.
- Gates g074 + g074b + g074c all GREEN. Two seams were fixed on the branch: b068352cf and da6f4628d.
- Merged worktrees removed. Disk: 128 GB.
- Next:
  1. The DMG lands → install on BOTH Macs at an #3u turn boundary (the new tag refuses mixed versions) →
     split-start smoke.
  2. Live-prove:
     - Q-447: decode ≥ 8 tok/s on a cache-hit turn, and entry bytes = 32,768/token in RANK_ADMISSION;
     - Q-428: set the demo strategy's Chat role to "Use the next node", then to "Wait", and re-run the
       shared-Mac case;
     - Q-426, Q-417, Q-450.
  3. Tell the owner about Q-428.
  4. Q-454 is ON MAIN beff2102e and goes into 3.0.75. Cause: a body starting at `## ` collapsed into ONE
     heading, which lost tables, lists and quotes. formattingQuality is now read from the written file.
     Independently re-checked: the pinned tarball plus the patch plus the lock gives 11/11.

## Queued / scheduled
- Q-427 QUEUED behind: Q-426 landing. Q-398 QUEUED behind: agent cap. Q-425 QUEUED behind: fork access.
- Q-424 SCHEDULED waits on: a Studio ladder (32k/64k/128k/176k peak Metal) with the engine free.
- Q-385 SCHEDULED waits on: the split free (the ' =>' logprob read).
- Ledger hygiene: Q-407 and Q-417 read 'open' but are DONE, and Q-164/181/209/210/221 have read 'cutting' since
  09-27. Reconcile them at the batch-4 merge.

## Standing rules for every tick
- Check CI, agents, disk ≥ 30 GB (126 GB now) and clean.sh. Merge via scratch plus ledger_resolve, gate, ff, push.
- The coordinator assigns Q ids; the next free id is Q-456. Agents use their own scratch folders.
- Never navigate the main window while an E2E runs. Kill pids, never killpg.
- Training: next round on the MacBook, ONLY on the owner's word (memory next-training-on-macbook).
