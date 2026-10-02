# SB7.2 — Meridian Payments Landscape

Build and finish the Meridian Payments Console in this workspace. This is a payments product,
not a generic visual demo. The full behavioral contract is `SB7-CONTRACT.md`; the SB7.2 visual
amendment is `VISUAL-CONTRACT.md`. Read both. Where the amendment explicitly replaces the
simple column geometry, the default camera or adds an inspector/animation, the amendment takes
precedence. All other SB7 behavior, consistency, crash recovery, rendering, interaction and
performance requirements still apply. The vendor documentation is at {DOCS_URL}; base
{BASE_URL}; API key {API_KEY}. Use the documented runtime vendor flag.

An editable transport/page-shell starter is already present. `STARTER.md` identifies what it
provides. HTTP 501 responses and empty page sections are unfinished work: implement the
payments behavior, durable storage, synchronization, workflow, recovery and 3D field. You may
replace the starter. No reference application or solved persistence/rendering algorithms are
provided. The backend uses Python standard library; the frontend remains self-contained raw
HTML/CSS/JavaScript and WebGL, with no package installs needed.

The 3D result must be a legible, well-framed payments landscape — a viewer reads each payment's
status from its cap at the default view — with the specified towers and a close-up ledger spire:
clipped corners, separate ribs, a recessed core and a hollow moving currency frame, driven by
actual payment data, currency exponents, statuses, versions and live events. The inspector and
the event animation belong to the same real scene. Financial state updates immediately;
decorative interpolation must not invent amounts, statuses or intermediate totals.

## Score bands

Your score is earned by actual tests, and the 3D field — structure, legibility, inspection,
interaction, animation and rendering performance — carries about half of it. Visual conditions
also cap the maximum; passing them awards nothing extra:

- No visible, data-backed 3D in the recorded browser state: maximum 0.599.
- Missing tower parts, wrong payment/currency mapping, or failing layout/height/scene-truth
  checks: maximum 0.699.
- Failing GPU picking, camera, coast, labels, brush, draw budget, incremental stream, inspector
  framing or the overview legibility: maximum 0.799.
- Without BOTH verified event animation AND full backend consistency/recovery excellence (every
  concurrent-history and crash-recovery check passing on its exercised schedule): maximum 0.899.

Text and control readability earns points but never caps. The evaluator keeps per-check
observations, screenshots and a short clip from the graded browser session; do not build a
video exporter.

## Completion handoff

Work only with this workspace, the supplied public documentation and the vendor API. Private
scorers, golden applications, previous entrants and operator logs are outside the workspace.
Test your own application against the documented behavior. When implementation and your checks
are complete, stop your temporary test processes and return a final summary of files, actual
checks and remaining limitations. That final response hands the artifact to the external
scorer. Do not wait for or poll grader results; grading starts AFTER your session finishes.
