# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-29 11:5x (date) · heartbeat cron + runwatch.sh per run · vigil-shot.mjs every tick (read the PNG)

## Live
- Installed: 3.0.76 on BOTH Macs. 3.0.78 DMG BUILT (RELEASE-EXIT=0, ~/goose-builds/release-3.0.78.log) = main
  183c1f7b4 (3.0.77 + Q-482). Install on both Macs → split-start smoke, AS SOON AS B's note turn (below) ends.
- E2E #3w COMPLETE 37/37 (E2E-RUNS). Needs-you proven: 9 cards, all delivered and cleared. Decode median 10.7 tok/s.
- #3w's note (A 20260929_12 → B 20260928_47 Harbourline):
  - deliver-pending clicked Steer 08:40. It never delivered (Q-488, reproduced: B shown and idle for 180 s,
    still waiting).
  - Manual door "Give it to goose now" 08:45:36 → delivered own_turn. B's turn is running; notegive.mjs
    (scratchpad) proves the marker and the reply use, and the A card, into the round's events.log.

## Agents (worktrees)
- Q-483..486 sidebar (aecb96c2c): merged with main on branch merge-q483 (cbb7794f5, ledger resolved). UI gate
  running (/tmp/g483.out). On green: ff main, push, then it rides in 3.0.79.
  - Not fixed: the "Waiting for you" menu shows the raw question text → file a row.
- Q-487 open_question false positive ("which" in a statement → "Noticed your answer"): memory-skills-surgeon, cutting.
- Q-488 Steer never reaches an idle chat: general-purpose agent, cutting (the cause is unknown: showing / offer /
  busy-at-load).
- #3x brief: goose-task-author writing briefs/2026-09-29-5-*.json (needs-you ×3, a note to the bakery chat with
  Steer, chat search, compaction, a skill).

## Next
1. B's turn ends → install 3.0.78 (REPO=~/Projects/goose-rel zsh install.sh 3.0.78) on both Macs → split-start.
2. Start E2E #3x on 3.0.78 with the new brief and r1 (acts on notes now). Q-488 will show as a FAIL line
   until it lands, which is expected.
3. Live walks on 3.0.78:
   - the app-drawn update and quit dialogs (Q-472);
   - the colours (Q-457/474–477/479);
   - Q-458..463, Q-467, Q-469..471, Q-473, Q-478, Q-480, Q-482.
4. Q-487 + Q-488 + merge-q483 → one gate → 3.0.79.

## CI
- The main pushes of 08:30 were in progress at 11:38. Check them each tick.

## Queued / scheduled
- Q-398 and Q-425 QUEUED behind: fork access.
- Q-424 SCHEDULED waits on: a Studio ladder with the engine free.
- Q-270, Q-373, Q-205 SCHEDULED waits on: their named measurements.
- Q-466 waits on: a CI red. Q-452 waits on: a second red.

## Standing rules for every tick
- Check CI, agents, disk ≥ 30 GB (106 GB now). Take a vigil screenshot and READ it.
- Merge via scratch plus ledger_resolve, then gate, ff, push. The next free id is Q-489.
- Nothing an E2E raises stays pending: every card answered, every note delivered, outcomes proven.
- Never navigate the main window while an E2E runs. Kill pids, never killpg.
- Training: ONLY on the owner's word.
