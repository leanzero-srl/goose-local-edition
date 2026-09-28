# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-28 03:53 (date) · heartbeat cron 90b0083a + runwatch.sh per run

## Live
- Installed: 3.0.64 (both Macs). E2E #3n RUNNING on 3.0.64 (runwatch bc4qcxhfg).
- Build 3.0.65 (main 035970561: S5 loader, S3b, S8, S6 Strategies/Nodes, Q-231, L0/L7/L8, L2a runner, CI fixes)
  is NOTARIZING. Job b8jqgxqow on DONE: stops #3n, install.sh 3.0.65, split-start 27B, clean.sh.
- On main after 3.0.65 (→ 3.0.66): Q-232 split tool-arg types (91f46cfba parent), L10 wake broadcast.

## CI
- 035970561 + 91f46cfba in progress. Check every tick.

## Agents (worktrees)
- L2b on_prompt door (afe6cc7…) · L4 start dialog + queue (a164e1f…) · Q-233/234/235 split 500 + pipeline pin
  convergence + relay-test wait (a695922…, mlx-backend).
- Gates running: b9kgrpb5r (Q-232 merge: sidecar distributed/launch tests, clippy, wincheck).

## Next actions (in order)
1. 3.0.65 installed → live J3/J4 loader swaps (both Macs), Q-223 quit prove (no goosed orphan), Q-231 wait prove,
   Q-232 read_image.crop on the split → start E2E #3o.
2. Live critic walk (mlx-ux-critic) of Nodes page, Strategies, loop rail on 3.0.65.
3. Merge L2b, L4, Q-233..235 as they land (each gated) → L2c + L9 (harness + ≥5-tick loop E2E) → 3.0.66.
4. Open agent notes to file: tick.py lacks engine-mount-failed / swarm-holder-unregistered; floating window does
   not link the node; panel mirror for Q-231 status fields; goose-side kind header; leftover-uv port on startup.

## Standing rules for every tick
- CI status, agent audit (processes, disk ≥ 30 GB, pushes), clean.sh orphan scan on both Macs.
- A DONE build is installed + split-start smoked in the same tick. Release builds use a private TMPDIR.
- Merged worktrees: `git worktree remove -f -f` (never rm under .claude/ — it prompts the owner).
- Before pushing a Rust merge: harness/wincheck.sh. One GPU user at a time. Nothing waits for the owner.
