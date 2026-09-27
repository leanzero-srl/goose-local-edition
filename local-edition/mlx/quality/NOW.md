# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-27 22:17 (date) · heartbeat cron 90b0083a + runwatch.sh per run

## Live
- Installed: 3.0.62 (both Macs, verified post-swap pids; diffs + Changes rail, LeanZero pill, report form, Q-185).
  After install Link was blocked by the 3.0.61 goosed that survived the quit (Q-223): orphans killed per pid,
  Retry connect → tailscaled 55892, split up "OK". install.sh + clean.sh now reap/scan orphan goosed/tailscaled.
- E2E #3l RUNNING on 3.0.62 (27B tensor, jira brief) · runwatch. #3k stopped mid-turn 7 for the owner's install.
- BUILDING 3.0.63 from 2b4e26900 (S1 nav, glance Q-215–218, Q-220/221, Q-211/212, path fixes) · release-3.0.63.log.
- leanzero.net Q-209/210 live.

## CI
- Green through dc701189c; merges since gated locally (tsc, vitest 3,243, eslint, i18n, cargo check, wincheck).

## Agents
- S0 nodes contract + store (lane D; unblocks S2/S3/S5, S7 after S1) · Q-211/212 audience.
- Merged + pushed today: Q-185, 187/188, 189/190, 191/192, 197/198/213/214, 215–218, 220, S1 (Q-193/194/202/203).
  Q-221 merged locally, gate blgf1b8ms. Site Q-209/210 live (probe 413 ok).
- 3.0.62 at notarization (a654dfa85 — before S1, the glance fixes, Q-220/221): next build 3.0.63 carries those.

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
