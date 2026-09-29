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
- E2E #3y RUNNING since 16:01: RU-2026-09-29-3y-split-tensor-coffee, session 20260929_19, the coffee brief (41 turns).
  r1 proves every send (SEND_LOST). Turn 1 LANDED (Q-492 proven under the brief) and saved both memories, but
  narrowed the privacy rule to this project (Q-506, model). Memory at 1 → recall at 33/39; note to the bakery chat at 26; needs-you at 8/12/15/21.
- E2E #3x STOPPED at turn 4 (E2E-RUNS): the Q-498 cache thrash with the owner's second chat; its memory turn was lost.

## Toward 3.0.80 (main has Q-495 4503c521a + Q-497 23c06bb8b)
- Q-498 DONE a6d70abba (branch worktree-agent-afa856b427b453865, KEEP): says why a prompt went cold. Keeping both chats
  cached needs 29.07 GB against the 17.33 GB plan.
- Q-502 cutting ON a6d70abba (mlx-backend): hold a side call instead of evicting another chat's kept prefix. The
  replay keeps #3x warm at 17.33 GB.
- Q-499 DONE 6b4e8fcfb (branch worktree-agent-aec73c85bddb00a5e, KEEP): readiness requires the engine's own listener.
  The CI collision came from a sibling test's port.
- Q-503 cutting ON 6b4e8fcfb: the sidecar probe/machine test flakes.
- Q-500/501 DONE c900ee4ea (branch worktree-agent-adf3890d8dcce9f43, KEEP): running rows shared across windows, Stop
  from a second window, the banner reads the engine's request list. Full vitest 4,638 green.
- Q-504 (a loop/schedule turn that no window sent cannot be stopped): general-purpose, cutting ON c900ee4ea.
- Q-505 SECURITY: a plaintext CONTEXT7 key sits in goose memory evolve-goose-test-loop.txt, which recall injects into
  prompts. memory-skills-surgeon is cutting save/recall redaction plus a no-values scan. The OWNER decides on the existing
  entry: tell him, never print the value.
- When the agents land: one scratch branch (Q-502 includes Q-498; Q-503 includes Q-499; Q-500/501) → full gate → ff main
  → release 3.0.80 → install when #3y allows.

## CI
- The last red was Q-499's (a sidecar test port collision on Linux). Q-499 is fixed on its branch. Watch the next main runs.

## Queued / scheduled
- Q-398 and Q-425 QUEUED behind: fork access.
- Q-424 SCHEDULED waits on: a Studio ladder with the engine free.
- Q-270, Q-373, Q-205 SCHEDULED waits on: their named measurements.
- Q-466 waits on: a CI red naming one of its tests. Q-452 waits on: a second red.

## Standing rules for every tick
- Check CI, agents, disk ≥ 30 GB (72 GB). Take a vigil screenshot and READ it. The next free id is Q-507.
- Merge via scratch plus ledger_resolve, then gate, ff, push.
- Nothing an E2E raises stays pending. Never navigate the main window while r1 runs.
- If the owner uses another chat mid-run, Q-498 thrash follows. Read the Engine glance; do not blame goose-in-one-chat.
- Kill pids, never killpg. Training ONLY on the owner's word.
