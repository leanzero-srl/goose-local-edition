# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-26 21:46 · heartbeat: session cron 90b0083a (:04/:14/…/:54, expires 2026-10-03)

## Live
- Installed on both Macs: 3.0.51. PROVEN on it: Q-114 (no stall past 2× the old hang, E2E #3c), Q-135, Q-136, Q-137.
- Building 3.0.52 from main 5baa065fa: Q-107 (tool loads keep the prefix), Q-138 (orphan MCPs), Q-130, Q-140,
  Q-141 (split streams tool calls), Q-142 (split keeps the prefix to the turn-context block). Log
  ~/goose-builds/release-3.0.52.log · DONE watcher running.
- E2E #3c stopped after turn 5 (recorded in E2E-RUNS.md; its memory deleted). GPUs idle until 3.0.52.
- Studio public Funnel still dead; Link runs over the tailnet (Q-137).

## Agents (worktree, NO GPU)
- Q-139 web-search writes docs/technical/ into the user's cwd.
- Q-143 pipeline split: take the turn-context tail + checkpoint before it (fork + pin).

## Next actions (in order)
1. 3.0.52 DONE → install.sh 3.0.52 → Link connected? → split-start.mjs --model 27B answers.
2. E2E #3d: tensor 27B, jira brief, on 3.0.52. Prove: calls read ≥90% cached after turn 1 (Q-142), a tool call
   streams with "writing a tool call to shell" (Q-141), load_tools keeps the cache (Q-107), turn times vs #3c
   (110/690/1380/516/924 s) and #4 Studio (170/81/1333 s). diskio.py on goosed (Q-115).
3. Then E2E #5b Flash pipeline (stdlib brief) + load.py 3 workers → Q-127/128/131/133/134 (+Q-143 once merged).
4. Critic pass (mlx-ux-critic) on 3.0.52 after #3d.
5. Agents return → read log → merge → gate (clippy, tests, wincheck.sh) → push → CI.

## Standing rules for every tick
- CI status, agent audit (processes, disk ≥ 30 GB, pushes, ~/.config/goose), clean.sh orphan scan on both Macs.
- A DONE build is installed + split-start smoked in the same tick. Never install without the smoke.
- Before pushing a Rust merge: harness/wincheck.sh. One GPU user at a time. Nothing waits for the owner.
