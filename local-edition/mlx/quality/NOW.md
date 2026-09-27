# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-27 20:53 (date) · heartbeat cron 90b0083a + runwatch.sh per run

## Live
- Installed on both Macs: 3.0.61 (engine glance + Q-183) — VERIFIED post-swap pids (studio 97418, macbook 10693);
  the split restored itself on launch and answered "OK." (split-start now waits for a restore in flight).
- E2E #3h stopped at turn 12 (3.0.59) · #3i stopped at turn 3 (3.0.60: 336/53/666/681 s; Q-185 found live; the
  view was moved to the Engine page twice → Q-186).
- E2E #3j RUNNING on 3.0.61 (27B tensor, jira brief; r1 with VIEW_RETURNED + inode call keys) · runwatch b7snujtmq.
  Prove: no 0-cached full re-reads (Q-182), Q-181/161/164/177, livecheck green; then the live critic round on the
  engine glance (dock, float, desktop mini window over another app).

## CI
- Green again from dc701189c on (the title-test flake fixed at the cause, fa15f595d).

## Agents
- Q-185 MERGED 96bcc680e (every background call carries its kind; card/Run it/sidebar/chat say 'Checking the
  reply'); gate: tsc, vitest 3,184, eslint ts/tsx, i18n, wincheck green. Ships in 3.0.62 → prove live on #3j's
  successor (livecheck counts the background state as marked).
- OWNER BATCH 2026-09-27 (Q-189..196): lane D ARCHITECT (read-only + CDP walk + research) → DESIGN-NODES-AND-STRATEGIES.md:
  Swarm Settings → left nav, My Macs → LeanZero MLX tab, node cards at a glance, virtual node definitions,
  STRATEGIES (roles planning/execution/testing/frontend/backend, primary/secondary, weights, MLX load/unload).
  On its return: adversarial review of the design, then dispatch its file-disjoint slices at once (owner: design
  AND implement, lots of testing). Lane A+B: edit diffs in the card (user-only content) + right Changes rail.
  Lane C: LeanZero pill + hitbox, Report a problem → form to office@leanzero.net + Discord.
- Q-185 (worktree): background requests (fact check, title, label…) carry their kind; engine card, Run it and
  sidebar agree on 'checking the reply'; serving label keeps the session suffix.
- Q-184 (memory-skills-surgeon, worktree): MemoryServer::new() writes global memories to ~/.config/goose under
  GOOSE_PATH_ROOT → through Paths.

## Next actions (in order)
1. 3.0.61 DONE (glance + Q-183) → install at #3i turn-3 boundary (watcher b3ugzqvvu) → split-start 27B → #3i continues (new session = #3j if needed).
2. Live critic round on 3.0.61: live states, needs-you card, engine glance on all three surfaces.
3. Merge Q-184 (Rust: wincheck.sh before push) → next build.
4. Flash pipeline E2E on lz-pipeline-qwen4.14 (Q-178/179/181 proof) + load run.

## Standing rules for every tick
- CI status, agent audit (processes, disk ≥ 30 GB, pushes), clean.sh orphan scan on both Macs.
- A DONE build is installed + split-start smoked in the same tick. Release builds use a private TMPDIR.
- Merged worktrees: `git worktree remove --force` (never rm under .claude/ — it prompts the owner).
- Before pushing a Rust merge: harness/wincheck.sh. One GPU user at a time. Nothing waits for the owner.
