# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-27 21:15 (date) · heartbeat cron 90b0083a + runwatch.sh per run

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
- LANE D design MERGED e78b6ae06: DESIGN-NODES-AND-STRATEGIES.md (Nodes nav first · Providers keeps 2 tabs ·
  LeanZero MLX = Engine/My Macs/Models/Sampling + setup strip · node = model + way · strategy = 6 roles × ordered
  nodes + when-rule failover/overflow/share · loader S5 · chat via swarm provider node:/strategy: ids · swarm Tier A
  env-projected, Tier B gated). Slices S0..S10; open questions proceed on the recommendations. Q-200..208 = its D1–D9.
  RUNNING: design REVIEW (refuter) · S1 nav/IA surgeon. On the review: fix the doc, then S0 contract → S2/S3/S5/S6/S7/S8.
- Lane A+B (edit diffs + Changes rail) · Lane C (LeanZero pill + report form) · Q-197/198 path leaks — running.
- Q-187/188 merged 9174cf5d9 locally, gate b1rmuck9y running (cargo check green) → push.
- Q-185 merged + pushed 96bcc680e → ships in 3.0.62.

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
