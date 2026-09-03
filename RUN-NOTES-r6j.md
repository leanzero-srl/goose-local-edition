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


## SCORE (the harness's own auto-score, verdict.json, 04:37)

**r6j 0.1112 (inner 0.5823, crit_mult 0.216) vs r6h 0.4616 (inner 0.8252, crit_mult 0.6). A REGRESSION of 0.35, and the engine's
own `passed: true` did not see it.**

| tier (weight) | r6h | r6j |
|---|---|---|
| T — the 3D field (0.14) | 0.690 | **0.000** |
| X — money correctness (0.16) | 0.833 | 0.485 |
| P — performance (0.08) | 0.833 | 0.200 |
| E — excellence (0.12) | 0.359 | 0.019 |
| C — endpoints (0.09) | 0.952 | 0.663 |
| B (0.09) | 1.000 | 0.892 |
| J — journeys (0.12) | 0.507 | 0.500 |
| R (0.16) | 0.900 | 0.880 |
| A / V / D | 1.0 / 1.0 / 0.8 | 1.0 / 1.0 / 0.8 |

**Three criticals (r6h had one):** `j_workflow_journey` (approval cannot complete through the UI — r6h's too),
`x_conservation_residual` (money created/destroyed), `x_no_lost_write` (an acknowledged mutation absent from final state).
Each multiplies by 0.6: 0.6³ = 0.216.

**The scorer's own root cause: `t_vs7dbg_truth`** — twelve T-tier checks (scene binding, layout basis, draw budget, pick buffer,
pick real pass, click semantics, camera math, coast identity/reality, label culling, brush link, stream diff) all failed together
off ONE debug-API truth check. The merged `web/viz.js` parses and the page renders, but the debug surface the grader drives it
through does not answer truthfully — so every 3D check reads 0. r6h scored 0.69 there.

**THE HONEST READING.** The split delivered a file that parses and a page that loads; it did NOT deliver a working 3D field.
`passed: true` measured what the engine can see (criticals closed, no render-class findings) and the app's actual behaviour was
worse than the golden run on 5 of 11 tiers. This is exactly the class VA-134 filed — the verdict's partition and the product's
reality disagreeing — and it is the strongest argument yet that `passed` must not be reported without the score beside it.

**What this does NOT say.** The engine changes measured earlier (dispatch order, mid-lane answers, the assembly, integrate booting
the app) all did what they claimed; they bought TIME and structure, not correctness. The 3-shard split is not exonerated either:
a single-lane web-viz scored 0.69 on T in r6h, three shards + a merger scored 0.000. That is the one measurement that should
decide whether the split survives, and it needs a second run to separate "the split broke it" from "this model drew badly tonight".

## EVERY REGRESSION vs r6h — the full check-level diff (91 checks scored in both runs)

**37 checks worse, 2 better, 0 unique to either run.** The tier table above is the shape; these are the behaviours.

**Collapsed from a perfect 1.000 to 0.000 (17 checks).** The 3D field: `t_layout_basis`, `t_draw_budget`, `t_pick_real_pass`,
`t_coast_identity`, `t_coast_reality`. Money: `x_l2_per_key_order`, `x_l4_convergence`, `x_conservation_residual`, `x_no_lost_write`.
Performance: `p_drag_frames`, `p_idle_flatness`, `p_under_stream`, `p_api_latency`. Excellence: `e_frames_under_drag`,
`e_under_load_latency`. API surface: `b_events_log` (the event log endpoint), `c_paged_walk` (cursor paging).

**The rest of the 3D tier fell from partial to zero**: `t_labels_culling` 0.900, `t_camera_math` 0.857, `t_context_real` 0.750,
`t_click_semantics` 0.667, `t_pick_buffer` 0.600, `t_stream_diff` 0.500, `t_height_pixels` 0.333, `t_brush_link` 0.333,
`t_scene_binding` 0.214, `t_vs7dbg_truth` 0.200 — all → 0.000. TWELVE of them hang off `t_vs7dbg_truth` by the scorer's own
`root_causes`, so the debug surface is the single upstream failure.

**Degraded but alive:** `c_conditional_resync` 1.000→0.584, `c_b1_drop_resume` and `c_b2_retry_after` 1.000→0.667,
`c_webhook_discipline` 1.000→0.722, `x_m3_terminal_conservation` 1.000→0.825, `b_viz_records` 1.000→0.806,
`r_b7_partition` 1.000→0.800, `j_sync_journey` 1.000→0.500, `j_error_state` 0.300→0.000, `e_mastery` 0.441→0.220.

**BETTER in r6j (only 2):** `j_first_use` 0.250→1.000 (the first-use journey now works — web-page's doing) and
`c_b5_generation_304` 0.667→1.000 (the generation/304 conditional path).

## PROCESS DIFFERENCES vs r6h (same fleet, same spec)

| | r6h | r6j |
|---|---|---|
| total wall | 507.3 min | 580.4 min (+73) |
| OPEN | 65.6 | 52.2 |
| RESEARCH | 8.2 | **144.3** |
| SYNTHESIS (incl. split) | 31.6 | 50.3 |
| BUILD | 319.1 | 295.9 |
| INTEGRATE | 46.3 | 36.3 |
| REPAIR + FIX | 1.4 + **35.2** | 1.3 + **0.0** |
| tasks / retries / failures | 10 / 0 / 0 | 11 / 0 / 0 |
| repair: verify rounds · repro · flips · promoted | 2 · 3 · 2 · **2** | 1 · 0 · 0 · **0** |

**The two process regressions that matter:**
1. **RESEARCH went 8.2 → 144.3 minutes.** r6h barely researched; r6j spent 2.4 hours, 131 of them on one lane holding its slice.
   That is where the +73 minutes came from and more.
2. **REPAIR DID NOTHING.** r6h ran two verify rounds, reproduced three findings, flipped two and PROMOTED TWO FIXES in a 35-minute
   fix phase. r6j ran one verify round, found one finding (no tests), reproduced nothing, promoted nothing, and its fix phase was
   0.0 minutes. r6h's repair is where two real defects got fixed — including the `viz-labels` DOM id — and r6j never entered it.
   With three criticals live in the tree, a repair phase that exits immediately is the single worst behaviour of this run.
