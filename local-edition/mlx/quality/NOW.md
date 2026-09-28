# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-28 14:0x (date) · heartbeat cron 90b0083a + runwatch.sh per run

## Live
- Installed: 3.0.70 on both Macs (main 95324ea8d: critic batches A–E, Q-298 needs-you supersede, Q-292/293 NOT yet).
  Split up. E2E #3r RUNNING (RU-2026-09-28-3r-split-tensor, jira brief) since 13:24 — #3q died when MY probe
  navigated the main window (r1's chatUrl then pointed at the Engine tab); turn 0 big call 40,764/56,280 cached.
- LIVE-PROVEN on 3.0.69 (#3p, 13 turns): Q-294/295/296 — every turn's first call 97–99% from cache (was 0% on #3o
  turns 3/5); Q-297 holding (skill loaded once). #3p turn 9 = a compaction that re-read ~143k cold → Q-342.
- On main after 3.0.70 (→ 3.0.71): Q-292/293 (Run it row + gate chip read Q-276's credit), Q-325 (focus ring
  drew nothing app-wide; 664 dead classes remapped + a guard — VISUAL blast radius ~400 host sites: walk it live),
  Q-337/338 (owner: cached vs new two-part prompt-read bar; split position includes the cached prefix).

## CI
- Green through fe97eb652; 0828e9ca8/838e041a9 running.

## Agents
- Q-344 (a card answer must not supersede its sibling; ON the Q-340/341 branch — they land together) ·
  Q-350 + Q-343 (activity says single/off while the split restores; 2,684 unfinished request logs) ·
  Q-347 (post-compaction cold head).
- Q-342 MERGED to main 757c3458a (gate green). Q-346 done → scratch /tmp/merge-q346, gate /tmp/g346.out queued
  behind g344's cargo (one main target). Disk swept 16 → 75 GB (my old scratchpad held 56 GB).
- #3r turn 2 (13:40): the 27B invented `websearch`/`fetch` 13× before the real web tool → Q-367 + Q-368
  (facts paragraph restated 4×) CUTTING (wire read first). Turn 2 ended 761 s, 27 tools, a correct
  sourced answer (EOL 2026-03-30 / 2028-03-30 / 2029-03-28). Turn 3 running. Studio-side: forge-tuner got a
  goose tool-call training proposal (docs/GOOSE-TOOLCALL-ROUND.md, 6631628) — owner picks public vs private data.
- Owner features (designs DESIGN-Q357/358/359-*.md): Q-357 compaction revamp CUTTING (all slices, on merge-q342) ·
  Q-358 search S1+S2 CUTTING; notes S3–S5 QUEUED behind: merge-q344 · Q-359 C0/C1/C2/C4 CUTTING; C3 SCHEDULED
  waits on: relay decode tok/s + 4-delegates-on-2-Macs wall time.
- Q-340/341+344 merged into scratch /tmp/merge-q344; gate → /tmp/g344.out (UI green: tsc, vitest 4180, eslint src 0, i18n; cargo after g342).
- Queued: Q-334 (focus ring colour outside the local edition), Q-335 (~55 faded opacity uses), Q-336/261/281
  (model behaviour, measured on #3q).

## Next actions (in order)
1. Merge Q-340/341+344, Q-342, Q-350/343 via scratch + gate → build 3.0.71 → install at a #3r turn boundary.
2. On 3.0.71: live critic walk (Q-325's visual change, Q-337's bar, needs-you fold/queue), then E2E #3s.
2b. Q-344 lands → dispatch Q-358 S3–S5 (notes between chats).
3. Live proves still open: Q-254/272 swap words, Q-257 two windows, Q-276 kill -9 → restore, Q-278 J2, J4.

## Standing rules for every tick
- CI, agents, disk ≥ 30 GB, clean.sh; kill stale shells; delete ~/goose-targets/<id> on merge.
- Merge via scratch (REAL pnpm install when the SDK changes), gate, ff main. Coordinator assigns Q ids; agents use
  their OWN scratch folders. rustfmt --check hand-resolved .rs conflicts. Tests never touch port 8090.
- Never navigate the main window while an E2E runs (split-start/probes moved #3q's page at 13:13).
