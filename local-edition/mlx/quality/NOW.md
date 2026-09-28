# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-28 05:0x (date) · heartbeat cron 90b0083a + runwatch.sh per run

## Live
- Installed: 3.0.65 on both Macs; split up. LIVE TESTER agent is the only GPU user (J3 strategy swap, J4 thrash
  probe, Q-231 wait, Q-232 crop). E2E #3o waits for it.
- Build 3.0.66 RUNNING from main 005e2b9fb (log ~/goose-builds/release-3.0.66.log; watcher in background).
  Carries: Q-232, L10 wake, L2b on_prompt door, L4 start dialog + composer Loop slot, Q-233/234/235, Q-236/238
  (turnWait words), Q-240/245 (port holders named/reaped per proof), Q-241/242/243 (quit waits at before-quit;
  mesh stops last; fixtures die with their test).
- INSTALL 3.0.66 only after the live tester reports (it drives the running app). Then prove live: quit with the
  split up (main.log 'waiting for 1 attached backend(s)' → 'every goose serve backend has exited'; goosed stops on
  SIGTERM; 'told 1 peer(s)'; rank 1 verified gone; no goosed after the app pid), Q-240 (kill -9 goosed while
  mounted → relaunch → Mount reaps its own leftover), loop start dialog + one tick live (J1/J2).

## CI
- GREEN through e71a16d2c; 005e2b9fb (L2b+L4+quit+Q-240 merges) pushed — check next tick.

## Agents (worktrees)
- L2c + Q-239 (loader priority, tick marker edges, kind header) · Q-237/246 (glance node name, leaving rows,
  strip turnWait) · Q-247 (error text loses to ink — the mechanism) · Q-248 (swarm fast path adopts leftovers)
  · live tester (no worktree).
- Queued behind the Q-237/246 panel agent: Q-249 (stray listener line names its holder).

## Next actions (in order)
1. Live tester report → rows + dispatch → install 3.0.66 → quit/Q-240 live proves → E2E #3o.
2. Merge agents as they land (read log, gate crate-wide, wincheck, push).
3. Live critic walk of Nodes, Strategies, loop dialog + rail on 3.0.66; then L9 (loop harness + ≥5-tick E2E).

## Standing rules for every tick
- CI status, agent audit, disk ≥ 30 GB (42 GB after a target/debug sweep), clean.sh on both Macs.
- A DONE build is installed + split-start smoked in the same tick — unless a live tester holds the app.
- Merged worktrees: `git worktree remove -f -f` (never rm under .claude/). Crate-wide lib tests before a push.
- Before pushing a Rust merge: harness/wincheck.sh (serialized). One GPU user at a time. Nothing waits for the owner.
