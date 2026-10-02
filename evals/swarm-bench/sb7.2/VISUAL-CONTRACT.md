# SB7.2 — payment towers, a legible overview and committed-update inspection

This amends `SB7-CONTRACT.md` §8. Everything there still holds (data, layout, arrival indices, raw WebGL, GPU picking, camera laws, brush, labels, SSE diffs, budgets) except the column shape, the default camera and the inspection mode below.

## Overview: a payment is a structured tower

Keep the SB7 center `(x,z)`, height `h` and base `y=0`. Render each payment as the union of four centered, axis-aligned closed solids (widths in world units, heights as fractions of `h`):

| Part | Width and depth | Vertical interval |
|---|---|---|
| Pedestal | 0.90 | 0 to 0.12h |
| Shaft | 0.54 | 0.12h to 0.90h |
| Status cap | 0.90 | 0.90h to h |
| Currency collar | EUR 0.62; USD 0.70; JPY 0.78; KWD 0.86 | 0.76h to 0.84h |

Visible rendering and the pick pass both use this union (never its bounding box): a ray through the open shoulder beside the shaft hits what lies behind it.

Colour is the SB7 status colour times a face factor, rounded per channel, brush dim applied first: top surface `y=h` 1; X-normal vertical faces 0.55; Z-normal vertical faces 0.72; every other horizontal surface 0.82. No gradients or textures. The digest still counts one payment with one `h`, and the four parts fit within the SB7 draw and upload budgets.

## Inspection: the ledger spire

Inspection renders the same real payment, at its real center, height and status, in detail on the same canvas — the only exception to SB7's no-LOD rule. Coordinates are relative to the payment center; heights are fractions of its `h`:

- Pedestal y=0..0.12h and status cap y=0.90h..h, octagonal: `abs(x)<=0.45`, `abs(z)<=0.45`, `abs(x)+abs(z)<=0.80` (clipped vertical corners).
- Recessed core shaft: width/depth 0.38, y=0.12h..0.90h.
- Four separate support ribs: width/depth 0.06, centered at x=±0.24, z=±0.24, y=0.12h..0.90h.
- Hollow currency frame: outer width EUR 0.62, USD 0.70, JPY 0.78, KWD 0.86; four rails 0.04 thick around an open square; y=0.76h..0.84h at rest. Core, ribs and background seen through the opening survive depth testing and picking.

Materials: core (42,55,73); pedestal and ribs (190,207,223); cap the status RGB; frame EUR (37,99,235), USD (6,182,212), JPY (234,88,12), KWD (147,51,234). Factor 0.55 on X-normal faces, 0.72 on Z-normal faces, 0.82 on horizontal faces except the cap's top (1); a clipped corner with normal `(nx,0,nz)` uses `(0.55*abs(nx)+0.72*abs(nz))/(abs(nx)+abs(nz))`. Round each channel. Inspection is undimmed. Visible and identity passes both use this detailed union while inspecting.

## Controls and cameras

Default overview camera: **yaw 70, pitch 50, distance 190**, target `(0,1,0)`; SB7's projection, clamps, drag, wheel and coast laws are unchanged. Double-click and **Full field** return to it.

`#inspector-controls` holds a text input `#inspect-payment` (a payment ID), **Inspect payment** `#inspect-open`, **Full field** `#field-view` and **Replay payment update** `#replay-event`. Inspection starts only from `#inspect-open`, so field clicks keep brushing. A visible legend `#tower-legend` explains the encoding.

Inspecting shows only the chosen payment's structure on `#viz3d`, camera target `(x,h/2,z)`, yaw 35, pitch 25, distance 6, vertical field of view 50°; `vs7dbg.camera()` reports the real camera and `vs7dbg.setCamera` accepts distance 6 while inspecting (it is checked at yaw 35 and 125). Entering inspection does not change the stored scene or digest and hides the full-field labels. `#inspect-details` shows `#inspect-id`, `#inspect-currency`, `#inspect-amount` (formatted in its currency), `#inspect-status` and `#inspect-version` of that payment. Full field restores target `(0,1,0)` and the default camera.

`#tower-annotations` holds four visible callouts with `data-part` `pedestal`, `shaft`, `cap`, `collar`, each naming its part and its actual width (shaft 0.38 in inspection), placed beside the tower at its part's projected height, inside the canvas, covering neither the tower nor each other. The desktop inspector canvas is at least 600 × 460 CSS px; at yaw 35 and 125 the seeded tallest payment per currency spans 40–90% of the canvas height, unclipped. Text in the legend, details, callouts and controls is at least 12 CSS px with at least 4.5:1 contrast against its composed background, unclipped and uncovered.

## Animate an actual committed payment update

When the inspected payment receives a newer committed SSE version, apply its data immediately and animate only its currency frame. With `t` the seconds since that version arrived, clamped to `[0,1]`, and `s=t²(3−2t)`, for exactly one second the frame's bottom is `(0.20 + 0.56s)h` and its top `(0.28 + 0.56s)h`, width unchanged; it then rests at `[0.76h,0.84h]` and demand rendering resumes. Pedestal, core, ribs, cap, height, amount and status never move; the camera stays put. A newer version of that payment restarts the motion; duplicate or stale versions change nothing. **Replay payment update** replays the inspected payment's latest observed update the same way, with no write. Leaving inspection cancels the animation, restores the default camera and idle rendering, and hides the callouts.

## A legible overview

At the default camera in a 1280 × 800 desktop viewport, measured on a composited screenshot with the whole canvas in view, label boxes excluded:

- Framing: pixels showing a published tower colour (status RGB × 1, 0.82, 0.72 or 0.55, ±8 per channel) span at least 60% of the canvas width and 60% of its height (0.5th to 99.5th percentile), and none lies within 4 CSS px of the canvas edge.
- Lighting: at up to 200 seeded pixels where the published geometry predicts a tower surface, the median WCAG contrast against `#101828` is at least 2.0:1.
- Status: of up to 96 seeded payments the grading schedule does not mutate, at least 64 must show a cap top covering a whole 2 × 2 block of screen pixels, and in at least 90% of those all four pixels must read as the payment's status: nearest published status colour, within RGB distance 40.

## Excellence rungs

0.12 × gate fraction × mean credit (1/.75/.5/.25): drag frames over 40 moves ≥40/32/24/12; stream visible ≤180/200/400/800 ms; burst p95 ≤150/300/600/1200 ms; optimistic note .5 painted while held, +.25×(1/.6/.3) by ≤100/250/800 ms, +.25 saved; T+X+R mean ≥.90 (else half, pro rata). Gate: 16 conditions (four journeys, clean console, 375 px, dates, scene binding, residual, no row loss, six P rungs).

## Evidence and grading

Framebuffer pixels and GPU picks are compared with independently computed geometry: the overview at the default camera, and one seeded payment per currency in inspection at yaw 35 and 125, where every part and every gap (cut corner, open frame) shows its published colour within ±8. Stream latency runs from SSE receipt to the first drawn frame showing the new status. The animation is exercised with a real note edit.
