# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-28 00:37 (date) · heartbeat cron 90b0083a + runwatch.sh per run

## Live
- Installed: 3.0.64 (both Macs; install.sh reaped the 3.0.63 goosed orphan — that app predates Q-223). Split "OK".
  E2E #3n RUNNING on 3.0.64 · runwatch. FIRST QUIT ON 3.0.64 = the live prove of Q-223/Q-229 (no orphan).
- CI RED on origin: Windows (S8 used cfg(unix) goose_sidecar::holders in goose-cli) + Linux (engine.rs:3162 a
  fast-failing load lost its stderr words). Fix agent running. wincheck.sh now compiles goose-cli + goose-server.
- Merged on main, not yet in a build: S5 loader (low-medium, needs J3/J4 live), S3b router gaps, S8, S6
  (Strategies UI + Nodes page wired), Q-231 (fact checks no longer hold a turn 38 s; needs both Macs), L8.
  S6 gate b92pnvdnt running. Next build 3.0.65 once CI is green → then J3/J4 + a live critic walk of Nodes.

## CI
- RED (see above) — fix in flight.

## Agents
- CI fix (Windows gating + stderr race).
- Loops: L0 DONE on its branch (held until CI is green) → then L2a, L3, L4r, L7, L5 in parallel.

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
