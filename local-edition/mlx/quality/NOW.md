# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-27 20:25 (date) · heartbeat cron 90b0083a + runwatch.sh per run

## Live
- Installed on both Macs: 3.0.60 (Q-182) — VERIFIED post-swap pids (studio 66645, macbook 35122); the 27B split
  engine restarted 20:18:13 (after the swap) and split-start answered "OK.".
- E2E #3h STOPPED at the turn-12 boundary (13/30 turns on 3.0.59; turn 5 alone 8,352 s) to install 3.0.60.
- E2E #3i RUNNING on 3.0.60 (27B tensor, jira brief, fresh session) · runwatch bf4ph09vl. Prove: no 0-cached
  full re-reads (Q-182), Q-181/161/164/177, livecheck green.
- BUILDING 3.0.61 from d08278dce (engine glance = the owner's PiP idea + Q-183 paths), private TMPDIR · watcher
  b82f7bmhy. Install at a #3i turn boundary, then a live critic round on the glance (dock, float, desktop mini window).

## Agents
- Q-184 (memory-skills-surgeon, worktree): MemoryServer::new() writes global memories to ~/.config/goose under
  GOOSE_PATH_ROOT → through Paths.

## Next actions (in order)
1. 3.0.61 DONE → install at a #3i turn boundary → split-start 27B → #3i continues (new session = #3j if needed).
2. Live critic round on 3.0.61: live states, needs-you card, engine glance on all three surfaces.
3. Merge Q-184 (Rust: wincheck.sh before push) → next build.
4. Flash pipeline E2E on lz-pipeline-qwen4.14 (Q-178/179/181 proof) + load run.

## Standing rules for every tick
- CI status, agent audit (processes, disk ≥ 30 GB, pushes), clean.sh orphan scan on both Macs.
- A DONE build is installed + split-start smoked in the same tick. Release builds use a private TMPDIR.
- Merged worktrees: `git worktree remove --force` (never rm under .claude/ — it prompts the owner).
- Before pushing a Rust merge: harness/wincheck.sh. One GPU user at a time. Nothing waits for the owner.
