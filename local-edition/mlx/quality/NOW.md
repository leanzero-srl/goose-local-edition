# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-27 08:10 · heartbeat cron 90b0083a (silent 00:35→07:50!) + runwatch.sh per run

## Live
- Installed on both Macs: 3.0.55 (Q-146 instrument, Q-159 sampling + guard, needs-you + Q-147 states, Q-145 aging).
  NOT yet in a build: engine surfaces Q-148..Q-157, Q-160, CI flake fixes → 3.0.56.
- E2E #3e (3.0.55, 27B tensor, sampled) FAILED unseen for 7 h: ONE answer of 221,604 tokens = 324 × identical
  write+mkdir (Q-161), then the hang rule killed the split mid prompt-cache search (Q-162), then r1 fired 29 dead
  turns (fixed: a notice ends the round). The cron heartbeat did not fire 00:35→07:50 — runwatch.sh now wakes on
  every stop rule for every run.
- 27B tensor split is UP for the Q-161 investigation agent (sole GPU user; direct requests on 8091).
- lz-ppm + CogniRunner sessions run the live-state method (lz-ppm: 5 defects found, B-119..B-125).

## Agents (worktree)
- Q-161 runaway answer on the tensor split — GPU replay on 8091, find the mechanism (restarted answer vs in-answer loop).
- Q-162 hang rule counts rank-0 CPU as progress + fast prefix search on 259k prompts.
- MERGE of the chat-surfaces branch (Q-151..Q-153, Q-158) onto main — conflicts in server.rs / openai.rs / sdk index.

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
