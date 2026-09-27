# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-27 11:26 (real clock) · heartbeat cron 90b0083a (silent 00:35→07:50!) + runwatch.sh per run

## Live
- Installed on both Macs: 3.0.56 (3.0.55 + engine/chat surfaces Q-148..Q-158, Q-160 slots, Q-162, open chat pinned).
- E2E #3e (3.0.55, 27B tensor, sampled) FAILED unseen for 7 h: ONE answer of 221,604 tokens = 324 × identical
  write+mkdir (Q-161), then the hang rule killed the split mid prompt-cache search (Q-162), then r1 fired 29 dead
  turns (fixed: a notice ends the round). The cron heartbeat did not fire 00:35→07:50 — runwatch.sh now wakes on
  every stop rule for every run.
- 27B tensor split is UP for the Q-161 investigation agent (sole GPU user; direct requests on 8091).
- lz-ppm + CogniRunner sessions run the live-state method (lz-ppm: 5 defects found, B-119..B-125).

## Agents
- none running. All merged on main 30d5b18aa (full gate green): Q-161 guard + Q-164 last-token reuse, Q-162, the 18
  reconciled open rows (composer Q-6/7/9/21/23/60 · engine tab/Run it/Link/tray Q-20/22/25/26/28/37/38 · core
  Q-14/18/24 · single-engine SRPF Q-103 lz.10).
- BUILDING 3.0.57 from 30d5b18aa (cargo started 11:22) · watcher btj4twspz. Both Macs must run it.
- LOAD 26e RUNNING on 3.0.56 Flash pipeline (fork with Q-145 aging + Q-160 slots): 3×30k + canary/60 s, 20 min;
  watcher b14y17s37 fires at 6 canaries or a failure. Pass = steady-state canaries in seconds (26d: 353/297 s).

## Next actions (in order)
1. Q-161 + Q-162 + chat merge land → 3.0.56 (with engine surfaces + Q-160) → install → split-start 27B → E2E #3f
   (jira brief) WITH runwatch.sh → prove Q-161 (no runaway), Q-162, Q-141/142/107 and livecheck green (Q-147).
2. Live critic round 2 during #3f (needs-you card, Active now, Q-148 confirm dialog seen in the app).
3. Flash pipeline load 26e (Q-145 aging + Q-160 slots): canaries every 60 s stay ~seconds in steady state.
4. Then E2E #5b Flash pipeline (stdlib brief).

## Standing rules for every tick
- CI status, agent audit (processes, disk ≥ 30 GB, pushes, ~/.config/goose), clean.sh orphan scan on both Macs.
- A DONE build is installed + split-start smoked in the same tick. Never install without the smoke.
- Before pushing a Rust merge: harness/wincheck.sh. One GPU user at a time. Nothing waits for the owner.
