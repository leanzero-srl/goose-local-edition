# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-28 08:1x (date) · heartbeat cron 90b0083a + runwatch.sh per run

## Live
- Installed: 3.0.66 on both Macs. E2E #3o RUNNING (RU-2026-09-28-3o-split-tensor, 27B split; runwatch bbw3fu2rn):
  turns 0–2 clean (359 s / 156 s / 895 s, 25 tools at 76k), turn 3 in progress.
- Live-proven on 3.0.66: Q-223/241/242 (quit), Q-231, Q-232, Q-243, loops J1/J2. Q-240 prove FAILED → Q-276.
- Build 3.0.67 RUNNING from main e2fc8e0be (log ~/goose-builds/release-3.0.67.log). Carries since 3.0.66: L2c
  complete, Q-247 colours, Q-248/249/250/251/252/253 port holders, Q-237/246, Q-254/255 swap words, Q-256/258
  (tests no longer kill the live engine), Q-259 dialogs, Q-260 images to text-only engines, Q-275 CI, Q-278/279/280.
  Install after E2E #3o ends (never under a running E2E).

## CI
- Q-275 fix + batch pushed (8396ea93e, e2fc8e0be) — check it goes green.

## Agents (worktrees)
- Q-263..267 per-session working dir, on top of the Q-257 branch (one goosed per app) — they land TOGETHER.
- Q-271/272/273/274: residency load phase, the five §8.7 loader strings, Auto mismatch line, S4 chip menu.
- Queued: Q-276/277 (fit check counts goose's own leftover → Q-240 never runs; Engine tab stacks the refusal) —
  dispatch now that the port-holder agent has merged. Q-261/281 (model behaviour) measured on E2E #3o first.

## Next actions (in order)
1. E2E #3o: read turns by their words, apply stop rules; at its end install 3.0.67 → proves (Q-254 swap words,
   Q-252 Unmount refusal, Q-260 image placeholder, Q-278 J2 rerun, J4 with the fixed r5.mjs, Q-276 after its fix).
2. Merge agents via scratch branch + gate + ff main (the skill trap). Delete ~/goose-targets/<id> on merge.
3. Live critic walk: Nodes, Strategies, loop dialog + rail; L9 (loop harness + ≥5-tick E2E).

## Standing rules for every tick
- CI status, agent audit, disk ≥ 30 GB, clean.sh on both Macs; kill stale background shells (self-matching pgrep).
- Merge via scratch branch, gate, ff main (projsync pushes main every 15 min). Tests never touch port 8090.
- One GPU user at a time. Nothing waits for the owner.
