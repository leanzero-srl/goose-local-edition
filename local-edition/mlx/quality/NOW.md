# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-26 22:35 · heartbeat: session cron 90b0083a (:04/:14/…/:54, expires 2026-10-03)

## Live
- Installed on both Macs: 3.0.52 (Q-107, Q-138, Q-130, Q-140, Q-141, Q-142). Split-start smoke: 27B tensor up in 17 s.
  (The relaunch restored the previous Flash split first; split-start now refuses a split serving another model.)
- E2E #3d RUNNING since 22:23: tensor 27B, jira brief, dir ~/goose-builds/quality/RU-2026-09-26-3d-split-tensor
  (r1.mjs + calls.py <dir> + sampler.sh). Ladder-0 all OK to 39k.
- Flash load 26c (3.0.51, fixed load.py): decode healthy (gap 0.7 s); prefills FIFO one at a time → canary 347 s (Q-145).
- Disk 22:30: cleaned 34 → 156 GB free (debug deps > 6 h, incremental > 2 h, ui/desktop/out, uv cache).
  Not touched (owner's): ~/.goose/models 128 GB, forge-live-harness/evidence 37 GB, ~/.codex 19 GB.

## Agents (worktree, NO GPU)
- Q-139 web-search writes docs/technical/ into the user's cwd.
- Q-144 pipeline fork line lacks single-engine fixes lz.3..lz.9 → audit + port + tag + guard.
- Q-145 pipeline shortest-remaining-prefill-first (own fork worktree, branch q145-srpf, untagged → rebase onto Q-144's tag).
- NEEDS-YOU (owner ask 22:3x): ask_user tool + persistent card above composer + sidebar marker + app-wide count;
  never in benchmark/swarm sessions.

## Next actions (in order)
1. Watch E2E #3d every tick. Prove: calls ≥90% cached after turn 1 (Q-142), tool call streams "writing a tool call
   to shell" (Q-141), load_tools keeps the cache (Q-107); turn times vs #3c (110/690/1380/516/924 s) and #4 Studio
   (170/81/1333 s). diskio.py on goosed (Q-115).
2. Q-144 + Q-145 land → one fork tag → pin → 3.0.53 → E2E #5b Flash pipeline + load.py rerun (canary TTFT).
3. Critic pass (mlx-ux-critic) on 3.0.52 after #3d; include the needs-you surface once merged.
4. Agents return → read log → merge → gate (clippy, tests, wincheck.sh) → push → CI.

## Standing rules for every tick
- CI status, agent audit (processes, disk ≥ 30 GB, pushes, ~/.config/goose), clean.sh orphan scan on both Macs.
- A DONE build is installed + split-start smoked in the same tick. Never install without the smoke.
- Before pushing a Rust merge: harness/wincheck.sh. One GPU user at a time. Nothing waits for the owner.
