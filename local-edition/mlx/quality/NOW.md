# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-27 17:02 (date) · heartbeat cron 90b0083a + runwatch.sh per run

## Live
- Installed on both Macs: 3.0.59 (verified post-swap pid).
- E2E #3h RUNNING on 3.0.59 (27B tensor, jira brief, 30 turns): turns 0–4 done (250/314/208/1827/419 s), no runaway;
  turn 5 in since 12:25Z and showing Q-182 full re-reads (prefill 103k, 0 cached). runwatch bbcribpnh.
- 3.0.60 BUILT (>>> DONE, private TMPDIR worked) = Q-182 + everything to ada1bbc39. Install at the turn-5 boundary
  (watcher bko701cg5) with install.sh, split-start 27B, then E2E #3i on 3.0.60 from the same brief + runwatch.
- Main 5955ddea5+ has the ENGINE GLANCE (owner's PiP idea): sidebar dock card, draggable in-app float with corner
  snap/pill, desktop mini window (always on top, never takes focus, shown while working + goose backgrounded),
  click → Engine. Gate on merged main green (tsc, vitest 3167 + 4 load-timeouts re-run alone green, eslint, i18n).
  Ships in 3.0.61.

## Agents
- Q-183 (panel-surgeon, worktree): main.ts reads ~/.config/goose for memories/proposals/agent-work under
  GOOSE_PATH_ROOT → one resolver mirroring paths.rs.

## Next actions (in order)
1. Turn-5 boundary → install 3.0.60 → split-start 27B → E2E #3i + runwatch: prove Q-182 (no 0-cached re-reads),
   Q-181, Q-161, Q-164, Q-177; livecheck green.
2. Merge Q-183 → build 3.0.61 (glance + Q-183, private TMPDIR) → install at a #3i boundary → live critic round:
   live states, needs-you card, engine glance (dock, float, desktop mini window over another app).
3. Flash pipeline E2E on lz-pipeline-qwen4.14 (Q-178/179/181 proof) + load run.

## Standing rules for every tick
- CI status, agent audit (processes, disk ≥ 30 GB, pushes), clean.sh orphan scan on both Macs.
- A DONE build is installed + split-start smoked in the same tick. Release builds use a private TMPDIR.
- Before pushing a Rust merge: harness/wincheck.sh. One GPU user at a time. Nothing waits for the owner.
