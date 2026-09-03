## r6j — 2026-09-02 15:36 → 2026-09-03 01:16 UTC (580.4 min), engine 5ef71b403 (1.41.109), passed=true

**Verdict.** `complete_result{passed: true, verified: true, render_class_known_bugs: 0, remaining_findings: 1, shipped: "final tree"}`.
The one known active bug is r6h's exactly: "the app ships NO executable tests (`pytest -q` collected 0)". 11/11 tasks done,
0 retried, 0 orphans, 1 repair round (`complete_verify` ran once, `fix_target_selected` once).

**Phases (wall min).** OPEN 52.2 · RESEARCH 144.3 · SYNTHESIS 12.0 · SPLIT 38.0 · BUILD 295.9 · INTEGRATE 36.3 · REPAIR 1.3.
r6h for comparison: OPEN 65.6 · BUILD 325 · INTEGRATE 69 · total 507 (scored 0.4616).

**What the engine changes bought (the falsifiers, all read from the run's own events).**
- VA-113 dispatch order: RESEARCH dispatched heaviest-first onto the fastest host (web-viz 10 sections → workhorse, api 9 → mihai,
  core 7 → gabee); BUILD dispatched by remaining chain weight (skeleton 27 → workhorse, web-page 16 → mihai, scene-stream 15 → gabee).
  The r6h skeleton-first prefix is GONE: 0 idle node-min during the skeleton (r5/r6c 112/128), and the skeleton finished in 30.6 min vs ~60.
- VA-118 + VA-119: 37 research answers landed MID-lane through the tool across all six lanes, and they reached the plan — 9 of 11 task
  briefs carry ANSWERS SETTLED AT PLAN TIME, 13 answers were routed cross-slice by owned file, 1 unowned.
- The SPLIT + MERGE bet delivered end to end for the first time: web-viz (10 spec sections, 1 file, the r6c 519-minute lane) became 3
  shards (59.6 / 79.8 / 61.4 min, overlapping) → `ASSEMBLED.js` 1,766 lines / 121 definitions / 0 parse errors → a merger writing glue
  in 61.4 min → `web/viz.js` 84,573 B, parses under `node --check`.
- INTEGRATE booted the app on a real port and drove the maker/checker approval journey over HTTP — the journey r6h's gate could not
  perform — in 36.3 min on one lane.

**What the run cost, and where.**
- The research wall (144 min) is ONE lane: ledgerd-api held its whole 9-section slice in reasoning for 131 min before landing its first
  answer, then landed 10 in 13. 85 idle node-min sat behind it. Every other lane: 22–68 min.
- Minutes-to-first-write per builder: skeleton 15.2 · ledgerd-core 19.8 · ledgerd-api ~33 · scene-stream 39.6 · web-page 56.8.
  r6h's median was ~35. The write-first BRIEF LINE does not produce a write-first ACTION.
- OPEN 52.2 min included the same six-slice list written 11 times (5 of them the same territory renamed), which the 48-char shingle
  meter cannot see.
- The judge looked ONCE in 9.7 hours. Its one look was right (DRIFTING with the exact missing file as NEXT) and the engine HELD it;
  the model wrote that file itself 15 minutes later.

**Findings filed from this run** (all with mechanism + fix, none dispatched — vigil-only after the 429): VA-142 ownership-seam briefs,
VA-144 write-alone first turn, VA-146 the engine's own defect steer as a drift witness, VA-151 no transport inactivity terminator,
VA-152 silence vs a buffered frame, VA-154 the shard scanner reads `16 / 255, X = 24 / 255` as a regex and swallows the middle
declarator, VA-155 cross-shard references read as undefined.

**Not done, deliberately.** No score was run and no r6k was launched: the standing order after the 429 was vigil + notes only.
`r6k-staging` (25bf05c33 + the later merges) holds ~20 engine changes, pushed and UNPROVEN — no cargo has ever run against it.

