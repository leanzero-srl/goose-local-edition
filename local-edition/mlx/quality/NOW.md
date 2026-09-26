# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-26 19:4x · heartbeat: session cron 90b0083a (:04/:14/…/:54, expires 2026-10-03)

## Live
- Installed on both Macs: 3.0.51. PROVEN on it: Q-137 (Link over tailnet with the Funnel dead), Q-136 (split
  restores + answers), Q-135 (plain "OK", no reasoning block). Ladder 7/7 to 39k.
- Studio public Funnel still dead (Tailscale on the Studio needs the owner's password to restart) — not needed now.
- main = 22b5e2763 + Q-138 merged (0544ca3d6), NOT pushed yet: full gate running →
  /private/tmp/claude-501/-Users-mihaiperdum-Projects-goose/6fecca04-8d69-4ea2-b03d-d1da3d17162d/scratchpad/gate-q138/summary.txt
  (lines: fmt/clippy/tests/tsc/ui with exit codes). All 0 → push → CI → build 3.0.52.

## Running
- E2E #3c: tensor 27B split, jira brief → ~/goose-builds/quality/RU-2026-09-26-3c-split-tensor
  (r1.mjs, sampler, calls.py). GPU OWNER: this run. Watch: turn 0 secs (#4 Studio 170 s), no stall past 10.5k
  generated tokens, checker rows reasoning-off in calls.csv, rank logs ~/.local/state/goose/logs/distributed/.
- Agent: leanzero-link test determinism (worktree). NO GPU.

## Next actions (in order)
1. Gate summary all 0 → `git push origin main` → CI green → build 3.0.52 (Q-138) with a DONE watcher.
2. Tick E2E #3c every tick: read the last turn's WORDS; stop rules; after it ends → E2E-RUNS.md row, delete
   the run's memories (find ~/.config/goose/memory ~/.goose/memory -newermt @start), critic pass.
3. Then E2E #5b Flash pipeline (stdlib brief) + load.py 3 workers → proves Q-127/128/131/133/134.
4. Dispatch Q-139 (web-search writes docs/technical/ into the user's cwd) and Q-130 (Add node copy).
5. Link-test agent returns → merge → gate → push.

## Standing rules for every tick
- CI status, agent audit (processes, disk, pushes, ~/.config/goose), clean.sh orphan scan on both Macs.
- A build that is DONE is installed + split-start smoked in the same tick. Never install without the smoke.
- One GPU user at a time (named above). Nothing waits for the owner.
