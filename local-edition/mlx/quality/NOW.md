# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-28 13:2x (date) · heartbeat cron 90b0083a + runwatch.sh per run

## Live
- Installed: 3.0.70 on both Macs (main 95324ea8d: critic batches A–E, Q-298 needs-you supersede, Q-292/293 NOT yet).
  Split up. E2E #3q RUNNING (RU-2026-09-28-3q-split-tensor, jira brief) since 13:13.
- LIVE-PROVEN on 3.0.69 (#3p, 13 turns): Q-294/295/296 — every turn's first call 97–99% from cache (was 0% on #3o
  turns 3/5); Q-297 holding (skill loaded once). #3p turn 9 = a compaction that re-read ~143k cold → Q-342.
- On main after 3.0.70 (→ 3.0.71): Q-292/293 (Run it row + gate chip read Q-276's credit), Q-325 (focus ring
  drew nothing app-wide; 664 dead classes remapped + a guard — VISUAL blast radius ~400 host sites: walk it live),
  Q-337/338 (owner: cached vs new two-part prompt-read bar; split position includes the cached prefix).

## CI
- Green through fe97eb652; 0828e9ca8/838e041a9 running.

## Agents
- Q-344 (a card answer must not supersede its sibling; built ON the Q-340/341 branch — needs-you fold + answer
  queue — they land together) · Q-342 (compaction reuses the cached prefix) · Q-350 + Q-343 (activity says
  single/off while the split restores; 2,684 unfinished request logs).
- Queued: Q-334 (focus ring colour outside the local edition), Q-335 (~55 faded opacity uses), Q-336/261/281
  (model behaviour, measured on #3q).

## Next actions (in order)
1. Merge Q-340/341+344, Q-342, Q-350/343 via scratch + gate → build 3.0.71 → install at a #3q turn boundary.
2. On 3.0.71: live critic walk (Q-325's visual change, Q-337's bar, needs-you fold/queue), then E2E #3r.
3. Live proves still open: Q-254/272 swap words, Q-257 two windows, Q-276 kill -9 → restore, Q-278 J2, J4.

## Standing rules for every tick
- CI, agents, disk ≥ 30 GB, clean.sh; kill stale shells; delete ~/goose-targets/<id> on merge.
- Merge via scratch (REAL pnpm install when the SDK changes), gate, ff main. Coordinator assigns Q ids; agents use
  their OWN scratch folders. rustfmt --check hand-resolved .rs conflicts. Tests never touch port 8090.
- Never navigate the main window while an E2E runs (split-start/probes moved #3q's page at 13:13).
