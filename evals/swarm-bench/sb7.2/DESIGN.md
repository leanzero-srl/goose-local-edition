# SB7.2 — Meridian Payments Landscape, 3D weighted into the score

Status: implemented and reference-validated on 2026-10-02 from the owner's approved design ("the scorer
needs to put sufficient emphasis on the 3D aspects", "very judicious on the 3D elements it demands", "a
really tough benchmark", "avoid benchmaxxed topics", "tests it in a usable, functional way", "economically
viable"). No model or paid run was made; the golden reference, six one-defect controls and re-scores of
archived SB7.1 entrant trees are the proof. The archived entrants were built against SB7.1's prompt, so their
SB7.2 numbers below are calibration only and must never be published as SB7.2 results.

## Why SB7.1 needed changing (receipts, measured before this tier)

- Composition: `final = min(earned, ceilings)`, `earned = (0.88·inner + 0.12·gate_fraction·e_mean) × critical
  multiplier` (score_sb7.py `compose_from_rows`). SB7.1's inner weights are SB7's (A .04 B .09 C .09 D .06 J .12
  V .06 P .08 T .14 X .16 R .16) and its S/Q/M visual rows are admission-only, weight 0 (score_sb71.py `admit`).
  3D was ≈22% of raw points; difficulty came only from the ceilings.
- Gemini 3.8 Flash earned raw 0.9683 but finished 0.799 on three small defects, one a 3.45:1 Replay-button
  contrast. A DeepSeek run's presentation band was tripped by a 4.49:1 button. Text contrast was capping a 3D
  benchmark.
- Every entrant's overview, and the golden's (runs/sb71-validation/20260920/sb71-reference-1a06212bc-…-evidence/
  sb7-shots/*-sb71-field.png), is an unreadable colour carpet: 12,288 towers at SB7's default camera (yaw 30,
  pitch 40, distance 260) on a 1170 × 460 canvas, about 1.7 px per world unit — caps 1.6 × 1.3 px. DeepSeek run
  6 rendered the field nearly black (s_visible_surface 0).
- The stream-apply excellence rung was 100 ms; the reference measures 131–135 ms on this host. Unpassable.
- E rungs (100 ms stream, 40 frames over 40 moves, 16 gate conditions) were not in the public contract.
- VISUAL-CONTRACT.md's "Evidence and grading" section was 6.6 KB of grader method (clocks, ±16 ms windows,
  phase windows), resident in every model turn and an invitation to rebuild the probe.

## What stays identical

Product, SB7 behavioural contract (`SB7-CONTRACT.md` = spec-build-sb7.md), starter (`sb7.1/starter`, reused),
seeded fixtures, vendor v3, gather, every SB7 check and every SB7.1 visual check, the 0.12 excellence term and
its 16 gate conditions, the critical registry and multiplier floor 0.6, the severity selftest, isolation,
completion receipts, fresh scoring vendor, provider-error refusal, reaping and cost recording. The token cost
profile is SB7.1's: the public contract is smaller (below).

## Weights (inner, sum 1.00)

| tier | S | Q | M | T | P | B | C | X | R | A | D | J | V |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| SB7.1 | 0 | 0 | 0 | .14 | .08 | .09 | .09 | .16 | .16 | .04 | .06 | .12 | .06 |
| SB7.2 | .10 | .08 | .06 | .16 | .06 | .08 | .08 | .10 | .10 | .02 | .04 | .08 | .04 |

3D = S + Q + M + T + P = **0.46** of inner (asserted in score_sb72.py). Rows:
S `s_visible_surface`, `s_tower_geometry`, `s_currency_collar`; Q `q_payment_context`, `q_inspector_framing`,
`q_overview_legibility` (3D legibility and inspection, no text contrast); M `m_committed_event_replay`;
V gains `v_presentation_text`. SB7.1's `q_legible_presentation` is split: its 8 inspector framing poses
become `q_inspector_framing` (3D), its four text groups (12 px, 4.5:1 effective contrast, unclipped, uncovered,
inked) become `v_presentation_text`, weighted points that never cap.

Single-defect costs through the real composition (test_score_sb72.py `compose`): a 3.4:1 button
(`v_presentation_text` .75) → 0.9985, no ceiling; wrong inspector values (`q_payment_context` .75) → 0.9941;
illegible overview (`q_overview_legibility` 0) → earned 0.9765, ceiling 0.799; no animation → earned 0.9472,
ceiling 0.899; one lost acknowledged write → 0.5956 (critical, unchanged).

## Ceilings (3D-pure; passing awards nothing)

| condition missing | max |
|---|---:|
| `s_visible_surface` (visible, data-backed 3D) | 0.599 |
| + `s_tower_geometry`, `s_currency_collar`, `t_layout_basis`, `t_scene_binding`, `t_height_pixels`, `t_vs7dbg_truth` | 0.699 |
| + the 10 T interaction checks (draw budget, pick buffer, real pick pass, click, camera, coast ×2, labels, brush, incremental stream), `q_overview_legibility`, `q_inspector_framing` | 0.799 |
| + `m_committed_event_replay` AND every X and R check (backend excellence) | 0.899 |

`q_inspector_framing` is the 3D half of SB7.1's "readable inspection" band leg; `q_payment_context` (DOM
values of the inspected payment) and all text readability earn points only.

## The framed default camera and overview legibility

SB7's default camera cannot frame the field: the field is 115 × 216 world units (96 days × up to 180 ranks,
1.2 pitch) and at yaw 30 / pitch 40 / distance 260 its long axis runs into the screen. Projected bounding-box
shares (scratch computation, R0 = 180): 1170 × 460 → 0.35 × 0.75 of the canvas at 1.75 px/unit; 1170 × 640 →
0.49 × 0.75. SB7.2 publishes **yaw 70, pitch 50, distance 190** (target, projection, clamps and laws unchanged):
1170 × 640 → 0.77 × 0.88 bounding box at ~2.9–3.5 px/unit. The probe's wheel step becomes 800 so one graded
step still reaches the 340 clamp from 190 (SB7.1's 400 from 260 did; the law is unchanged).

`q_overview_legibility` (Q, 0.799 band) is read from the same composited screenshot as `s_visible_surface`, at
the default camera, label boxes excluded. Every number is in sb7.2/VISUAL-CONTRACT.md and the parity test
`test_every_legibility_number_and_the_camera_are_published` fails the build if the probe and contract drift.

| leg | rule | golden-sb72 (5 runs, identical) | margin |
|---|---|---|---|
| framing | pixels within ±8 of a published tower colour span ≥ 60% of canvas width and height (0.5–99.5 percentile); none within 4 px of the edge | 0.681 × 0.717, 0 edge pixels | 13% / 20% |
| lighting | median WCAG contrast vs #101828 over ≤ 200 seeded pixels predicted to be tower surface ≥ 2.0:1 | 2.743:1 (200 pixels) | 37% |
| status | ≥ 64 of ≤ 96 seeded unmutated payments show a cap top covering a whole 2 × 2 px block; ≥ 90% read as their status (nearest legend colour within RGB distance 40, all four pixels) | 96 sampled, 96 correct | 32 caps / 10 points |

The legend distance 40 is below half the closest pair of status colours (pending–failed, 99). Score: mean of
the three legs, each 1 when met, else proportional. It demands no new feature: the published camera plus a
canvas the field can fill. The reference's canvas went from 460 to 640 CSS px; its colours are the published
ones (the contract fixes overview shading, so "lighting" is checked, not re-specified).

## Excellence rungs, now public

Published in the contract's "Excellence rungs" section (437 bytes). One recalibration: the stream-apply top rung
100 → **180 ms** (sb72-thresholds.json carries the receipt: reference 156.1 / 141.2 / 144.9 / 141.9 ms at load
31 / 8 / 7 / 10, so 15% above the worst and 27% above the median; still below the 200 ms 0.75 rung and the
public 250 ms P budget). No other threshold moved.

## Public contract size

| | spec | visual contract | SB7-CONTRACT | total |
|---|---:|---:|---:|---:|
| SB7.1 | 3,598 | 16,287 | 54,146 | 74,031 |
| SB7.2 | 3,322 | 12,229 | 54,146 | 69,697 |

−4,334 bytes (−5.9%) per resident copy. "Evidence and grading" 6,610 → 925 bytes: what is checked, not how.

## Probe and harness changes (SB7.1 behaviour unchanged)

`product_probe_sb72.mjs` sets `globalThis.__BENCH_PROBE_TIER = 'sb-7.2'` and imports product_probe_sb71.mjs —
one probe implementation, a profile that no environment can leak into SB7.1 grading. Under the profile only:
the camera defaults and wheel step above; `q_overview_legibility`; and three precision fixes the framed pose
exposed on the reference (each traced on the evidence, see commit 2e38bdfdc): the brush colour witness reads
one homogeneous 3×3 face patch (the decisive ray met a 0.84h collar ledge, the read pixel was the shaft side);
coast "pixels moved" spots are those predicted to change colour at every reachable yaw (a framed field put a spot
on a same-coloured tower, 4/5); the brush-clear restore pixel uses the same witness. The golden finishes its
table reveal after `scrollIntoView('nearest')` (a row sat 0.36 px below the fold) and drops stale table
responses (an auto-navigation to the sent payment and the probe's page click raced; offset 400 landed after
3500, `j_workflow_journey` 0.857 once in five runs — an inherited SB7.1 golden race, seen under load in SB7.1's
own release notes).

score_sb72.py wraps score_sb71: `tier_runtime()` swaps PROBE_NAME, VERSION, the weight table, ROOT_BLOCKS
(visual rows downstream of `sync_completeness`) and the stream rung for the duration of gather/evaluate; it
recomposes SB7.1's rows plus the eight visual rows through `compose_from_rows`, then applies the 3D-pure bands.
`score_sb71.run_cli` is the shared hermetic CLI. `isolated_tiers.py` is the single table run_build.py and
bench_rescore.py read: `--sb72` gets SB7.1's isolation, starter, limits, reaping, fresh scoring vendor,
provider-error refusal, cost record and completion receipt; receipts carry their tier, and the retry replays
that tier's scorer and contract. `severity_selftest` runs SB7's selftest under SB7.2 weights plus SB7.2
orderings (text costs V points and never caps; legibility outweighs text; every visual row earns).

## Proof

All runs: this Mac Studio, bundled runtime (Python 3.12, node 24, chrome-headless-shell 153 from Goose
Swarm.app), serial, hermetic CLI (`score_sb72.py`/`score_sb71.py --tree … --seed … --port …`), 1-minute load
average at start in brackets. Reports: `runs/sb72-validation/20261002/` (local, gitignored); the reference
report is committed under `sb7.2/release-validation/`, all summarised in `bench/sb72-release-validation.json`.

### Golden reference (seed 5a05d7631d9276e3, port 8899)

`golden-sb72` with final bytes, `--reference`: **1.000** — earned 1.0000, inner 1.0000, excellence 1.0, critical
multiplier 1.0, no unsuppressed criticals, every band open, freeze gate and severity selftest passed, every
non-calibration check ≥ 0.95 and in fact every check 1.0 [9.60]. Stream apply 137.4 ms (rung 180).
Legibility 0.681 × 0.717, 2.743:1, 96/96. Earlier runs on the way, kept as the record: 0.799 (t_brush_link
colour witness, [31]), 0.799 (brush row reveal, [8]), 0.799 (brush + coast spot, [7]), 1.000 [10],
0.941 (j_workflow_journey race, [22]) — each failure traced and fixed in the commits above.

### One-defect controls (`bench/sb72_controls.py`, same seed and port)

| control | final | earned | ceiling | what it lost (vs the 1.0 reference) |
|---|---:|---:|---:|---|
| unlit field (overview × 0.08) [7.9] | 0.599 | 0.8887 | 0.599 | s_visible_surface 0; q_overview_legibility 0.05 (lit and framing and status legs); every colour-witness row: t_height_pixels 0, p_stream_apply 0, e_stream 0, t_vs7dbg_truth .7, t_brush_link .67, t_stream_diff .8, t_camera_math .86 |
| mis-framed canvas (SB7.1's 460 px under SB7.2's camera) [7.0] | 0.799 | 0.9915 | 0.799 | q_overview_legibility 0.637 only (framing 0.49 wide; 6 readable caps of 64) |
| status colours swapped (pending↔failed) [4.2] | 0.599 | 0.8329 | 0.599 | s_visible_surface 0, legibility .76 (status leg), s_tower_geometry .80, q_inspector_framing 0, m 0, colour-witness T/P rows |
| cuboid towers (overview shaft and inspector core fill the footprint) [4.3] | 0.699 | 0.8872 | 0.699 | s_tower_geometry .75, s_currency_collar 0, q_inspector_framing 0, m 0 (the collar is buried in the solid) |
| frozen animation [5.3] | 0.899 | 0.9472 | 0.899 | m_committed_event_replay 0 only |
| 3.37:1 Replay button [4.7] | 0.9985 | 0.9985 | 1.0 | v_presentation_text 0.75 only — V points, no band |

Judgement: the mis-framed, frozen and low-contrast controls lose exactly their own row. The cuboid loses the
structure band and the motion it hides. The unlit and swapped controls fall to 0.599: SB7.2, like SB7.1, treats
the overview's published colours as data, so a black field or a field showing the wrong status fails
visible-surface admission and every colour-witness check — this is inherited SB7.1 behaviour, not a new
penalty, and the owner may judge it harsh for the swap.

### SB7.1 controls

- `golden-sb71` under the SB7.1 scorer after these changes [7.6]: **0.994**, earned 0.9940, inner 1.0, reference
  gate passed — identical to its validated release receipt (0.994; its 132 ms stream apply still misses
  SB7.1's 100 ms rung). SB7.1 behaviour is unchanged.
- `golden-sb71` under SB7.2 [9.2]: 0.799 (earned 0.9691) — an SB7.1-correct app that keeps SB7's camera and a
  460 px canvas fails t_camera_math (defaults, reset), t_coast_reality, t_brush_link, the legibility framing and
  the inspection-exit camera leg.

### Calibration only — archived SB7.1 entrant trees (built against SB7.1's prompt; never publish)

Each with its own fixture seed at port 8850, both scorers on today's bytes (SB7.1 column reproduces the archived
hermetic numbers where they exist: c003209f 0.4900 → 0.4932, 43896353 0.799 → 0.799, 01dae737 0.699 → 0.699,
Gemini raw 0.9683 → 0.9683).

| tree | SB7.1 final (earned, inner, crit) | SB7.2 final (earned, inner, crit) | SB7.2 bands failed |
|---|---|---|---|
| Gemini 3.8 Flash 20940258 [8.2/5.5] | 0.799 (0.9683, 0.9764, ×1.0) | 0.699 (0.9466, 0.9453, ×1.0) | t_height_pixels, t_vs7dbg_truth; camera, coast, brush, legibility |
| DeepSeek pinned 43896353 [5.8/5.6] | 0.799 (0.8715, 0.9055, ×1.0) | 0.799 (0.8348, 0.8639, ×1.0) | 10-T leg + legibility, inspector framing |
| DeepSeek pinned 01dae737 [5.1/26.1] | 0.699 (0.7721, 0.9023, ×0.8674) | 0.699 (0.7738, 0.8979, ×0.8674) | tower geometry, scene truth |
| DeepSeek c003209f [6.1/7.0] | 0.4932 (0.4932, 0.8789, ×0.6) | 0.4520 (0.4520, 0.8009, ×0.6) | tower geometry, height, scene truth |
| DeepSeek pinned c605215d (run 6, black field) [—/4.6] | refused today: `fire_d1_mutation:failed`, no decisive D1 pixel witness on its dark field (archived hermetic 0.5051 on the same SB7.1 probe bytes — the D1 arm is not deterministic on this tree) | 0.4593 (0.4593, 0.782, ×0.6) | s_visible_surface (black field) |

Unsuppressed criticals: c003209f and c605215d `j_workflow_journey`; 01dae737 `b_money_rendered`,
`j_workflow_journey`; none for the others. Every SB7.1 tree loses SB7.2's camera-default checks because it was
never asked for the framed camera: the comparison measures the weighting, not these entrants' SB7.2 ability.
Gemini's 3.45:1 button now costs only V points (v_presentation_text 0.75); it falls to 0.699 because its
default camera is SB7's and every colour check after a double-click reset is taken at the wrong pose.


## Confidence and limits

- High: the weighting, bands, split, selftest, wiring and receipts (unit-tested; the SB7.1 reference
  reproduces 0.994 exactly; Gemini's SB7.1 raw reproduces 0.9683 exactly).
- Medium: legibility thresholds rest on one reference app and one seed (five identical runs). The framing
  share depends on canvas aspect only; the status leg is a good discriminator (golden 96/96, mis-framed 6
  readable caps); the lighting leg is redundant with s_visible_surface for apps that render the published
  colours and exists for apps that do not.
- Lower: the three probe precision fixes are tier-gated to SB7.2 and proven on the reference, the six controls
  and five archived trees only; an entrant whose geometry differs could expose another sub-pixel assumption in
  the inherited SB7 checks at the framed pose. The camera change is the single heaviest consequence for an
  entrant that misses it (Gemini's archived tree: 0.799 → 0.699), stated in bold in the contract.
- Inherited, not fixed: the D1 stream-witness refusal on a near-black field (c605215d) is SB7.1 behaviour;
  `j_workflow_journey` races in golden-sb71 remain (SB7.1's frozen reference was not edited).
- Not done (scope cut by the owner's coordinator, 2026-10-02): the second reference repeat with final bytes,
  the opt-in browser test file run (test_sb72_visual.py, SB72_BROWSER_TESTS=1), the SB7.1 re-run of c605215d.
  Desktop packaging verifies sb7.2/release-manifest.json (main 3c65824af); the starter text still says SB7.1.

