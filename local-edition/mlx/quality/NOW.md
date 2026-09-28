# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-28 14:5x (date) · heartbeat cron 90b0083a + runwatch.sh per run

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
 
- Q-342 MERGED to main 757c3458a (gate green). Q-346 ON MAIN d710d7059 (gate green). Q-350+343 /tmp/merge-q350: UI green (vitest 4169), cargo running. Q-347 (stable head kept
  entry; wire tag → BOTH Macs need 3.0.71) → /tmp/merge-q347, gate /tmp/g347.out after g350. All gates run from
  scratchpad/gates.sh + gate347.sh in their own session (five background gates were killed, cause unproven). Disk swept 16 → 75 GB (my old scratchpad held 56 GB).
- Q-367 (tool-not-found error leads with the closest real tools) + Q-369 (load_tools' false message) DONE →
  /tmp/merge-q367, gate /tmp/g367.out after g347. Q-368 = model behaviour (no goose cause; 4 restatements, turn 2). Turn 2 ended 761 s, 27 tools, a correct
  sourced answer (EOL 2026-03-30 / 2028-03-30 / 2029-03-28). Turns 3–4 done (1106 s / 176 s);
  turn 3's `write` lost every `=>` (SyntaxError, two rewrites) → Q-371 CUTTING. Turn 5 running. Studio-side: forge-tuner got a
  goose tool-call training proposal (docs/GOOSE-TOOLCALL-ROUND.md, 6631628) — owner picks public vs private data.
- Owner features (designs DESIGN-Q357/358/359-*.md): Q-357 compaction revamp CUTTING (all slices, on merge-q342) ·
  Q-358 search S1+S2 CUTTING; notes S3–S5 QUEUED behind: merge-q344 · Q-359 C0/C1/C2/C4 CUTTING; C3 SCHEDULED
  waits on: relay decode tok/s + 4-delegates-on-2-Macs wall time.
- Q-340/341/344 ON MAIN 838b93b62 (gate green). Q-358 notes S3–S5 DISPATCHED (was queued behind it).
- Queued: Q-334 (focus ring colour outside the local edition), Q-335 (~55 faded opacity uses), Q-336/261/281
  (model behaviour, measured on #3q).

## Next actions (in order)
1. Merge Q-340/341+344, Q-342, Q-350/343 via scratch + gate → build 3.0.71 → install at a #3r turn boundary.
2. On 3.0.71: live critic walk (Q-325's visual change, Q-337's bar, needs-you fold/queue), then E2E #3s.
2a. Owner: the E2E must ANSWER needs-you cards → Q-376 CUTTING (r1.mjs answers via the UI from brief guidance,
   records delivery/clear/siblings/next words). On 3.0.71, before #3s: answer #3p's 2 open cards (20260928_19) and
   20260928_17's by hand over CDP as a first live prove.
2b. Before 3.0.71: one FULL gate on merged main (lanes were gated on different bases).
3. Live proves still open: Q-254/272 swap words, Q-257 two windows, Q-276 kill -9 → restore, Q-278 J2, J4.

## Standing rules for every tick
- CI, agents, disk ≥ 30 GB, clean.sh; kill stale shells; delete ~/goose-targets/<id> on merge.
- Merge via scratch (REAL pnpm install when the SDK changes), gate, ff main. Coordinator assigns Q ids; agents use
  their OWN scratch folders. rustfmt --check hand-resolved .rs conflicts. Tests never touch port 8090.
- Never navigate the main window while an E2E runs (split-start/probes moved #3q's page at 13:13).
