# SB7.1 — Meridian Payments Landscape

Build and finish the Meridian Payments Console in this workspace. This is a payments product,
not a generic visual demo. The full behavioral contract is `SB7-CONTRACT.md`; the SB7.1 visual
amendment is `VISUAL-CONTRACT.md`. Read both. Where the amendment explicitly replaces the
simple column geometry or adds an inspector/animation, the amendment takes precedence. All
other SB7 behavior, consistency, crash recovery, rendering, interaction and performance
requirements still apply. The vendor documentation is at {DOCS_URL}; base {BASE_URL}; API key
{API_KEY}. Use the documented runtime vendor flag.

An editable transport/page-shell starter is already present. `STARTER.md` identifies what it
provides. HTTP 501 responses and empty page sections are unfinished work: implement the
payments behavior, durable storage, synchronization, workflow, recovery and 3D field. You may
replace the starter. No reference application or solved persistence/rendering algorithms are
provided. The backend uses Python standard library; the frontend remains self-contained raw
HTML/CSS/JavaScript and WebGL, with no package installs needed.

The 3D result must be a legible, well-framed payments landscape with the specified detailed
payment towers. It is driven by actual payment data, including currency exponents, statuses,
versions and live events. The close-up inspector and short event animation are part of the
same real scene, never a pre-rendered demo or separate grading image. Financial state updates
immediately; decorative interpolation must not invent amounts, statuses or intermediate totals.

## Score bands

The existing SB7 behavioral score is earned by actual tests. Additional visual conditions
restrict the maximum attainable score; passing them awards no automatic points:

- Without visible, data-backed 3D in the actual recorded browser state: maximum 0.599.
- Without all specified tower parts, correct payment/currency mapping, original layout/height
  pixel checks and scene-truth cross-checks: maximum 0.699.
- Without the published presentation, readable inspection and original GPU picking, camera,
  coast, labels, brush, draw-budget and incremental-stream requirements: maximum 0.799.
- Without BOTH verified event-driven animation/finesse AND full backend consistency/recovery
  excellence: maximum 0.899.

Backend excellence requires all existing concurrent-history and crash-recovery checks (X and R)
to pass, with the actual schedules exercised. Visual excellence is defined in the visual
contract. Scores above 0.9 require both, and still require enough earned behavioral credit.
The evaluator retains per-check observations, screenshots, and a short clip from the same
browser session that supplies the visual evidence. Do not implement a video exporter: recording
belongs to the evaluator, not the candidate application.

## Completion handoff

Work only with this workspace, the supplied public documentation and the vendor API. Private
scorers, golden applications, previous entrants and operator logs are outside the workspace.
Test your own application against the documented behavior. When implementation and your checks
are complete, stop your temporary test processes and return a final summary of files, actual
checks and remaining limitations. That final response hands the artifact to the external
scorer. Do not wait for or poll grader results; grading starts AFTER your session finishes.
