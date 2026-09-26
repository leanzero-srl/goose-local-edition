# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-26 19:1x · coordinator: the Claude session running the goose MLX loop (cron tick ~10 min)

## Live
- Installed on both Macs: 3.0.50 (broken for the 27B tensor split: Q-136 readiness 503 — fixed on main, not installed).
- Building: 3.0.51 from main 2d38c8ee8 (Q-136 split start + formation handshake, Q-137 Link over tailnet,
  link socket-test flake fix). Log ~/goose-builds/release-3.0.51.log · watcher running.
- CI (gh -R leanzero-srl/goose-local-edition -b main): last known red = flaky leanzero-link test (being fixed).
- LeanZero Link: Studio's public Funnel dead since ~18:3x (Tailscale on the Studio, not goose); Q-137 routes around it.
- Studio LAN moved to 192.168.10.x (Tailscale logged it). Owner closed both goose apps at ~19:0x.

## Agents running (one writer per tree)
- Q-138 orphaned bundled MCPs spin at 100% forever (6 found, ~600% CPU) — MCP exit on stdin close +
  goose teardown + startup reaper + clean.sh listing. Worktree agent. NO GPU.
- leanzero-link test determinism (20 clean runs). Worktree agent. NO GPU.

## Next actions (in order)
1. 3.0.51 DONE → install.sh 3.0.51 (REPO=goose-rel) → Link tab connected? → split-start.mjs --model 27B MUST
   answer before anything else (the 3.0.50 lesson: never skip the smoke).
2. E2E #3c: tensor 27B, jira brief (RU-2026-09-26-3c-split-tensor) — proves Q-114 (past 10.5k tokens),
   Q-135 (turn 0 ≈ Studio's 170 s, no xhigh thinking), Q-132 (checker reasoning off, no block).
3. Then E2E #5b: Flash pipeline, stdlib brief — proves Q-127/128/131/133/134.
4. Merge Q-138 + link-test branches → full gate on merged main → push → CI green → 3.0.52.
5. After each round: critic pass (mlx-ux-critic) on the new build; findings → agents at once.

## Standing rules for every tick
- CI status, agent audit (processes, disk, pushes, ~/.config/goose), orphan scan (clean.sh) on both Macs.
- A build that is DONE is installed + smoked in the same tick. Nothing waits for the owner.
- Only one agent/run may use the GPUs at a time; say who in "Agents running".
