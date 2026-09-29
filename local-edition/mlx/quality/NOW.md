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

## 3.0.79 — DMG BUILT 15:3x (RELEASE-EXIT=0), main 97d1e6f54
- Carries Q-483..486 (the sidebar), Q-487 (the question test), Q-488 (a Steer note to an idle chat), Q-489 (the
  need-you menu), Q-490 (a window close killed the app), Q-491 (a folder-only window), Q-492 (an in-word @
  swallowed Enter), Q-493 (a dead turn leaves a line), Q-494 (counts are not years), Q-496 (the chat follows the turn).
- Gates:
  - g079 Rust green; the one flake is Q-497.
  - g079b + g079d UI green (4603 passed).
  - g079c Rust green on the merged tree (lib 2190, cross_note, recall_wording, clippy).
- INSTALL PLAN: #3x is now measuring Q-498's cache thrash (turn 4 has run 2 h of 200k cold reads between the owner's
  Jira chat and #3x; turn 1 was lost). jiradone.sh (scratchpad) fires when the owner's turn in 20260928_19 ends →
  kill r1 (per pid) → install 3.0.79 on both Macs → split-start → E2E #3y with the same coffee brief (r1 proves
  each send). Then prove live:
  - Q-490: closerepro2, clicking [data-testid=confirm-close-run-stop]; the app stays up;
  - Q-488: a Steer note to an idle chat is taken;
  - Q-492: an email at the end of a message sends;
  - Q-496: a long chat keeps following;
  - Q-491: a folder-only window;
  - Q-493;
  - Q-489.

## Agents (worktrees)
- Q-495 4503c521a + Q-497 23c06bb8b LANDED on main 5262589e2 (gated in the worktree: turnWorking 42/42, the full
  vitest with 6 load timeouts green alone 168/168, acp_server_test 47/47). They ride in 3.0.80.
- Q-499 (CI red: sidecar healthy() false right after a slow start): general-purpose agent, cutting.
- Q-498 DONE a6d70abba (branch worktree-agent-afa856b427b453865, KEEP until merged): says why a prompt went cold
  (evicted_prefix / GOOSE_RANK_PREFIX_LOST). Both prefixes need 29.07 GB vs the 17.33 GB plan, so no memory fix.
- Q-502 (the no-memory fix: hold a side call instead of evicting another chat's kept prefix; the replay keeps #3x
  warm at 17.33 GB): mlx-backend, cutting, ON TOP of a6d70abba.
- Q-500/501 (a second window shows a running chat idle; the busy banner names the wrong chat): panel-surgeon,
  cutting.
- Q-503 (sidecar probe/machine test flakes): QUEUED behind Q-499 landing.

## #3x (running)
- Turn 1 (the memory turn) never reached the chat (Q-492), so its memory checks at 33/39 are VOID for this run.
- Turn 2 ran 8,349 s (152 tools): a determinism/timezone grind in the fake-log generator, ending 36/36 verified.
- Past 181k/262k after turn 3, compaction is near.
- 12:05:47Z: the owner answered 20260928_19's folder card ("Yes"), and that chat is working now (so the stale
  needs-you leftover is being handled by him). r1 holds (VIEW_AWAY). Each chat's calls evict the other's cache (Q-498).
- r1 proves every send from #3y on (SEND_LOST + one resend).

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
- Check CI, agents, disk ≥ 30 GB (101 GB). Take a vigil screenshot and READ it. The next free id is Q-504.
- Merge via scratch plus ledger_resolve, then gate, ff, push.
- Nothing an E2E raises stays pending. Never close a window whose chat is mid-turn (Q-490) until the fix ships.
- Never navigate the main window while an E2E runs. Kill pids, never killpg. Training ONLY on the owner's word.
