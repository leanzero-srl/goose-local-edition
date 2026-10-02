# SB7.2 — payment towers, a legible overview and committed-update inspection

This extends the supplied SB7 contract. Preserve its full payment collection, money arithmetic, Berlin-day placement, stable arrival indices, transaction semantics, fault recovery, raw WebGL, GPU picking, camera interaction, linked brush, label culling, SSE diffs, and rendering budgets. There is no new backend service. Only the geometry, the default overview camera and the explicitly described inspection mode below supersede the old solid-column shape, camera defaults and fixed camera target.

## Overview: a payment is a structured tower

Keep the original center `(x,z)`, original height `h`, base `y=0`, and maximum footprint `0.9 × 0.9`. Render each payment as the union of these four centered, axis-aligned closed solids; dimensions are world units and vertical fractions multiply that payment's `h`:

| Part | Width and depth | Vertical interval |
|---|---|---|
| Pedestal | 0.90 | 0 to 0.12h |
| Shaft | 0.54 | 0.12h to 0.90h |
| Status cap | 0.90 | 0.90h to h |
| Currency collar | EUR 0.62; USD 0.70; JPY 0.78; KWD 0.86 | 0.76h to 0.84h |

The collar overlaps the shaft. Surface intersection and occlusion are the union of the actual solids, not the enclosing rectangular box. Both visible rendering and the offscreen identity pass use that same structure. A ray through the open shoulder beside the narrow shaft must not select an invisible bounding-box face.

Keep the exact SB7 status colors and dimming rules. Only the absolute top surface `y=h` uses full status color. For explicit directional depth, X-normal vertical surfaces use factor 0.55, Z-normal vertical surfaces use 0.72, and all other horizontal surfaces use 0.82. Multiply the status RGB by that factor and round each channel; apply the brush dim before the factor. This published directional shading supersedes SB7’s uniform side factor. No gradients, textures or outline replace this diagnostic color encoding. No amount, height, position, or status may be invented for visual effect.

The original data digest still represents one payment and one `h`, not four parts. The four parts must fit within the existing draw and upload budgets; a scene assembled from four unbatched draws per payment does not pass. The expanded pick oracle uses the four-solid union, while the original field layout, identity, depth, moment, height, and streaming requirements remain.

## Detailed inspection: the ledger spire

The overview above remains unchanged. Inspection renders a detailed version of the **same real payment** on the same canvas. This is an explicit inspection-only exception to SB7's prohibition on LOD: every overview payment still uses all four published solids, and no payment is omitted or decimated. The detailed object retains its exact center, amount-derived height, maximum 0.90 footprint and status truth.

In inspection, replace the overview solids with this union of closed solids. Coordinates below are relative to the payment center in X/Z, while Y fractions multiply its actual height:

- Pedestal: y=0..0.12h; status cap: y=0.90h..h. Both have an octagonal footprint defined by `abs(x)<=0.45`, `abs(z)<=0.45`, `abs(x)+abs(z)<=0.80`. These are clipped vertical corners, not rounded texture details.
- Recessed core shaft: width/depth 0.38, y=0.12h..0.90h.
- Four separate support ribs: width/depth 0.06, centered at each combination of x=±0.24, z=±0.24, y=0.12h..0.90h. They are actual geometry, not painted stripes.
- Hollow currency frame: outer width/depth remains EUR 0.62, USD 0.70, JPY 0.78, KWD 0.86. Four rails of thickness 0.04 enclose a square opening; y=0.76h..0.84h at rest. The EUR frame's inner corners may touch the ribs' outer corners. The opening is not filled; core/ribs/background seen through it must survive depth testing and picking.

Use a deliberate material hierarchy: core RGB (42,55,73), pedestal/ribs (190,207,223), status cap the original status RGB, and currency frame EUR (37,99,235), USD (6,182,212), JPY (234,88,12), KWD (147,51,234). Apply factor 0.55 to X-normal faces, 0.72 to Z-normal faces, and 0.82 to horizontal faces except the cap's absolute top, which uses 1. For a clipped vertical corner with normal `(nx,0,nz)`, factor is `(0.55*abs(nx)+0.72*abs(nz))/(abs(nx)+abs(nz))`. Round each resulting RGB channel. Inspection is undimmed. This gives dimensional contrast and distinct structural materials without adding a lighting library, textures or another service.

Both visible and identity passes use this actual detailed union while inspecting. The frame follows the committed-update trajectory below as one rigid hollow structure; the core, ribs, pedestal and cap remain stationary. Its movement must expose the actual structure behind it. Layout, typography, control styling and presentation are authored by the application; do not copy a separate showcase. The field and inspector should feel like one finished payments product.

## Overview and inspection are one product

The full-field default camera is **yaw 70, pitch 50, distance 190** around SB7's target `(0,1,0)`; SB7's projection, clamps, drag, wheel and coast laws are unchanged, and double-click and **Full field** return to these defaults. Keep all 12,288 payments and later creates. Provide a readable explanation of the axes, height, status color and currency-collar encoding near the field. `#tower-legend` names the four structural parts and all four currency widths. It must be actual visible text, not a tooltip-only or debug-only declaration.

Provide a labeled text input `#inspect-payment` accepting an existing payment ID, an **Inspect payment** button `#inspect-open`, a **Full field** button `#field-view`, and a **Replay payment update** button `#replay-event`. No native select or browser dialog. These controls live in `#inspector-controls`. Selecting a payment in the field can populate the ID input; inspection begins only when its control is used, so the existing brush interaction remains valid.

Inspection uses the same visible `#viz3d` canvas and real payment data. Show only the chosen payment's structure, with camera target `(x,h/2,z)`, yaw 35 degrees, pitch 25 degrees, distance 6, and the original 50-degree vertical field of view. This distance is an explicit inspection-only exception to the full-field minimum of 15. Do not change the stored scene or digest when entering inspection. The `vs7dbg.camera()` values continue to describe the real camera. Hide the full-field top-12 labels in this mode; show the complete selected-payment identity, currency, correctly formatted amount, status, and version in `#inspect-details` adjacent to the canvas. Those visible values occupy `#inspect-id`, `#inspect-currency`, `#inspect-amount`, `#inspect-status`, and `#inspect-version` within `#inspect-details`. Inspection uses the detailed materials above regardless of the overview brush. `vs7dbg.setCamera` accepts distance 6 while inspecting, so the same object can be checked at multiple azimuths. Full field restores target `(0,1,0)` and the default camera above. Inspection and all controls remain usable at 375 px width without horizontal page scrolling.

In inspection, `#tower-annotations` contains four visible part callouts, each with `data-part` equal to `pedestal`, `shaft`, `cap` or `collar`. Name the part and its actual width (shaft 0.38 in inspection); explain the four 0.06 support ribs in the visible legend. Place each callout beside the tower at its part’s projected vertical position, without covering the geometry or other callouts. At the documented desktop inspector pose, the seeded tallest payment per currency used by the geometry probe must use 40–90% of the canvas height. Every inspected payment must remain unclipped. The legend, details, callouts and controls must have readable text at least 12 CSS px, have at least 4.5:1 text contrast against their composed background, must not overlap or clip their essential content, and must not cover the tower. The desktop inspector canvas is at least 600 × 460 CSS px so ribs, corner cuts and frame openings are inspectable; mobile retains a 320 px minimum height and may reflow callouts beside or below the canvas without horizontal page scrolling. Use the additional space to make structural differences inspectable. The application still owns its visual design; the reference is not a layout template.

## Animate an actual committed payment update

When the currently inspected payment receives a newer committed SSE version, immediately apply its authoritative data as required by SB7 and animate its currency collar. This is a **payment update**, not a claim that money moved or a payment settled. Amount, height, pedestal, shaft, ribs, cap and status truth remain fixed by the backend throughout the animation.

Let `t` be elapsed seconds since receiving that newer version, clamped to `[0,1]`, and `s=t²(3−2t)`. For exactly one second the collar's bottom is `(0.20 + 0.56s)h` and its top is `(0.28 + 0.56s)h`. Its width remains currency-dependent. At the end it rests in its original interval `[0.76h,0.84h]` and demand rendering resumes. The camera does not move as a substitute for object animation. A newer update of that inspected payment restarts this trajectory from the new version. Do not animate duplicate or stale versions. Leaving inspection cancels its presentation animation without changing any backend state.

Keep the latest actually observed committed update for each payment while the page is open. Replay payment update is disabled until the inspected payment has such an update; clicking it replays that same one-second presentation without issuing a write or changing payment truth. `#replay-status` states **Payment update**, the version, and whether this is live or replay. Reloaded pages need not retain presentation replay history; backend durability remains governed by SB7.

## A legible overview

At the default camera in a 1280 × 800 desktop viewport the overview must read as a payments field, not a texture: size and place the canvas so it does. Measured on a composited screenshot with the whole canvas in view, label boxes excluded:

- Framing: pixels showing a published tower colour (status RGB × 1, 0.82, 0.72 or 0.55, ±8 per channel) span at least 60% of the canvas width and 60% of its height (0.5th to 99.5th percentile), and none lies within 4 CSS px of the canvas edge.
- Lighting: at up to 200 seeded pixels where the published geometry predicts a tower surface, the median WCAG contrast against `#101828` is at least 2.0:1.
- Status: of up to 96 seeded payments the grading schedule does not mutate, at least 64 must show a cap top covering a whole 2 × 2 block of screen pixels, and in at least 90% of those all four pixels must read as the payment's status: nearest published status colour, within RGB distance 40.

## Excellence rungs

0.12 × gate fraction × mean credit (1/.75/.5/.25): drag frames over 40 moves ≥40/32/24/12; stream visible ≤175/200/400/800 ms; burst p95 ≤150/300/600/1200 ms; optimistic note .5 painted while held, +.25 by ≤100/250/800 ms, +.25 saved; T+X+R mean ≥.90. Gate: 16 conditions (four journeys, clean console, 375 px, dates, scene binding, residual, no row loss, six P rungs).

## Evidence and grading

The grader compares the visible framebuffer and GPU picks with independently computed geometry: the overview at the default camera, and one seeded payment per currency in inspection at yaw 35 and 125, where every surface part and every gap (cut corner, open frame) must show its published colour within ±8 per channel. Stream latency runs from SSE receipt to the first drawn frame whose pixels show the new status. For animation it makes a real note edit: the collar must move along the published trajectory in the framebuffer and in screenshots through the second, replay must not write, a newer version restarts the motion, duplicate and stale versions change nothing, and leaving inspection restores the default camera, idle rendering and no visible callouts. Text is checked where it is drawn: 12 px, 4.5:1, unclipped, uncovered. Screenshots and a short clip come from this same graded session.
