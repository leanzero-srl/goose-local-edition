# sb-7 — FINAL DESIGN

Synthesized 2026-08-19 from the three authored sections (3D, SYSTEMS, SCORING) and the two
red-team reports (A: R1–R9, B: F1–F9). Every red-team fix below is APPLIED in place — the
contracts in this document are the post-fix contracts, not the drafts. Killed or replaced
features are recorded in §6 (red-team log / design decisions). Both red-team reports arrived
truncated mid-finding (R9, F9): resolutions cover their stated heads, and §7 step 1 re-runs a
red-team pass over THIS document before anything is built on it.

House rules carried from sb-6: every number is either printed in the spec (spec-derived, fair
game) or derived from the per-run seed (non-memorizable); every check is graded by analytic
recomputation, wire-trace accounting, or pixel readback under the verified
SwiftShader-deterministic harness (`sb6/SB6-PROBE-3D.md` facts carry verbatim: pinned launch
args, forced `preserveDrawingBuffer`, wrapped GL entry points, bottom-left `readPixels` mapping,
±8/channel color tolerance, DPR forced to 1, `--js-flags=--random-seed`). No LLM judges.
Scoring is SERIAL and HERMETIC at the tree's advertised vendor port (F895–F897), and every
graded verdict records its `fixture_seed`.

---

## 1. Purpose & ceiling target

The final sb-6 board: Sol 0.9956 · Opus 0.9307 · Sonnet 0.8543 · fleet 0.5059 · Haiku 0.4489.
Two verdicts against the instrument:

1. **The top saturated.** Every sb-6 hard axis was spec-frozen with exact formulas, hence
   independently aceable from the spec text; the conjunction cost Sol 0.0044 total.
2. **The middle was empty.** 0.5059 → 0.8543 with nothing between: the hard tier was one
   mechanism a model either had (~1.0) or didn't (~0.5).

sb-7's mission, stated honestly (red-team F7 accepted — see §6):

- **The separating mass sits mid-board, where the local fleet lives** — which is where the
  campaign actually measures. Every hard tier is a fraction-of-many-scenarios instrument by
  construction: the 3D tier is five independent mechanisms with partial rungs, X pays per
  invariant point held, R pays per boundary survived. Partial systems competence lands 0.3–0.7
  instead of at a cliff.
- **The top is aceable by construction and we say so.** The freeze gate requires the golden
  reference to ace every check, and every check's semantics are printed deterministically —
  therefore a Sol-class single-shot run is EXPECTED at ~0.90–0.96, not 0.7–0.85. sb-7 does not
  claim an unaceable ceiling. What prices the top is breadth × single-shot discipline across
  ~110 checks under schedules the entrant did not choose: error mass compounds, and the ordering
  Sol > Opus > Sonnet is expected to survive even with the ceiling compressed.
- **Design targets for the calibration pass** (goals, not gates): fleet arms spread across
  ≥ 0.25 of the scale instead of collapsing to one value ± 0.003; a mid model lands ~0.45–0.65
  on the 3D tier alone; the mutant-reference battery (§5.8) proves the mid-scale ruler is
  seed-invariant, which sb-6 never proved.

Saturable mass (tiers a pattern-matcher can perfect) is capped at 0.405 of the composed score;
non-saturable mass is 0.595 (§5.2). sb-6's were 0.581 / 0.419.

---

## 2. The task

The entrant builds, single-shot from the spec, a two-service payments console against the
**Meridian API v3** vendor mock: `ledgerd` (vendor sync, event ledger, drafts/approval, UI
host) and `notifierd` (idempotent event consumer, notifications), plus a browser frontend with
the payments table, the 3D instanced field, screen-space labels, linked brush, live SSE stream,
the approval workflow, and the notifications feed. Backend: stdlib-only Python. Frontend: no
libraries (the asset budget enforces it), raw WebGL.

### 2.1 Deliverables

| Deliverable | Contract |
|---|---|
| `app/` package | `python -m app --db-dir P --ledger-port N --notifier-port M --vendor URL --tokens-file T` boots both services; `python -m app.ledgerd --db-dir P --port N --notifier http://127.0.0.1:M --vendor URL --tokens-file T` and `python -m app.notifierd --db-dir P --port M` boot one each (the harness starts and kills them independently) |
| `ledger.db`, `notifier.db` | one SQLite file per service under `--db-dir`; each service touches only its own file |
| `web/index.html`, `web/styles.css`, `web/app.js`, `web/viz.js` | served by ledgerd at `/` with correct content types; self-contained (zero external requests); combined ≤ **150 KB** uncompressed (carried from sb-6 spec-build-v3 — hand-written code fits in a tenth; the budget exists so a vendored library cannot) |
| `DECISIONS.md` | the documented-corner register (§5.7): corners D1–D3, frozen heading format |

### 2.2 Boot contract

- Both services bind `127.0.0.1` only; listening within 10 s of process start.
- Boots with the vendor down (fault B4) and with the peer service down; neither service may
  crash on the other's absence, at boot or later.
- Restart against an existing `--db-dir` resumes cleanly: idempotent schema init, no data loss,
  no duplicate application of anything already committed.
- No operator action, ever: syncs, retries, reconnects, and heals are self-driven.
- Tokens file: JSON `{"maker": <32 hex>, "checker": <32 hex>, "admin": <32 hex>}`, written by
  the harness before boot; bearer auth per §4.5.

### 2.3 The collection — ONE dataset, structure frozen, values seeded

Red-team F5 resolution: the SCORING draft (N = 12,288 / 14-day span) and the 3D draft
(N ∈ [10500, 12000] / 90–110 days) described the same data with incompatible numbers, and the
brush link (table row ⇄ 3D instance) only makes sense over one collection. Final contract —
**one payments collection feeds the API tier, the 3D tier, and the systems tier**:

- **N = 12,288 exactly** (frozen). Every budget that says "full count" means this number.
  Scheduled creates during the run (1 mid-walk + ≤ 3 approved drafts) add ≤ 4 records; budgets
  quote 12,288 and the delta is immaterial.
- **Span D0 = 96 consecutive Europe/Berlin calendar days** (frozen), containing exactly one
  Berlin DST transition, placed ≥ 7 days from both window ends. Which transition is seeded from
  a 4-item menu: 2026-03-29, 2026-10-25, 2027-03-28, 2025-10-26. Bucketing is INSTANT-based
  (UTC instant → Berlin calendar date via `zoneinfo`), unambiguous in both directions;
  ambiguity only bites string-based local parsing, which is the defect class being graded.
- **Per-day count ≤ 180** (frozen cap; mean 128). Seeded per-day×status profile under frozen
  constraints: sum = N, every day ≥ 1, bounded skew.
- Statuses `settled | pending | refunded | failed`, each ≥ 8% of N. Currencies EUR/USD/JPY/KWD,
  exponents 2/2/0/3. Amounts span ≥ 3 decades in major units. All amounts integer minor units;
  all timestamps RFC3339; no cross-currency sum anywhere, API or UI.
- Every amount, currency assignment, counterparty, and timestamp is seeded; the vendor
  **value-dates** created payments (mid-walk create and approved drafts get a seeded
  `created_at` INSIDE `[d0, dLast]`, in a day whose load-time count ≤ R0 − 2) so the 3D layout
  basis never moves — a fixture guarantee, printed in the v3 docs.
- Vendor list pagination: **server-fixed page size 64** (red-team R3: v2 honored a client
  `limit`, which made the race schedule app-controlled; v3 has no limit param). 12,288/64 =
  **192 pages** — a fixture constant both sides can compute.

---

## 3. 3D contract (tier T) — THE INSTANCED FIELD

Separation intent (the sb-6 lesson): sb-6's 3D tier was one mechanism; sb-7's is FIVE
independent mechanisms of graduated depth — instanced rendering under a draw budget, a GPU pick
buffer with occlusion truth, an inertial camera with a closed-form coast law, deterministic
label culling that must consume the pick buffer, and diff-based streaming updates with byte
accounting — each with partial rungs. A mid model lands the sb-6-grade parts (context, layout,
drag) and misses the mechanisms → ~0.45–0.65 of the tier; a strong model bleeds on the coast
identity, the diff budget, and occluded picks → 0.70–0.88. The all-or-nothing cliff is gone by
construction. Probe emits facts; `score_sb7.py` judges; rungs marked CAL are owned by
`sb7-thresholds.json` behind the CALIB sha-pin; everything else the reference must ace at the
freeze gate.

### 3.1 Data → scene

**Source.** `GET /api/viz/records` returns the full collection, columnar, one fetch:

```json
{"count": N,
 "id": [...], "amount_minor": [...], "currency": [...], "status": [...],
 "created_at": [...], "day": [...], "version": [...]}
```

All arrays length N, initial order `(created_at instant ASC, id ASC)`. `day` is the
**server-computed Europe/Berlin calendar date** (`YYYY-MM-DD`) — the backend owns DST; a
frontend recomputing days in UTC produces probeably wrong x positions.

**Instance identity (amended, R2).** `n` = **stable arrival index**: initial records take their
serve-order position (0-based); each streamed create appends at `n = current count`. `n` NEVER
re-sorts. Pick idNum encoding, digest indexing, label `data-id` binding, and every `vs7dbg`
answer key off this `n` (and the record `id`). This is what makes a streamed create a genuine
`|S| = 1` diff (§3.7) instead of a full ID-attribute re-upload.

**Layout basis** — locked at page load, exposed via `vs7dbg.layout()`: `d0` = first day
present, `D0` = 96, `R0` = max in-day count at load. The basis does NOT change when streamed
records arrive (fixture guarantees: in-span value dates, target-day headroom ≤ R0 − 2).

**Per-instance transform** — `d` = Berlin day − d0 in calendar days; `r` = in-day rank at load
for initial records (same sort restricted to the day); a streamed create takes
`r = current in-day count` at apply time:

```
Δ = 1.2                                  (cell pitch, world units)
x = (d − (D0 − 1)/2) · Δ
z = (r − (R0 − 1)/2) · Δ
footprint 0.9 × 0.9 centered at (x, z), base y = 0
a_major = amount_minor / 10^exp(currency)      exp: EUR 2, USD 2, JPY 0, KWD 3
h = clamp(0.9 + 0.55 · log10(a_major), 0.2, 4.2)
```

Height goes through the currency exponent on purpose: a client that forgets JPY=0 or KWD=3
renders measurably wrong heights. Every record renders every frame (view-frustum culling legal;
no LOD/decimation — digest and close-up probes both count).

**Colors** (flat, unlit, exact):

| role | rule |
|---|---|
| top face | `settled #059669 (5,150,105)` · `pending #D97706 (217,119,6)` · `refunded #7C3AED (124,58,237)` · `failed #B91C1C (185,28,28)` |
| side faces | `round(0.55 · top)` per channel |
| brushed-dim (brush non-empty, instance NOT in it) | base `c' = round(0.30 · c)` applied BEFORE the side factor: side-of-dim = `round(0.55 · round(0.30 · c))` |
| background | `#101828 (16,24,40)`; every non-face pixel is exactly this — no floor, grid, axes, or in-canvas text |

All dim/side composites stay > 8/channel from the background (verified arithmetic) so the ±8
pixel tolerance never aliases a face into sky.

**Scene digest** — `vs7dbg.sceneDigest()`:

```
{count, Sh: Σh, Sh2: Σh², Sx: Σx, Sz: Σz, Sxh: Σx·h, Szh: Σz·h, brushedCount}
```

rounded to 4 decimals; graded `|Δ| ≤ max(0.5, 1e-4·|expected|)` against the probe's float64
recomputation from the seed. The moments are index-free (order-independent sums over records),
so the arrival-index pin costs nothing here (R2). Second moments make compensating-error gaming
implausible; a single wrong cell (1.2 in Sx) exceeds the absolute tolerance.

**Height pixel truth (added, R9).** The digest is app-reported, so it gets a pixel leg: at a
probe-chosen decisive close-up pose, for 6 seeded instances (≥ 1 JPY and ≥ 1 KWD among them),
the probe scans a device-pixel column at the instance's projected center, locates the top-face →
background/neighbor transition, and requires the measured top within ±3 px of the projected
`h`. Cross-checked against `sceneDigest` so a fabricated digest is caught (feeds
`t7_vs7dbg_truth`), and the money doctrine gets per-instance pixel evidence.

### 3.2 Rendering — bounded draw calls, pinned window, demand rendering

- `<canvas id="viz3d">`, context `webgl` or `webgl2` created `{antialias:false, alpha:false}`,
  main thread, no OffscreenCanvas/Worker (vs7dbg needs synchronous scene access). Backing store
  = `clientWidth × DPR` (likewise height). Raw WebGL; the 150 KB budget forbids libraries.
- Depth testing ON; draw order free (the occluded-pick constructions kill last-drawn-wins in
  both index orders — §3.3).
- **Draw budget: ≤ 8 draw calls to the default framebuffer per rendered frame at full count.**
  Counting is part of the contract: the probe's init-script wrapper counts
  `drawArrays/drawElements[Instanced]` and classifies each by the framebuffer bound at call
  time (wrapped `bindFramebuffer` tracks state; `null` = default = scene draw).
- **Wrapper v2 (amended, R6).** The sb-6 wrapper wrapped only the context object and was blind
  to the exact technique the budget forces. sb-7's wrapper also wraps `getExtension` and
  returns proxied extension objects with counted entry points (`ANGLE_instanced_arrays`
  `draw*InstancedANGLE`, `WEBGL_multi_draw`, `WEBGL_draw_buffers` passthrough), and counts
  `bufferSubData` bytes per target. The freeze gate includes a WebGL1+ANGLE reference variant
  so the wrapper's coverage is itself gated.
- **Budget window pinned (amended, R5):** the scripted budget drag is M = 40 pointer moves; the
  counting window is [dispatch of the first move, dispatch of pointerup]; counters are sampled
  synchronously at pointerup + one rAF; the drag ends with a slow release (< 6 px/s) so no
  coast starts inside the window. With `ΔD` = default-FBO draw delta (probe-owned) and `ΔF` =
  `vs7dbg.frames()` delta (app-owned, cross-checked): pass requires `ΔD ≤ 8·max(ΔF, 1)` AND
  `ΔD ≤ 8·(M + 8)`. A lying `frames()` is separately caught by `t7_vs7dbg_truth`.
- **Demand rendering, first-class (amended, R5):** at rest — no input, no active coast, no
  pending stream batch — **0 default-FBO draws over any 500 ms probe window**. The P tier
  grades idle flatness on the same rule; continuous-rAF renderers fail it by design and the
  rule is stated here, not hidden in a camera-section aside.
- Per-frame uniform uploads are free. "Buffer realloc" = any `bufferData` call with
  `byteLength > 4096` (small camera-UBO uploads stay legal everywhere); where reallocs and
  upload bytes are forbidden/bounded is §3.7.

At N = 12,288 the budget forces instanced draws or a merged buffer — both legitimate; the
budget is the interface, not the technique.

### 3.3 GPU pick buffer

**Structure.** Offscreen framebuffer (RGBA8 color + depth attachment), sized exactly to the
drawing buffer (device px), no MSAA. Every instance renders into it with an identity color:

```
idNum = n + 1                (n = stable arrival index; 0 = background)
r = idNum & 255,  g = (idNum >> 8) & 255,  b = (idNum >> 16) & 255,  a = 255
clear color (0,0,0,0 or 0,0,0,255 — decode ignores a)
decode: idNum = r + 256·g + 65536·b;  0 → background, else record id[idNum − 1]
```

Depth testing ON in the pick pass — picking is geometric truth: nearest rendered surface wins,
exactly as the depth buffer says.

**Surface.** `vs7dbg.pick(sx, sy)` → `{id, index}` or `null`, answered from the pick buffer at
device pixel `(round(sx·DPR), Hdev − 1 − round(sy·DPR))` against the live camera.
`vs7dbg.pickPixel(sx, sy)` → raw `[r,g,b,a]` from the pick FBO. Graded three ways at once:
decode(pickPixel) == pick's answer == the probe's analytic front instance.

**Real-pass evidence** (structural, counter-based): the wrapper classifies `readPixels` by
bound FBO. After each scene invalidation (camera change or applied batch), the first
`pick`/`pickPixel` call must be accompanied by ≥ 1 offscreen draw AND ≥ 1 offscreen
`readPixels` since the invalidation; subsequent picks may serve from a CPU-side cache (legal,
good engineering). Pick-pass budget ≤ 4 offscreen draws per refresh. A `pick()` call causes
**0 default-FBO draws** — ID colors never flash on the visible canvas. CPU raycasts with
fabricated `pickPixel` bytes fail the counters; visible-canvas ID rendering fails the
default-FBO accounting.

**Decisive targets (amended, R4).** Every graded pick point is probe-chosen so its analytic
front instance wins by ≥ 3 device px laterally AND by ≥ 0.002 NDC depth over the runner-up,
with a unanimous 3×3 device-pixel neighborhood in the probe's own model. The freeze gate
asserts decisiveness holds across the calibration seed battery — the golden must never bleed on
a rasterization edge rule.

**Occlusion constructions.** Per seed, the graded set includes at least: one target occluded by
a nearer instance with LOWER `n`; one occluded by a nearer instance with HIGHER `n` (together
they kill last-drawn-wins in both index orders); one partially-occluded case at the ≥ 3 px
margin; one background point inside the field's convex hull.

**Click semantics:** pointerup within 5 px and 300 ms of pointerdown on the canvas is a click;
click on an instance toggles it in the brush set (§3.6); click on background clears the brush.

### 3.4 Camera — orbit + inertia

Projection and orbit math carried structurally from sb-6 (grader recomputes independently):

```
θ = yaw·π/180   φ = pitch·π/180   T = (0, 1, 0)
eye = T + distance · (cos φ · sin θ,  sin φ,  cos φ · cos θ)
f = normalize(T − eye)   r = normalize(f × (0,1,0))   u = r × f
q = p − eye;  xc = q·r;  yc = q·u;  zc = q·f;   zc ≤ 0.5 → does not project
fovY = 50°,  k = 1/tan(fovY/2),  aspect = Wcss/Hcss,  near 0.5 / far 1000
ndcx = (k/aspect)·xc/zc    ndcy = k·yc/zc
sx = (ndcx+1)/2·Wcss       sy = (1−ndcy)/2·Hcss        (CSS px, canvas top-left)
```

Defaults `yaw 30, pitch 40, distance 260`. Clamps: pitch `[5, 85]`, distance `[15, 340]`; yaw
unbounded, compared modulo 360. **Drag:** `yaw ← yaw − 0.30·Δx`,
`pitch ← clamp(pitch + 0.30·Δy, 5, 85)` per pointermove (CSS px). **Wheel:**
`distance ← clamp(distance · exp(0.0012·deltaY), 15, 340)`; canvas consumes its wheel events.
**Double-click:** reset to defaults AND zero all velocity.

**Inertia (the graded mechanism).** Angular velocity `(vyaw, vpitch)` in deg/s; at release it
equals the rate implied by the last two move events (`v = 0.30·Δpx/Δt`, drag sign preserved).
After pointerup the camera coasts under exponential decay, **τ = 0.4 s**:

```
v(t) = v0 · e^(−t/τ)          yaw(t) = yaw0 + v0·τ·(1 − e^(−t/τ))
stop when |vyaw| < 2 and |vpitch| < 2 deg/s   (demand rendering resumes: no further draws)
pitch clamps apply continuously during coast; hitting a clamp zeroes vpitch
pointerdown or dblclick cancels the coast; wheel does not
```

Graded rungs (amended, R7):

- **Remaining-coast identity:** at any coasting instant, `yaw_rest − yaw(t) = v(t)·τ`.
  `vs7dbg.camera()` reports `{yaw, pitch, distance, vyaw, vpitch}` synchronously; the probe
  samples `(yaw_t, v_t)` twice mid-coast and once at rest and checks
  `|yaw_rest − (yaw_t + v_t·τ)| ≤ max(1.0°, 0.15·|v_t·τ|)`.
- **Reality (non-circularity):** after a fast scripted flick (≥ 600 px/s ⇒ v0 ≈ 180°/s ⇒ ~72°
  coast), yaw keeps moving in the drag direction ≥ 3° past release, confirmed from `camera()`
  AND from mid-coast pixel projection spot-checks.
- **Slow release:** last-move rate **< 6 px/s** (⇒ v0 = 1.8°/s, genuinely below the 2°/s stop
  threshold — the draft's 8 px/s was arithmetically above it) ⇒
  `|yaw_rest − yaw_release| ≤ 0.5°`.
- **Settle budget:** ≤ `τ·ln(max(v0_reported, 2)/2) + 0.7 s`, capped at 2.5 s, with
  `v0_reported = |camera().vyaw|` sampled at release — jitter in dispatch timing inflates v0
  and the budget scales with it instead of failing the reference.
- **Harness guarantees (spec-level):** drag scripts have pinned move counts and spacing; the
  two release-velocity moves are dispatched ≥ 30 ms apart; drags that must not coast end
  < 6 px/s.
- **Cadence fact rule (R8):** the SwiftShader frame time at full count is MEASURED on the
  reference at freeze and recorded in SB7-PROBE facts. If median frame < 22 ms, the coast check
  runs under a pinned CDP `Emulation.setCPUThrottlingRate` so per-frame-constant decay
  (`v *= k` per frame, tuned at 60 Hz) drifts measurably outside tolerance. The trap's
  discriminating power is a measured fact, not an assumed one.

### 3.5 Screen-space labels — deterministic collision culling

- **Candidates:** the 12 records with highest `a_major` (tie-break `id` ASC). Fixture
  guarantees distinct amounts and ≥ 6 distinct days among them.
- **Anchor:** `A = project(x, h, z)` of the instance's top-center, live camera.
- **Eligibility:** `A` non-null, inside the canvas, AND `pick(A.sx, A.sy)` returns that
  instance — labels are occlusion-culled through the app's own pick buffer. This is the forcing
  function: label culling cannot be faked without a working pick pass.
- **Geometry:** each label is a DOM element in `#viz-labels` (absolutely positioned over the
  canvas, never drawn into it), class `viz-label`, `data-id`, border-box exactly 110 × 18 CSS
  px (single line, ellipsized), top-left at `(A.sx + 10, A.sy − 9)` ± 2 px. Text contains the
  record's amount formatted in its OWN currency (money rules apply here too).
- **Culling:** consider candidates in priority order (`a_major` DESC, `id` ASC); show iff the
  rect intersects (≥ 1 px) no already-shown rect AND eligible; else cull (hidden or absent — no
  nudging, no alternate positions). Labels update per rendered frame and re-cull after
  `vs7dbg.setCamera`.
- **Grading poses (amended, R4):** the exact-shown-set is graded ONLY at probe-chosen decisive
  rest cameras — every candidate's anchor pick unanimous over a 3×3 device-px neighborhood in
  the probe's model, and every candidate pair either ≥ 5 px overlapped or ≥ 5 px clear at exact
  positions (so the legal ±2 px placement cannot flip an intersection). The probe's pose search
  is bounded and seed-deterministic; the freeze gate asserts a decisive pose exists per
  calibration seed. The app-side zero-overlap assertion among rendered rects is margin-free and
  is asserted at any pose. Pixel sampling elsewhere skips points under label rects (the probe
  knows every rect analytically).

### 3.6 Linked brush — table ⇄ instances

One brush set of record ids, two doors, one truth (`vs7dbg.brush()` returns it sorted):

- **Table → 3D:** clicking a table row toggles that record; the row carries
  `data-brushed="true"` while in the set; `#brush-count` shows the size. When non-empty,
  non-members render at the 0.30 dim and members keep exact status hex — graded by pixel probes
  on member and non-member tops at a decisive close-up camera. A brush toggle is bounded by the
  §3.7 byte budget (dim must be a per-instance flag + uniform, not a rebuild).
- **3D → table:** clicking an instance toggles it AND, when the record matches active filters,
  navigates the table to the page containing it under the current sort, row
  `data-brushed="true"` and scrolled into view. Background click clears the set and lifts the
  dim (pixel-verified back to full hex).
- **DECISIONS corner D1:** whether the brush survives a streamed mutation of a brushed record
  is deliberately unstated. Document the choice in DECISIONS.md; the grader verifies documented
  == observed across a scripted mutation of a brushed record. Either answer passes; an
  undocumented or contradicted one does not.

### 3.7 Streaming updates — diff semantics with byte accounting

- **Transport:** the page consumes `GET /api/stream` (SSE, `text/event-stream`) from ledgerd.
  Each message is one atomic batch:
  `{"batch": k, "records": [{id, amount_minor, currency, status, created_at, day, version}, …]}`.
  Batches carry the mutations the systems tier commits (webhook-applied flips, the refund pair,
  creates); store-truth → pixels is what this section grades.
- **Diff rule (amended, R2):** applying a batch touches exactly the minimal changed-instance
  set `S` under §3.1 — a status flip or amount change touches 1; a create touches 1 (appends at
  `n = count`, `r = in-day count`; NOTHING re-ranks — the draft's mid-day-rank-insert clause is
  DELETED, and the contract now matches what the fixture fires).
- **Upload accounting (amended, R6):** during a batch-apply window, uploaded buffer bytes
  (`bufferData` + `bufferSubData`, all targets) ≤ `|S|·stride + 4096`, and no realloc
  (`bufferData > 4096`). A full-array `bufferSubData` re-upload no longer sails under a
  realloc-only rule.
- **Graded:** digest delta vs probe recomputation; changed-instance pixels at a decisive
  close-up (new color/height); upload-byte accounting; apply latency (P tier, CAL); brushed-
  record mutation behavior vs DECISIONS D1.

### 3.8 vs7dbg surface & truth

`window.vs7dbg`, all synchronous: `layout()`, `sceneDigest()`, `camera()`,
`setCamera(yaw, pitch, distance)`, `pick(sx, sy)`, `pickPixel(sx, sy)`, `brush()`, `frames()`.

`t7_vs7dbg_truth` cross-checks the surface against reality: `camera()` vs pixel projection
spot-checks; `sceneDigest()` vs seed recomputation AND the height-pixel rung; `frames()` vs the
wrapper's draw deltas; `pick()` vs `pickPixel()` vs analytic truth. A lying surface collapses
the check and root-blocks its dependents — it is never `unavail()` (§5.6 doctrine).

---

## 4. Systems contract

Backend stdlib-only Python. `ledgerd` and `notifierd` as in §2.1; vendor mock is
`bench/vendor_service_v3.py` serving Meridian API v3 at the tree's advertised port.

### 4.1 S0 — determinism spine: `bench/schedule_sb7.py`

Everything scheduled derives from one function in one module, imported by the vendor mock, the
harness driver, and the scorer — never three copies:

```python
def derive(seed: str) -> Schedule   # seed: 16-hex run seed from the manifest
# D(label, i) = HMAC_SHA256(seed, f"{label}:{i}") -> bytes -> ints/choices, documented bands
```

**Cardinalities are FROZEN; only placements/targets/values are seeded** (red-team F4a: fraction
denominators must not vary by seed). Classification per red-team R1: **[U]** = unconditionally
firable (vendor-side commits/arms, driver kills by trace-tail or state-poll) — unfired [U] ⇒
structural refusal, a harness failure, never an app zero; **[A]** = app-dependent (needs the
walk to reach a page, a conditional request, sync #2 to start) — unfired [A] ⇒ the driver logs
`{"sched-unreached": "<field>", "reason": ...}` on a timeout, the scorer zeroes exactly the
dependent rungs, grades the underlying spec violation where one exists (never sending
conditional requests is itself a graded C-tier failure), and scores everything else. No app
behavior can convert its own failure into a refusal.

| Field | Frozen / seeded | Class |
|---|---|---|
| `race_pages: [k1<k2<k3<k4]` | 4 distinct pages in `[2, 191]`, pairwise ≥ 2 apart from each other and from `j`, `j2`; keyed to the k-th **200-served list response** of sync #1 (retries, 304s, reconnects don't count) | [A] |
| `race_mutations[k]` | exactly 4 target ids per trigger page: 2 from already-served pages, 2 from not-yet-served (well-defined now that page size is server-fixed) | [A] |
| `race_order[k]` | per trigger page, seeded flag: webhooks-before-page-flush or page-flush-then-webhooks-before-next-request (§4.3 barriers; 2 of each across the 4 pages) | [A] |
| `refund_txn` | one (payment, reversal) pair, target from an already-served page | [A] |
| `ooo_pair` | one payment given two bumps, v+2 delivered before v+1; rides a seeded race page of sync #1 | [A] |
| `dup_event` | one event id delivered twice; rides a seeded race page of sync #1 | [A] |
| `forged_event` | one delivery with a bad signature; rides a seeded race page of sync #1 | [A] |
| `midwalk_create` | one payment created between k2 and k3 (value-dated in-span, §2.3); fires when the walk reaches k2 | [A] |
| `drop_after_page: j` | connection dropped after page `j ∈ [2, 191]`, `j ∉ race_pages`, sync #1 | [A] |
| `http500_page: j2` | one 500 + `Retry-After` at page `j2 ∉ race_pages ∪ {j}`, sync #1 | [A] |
| `sigkill_after_list: n` | SIGKILL ledgerd after the n-th list response of sync #2, `n ∈ [2, 5]` | [A] |
| `vendor_down_boot_secs: w` | `w ∈ [3, 8]` — vendor refuses connections for the first `w` s of boot | [U] |
| `stale_304_sync: s` | `s ∈ {3, 4}`: the (s−2)-th sync that PRESENTS a stale conditional gets the armed stale-validator 304 (keying on staleness, never wall order — the arm cannot land on a sync whose validator is genuinely current) | [A] |
| `partition_after_event: X` | notifierd killed once its durable processed set reaches ledger seq ≥ X | [U] |
| `partition_commits` | **K = 8 exactly** (frozen count; seeded contents), ≥ 3 outbox-crossing, committed while the notifier is down | [U] |
| `approval_fixture` | 3 draft payloads (seeded amounts/currencies) + kill placements A1/A2 (§4.5) | [U] |
| `tokens` | maker/checker/admin bearer values, 32 hex each | [U] |

The app never sees the seed — only its effects. The vendor and driver log every armed/fired
entry into the trace as `{"sched": "<field>", "fired_at": ...}`; the scorer recomputes
`derive(seed)` and refuses to score only on unfired [U] entries. Every driver trigger is armed
with a timeout that logs `sched-unreached` instead of hanging.

**Kill-placement honesty:** kills trigger by tailing the trace (request counts) or polling
durable state (`processed ≥ X`), so a kill lands *at or after* the named boundary, never at a
guaranteed sub-request instant. Every graded invariant is placement-independent (idempotent
resume, dedupe, conservation, convergence) — grading never depends on where inside the window
the kill landed. That is what makes the class deterministically gradeable.

**Seed plumbing:** the harness draws a fresh seed per graded run, passes it to vendor + scorer,
stamps it into the verdict (`fixture_seed`), the vendor trace header, and the run archive.
Re-scoring an archived tree replays its recorded seed. Campaign-side, runs are
**seed-PAIRED**: all arms in one tick draw the same seed, so lever deltas stay seed-controlled
while cross-run memorization stays dead (F4c).

### 4.2 The two services

`ledgerd` owns `ledger.db`; `notifierd` owns `notifier.db`. Neither crashes when the other (or
the vendor) is down, at boot or later.

**ledgerd — additions to the core API** (core payments/summary/buckets API carries the sb-6
v3-spec shapes: envelopes, filters, paging, money rendering, DST buckets):

| Method | Path | Response |
|---|---|---|
| `GET` | `/api/events?after=<seq>&limit=<int>` | `{"events": [...], "latest_seq": <int>}` — append-only ledger |
| `GET` | `/api/outbox/status` | `{"pending", "delivered", "last_delivered_seq", "notifier": "up"\|"down"}` |
| `GET` | `/api/notifications?limit=&offset=` | proxied to notifierd; unreachable → `502`, envelope code `"notifier_unreachable"` |
| `GET` | `/api/viz/records` | §3.1 |
| `GET` | `/api/stream` | §3.7 (SSE) |
| — | drafts endpoints | §4.5 |

**Event log.** Every applied state change appends exactly one event; `seq` strictly increasing
and contiguous from 1 (a gap is evidence of a lost write and is graded as one):

```json
{"seq": 217, "type": "payment.updated", "payment_id": "pay_x", "version": 3,
 "source": "webhook", "txn": null, "at": "<rfc3339 UTC>"}
```

`type` ∈ `payment.created | payment.updated | reversal.created | draft.created |
draft.submitted | draft.approved | draft.rejected | payment.sent`. `source` ∈
`sync | webhook | local | approval`. `txn` carries the vendor transaction-group id when part of
one, else `null`.

**Outbox.** Event types that cross to the notifier — `draft.submitted`, `draft.approved`,
`draft.rejected`, `reversal.created`, `payment.sent` — are written to an outbox table **in the
same SQLite transaction as the state change**. A background relay delivers batches of ≤ 50,
ascending seq, `POST /notify/events`, retry with backoff capped at 2 s, at-least-once; a row is
marked delivered only after a 200. The relay never runs inside a user request handler; a user
write never blocks on notifier availability. Dual-write (POST then commit) is the trap: a kill
between the two loses the event, and B6 arranges exactly that window.

**notifierd — full surface:**

| Method | Path | Response |
|---|---|---|
| `POST` | `/notify/events` | `{"events": [...]}` → `{"accepted": [seq...], "duplicate": [seq...]}` |
| `GET` | `/health` | `{"status": "ok", "received", "applied", "duplicate", "notifications"}` |
| `GET` | `/notify/processed?after=<seq>` | `{"processed": [{"seq", "type"}...], "latest_seq"}` — durable |
| `GET` | `/notify/notifications?limit=&offset=` | `{"data": [{"id", "event_seq", "kind", "message", "at"}...], "total"}` newest first |

**Idempotent consumer.** Dedupe key = ledger event `seq`; a seq already in the durable
processed set → `duplicate`, state untouched. `received`/`applied`/`duplicate` counters are
in-memory per-process (sb-6 lineage); the processed set and notifications are DURABLE in
`notifier.db` and are what exactly-once is graded on.

**Selective materialization.** Exactly four types produce exactly one notification row each:
`draft.submitted`, `draft.approved`, `draft.rejected`, `reversal.created`. `payment.sent` is
processed and recorded but produces NO notification. "Notify everything" and "notify nothing"
are both wrong; the expected notification multiset is computed by the scorer from the run's
committed ledger events, never hand-written.

**UI feed.** `#notifications` reads only the ledgerd proxy. Notifier down → visible degraded
state, `#notifications[data-state="degraded"]`; recovers to `data-state="live"` without reload
after heal (poll ≤ 5 s). Both states are probe-clicked.

### 4.3 The racing window — deterministic barriers (amended, F6)

The draft let response bytes and webhook POSTs interleave on wall-clock; that makes cliff
criticals nondeterministic run-to-run. Final contract: **all X/R interleavings are
request-order BARRIERS enforced by the vendor; only the P tier measures wall time.**

During sync #1, on serving the k-th 200 list response with `k ∈ race_pages`, the vendor commits
`race_mutations[k]` (status/note bumps, `version` +1 each) against 2 already-served and 2
not-yet-served payments, then per `race_order[k]`:

- **order A (webhook-first):** deliver the signed webhooks and await the app's 2xx per
  delivery, THEN flush the page body;
- **order B (page-first):** flush the page body, then deliver the webhooks and hold the app's
  NEXT list response until deliveries complete.

Two race pages run each order (seeded assignment), so both webhook-then-page and
page-then-webhook cases are exercised deterministically. Delivery attempts time out at 10 s →
`sched-partial` logged, R1's [A] path applies. The schedule also fires, at seeded positions:
the out-of-order pair (v+2 before v+1), the duplicate delivery, the forged signature (401,
`rejected` counter +1, state untouched), and the mid-walk create (the v3 walk is documented as
NOT snapshot-isolated: the created payment may or may not appear on a later page; the webhook
is authoritative; the app must end with exactly one row).

**Refund transaction group.** A refund commits two changes atomically on the vendor: the
payment flips `status → refunded` (version bump) and a reversal appears in
`GET /v3/reversals` — `{"id", "payment_id", "amount_minor" (same), "currency" (same),
"created_at"}`. Both webhooks carry `"txn": {"id": "txn_9", "part": 1, "of": 2}`. The v3 docs
state: consumers MUST apply a transaction group atomically — stage parts until complete, then
apply in one local transaction. Payments keep the frozen 4-status vocabulary; reversals surface
only in the summary block and the notifier. `GET /api/summary` gains
`"reversals": [{"currency", "count", "total_minor"}...]` (ascending by currency, only
currencies with reversals). Still no cross-currency sum anywhere.

**The run ledger** the scorer grades over: the vendor's commit ledger (global `commit_seq` in
the trace), the wire trace, the scorer's timestamped read stream (polls `/api/summary` and two
seeded `/api/payments/<id>` every 250 ms through the window), the app's `/api/events`, and the
notifier's processed set.

**Linearizability battery** (per-payment versions are a total order, so per-key linearizability
reduces to exactly these; the txn group adds the one cross-key clause):

- **L1 — no invented states.** Every `(payment_id, version)` applied (event log) or served
  (read stream) is in the vendor's committed set.
- **L2 — per-key order.** Applied versions per payment strictly increase in event-log order;
  duplicate/stale outcomes appear in webhook counters, never as events.
- **L3 — monotonic reads.** The version served for a payment never decreases within the read
  stream. A sync page landing after a webhook applied v+1 must not regress the row (the
  buffered-blind-upsert failure).
- **L4 — convergence.** At quiescence (sync complete + webhook queue drained, vendor-signaled
  in the trace), every payment's version/status equals the vendor's final committed state and
  row count = vendor count (mid-walk create exactly once).
- **L5 — group atomicity.** No scorer read observes a half-applied transaction group (M2 is
  the observable form).

**Money-conservation battery:**

- **M1 — immutability.** No served row ever shows an `amount_minor` different from the
  vendor-committed amount (amounts never change in v3; only status/note/version do).
- **M2 — pair conservation, continuously.** In EVERY summary snapshot, per currency:
  `sum(reversals.total_minor)` == `sum(amount_minor of refunded rows)`. Both halves visible or
  neither, at every observable instant.
- **M3 — terminal conservation.** At quiescence, per currency: app totals/counts equal vendor
  ground truth (fixture + scripted mutations + app-created payments), reversals included.
- **M4 — no cross-currency sum**, API or UI.

### 4.4 The fault registry (brief item 5) — B1–B8

Every boundary where sb-6-class apps died by hand-found bugs is scheduled, fired, and
replay-verified. All graded behaviors are placement-independent.

| # | Boundary | Fired by | Correct behavior graded |
|---|---|---|---|
| B1 | Connection dropped after list page `j`, sync #1 | vendor [A] | walk resumes per docs (retry / cursor restart on 410); no duplicate or lost rows; sync in budget |
| B2 | One 500 + `Retry-After` at page `j2`, sync #1 | vendor [A] | single documented retry, then continue; never a fresh unconditional restart of committed work |
| B3 | SIGKILL ledgerd after the n-th list response of sync #2; restart; re-sync | driver [A] | convergence to L4/M3; no duplicates (idempotent upsert); clean cursor restart |
| B4 | Vendor refuses connections for first `w` s of boot | driver [U] | app binds, serves local data, UI shows degraded/error states; first post-recovery sync succeeds with no operator action |
| B5 | Stale-validator 304 on the (s−2)-th stale-conditional-presenting sync (`s ∈ {3, 4}`): vendor answers `304` to the app's STALE conditional request while carrying `X-Collection-Generation: g2` ≠ the stored generation. Docs: a 304 whose generation disagrees with your stored generation is a cache miss — drop the validator and refetch unconditionally, once | vendor [A] | exactly one unconditional refetch in the trace; mutations propagate; > 3 identical conditional requests in the window = the infinite-loop bug, scored; never stale served as fresh |
| B6 | SIGKILL ledgerd between an outbox commit and its relay delivery (arranged inside the B7 partition: notifier down, `pending > 0`, kill, restart, heal) | driver [U] | restart resumes the relay from durable outbox rows; notifier dedupes; every committed event delivered exactly once; none lost — the dual-write trap detector |
| B7 | Notifier partition: SIGKILL notifierd at durable processed ≥ X; ledgerd commits K = 8 events (≥ 3 outbox-crossing) while it is down; restart notifierd | driver [U] | user writes never block; `/api/outbox/status` reports `"down"` + growing `pending`; UI shows `data-state="degraded"`; after heal the relay catches up in seq order, exactly-once on the processed set; notification multiset matches the scorer's computation; UI back to `"live"` ≤ 5 s, no reload |
| B8 | Workflow kills A1/A2 (§4.5) | driver [U] | submitted/approved states durable across SIGKILL; exactly one vendor payment per approved draft (idempotency-key reuse on retry); no revert, no dupe |

### 4.5 Approval workflow (brief item 6)

**Roles.** Bearer tokens from the tokens file: `maker` (create/submit drafts), `checker`
(approve/reject — four-eyes: 403 `approval_forbidden` on a draft the same token submitted),
`admin` (read-only everything: drafts list, `/api/events`). Missing/bad token → 401; wrong role
→ 403. Envelope shapes carry sb-6's error contract.

**Draft state machine (frozen):** `draft → submitted → approved | rejected`; `approved → sent`
after the vendor accepts the payment. Rejected-draft disposition (terminal vs resubmittable) is
DECISIONS corner D2.

| Method | Path | Role | Effect + event |
|---|---|---|---|
| `POST` | `/api/drafts` | maker | create `{amount_minor, currency, counterparty, note}` → `draft.created` |
| `POST` | `/api/drafts/<id>/submit` | maker | state `submitted` → `draft.submitted` (outbox-crossing) |
| `POST` | `/api/drafts/<id>/approve` | checker | state `approved` → `draft.approved` (outbox-crossing), then SEND: `POST /v3/payments` with an `Idempotency-Key` (v3-documented; the vendor returns the same payment for a reused key); on 2xx append `payment.sent` (outbox-crossing, no notification) |
| `POST` | `/api/drafts/<id>/reject` | checker | state `rejected` → `draft.rejected` (outbox-crossing) |
| `GET` | `/api/drafts?state=` | any role | list |

The vendor-created payment is value-dated in-span (§2.3) and flows back through
webhook/sync like any other payment; M3's ground truth includes it. A send retry (crash,
timeout) MUST reuse the stored idempotency key — a fresh key per retry is the seeded dupe that
`r_no_dupe_effect` catches.

**Fixture drafts.** F1: driven through the UI by the probe — maker token entered in
`#role-token`, draft created via `#draft-form`, submitted, checker approves via `#approve-btn`;
`#draft-list` rows carry `data-draft-id` and `data-state`; the notifications feed shows
submitted/approved; the payment appears in the table after the vendor round-trip
(`j_workflow_journey`). F2: driven through the UI to rejection (D2 corner verified). F3: driven
over the API by the driver, carrying the kills:

- **A1** — SIGKILL ledgerd after F3's submit 200, inside the B7 partition window (notifier
  down, so `draft.submitted` sits in the outbox). Restart → state still `submitted`, outbox
  delivers exactly once after heal.
- **A2** — SIGKILL ledgerd after F3's approve 200, inside the send window (the vendor holds the
  create response to widen it). Restart → state still `approved`; the app completes or safely
  retries the send with the SAME idempotency key; exactly one vendor payment exists; exactly
  one `payment.sent` event.

---

## 5. Scoring — properties over schedules, not math over specs

Target file `bench/score_sb7.py`, same architecture as `score_sb6.py`: `@check(name, tier)`
registry, `g()` results, `unavail()` refusals, `compound(gate × min)`, DIAGNOSTIC
anti-stacking, ROOT_BLOCKS attribution, probe-unavailable exclusion (F17/F18), pure
`compose_from_rows`, `severity_selftest()` in the `--reference` freeze gate,
`sb7-thresholds.json` with `CALIB_SHA256` pin and the UNCALIBRATED banner. Composition, γ
tempering (cap 4.0), `score = 0.88·inner + 0.12·gate_fraction·e_mean`, and the critical
multiplier carry over byte-for-byte in shape.

### 5.1 What sb-6 proved

The two instrument verdicts of §1 (saturated top, empty middle), plus the sham-audit's severity
inversions (a benign console error cost −4.36 while wrong money was floor-protected at −0.72
and a SIGKILL-lost row cost 0.002) which were fixed late in sb-6. In sb-7 the severity
registry, multiplier, cliffs, and monotonicity selftest exist in the scorer skeleton BEFORE the
first entrant runs, and sb-6's 17 unjudged spec affordances become an enforced coverage ledger.

### 5.2 Tiers and weights

Inner weights sum 1.0, scaled by 0.88; E is the 0.12 excellence slice (proportional gate — the
all-or-nothing gate is dead and stays dead).

| Tier | w | Carries |
|---|---|---|
| A structure/boot | 0.04 | named files, interfaces, server runs, 150 KB asset budget |
| B API correctness | 0.09 | shapes, totals, money rendering, DST buckets, envelopes |
| C vendor discipline | 0.09 | pagination, Retry-After, ETag/304, batch partials, traps |
| D robustness/judgment | 0.06 | timeouts, content types, validation, `d_decisions_doc` |
| J journeys | 0.12 | first use, sync journey, approval workflow through the UI, error, empty |
| V visual/product | 0.06 | dates, money text, badges, responsive, styling |
| P performance | 0.08 | frame budget under drag at N = 12,288, idle draw/frame flatness, stream-apply latency, latency under stream, sync wall |
| T 3D | 0.14 | the five §3 mechanisms |
| X concurrency truth | 0.16 | L1–L5, M1–M4 over the run ledger, racing webhooks, mutation-during-pagination |
| R resumability/faults | 0.16 | B1–B8, convergence, outbox across the partition, workflow durability |
| E excellence | 0.12 slice | drag frames at full count, stream-apply latency, under-load latency, optimistic paint, mastery (mean of T+X+R ≥ 0.90 rung) |

```python
TIER_WEIGHT_SB7 = {"A": 0.04, "B": 0.09, "C": 0.09, "D": 0.06, "J": 0.12,
                   "V": 0.06, "P": 0.08, "T": 0.14, "X": 0.16, "R": 0.16}
E_WEIGHT = 0.12
assert abs(sum(TIER_WEIGHT_SB7.values()) - 1.0) < 1e-9
assert TIER_WEIGHT_SB7["T"] + TIER_WEIGHT_SB7["X"] + TIER_WEIGHT_SB7["R"] >= 0.46
assert E_WEIGHT >= 0.10
assert not (CRITICAL_CHECKS.keys() & CALIBRATION_OWNED)
```

Saturable mass (A+B+C+D+J+V = 0.46) × 0.88 = 0.405; non-saturable = 0.475 + 0.12 = 0.595.
sb-6's HARD (0.20) is split, not renamed: X grades what happens under concurrency the entrant
did not schedule; R grades what survives faults the entrant did not choose.

**E gate conditions** (proportional shares): `j_first_use`, `j_workflow_journey`,
`j_error_state`, `j_empty_state`, `console_clean` (the only binary condition),
`v_responsive_375`, `v_dates_readable`, `t_scene_binding`, `x_conservation_residual`,
`r_no_row_loss`, and the P rungs. Score-typed conditions contribute their measured value; a
frontier build with X/R gaps sees its E slice shrink continuously instead of vanishing.

### 5.3 The coverage ledger

```python
AFFORDANCE_ROSTER = {  # spec anchor -> (probe scenario, evidence key, check)
  "#sync-now":        ("sync",  "syncCausal",       "j_sync_journey"),
  "#approve-btn":     ("flow",  "approveCausal",    "j_workflow_journey"),
  "#viz3d.pick":      ("viz",   "pickOccluded",     "t_pick_buffer"),
  "#brush-link":      ("viz",   "brushHighlight",   "t_brush_link"),
  # ... full roster authored at scorer-skeleton time, seeded from sb-6's 17-item audit
}
_R = {n for n, _t, _f in SB7_CHECKS}
assert {c for (_s, _k, c) in AFFORDANCE_ROSTER.values()} <= _R
```

Import-time: every spec-named affordance maps to a registered check. Freeze-time: every roster
row must have produced non-vacuous causal evidence on the golden — probe-emitted, click-driven,
rendered-means-seen. A spec affordance nobody grades can no longer ship silently.

### 5.4 Severity — the invariant-leg doctrine, amended

**Doctrine.** Fractions price quality through tier weights (how many boundaries recovered, how
many invariant points held). **Invariant legs drive the multiplier**: cliff-shaped,
consequence-class facts — money created or destroyed, an acknowledged write lost, a delivery
effect duplicated, a committed row vanished, an approved state reverted, the primary flow dead.
Formula: `factor = m + (1−m)·severity_input`, `m = critical_multiplier_floor` (thresholds,
default 0.6), compounding across criticals. A held invariant multiplies by exactly 1.0.

Three red-team amendments (F1, F2, F3) are structural:

1. **Evidence-disjoint criticals (F1).** Conservation is defined as a RESIDUAL: unexplained
   creation/destruction after duplicated effects and lost writes are attributed. One dupe fires
   exactly one critical; a loss + an equal-value dupe no longer net to zero — the residual
   check fails on the attribution, so the offset fires two.
2. **Severity-input transforms (F2).** Raw fraction scores neutralize the multiplier exactly
   where harm is named (a 0.98-complete sync silently missing ~250 payments multiplied by
   0.992). Each critical owns a `_critical_severity_input` transform: data-loss-class fractions
   cliff to ≤ 0.5 severity input on ANY confirmed silent loss; `b_buckets_dst`'s severity leg
   is the DST-window cells specifically (the fraction still prices the tier weight).
3. **Multiplier dedup via ROOT_BLOCKS (F3).** ROOT_BLOCKS extends from attribution-only to
   multiplier dedup: a root critical that already multiplied suppresses its dependents'
   multipliers; vacuous legs (nothing to lose because nothing loaded) attribute to the root,
   price zero weight, and fire no multiplier. Dead-sync no longer compounds ×0.6⁴.

**Admission rule (enforced):** a check enters `CRITICAL_CHECKS` only if the golden provably
scores exactly 1.0 on it under every calibration seed — criticals are facts a correct app
achieves, never budgets. Import assert `CRITICAL_CHECKS ∩ CALIBRATION_OWNED = ∅`; freeze-gate
condition golden == 1.0 (not ≥ 0.95) on every critical.

**The registry** (12 members, every one golden-1.0-provable; cliffs are 1.0-or-collapse at the
check level, with vacuous-pass gates — `had_rows_before_kill`-style — so an app that never had
rows earns nothing for "not losing" them):

| Check | Tier | Consequence | Cliff inside |
|---|---|---|---|
| `server_runs` | A | crash — the tool does not run | boot ladder |
| `sync_completeness` | B | data loss — silently missing payments | severity input cliffs ≤ 0.5 on any confirmed loss; root-blocks dependents |
| `b_money_rendered` | B | wrong money — wrong exponent/digits or a cross-currency sum | cross-currency sum → leg 0 |
| `b_buckets_dst` | B | wrong money — mis-bucketed days | severity leg = DST-window cells |
| `x_conservation_residual` | X | wrong money — unexplained minor units created/destroyed after dupe/loss attribution | any residual ≠ 0 → 0 |
| `x_no_lost_write` | X | wrong money — an acknowledged mutation absent from final state | any lost ack → 0 |
| `r_no_row_loss` | R | data loss — a committed row missing after any seeded kill | any lost row → 0 |
| `r_no_dupe_effect` | R | wrong money — a ledger effect applied twice (outbox replay, send retry with fresh key) | any dupe → 0 |
| `r_cache_truth` | R | data loss — 304-vs-cache mismatch served as fresh | stale-as-fresh → 0 |
| `r_workflow_durability` | R | data loss — submitted/approved state reverting after SIGKILL | any revert → 0 |
| `j_loads_data` | J | dead primary flow — no data visible | DIAGNOSTIC, multiplier-only |
| `j_workflow_journey` | J | dead primary flow — approval cannot complete through the UI | journey ladder |

**Unavailability doctrine (F9).** `unavail()` is legal ONLY with recorded positive-control
evidence that the harness side works (probe alive, measurement path verified against the
reference surface) and the blocker is not the app. Every REQUIRED app surface — `vs7dbg.*`,
`/api/events`, drafts endpoints, workflow state — that is missing scores `g(0)` with ROOT_BLOCKS
attribution, never `unavail()`; criticals riding an absent surface take severity input 0 (an
invariant that cannot be evidenced is unproven, and the vacuous-pass gates already demand
positive evidence). Non-implementation must never outscore implemented-with-one-violation.

**The monotonicity selftest** — skeleton-first, wired into `--reference`; an inversion refuses
the freeze; passes on stubbed checks before the vendor or probe are written:

- sb-6 chain verbatim: `wrong_money < console_err`, `data_loss < console_err`,
  `dead_flow < console_err`, `console_err < cosmetic`, `console_err < minor`;
- single-defect scenarios: duplicated effect, stale-cache-served, approval revert, lost ack —
  each in its consequence class's cost band;
- **F1 scenarios:** one dupe fires exactly one critical; loss + equal-value dupe still fails
  `x_conservation_residual` (fires two);
- **F2 sweep:** severity inputs over {0.25, 0.5, 0.75, 0.98} per critical class with asserted
  per-class cost bands — not only orderings at zero;
- **F3 bands:** the dead-sync composite lands in a stated band (0.10–0.25) and strictly below
  every working-sync scenario — the bottom of the scale is pinned;
- **F9 scenarios:** composed runs containing `unavail` rows; absent-surface runs score strictly
  below present-with-one-violation runs;
- dominance: one violated invariant costs more than ALL cosmetic and minor defects combined;
- gradient sanity: kill-matrix 0.5 with all invariants held scores strictly above kill-matrix
  1.0 with one invariant violated — quality never outbids harm.

### 5.5 Seeded fixtures — structure frozen, values seeded, expectations derived

Frozen vs seeded per §2.3 and §4.1: N, span, page size, caps, vocabularies, API shapes, trap
COUNTS and classes, kill-boundary matrix coverage, partition K, workflow shape, and all
latency/frame/asset budgets are FROZEN; every value, placement, target, token, and window
choice is SEEDED. Fraction denominators are therefore seed-invariant (F4a).

Derivation discipline: `fixtures_v3.build(seed)` yields the dataset, event script, fault
schedule, and ALL expectations (`EXPECTED_TOTAL / EXPECTED_BUCKETS / EXPECTED_BY_CURRENCY /
EXPECTED_LEDGER_FINAL / OPTIMAL_REQUESTS / EXPECTED_WEBHOOK_COUNTERS /
EXPECTED_WORKFLOW_STATES / EXPECTED_NOTIFICATION_MULTISET`) as pure functions of the seed — one
derivation, three consumers (vendor serves it, harness renders the probe's expectation pack,
scorer recomputes independently); drift is impossible by construction. The sb-6 assert battery
(cells sum to N, DST cell present, walk non-degenerate) runs per-seed at import.

Determinism enforcement (F6): every X/R interleaving is a request-order barrier (§4.3); only P
measures wall time; the freeze gate runs the golden ≥ 2× per seed and demands identical
critical verdicts — the double-run doubles as the determinism detector.

### 5.6 CAL rungs and thresholds

CAL-owned rungs (perf budgets, stream-apply latency, settle margins beyond the frozen formulas)
live in `sb7-thresholds.json` behind `CALIB_SHA256`; everything else the reference must ace.
CAL rungs are set from the golden's measured distribution: **worst-of-5 minus margin** (F8), so
the golden never flakes against its own thresholds. `CRITICAL_CHECKS ∩ CALIBRATION_OWNED = ∅`
stays asserted. UNCALIBRATED banner until the pin lands.

### 5.7 DECISIONS.md — the documented-corner register

The spec deliberately leaves three corners unstated; `DECISIONS.md` (frozen headings `## D1` /
`## D2` / `## D3`) documents the builder's choice; `d_decisions_doc` (tier D) grades
documented ∧ consistent-with-observed per corner. Either choice passes; an undocumented or
contradicted one does not.

- **D1** — does the brush survive a streamed mutation of a brushed record? (verified across a
  scripted mutation, §3.6)
- **D2** — is a rejected draft terminal or resubmittable? (verified by attempting resubmit on
  fixture draft F2)
- **D3** — before sync #1 completes, does the table render empty-with-progress or block?
  (verified during the boot journey)

### 5.8 Golden plan & calibration (F4, F7, F8)

- **The golden is built FIRST**, as the executable spec oracle (sb-6 precedent:
  `bench/golden-vspro` exists and passed) — a first-class work item BEFORE spec freeze; the
  spec and scorer iterate against it. If the golden cannot ace a rung, the rung is wrong.
- **Calibration seeds:** K = 6 pinned inside `sb7-thresholds.json` under the CALIB pin.
- **Mutant references (F4b) — the mid-scale ruler proof.** Golden-minus-feature variants with
  known defect classes: `no-inertia`, `sorted-index-picks`, `dual-write-outbox`,
  `blind-upsert-sync`, `notify-everything`, `no-pick-buffer`. Each is scored across the frozen
  calibration seed set; per-mutant composed-score spread across seeds ≤ 0.02; each lands
  strictly below the golden and inside its expected band. Golden seed-invariance alone proves
  the ruler only at the top; the mutants prove it where sb-7's mission lives.
- **Tiered freeze (F8):** fast per-iteration subset (schedule + scorer selftests + probe smoke,
  minutes) vs the full sweep nightly on the workhorse (seeds × double-runs × mutants × kill
  matrix, hours).
- **Freeze-gate conditions:** golden aces every non-CAL check; every critical exactly 1.0 on
  every calibration seed; `severity_selftest()` passes; label/pick decisiveness asserts hold
  per seed; coverage ledger fully non-vacuous; double-run critical verdicts identical;
  WebGL1+ANGLE golden variant passes (wrapper coverage); mutant bands and ≤ 0.02 spread hold;
  SwiftShader cadence measured and recorded (R8).

---

## 6. Red-team log — finding → resolution

**Design decisions (features killed or replaced):**

1. KILLED — sorted-index instance identity and the V.7 mid-day-rank-insert clause (R2) →
   stable arrival index; creates are true `|S|=1` diffs.
2. KILLED — client-chosen page size during sync and wall-clock race keying (R3) → server-fixed
   page size 64; schedules keyed to 200-served list responses.
3. KILLED — blanket structural refusal on any unfired schedule entry (R1) → [U]/[A]
   classification; app-dependent unfired entries zero their rungs and scoring continues.
4. KILLED — unconstrained wall-clock webhook/response interleaving (F6) → vendor-enforced
   request-order barriers; only P measures time.
5. KILLED — exact label-set grading at arbitrary cameras and margin-blind pick targets (R4) →
   decisive-pose and decisive-target discipline, freeze-asserted per seed.
6. KILLED — open-ended draw-budget window and the coast-scoped demand-render aside (R5) →
   pinned budget window; demand rendering is a first-class §3.2 rule.
7. KILLED — the seeded N band [10500, 12000], the 14-day payments span, and the two-dataset
   ambiguity (F5) → ONE collection, N = 12,288 frozen, 96-day span, per-day ≤ 180.
8. KILLED — the "unaceable top" ceiling claim (F7) → §1's honest restatement; separation mass
   is mid-board by design.
9. REPLACED — raw-fraction critical multipliers and overlapping ledger criticals (F1, F2, F3)
   → residual conservation, severity-input transforms, multiplier dedup via ROOT_BLOCKS.
10. REPLACED — context-object-only GL wrapper and realloc-only byte accounting (R6) → wrapper
    v2 with proxied extensions and `bufferSubData` accounting.

**Full log:**

| # | Finding (condensed) | Resolution |
|---|---|---|
| R1 | Refusal rule lets a weak app poison its run into a no-score | ACCEPTED — §4.1 [U]/[A] classes, `sched-unreached` logging, timeouts; refusal unreachable via app behavior |
| R2 | Sorted index × pick IDs × byte budget mutually inconsistent under creates; V.1/V.7 contradiction | ACCEPTED — §3.1 stable arrival index; mid-day-insert clause deleted; digest noted index-free |
| R3 | Race schedule keyed to app-controlled quantities (limit param, retry counts) | ACCEPTED — §2.3/§4.1 fixed page size 64, 200-served keying, `j2 ∉ race_pages ∪ {j}`, ≥ 2 spacing, B1/B2 pinned to sync #1 |
| R4 | Exact-shown-set and pick grading falsifiable at geometric margins (golden bleeds) | ACCEPTED — §3.3/§3.5 decisive targets (≥ 3 px lateral, ≥ 0.002 NDC) and decisive poses (3×3 unanimity, ≥ 5 px pair separation), freeze-asserted |
| R5 | Budget window vs coast contradiction; demand rendering hidden in an aside | ACCEPTED — §3.2 pinned window [first move, pointerup], slow-release ending, first-class demand-render rule (0 draws / 500 ms at rest) |
| R6 | Wrapper blind to `getExtension` draw paths and `bufferSubData` uploads | ACCEPTED — §3.2/§3.7 wrapper v2, upload accounting `≤ |S|·stride + 4096`, WebGL1+ANGLE golden variant gates coverage |
| R7 | Slow-release numbers self-contradictory (8 px/s ⇒ 2.4°/s > threshold); settle budget has no jitter margin | ACCEPTED — §3.4 slow release < 6 px/s; settle ≤ τ·ln(max(v0,2)/2) + 0.7 s cap 2.5 s; ≥ 30 ms release-move gaps pinned as harness guarantees |
| R8 | 25–50 ms SwiftShader cadence asserted, not measured; the decay trap may not discriminate | ACCEPTED — §3.4 cadence measured at freeze, recorded in SB7-PROBE facts; CPU-throttle contingency if < 22 ms |
| R9 | (truncated in transit) Heights have no per-instance pixel truth; digest is app-reported | ACCEPTED for the stated head — §3.1 height-pixel rung (6 seeded instances incl. JPY/KWD, ±3 px, cross-checked vs digest). Any content beyond the truncation point was never received — §7 step 1 re-runs red-team on this document |
| F1 | Overlapping ledger criticals double-multiply; offsetting defects mask conservation | ACCEPTED — §5.4 residual conservation, evidence-disjoint criticals, F1 selftest scenarios |
| F2 | Fraction-valued criticals neutralize the multiplier where harm is named | ACCEPTED — §5.4 `_critical_severity_input` transforms, loss-cliff ≤ 0.5, DST-cell severity leg, {0.25…0.98} selftest sweep |
| F3 | Correlated-critical compounding empties the middle from below | ACCEPTED — §5.4 ROOT_BLOCKS multiplier dedup, vacuous-leg attribution, dead-sync band assert 0.10–0.25 |
| F4 | Golden seed-invariance proves the ruler only at the top; mid-scale comparability unproven | ACCEPTED — §4.1 frozen cardinalities (K = 8, 4 mutations/page, counts fixed), §5.8 mutant references with ≤ 0.02 spread, seed-paired campaign ticks |
| F5 | SCORING and 3D describe incompatible datasets; "full count" ambiguous | ACCEPTED — §2.3 one collection, N = 12,288 frozen, 96-day span, per-day ≤ 180; every budget names N. The 14-day span and the N band are dead |
| F6 | Wall-clock races make cliff criticals nondeterministic; freeze gate becomes a coin flip | ACCEPTED — §4.3 request-order barriers, both orders seeded; §5.5 golden double-run demands identical critical verdicts |
| F7 | "Sol lands 0.7–0.85" contradicts golden-aces-all; ceiling claim not credible | ACCEPTED (restatement chosen over unpublished property families, which would trade honesty for flakiness) — §1 states the aceable top, expected Sol-class 0.90–0.96, mid-board mission |
| F8 | Golden is a major unbudgeted deliverable; threshold separation capped by golden margins; freeze sweeps cost hours | ACCEPTED — §5.8 golden-first, K = 6 pinned seeds, CAL = golden worst-of-5 minus margin, tiered freeze with nightly full sweep on the workhorse |
| F9 | (truncated in transit) App-caused unavailability excludes-and-renormalizes; criticals skip the multiplier when unavail | ACCEPTED for the stated head — §5.4 unavailability doctrine (positive-control-gated `unavail`, absent surfaces = g(0) + root-block, severity input 0 on criticals, selftest composition scenarios). Content beyond the truncation point was never received — re-run in §7 step 1 |

---

## 7. Build plan

Every step is free/local except step 9, which is the ONLY paid step and requires Mihai's
sign-off. Each step commits as it lands.

1. **Red-team re-pass on THIS document** — both input reports were truncated (R9, F9); run a
   fresh red-team over the synthesized contracts, plus a consistency lint (every §3/§4 constant
   referenced by §5 exists; every roster anchor exists in §2–§4). FREE/LOCAL.
2. **`bench/schedule_sb7.py` + `bench/fixtures_v3.py`** — `derive(seed)`, frozen cardinalities,
   [U]/[A] classes, `fixtures_v3.build(seed)` with the full expectation pack and the per-seed
   assert battery. FREE/LOCAL.
3. **`bench/score_sb7.py` skeleton** — check registry stubs, `compose_from_rows` carried,
   severity registry + transforms + ROOT_BLOCKS dedup, `severity_selftest()` passing on stubbed
   checks BEFORE vendor or probe exist (§5.1's day-one demand). FREE/LOCAL.
4. **`bench/vendor_service_v3.py` + v3 docs** — fixed page size, barrier scheduler, commit
   ledger + trace (`sched`/`sched-unreached` lines), idempotency keys, value-dated creates,
   generation-header 304 arm, refund txn groups. FREE/LOCAL.
5. **Harness driver** — process lifecycle, kill matrix (trace-tail/poll triggers with
   timeouts), partition orchestration, tokens file, seed plumbing into verdict/trace/archive.
   FREE/LOCAL.
6. **SB7 probe** — GL wrapper v2 (proxied extensions, `bufferSubData` accounting, FBO
   classification), decisive-pose/target search, drag scripts with pinned timing, vs7dbg truth
   battery; MEASURE SwiftShader cadence at 12,288 × 2 passes and write SB7-PROBE facts (R8
   gates the coast trap's design). FREE/LOCAL.
7. **Golden reference** (`bench/golden-sb7/`) — the full app, hand-built; iterate spec/scorer
   against it until it aces; then the mutant set (§5.8). This is the largest single work item
   and is scheduled BEFORE spec freeze on purpose. FREE/LOCAL.
8. **Calibration + freeze** — pin K = 6 seeds, set CAL rungs from golden worst-of-5, run the
   full freeze gate (double-runs, mutants, decisiveness, coverage ledger, WebGL1 variant);
   `sb7-thresholds.json` + `CALIB_SHA256` land together. Nightly full sweeps on the workhorse.
   FREE/LOCAL.
9. **Cloud baseline — REQUIRES MIHAI'S SIGN-OFF.** Run the frontier baselines (Anthropic
   models: Sol-class, Opus, Sonnet, Haiku) single-shot against the frozen spec to anchor the
   board's top. Paid API spend; nothing before this step costs money. BLOCKED ON SIGN-OFF.
10. **Fleet campaign** — seed-paired arms per tick under the existing nodeloop protocol; board
    assembly; bake-in per the golden-formula direction. FREE/LOCAL.

---

## 8. Open questions for Mihai

Only decisions that are his:

1. **Cloud-baseline go/no-go (build step 9):** approve the paid frontier runs, and which
   models/how many repeats. Everything up to that step runs free and local without you.
2. **Mission acceptance after F7:** sb-7 as designed has an honest, aceable top (Sol-class
   expected 0.90–0.96) and puts its separating mass mid-board where the fleet lives. If you
   want a top-separating instrument instead, that is a different design (unpublished
   property-family checks) with real flakiness and fairness costs — say so before build step 2,
   because it changes the spec's publication contract.
3. **Publication:** does sb-7 replace sb-6 on the leanzero.net board or run alongside it? This
   drives the site/SEO work and when the sb-6 page gets its "superseded" note.