# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-26 20:28 · heartbeat: session cron 90b0083a (:04/:14/…/:54, expires 2026-10-03)

## Live
- Installed on both Macs: 3.0.51. PROVEN on it: Q-137 (Link over tailnet with the Funnel dead), Q-136 (split
  restores + answers), Q-135 (plain "OK", no reasoning block). Ladder 7/7 to 39k.
- Studio public Funnel still dead (Tailscale on the Studio needs the owner's password to restart) — not needed now.
- main = b4b325848 pushed: Q-138, Q-130, Windows fix (CI green), Link tests deterministic (CI running).
- 3.0.52 build SCHEDULED waits on: E2E #3c ending (a release compile on the MacBook would skew rank 0's timing).
  Note for #3c's record: agents' cargo builds ran alongside (CPU load on rank 0's Mac).

## Running
- E2E #3c: tensor 27B split, jira brief → ~/goose-builds/quality/RU-2026-09-26-3c-split-tensor
  (r1.mjs, sampler, calls.py). GPU OWNER: this run. Watch: turn 0 secs (#4 Studio 170 s), no stall past 10.5k
  generated tokens, checker rows reasoning-off in calls.csv, rank logs ~/.local/state/goose/logs/distributed/.
- Agents (worktree, NO GPU): Q-139 web-search cwd; Q-141 tensor split streams nothing during a tool call; Q-140 load-flaky tests.
- E2E #3c turn 0: the agent call ran 21,741 tokens / 32 min with ZERO streamed (Q-141); stopped from the chat's Stop
  at 20:27 (engine released it within ~15 s). Q-114 proven live (2× the old hang point). Run continues: turn 1.

## Next actions (in order)
1. CI on cf48aaf46 green? (red → fix first). E2E #3c ends → build 3.0.52 (Q-138 + whatever merged) with a DONE watcher.
2. Tick E2E #3c every tick: read the last turn's WORDS; stop rules; after it ends → E2E-RUNS.md row, delete
   the run's memories (find ~/.config/goose/memory ~/.goose/memory -newermt @start), critic pass.
3. Then E2E #5b Flash pipeline (stdlib brief) + load.py 3 workers → proves Q-127/128/131/133/134.
4. Q-139 and Q-130 agents return → merge → gate → push (with Q-138 in 3.0.52 if in time).

## Standing rules for every tick
- CI status, agent audit (processes, disk, pushes, ~/.config/goose), clean.sh orphan scan on both Macs.
- A build that is DONE is installed + split-start smoked in the same tick. Never install without the smoke.
- Before pushing a merge that touches Rust: harness/wincheck.sh (Windows compile) — CI went red twice on Windows.
- One GPU user at a time (named above). Nothing waits for the owner.
