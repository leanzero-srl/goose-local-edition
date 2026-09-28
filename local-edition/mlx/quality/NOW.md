# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-28 06:27 (date) · heartbeat cron 90b0083a + runwatch.sh per run

## Live
- Installed: 3.0.65 on both Macs. LIVE TESTER agent holds the app: J3 FAIL (words/records — Q-254..Q-258),
  J4 no starvation / no swap mid-reply; finishing Q-231 + Q-232 (red control; Q-232 not in 3.0.65), then restore.
- 3.0.66 BUILT + notarized (goose-rel/ui/desktop/out/make/Goose-Swarm-3.0.66.dmg, main 005e2b9fb) — INSTALL the
  moment the tester reports. Then: quit prove (Q-241/242), Q-240 kill -9 prove, loop start + one tick (J1/J2),
  rerun J4 → start E2E #3o.
- On main after 3.0.66 (→ 3.0.67): L2c part 1 + Q-239 refuted, Q-248, Q-250, Q-249 banner, Q-237/246, Q-247.

## CI
- 2b6441ccf RED: Q-248 + Q-249 merged green alone, did not compile together, projsync pushed before the gate.
  Fixed e423143b8 (gated: sidecar, swarm_engine 52, mlx_engine 10, clippy, schema, wincheck) — CI running.
- RULE NOW: merge agent batches in a scratch worktree, gate there, then fast-forward main (skill trap).

## Agents (worktrees)
- L2c rest: mac_wide.rs (same-way yield across processes, WayHeld) · Q-251/252/253 (restart-goose step, Unmount
  refuses a foreign engine, holder cache) · Q-258/256 (silent single-engine death mid-prefill, load row) ·
  Q-254/255 (swap words §8.7, chip node names) · Q-257 one goosed per app shared by all windows (design chosen, 75%).
- Live tester (no worktree).

## Next actions (in order)
1. Tester report → rows → install 3.0.66 → live proves → E2E #3o.
2. Merge agents as they land, batch-gated in a scratch worktree → build 3.0.67.
3. Live critic walk: Nodes, Strategies, loop dialog + rail; then L9 (loop harness + ≥5-tick E2E).

## Standing rules for every tick
- CI status, agent audit, disk ≥ 30 GB (77 GB now), clean.sh on both Macs.
- A DONE build is installed + split-start smoked in the same tick — unless a live tester holds the app.
- Merged worktrees: `git worktree remove -f -f` (never rm under .claude/). Crate-wide lib tests before a push.
- Before pushing a Rust merge: harness/wincheck.sh (serialized). One GPU user at a time. Nothing waits for the owner.
