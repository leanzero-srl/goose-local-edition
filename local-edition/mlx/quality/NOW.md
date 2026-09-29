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

## Gate / merge
- merge-487-488 at ~/goose-targets/wt-m487 (1813810f2 = main + Q-487 204457a56 + Q-488 5a8d72d49). The full gate
  runs at background priority (taskpolicy -b), target g079 → /tmp/g079.out.
  - On green: merge Q-489 (0d11170ec, full vitest green in its worktree), rerun the UI part, ff main, push →
    release 3.0.79.
- Q-483..486 landed (4865e6dd3). The merge gate's 11 vitest reds were load timeouts (load 74): all 5 files green
  alone, and main failed the same StrategiesTab test under the same load.

## Agents (worktrees)
- Q-490 (SEVERE): closing a second window mid-turn kills the app, goosed and the engine, with no log (reproduced
  on 3.0.78 at 09:00:56Z). general-purpose agent, cutting. Lead: "1 window(s) attached" logged with 2 windows open.
- Q-491 DONE 078115bc8 (worktree-agent-aa019a3dc35cd9561): a folder-only /pair gets newSession=1 → PairRoute creates
  the chat in that folder; handleFileOpen now opens a folder as a new chat (a behaviour change). Merge after g079
  with Q-489; it touches main.ts createChat, and so may Q-490 (resolve there).
- Q-487, Q-488, Q-489: done, in the gate/merge above.

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
- Check CI, agents, disk ≥ 30 GB (101 GB). Take a vigil screenshot and READ it. The next free id is Q-492.
- Merge via scratch plus ledger_resolve, then gate, ff, push.
- Nothing an E2E raises stays pending. Never close a window whose chat is mid-turn (Q-490) until the fix ships.
- Never navigate the main window while an E2E runs. Kill pids, never killpg. Training ONLY on the owner's word.
