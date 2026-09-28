# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-28 10:5x (date) · heartbeat cron 90b0083a + runwatch.sh per run

## Live
- Installed: 3.0.69 on both Macs (main 435b6f0d4: + Q-294/295/296 split cache fixes, Q-297 skill re-load note, and the
  batch Q-271..274, Q-276/277, Q-282..284, Q-286/290). Install reaped no orphan. Split up.
- E2E #3p RUNNING (RU-2026-09-28-3p-split-tensor, 27B split, jira brief, 30 turns). THE MEASUREMENT: every turn's
  first big call reads ~its whole prefix from cache (Q-294), and a big skill is never re-sent in full (Q-297).
  Compare against #3o: turns 3/5 read 0, turn 6 40k/102k; turn 7 grew 102k → 193k.

## CI
- Green through bcb9fecc0/d5c944a81; 4f63906d4 running.

## Agents (the 3.0.68 live critic's 24 rows, Q-298..Q-321, in five file-disjoint batches)
- A needs-you supersede (Q-298, Q-319) · B theme/glance/window title/460 (Q-300, 313, 315, 318, 321) ·
  C transcript working row, error events, reviewing words (Q-301, 302, 307) · D Nodes/New node/planner words
  (Q-299, 303–306, 308–311, 317) · E Run it details/tray/tick markers/dark contrast (Q-312, 314, 316, 320).
- Queued: Q-292/293 (Run it row + gate chip vs the leftover credit); Q-261/281 measured on #3p.

## Next actions (in order)
1. Read #3p by its words each tick; check calls.tsv first-call cache per turn; stop rules.
2. Merge batches A–E via scratch + gate (REAL pnpm install if the SDK changes) → build 3.0.70.
3. Live proves on 3.0.69 between E2E runs: Q-254/272 swap words, Q-257 two windows, Q-276 kill -9 → restore,
   Q-260 image, Q-278 J2, J4 with r5.mjs.

## Standing rules for every tick
- CI, agents, disk ≥ 30 GB, clean.sh; kill stale shells; delete ~/goose-targets/<id> on merge.
- Merge via scratch, gate, ff main. Coordinator assigns Q ids (A: 322-323, B: 324-325, C: 326-327, D: 328-329, E: 330-331).
- rustfmt --check hand-resolved .rs conflicts. Tests never touch port 8090. One GPU user at a time.
