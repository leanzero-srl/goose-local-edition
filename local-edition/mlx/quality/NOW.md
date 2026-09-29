# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-29 12:0x (date) · heartbeat cron + runwatch.sh per run · vigil-shot.mjs every tick (READ the PNG)

## Live
- Installed: 3.0.78 on BOTH Macs 11:5x. Split restarted twice (the Q-490 deaths), serving "OK." at 12:0x.
- E2E #3x RUNNING since 12:02: RU-2026-09-29-3x-split-tensor-coffee, brief
  2026-09-29-5-coffee-checkout-double-charge-postmortem (41 turns).
  - needs-you at 8 / 12 / 15 (three at once) / 21;
  - chat search at 6 / 25 / 27 / 35;
  - a Steer note to the bakery chat at 26, and "what became of it" at 36;
  - memory saved at 1, used after compaction at 33 / 39;
  - the unattended-loop skill at 17.
  On 3.0.78 the note at 26 is EXPECTED to FAIL on Q-488 (the fix is not in this build).
  - Watch: r1 closes B's window after B's turn. If it closes it mid-turn, Q-490 kills the app.
- #3w's note: Steer never delivered (Q-488). The manual "Give it to goose now" door DELIVERED it (own_turn,
  marker in B, the reply acted on it: B checked its own PDF for the blank-page issue). No note left pending.

- Leftover to clear when #3x ends: "Jira DC to Cloud migration assessment" (20260928_19) still holds 2 needs-you
  cards from yesterday's run. Answer them through the UI and prove the outcome (owner rule: nothing stays
  pending). Not done during #3x, because its turn would share the split with the run.

## Gate / merge → 3.0.79
- g079 (/tmp/g079.out): the full gate on merge-487-488 (1813810f2 = Q-487 204457a56 + Q-488 5a8d72d49), background
  priority.
- g079b (/tmp/g079b.out): chained to start when g079 ends. UI gate on merge-079b (968b6b7e3 = merge-487-488
  + Q-489 0d11170ec + Q-491 078115bc8 + Q-490 70d84fae2), no conflicts.
- Both green → ff main to merge-079b, push, release 3.0.79. Install when #3x ends (or is stopped).
  Then prove Q-490 live: closerepro2 must click [data-testid=confirm-close-run-stop]; expect main.log
  "close held … chat turn(s) in flight", the app alive, no exit.
- Q-490's cause: the main process exit(7). A broadcast threw on the closing window (Electron 41: isDestroyed()
  is not enough), then the uncaught handler threw again. goosed saw stdin EOF → the engine stopped.

## Agents (worktrees)
- Q-492 (an "@" in the last word opens the file picker and swallows Enter, so #3x's turn 1 was LOST) and Q-493 (a
  turn killed with the app leaves no trace): panel-surgeon, cutting.
  - r1 now proves every send (SEND_LOST + one resend), from #3y on.
- #3x: turn 1 (the memory turn) never reached the chat, so its memory checks at 33/39 are VOID for this run.
- Q-494 (the reply check calls counts "years": "Total rows: 1999"): general-purpose agent, cutting.
- Q-487..491: done, all in merge-079b.

## Next
1. Every tick: read #3x's words (r1.out, events.log, turns.tsv, the latest turn PNG) + a vigil screenshot.
2. g079 green → 3.0.79. Install it only when #3x ends or is stopped under the stop rules.
3. Live walks on 3.0.78 while #3x runs, READ-ONLY (never navigate the main window):
   - the colours (Q-457/474–477/479);
   - Q-458..463, Q-467, Q-469..471, Q-473, Q-478, Q-480, Q-482.

## CI
- Green through 08:31. The later runs were in progress at 11:47.

## Queued / scheduled
- Q-398 and Q-425 QUEUED behind: fork access.
- Q-424 SCHEDULED waits on: a Studio ladder with the engine free.
- Q-270, Q-373, Q-205 SCHEDULED waits on: their named measurements.
- Q-466 waits on: a CI red. Q-452 waits on: a second red.

## Standing rules for every tick
- Check CI, agents, disk ≥ 30 GB (101 GB). Take a vigil screenshot and READ it. The next free id is Q-495.
- Merge via scratch plus ledger_resolve, then gate, ff, push.
- Nothing an E2E raises stays pending. Never close a window whose chat is mid-turn (Q-490) until the fix ships.
- Never navigate the main window while an E2E runs. Kill pids, never killpg. Training ONLY on the owner's word.
