# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-27 23:07 (date) · heartbeat cron 90b0083a + runwatch.sh per run

## Live
- Installed: 3.0.63 (both Macs; the MacBook's 3.0.62 needed SIGKILL — Q-229, the floating window held the quit open).
  Split up "OK." · E2E #3m RUNNING on 3.0.63 · runwatch. Disk 63 GB free (target/debug/incremental cleared).
- Merged + pushed since 3.0.63's cut: S0, S7, Q-224/226 (floating window), Q-211/212 · merged, gate b7bc4csxo: S3
  (chat routing to node/strategy), L1 (Recipes & loops removed). Next build 3.0.64 once S2/S5 land or the owner asks.

## CI
- Green through c4639cebf.

## Agents
- Nodes: S2 cards + New node (+ S7 leftovers) · S5 loader + holders (+ kind user|tick, S3's two contract items).
- Loops: design REVISION (17 review findings; L1+L6 cleared) · L6 shared clock.
- Q-223 + Q-229 app quit orphans / hang · Q-230 keep-awake does nothing.

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
