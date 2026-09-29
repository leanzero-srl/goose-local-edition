# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-29 16:0x (date) · runwatch.sh per run · vigil-shot.mjs every tick (READ the PNG)

## Live
- Installed: 3.0.79 on BOTH Macs at 15:5x. Split serving ("OK.").
- 3.0.79 proven live 16:00 (PROVE-3.0.79/, prove079.mjs):
  - Q-490: closing mid-turn asks (app-drawn dialog), Stop-and-close, app + engine alive.
  - Q-491: a folder-only window gets a chat.
  - Q-492: an email at the end sends.
  - Q-483/484/485: the sidebar.
- Still to prove on 3.0.79:
  - Q-488 (a Steer note to an idle chat: #3y turn 26);
  - Q-489 (the need-you menu);
  - Q-493 (an unanswered prompt line);
  - Q-494 (counts not years: watch #3y);
  - Q-496 (the chat follows the turn: watch #3y, data-following);
  - Q-486.
- E2E #3y ENDED at turn 25 (a harness crash after compaction, fixed 9d2bf826a; E2E-RUNS). Was: RUNNING since 16:01 — COMPACTED at turn 25 (214k → 59.8k); post-compaction calls warm (58–59k from cache) (turn 2: 1,955 s / 9 tools, against #3x's 8,349 s / 152): RU-2026-09-29-3y-split-tensor-coffee, session 20260929_19, the coffee brief (41 turns).
  r1 proves every send (SEND_LOST). Turn 1 LANDED (Q-492 proven under the brief) and saved both memories, but
  narrowed the privacy rule to this project (Q-506, model). Memory at 1 → recall at 33/39; note to the bakery chat at 26; needs-you at 8/12/15/21.
- E2E #3x STOPPED at turn 4 (E2E-RUNS): the Q-498 cache thrash with the owner's second chat; its memory turn was lost.

## 3.0.80 — INSTALLED on BOTH Macs 20:2x (split serving "OK."), main 8368ce115
- Carries:
  - Q-495, Q-497;
  - Q-498/502/508 (the split's cache across chats: say why + keep every chat's prefix + name chats by session id;
    NEW SPEC TAG, so BOTH Macs need 3.0.80);
  - Q-499/503 (sidecar readiness, live memory reads, lock unlock);
  - Q-500/501/504/507 (running state and Stop across windows, app-wide turns, schedules);
  - Q-505 (secrets redacted);
  - Q-466 (test races);
  - Q-511/512 (nested components).
- Gates:
  - g080 full: UI 4647, goose lib 2195, every acp test, clippy, dev gates 11, schema, Windows;
  - g080b: sidecar 427 + session_id 5 + clippy;
  - g080c: tsc + 199 + eslint.
- Install when #3y reaches a boundary we can stop at (it's at turn 10/41; the note turn 26 needs Q-488, already in
  3.0.79). Plan: let #3y run to its end on 3.0.79, then install 3.0.80 and prove live.
- Then prove live:
  - Q-502/508 (two chats, then a compaction);
  - Q-500/501/504/507;
  - Q-493/495;
  - Q-511/512;
  - Q-505 (scan-secrets).

## Toward 3.0.81 — main b661ce43b carries Q-515 + Q-516 (g081 green: goose lib 2196, agent 38, acp_* all, clippy)
- Q-517 (the 1.2–1.6 s before every provider call) is cutting; it joins, then release 3.0.81.
- The plan: install 3.0.80 now-built OR 3.0.81 (whichever is ready) when #3y ends; both need BOTH Macs.
- Q-514 (a side call evicting one chat's prefix) waits on the 3.0.80+ live measurement.

## CI
- Red 13:18Z: the RecipesView delete test hit its 5 s timeout (the vitest-under-load class). Q-466 is cutting
  (general-purpose): make the slow tests cheap, or give each its measured wait; never the global timeout.
- Q-499's sidecar port race is fixed on its branch.

## Queued / scheduled
- Q-398 and Q-425 QUEUED behind: fork access.
- Q-424 SCHEDULED waits on: a Studio ladder with the engine free.
- Q-270, Q-373, Q-205 SCHEDULED waits on: their named measurements.
- Q-452 waits on: a second red.

## Standing rules for every tick
- Check CI, agents, disk ≥ 30 GB (tight during release builds: g-targets and finished worktrees go first). Take a vigil screenshot and READ it. The next free id is Q-518.
- Merge via scratch plus ledger_resolve, then gate, ff, push.
- Nothing an E2E raises stays pending. Never navigate the main window while r1 runs.
- If the owner uses another chat mid-run, Q-498 thrash follows. Read the Engine glance; do not blame goose-in-one-chat.
- Kill pids, never killpg. Training ONLY on the owner's word.
