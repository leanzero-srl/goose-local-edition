# SB7.1 review backlog

## Functional 3D credit versus visible quality

Recorded 2026-08-24 for a future SB7.1 iteration. Do not change SB7.0, its scorer,
thresholds, published verdicts, or stable website identity in response to this note.

Gemini 3.7 Flash's sealed hermetic verdict is correctly composed as **0.7538** by the
current SB7 scorer. The archived evidence shows a real WebGL field with 12,288 instanced
columns, and the probe measured real draws, offscreen pick-buffer work, six decisive picks,
and camera/pixel movement. The concern is therefore not missing 3D functionality: it is that
the visibly crude, low-coverage scene still earns a 0.9685 mean for tier T
(`t_context_real` is only reduced to 0.75).

For SB7.1, review whether functional proof and human-visible visualization quality need
separate credit. Test candidate changes against the golden reference, mutants, and every
archived SB7.0 entrant before accepting them. Specifically examine scene coverage, useful
scale, legibility, occlusion/clutter, and whether a technically valid but visibly poor scene
can retain too much tier credit. Red-team and calibrate any new ruler before freezing it;
never retroactively rescore or relabel SB7.0 with SB7.1 rules.

## Visible 3D availability must be deterministic and score-critical

Recorded 2026-08-24 after the sealed Alibaba Qwen3.8 Max run. Its SB7.0 score of
**0.6255** is mathematically correct under the frozen scorer, but the public scorer
screenshot visibly says `3D is unavailable in this browser (no WebGL)`. Despite that,
the separate visualization probe awarded `t_context_real=1.0`, tier T retained a
0.5908 mean, performance tier P retained 0.6667, and the missing visible 3D surface
triggered no critical multiplier. This is a concrete evidence-alignment and severity
defect for the next iteration, not grounds to mutate or rescore SB7.0.

SB7.1 must be substantially more exigent about the 3D contract. A context, debug API,
scene digest, or offscreen draw is not enough: the required object must be visibly
rendered in the same browser state whose evidence is published. Make that deterministic
with all of the following review requirements:

- Grade the screenshot and the pixel/mechanism probe from the same browser launch,
  route, viewport, data state, and WebGL capability profile. Conflicting evidence must
  fail closed instead of letting the more favorable probe win.
- Add a visible-surface root gate that detects fallback copy, missing/hidden canvases,
  zero useful canvas coverage, off-viewport geometry, and a real WebGL context whose
  default framebuffer remains visually blank. Tie sampled non-background pixels back
  to projected instance positions and the seeded records rather than accepting arbitrary
  colored pixels.
- Make failure of that visible-surface gate a critical consequence or a hard score
  ceiling. Backend excellence must not hide the absence of the benchmark's defining 3D
  deliverable. Keep finer partial credit for height, camera, picking, brushing, streaming,
  and performance only after visible-surface admission passes.
- Red-team deterministic mutants for: fallback-only UI, fabricated `vs7dbg`, offscreen-
  only rendering, a cleared default framebuffer, one decorative triangle, geometry
  outside the frustum, and a canvas covered by another element. Every mutant must score
  materially below an application that renders and exposes the full seeded field.
- Re-run the golden reference and every archived SB7.0 entrant through the candidate
  ruler, visually inspect the ordered results, and freeze only when both functional 3D
  correctness and human-visible quality have a justified monotonic gradient.
