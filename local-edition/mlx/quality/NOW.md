# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-28 10:2x (date) · heartbeat cron 90b0083a + runwatch.sh per run

## Live
- Installed: 3.0.68 on both Macs (main 8c78d5be6: + Q-257 one goosed per app, Q-263..267 per-chat folders, and
  everything from 3.0.67). Split Ready (restored itself). The install reaped NO goosed orphan (quit fix holds).
- E2E #3o STOPPED after turn 8 for the install: 8 turns, 2 defects found — Q-294 (split cache misses at turn start;
  3 causes, fixed cd7bef737 + Q-295/Q-296) and Q-297 (same 15k skill loaded 5×).
- LIVE CRITIC walking 3.0.68 now (Nodes, chip, Engine tab, loops, glance, error colours) — read-only.
- NEXT BUILD 3.0.69 = main + Q-294 (+ Q-297 when it lands). E2E #3p starts ONLY on 3.0.69 — 3.0.68 would repeat the
  cache misses (the wrapper change needs the new goose on BOTH Macs).

## CI
- Green through 8c78d5be6/19cdd4eb8; bcb9fecc0 (the batch: Q-271..274, Q-276/277, Q-282..284, Q-286/290) running.

## Agents
- Q-297 (skills: short answer for an already-loaded skill) · live critic (no worktree).
- Queued (next free slot): Q-292/293 (Run it row + Mount gate chip vs the leftover credit), Q-261/281 (model
  behaviour — measure on #3p), Q-237-class leftovers.

## Next actions (in order)
1. Q-294 scratch gate (g294) → ff main → build 3.0.69 → install → E2E #3p; verify every turn's first call hits cache.
2. Critic report → rows (coordinator assigns ids) → dispatch.
3. Live proves on 3.0.69: Q-254/272 swap words, Q-257 two windows one goosed, Q-276 kill -9 → restore, Q-260 image,
   Q-278 J2 rerun, J4 with the fixed r5.mjs.

## Standing rules for every tick
- CI, agents, disk ≥ 30 GB, clean.sh; kill stale shells; delete ~/goose-targets/<id> on merge.
- Merge via scratch branch (REAL pnpm install when the SDK changes), gate, ff main. Coordinator assigns Q ids.
- rustfmt --check any hand-resolved .rs conflict. Tests never touch port 8090. One GPU user at a time.
