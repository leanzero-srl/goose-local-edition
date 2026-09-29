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
- E2E #3y RUNNING since 16:01 (turn 2: 1,955 s / 9 tools, against #3x's 8,349 s / 152): RU-2026-09-29-3y-split-tensor-coffee, session 20260929_19, the coffee brief (41 turns).
  r1 proves every send (SEND_LOST). Turn 1 LANDED (Q-492 proven under the brief) and saved both memories, but
  narrowed the privacy rule to this project (Q-506, model). Memory at 1 → recall at 33/39; note to the bakery chat at 26; needs-you at 8/12/15/21.
- E2E #3x STOPPED at turn 4 (E2E-RUNS): the Q-498 cache thrash with the owner's second chat; its memory turn was lost.

## Toward 3.0.80 (main has Q-495 4503c521a + Q-497 23c06bb8b)
- Q-498 DONE a6d70abba (branch worktree-agent-afa856b427b453865, KEEP): says why a prompt went cold. Keeping both chats
  cached needs 29.07 GB against the 17.33 GB plan.
- Q-502 DONE 9cb716449 (branch q502, contains Q-498): every chat's prefix kept; #3x replay 0 → 199,798 cached. BOTH
  Macs need the same build (a new spec tag). Q-508 (key conversations by session id, not tokens): cutting ON 9cb716449.
- Q-499 DONE 6b4e8fcfb (branch worktree-agent-aec73c85bddb00a5e, KEEP): readiness requires the engine's own listener.
  The CI collision came from a sibling test's port.
- Q-503 DONE 43f3431e1 (branch q503, contains Q-499): two PRODUCT bugs. The memory reading was rate-limited stale
  (host_statistics64 → sysctl live), and a refused load lock stayed held by a forked child (explicit unlock).
- Q-500/501 DONE c900ee4ea (branch worktree-agent-adf3890d8dcce9f43, KEEP): running rows shared across windows, Stop
  from a second window, the banner reads the engine's request list. Full vitest 4,638 green.
- Q-504 DONE 0d28abcf2 (branch q504, contains Q-500/501): a window's cancel reaches app-wide and link-bound turns,
  never another window's; Stop plus a "Running in the background" bar. Q-507 DONE 3cfd23a94 (q507): scheduled runs read Running everywhere, and any window's Stop stops them. Q-509
  (Link advertises Idle during a schedule) waits on 3.0.80.
- Q-505 DONE 65126cc51 (worktree-agent-acc78227175574ec2): secrets redacted on save/import/read/recall, plus
  `goose memory scan-secrets`. The Context7 key WAS recalled into a turn before the fix, so the OWNER decides whether to
  rotate it and delete the entry (told 17:1x). Q-510 (the desktop editor writes raw) is queued behind 3.0.80.
- 3.0.80 merge set (branches): q508 (⊃ q502 ⊃ Q-498), q503 (⊃ Q-499), q507 (⊃ Q-504 ⊃ Q-500/501), Q-505's branch,
  Q-466's branch → one scratch → full gate → ff main → release → install when #3y allows (BOTH Macs: new spec tag).

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
- Check CI, agents, disk ≥ 30 GB (40 GB after a cleanup at 16:5x). Take a vigil screenshot and READ it. The next free id is Q-511.
- Merge via scratch plus ledger_resolve, then gate, ff, push.
- Nothing an E2E raises stays pending. Never navigate the main window while r1 runs.
- If the owner uses another chat mid-run, Q-498 thrash follows. Read the Engine glance; do not blame goose-in-one-chat.
- Kill pids, never killpg. Training ONLY on the owner's word.
