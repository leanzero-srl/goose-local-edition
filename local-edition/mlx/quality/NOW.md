# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-28 07:0x (date) · heartbeat cron 90b0083a + runwatch.sh per run

## Live
- Installed: 3.0.66 on both Macs, split up. Q-223/241/242 LIVE-PROVEN (quit waits 0.7 s, peers told, rank 1 verified
  gone, mesh last). LIVE TESTER resumed (after a spend-limit stop): Q-232 crop on the split, loops J1/J2, Q-240
  kill -9 prove, Q-243. E2E #3o after it.
- On main after 3.0.66 (→ 3.0.67): L2c part 1, Q-248/249/250, Q-237/246, Q-247, Q-254/255 (swap words), Q-258
  (sidecar tests no longer SIGTERM the live engine — the cause of every "silent engine death"), Q-256, Q-259.

## CI
- RED from ae1276948: Q-256's restart-measurement test misses the Loading phase on Linux (Q-275) — agent on it.

## Agents (resumed 07:0x after the spend limit; WIP committed in each worktree first)
- Q-260 images to text-only engines · Q-263..267 per-session working dir (on top of the Q-257 branch, which is NOT
  on main — they land together) · L2c mac_wide · Q-251/252/253 port holder · Q-275 (fresh) · live tester.
- Queued behind the L2c agent (nodes_loader): Q-271, Q-272 → Q-273, Q-274.

## Next actions (in order)
1. Q-275 fix → CI green. Live tester report → rows.
2. Merge agents via a scratch branch + gate + ff (skill trap) → build 3.0.67 → live proves → E2E #3o.
3. Live critic walk: Nodes, Strategies, loop dialog + rail; L9 (loop harness + ≥5-tick E2E).

## Standing rules for every tick
- CI status, agent audit, disk ≥ 30 GB, clean.sh on both Macs; no stale background shells (4 killed 07:0x).
- A DONE build is installed + split-start smoked in the same tick — unless a live tester holds the app.
- Merge via scratch branch, gate, ff main (projsync pushes main every 15 min). Tests never touch port 8090.
- One GPU user at a time. Nothing waits for the owner.
