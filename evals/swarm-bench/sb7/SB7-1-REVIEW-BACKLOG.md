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
