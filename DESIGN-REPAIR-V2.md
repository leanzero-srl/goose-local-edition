# REPAIR v2 — reproduce first, promote on the flip, exceptions in an advertised surface are critical

Mihai, 2026-09-02: *"the repair phase has never been very good. also our models never really managed to create the 3d
object… I am not asking for anything hard coded… The benchmark is just a tool."* Research note in TICK-NOTES 09-02 00:2x.

## Ground truth from the archives (why nothing was ever fixed)

- **The 3D field never drew, and each time it was ONE line the gate had already seen.** r5 `web/viz.js` (1,137 lines,
  a real WebGL engine): boot dies at `document.addEventListener(…, onBrushChangeTracked)` — the function is
  `onBrushChange`; the render gate logged `console_errors=4`, first `ReferenceError: onBrushChangeTracked is not
  defined`, and filed it as MINOR → no fix shard ever got it; score heights 0/6, camera 0/5, labels 0/7. r6c (993
  lines): WebGL methods called detached from `gl` (`(isGL2 ? gl.vertexAttribDivisor : …)(loc, d)`) → the probe's
  `TypeError: Illegal invocation`; `compile()` returned null on shader failure with a `console.warn` (a silent
  fallback); `complete_result.passed=true` while both render criticals stood.
- **REPAIR spent its minutes not writing.** r5: `__main__.py` shard 4,229 s + 4,811 s, `fix_attempt_progress samples
  70, changed_samples 0` — 70 minutes without one byte. r6c: 458 lane-min, one promotion (an easy DOM id), `ledgerd/
  __init__.py` `first_change_secs: 7020` — 117 minutes before its first edit; six of nine findings were false probes.
  Neither run localized from evidence it already had: both console errors name the file and the symbol.

## Mechanisms, ranked by confidence (general — derived from the spec and the tree; MILD; no caps)

1. **HIGH — the finding's probe IS the repro, and the shard runs it FIRST.** A fix shard's brief opens with the exact
   check that produced the finding (boot argv + curl / the console line / the failing test) and its first action is to
   re-run it and quote the failure: `repro_confirmed{finding, check, quote}`; a shard that edits before reproducing is
   `edit_before_repro{finding}` (said, not blocked). Localization is carried IN the brief from the evidence: an
   exception → `file:line`; `ReferenceError X` → the grep for X's definition and its callers (Agentless: file → class/
   function → edit location; "agents that gather context before editing and invest in validation succeed more often").
2. **HIGH — promotion iff THAT check flips.** A shard's preview is promoted only when the finding's own check passes on
   the merged preview AND no other check regresses: `finding_flipped{finding, check}` / `finding_still_failing{finding,
   quote}` — never "count strictly lower" (r6c's `web/viz.js` shard was promoted for closing a DOM id while the
   exception stood).
3. **MEDIUM-HIGH — a GENERAL render check, derived from the spec's own words.** Headless Chromium (`--use-angle=gl`;
   Firefox headless has no webgl2): every element id the spec advertises exists; every debug API member the spec
   advertises is `typeof 'function'`; `pageerror` count is 0; every `<canvas>` has a context AND a screenshot clipped
   to it has more than one distinct color. Nothing names sb-7; the check reads the spec's identifiers.
4. **MEDIUM — an uncaught exception in an advertised surface's boot path is CRITICAL** (engine-observed → critical by
   construction), and `complete_result.passed` requires zero render-class `known_active_bugs`. This is the ruling VA-006
   waited for, read from Mihai's words above ("never really managed to create the 3d object") — he can veto.
5. **MEDIUM — fat visual modules: smoke the plumbing before the scene.** Shard by concern (GL plumbing / instance
   buffer / camera math / events); the plumbing shard's CHECKED_WITH is "draw ONE instance, `readPixels` at its
   projected point equals its color" before N; a math oracle proves math, not drawing (r5 had one, 6 passed, 0 drawn).
   ShaderMatch: GLSL is "a low-resource language rarely found in pretraining datasets" — verification, not hope.
6. **MEDIUM — the fix shard writes early.** Its brief: reproduce, then the FIRST edit at the localized line; the vigil
   reads `fix_attempt_progress.first_change_secs` — a shard past its first sample window with `changed_samples 0` is
   the finding (words quoted), never a cap.

## Falsifiers for the next REPAIR (the vigil reads these)

`FIX CLAIMED WITHOUT EDIT 0` · every `shard_promoted` names a finding absent from the next `complete_verify` · no shard
with `first_change_secs: null` past its first window · `complete_fix_converged` never with `promoted: 0` while a render
finding stands · `render_gate` reads `console_errors=0, rows>0` · `passed=true` only with zero render-class known bugs ·
score `t_height_pixels` > 0/6.
