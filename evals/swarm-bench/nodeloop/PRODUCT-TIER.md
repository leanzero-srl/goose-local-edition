# THE PRODUCT TIER (sb-5) — grading the app a USER gets, not only the API a probe sees

Mihai, 2026-08-15: "the benchmark should also review the end product… we need to identify the
quality aspects of the actual result — a product that performs well, looks good."

## What today's evidence says (audit wf_5305a410-c6f + a live boot of the 0.9704 app)

sb-4 is NOT process-grading — it boots the real app, syncs it against a real vendor, probes
every documented endpoint including error paths, reads raw wire bytes, mutates the vendor and
checks propagation, SIGKILLs the process and checks persistence, and races concurrent syncs.
46 checks, all on the artifact. The 0.9704 was earned by the product's BEHAVIOR.

But three whole quality dimensions have ZERO coverage, measured exactly:

- **PERFORMANCE: nothing is ever timed.** Every "performance" check counts requests in a trace
  or greps for an index. An app that syncs in 200 s scores identically to one that syncs in 2 s.
- **VISUAL: no browser is ever launched.** Every ui_* check is a regex over HTML SOURCE. An
  unstyled wall of text scores identically to a designed page if the magic strings appear.
- **JOURNEY: the frontend JS never executes.** Whether the Sync button actually works in a
  browser, whether states actually display — unobserved. Console errors are never captured.
- Accessibility: one regex (`<th|scope=`), worth 1/5 of one D-tier check. Responsiveness: zero.

The 0.9704 app, actually booted and screenshotted (best-app-0.97.png): a tidy minimal internal
tool — real CSS, color-coded state banners, working sync — but it shows users raw ISO-8601
timestamps with offsets, dumps every row in one scroll while ignoring its own paginated API,
gives statuses no visual distinction, has no filter/search/pagination controls, and threw one
console error on load. Nobody would call it a product that "performs well and looks good."

## Root cause — and why this is fixable without discarding anything

The SPEC asks for a minimal page (summary line, table, button, three states) — so the swarm
builds one and the scorer fairly grades one. Quality that is not demanded cannot be graded.
And every graded tree is ARCHIVED with its db and traces, so when the product tier ships, the
entire campaign's corpus RE-SCORES under it retroactively — no measurement is wasted.

## The design — three legs, each respecting the campaign's laws
(deterministic > advisory; GRADER TRUSTED via high/determinism/isolation controls;
SCORER_VERSION comparability; budgets run-derived, never hardcoded)

### Leg 1 — the spec demands a product (spec-build v2, gradeable asks only)
Dates rendered human-readable in the user's locale (never raw ISO-with-offset); statuses
visually distinct at a glance; table paginated or virtualized past N rows with controls wired
to the documented API; filter by status; responsive at 375 px; a stated perf budget (page
interactive under B1 on the fixture, sync under B2, list endpoint p95 under B3); intentional
design (real palette, hierarchy, branded header). Each sentence exists to be checked.

### Leg 2 — the scorer verifies the rendered, timed product (new families)
- **JOURNEY (headless browser, deterministic DOM assertions):** load → rendered row count
  reconciles with the API total; click Sync → button disables → view refreshes; backend killed
  → error state VISIBLY appears; empty db → empty state; any console error = deduction.
- **PERF (measured, budgets relative to the reference):** p95 of N list GETs, time-to-first-
  render, sync wall, page weight — budgets = the known-good reference app's numbers measured in
  the SAME session × a slack factor, so machine load cancels out (the controls pattern applied
  to time).
- **VISUAL rung (a) (deterministic, computed styles):** status cells differ in computed color;
  rendered date text matches locale format, not ISO regex; no horizontal scroll at 375 px;
  contrast ratio from computed colors; real stylesheet with non-default decisions.
- **VISUAL rung (b) (PARKED, Mihai's call):** screenshot rubric judged by a local VLM with
  known-good/known-bad CONTROL screenshots each run (grader trusted only if controls separate).
  Requires a vision model on the fleet — a fleet decision, never mine.
- Every new check passes the ISOLATION discipline before it counts: inject the defect
  (unstyled page, ISO dates, missing pagination), prove only the expected checks drop.

### Leg 3 — the engine gate gets the same checks, or scores won't move
The campaign's oldest law: only checks wired into check→block→repair convert into score.
The completion gate gains a headless-render smoke (sibling of the F816 vendor-stub gate):
render, assert the journey basics, feed failures to the fix loop as findings. Without this the
product tier would measure a gap the swarm cannot close.

## Sequencing (comparability preserved)
1. NOW: this design; sb-4 stays frozen — the running curve and treatments stay comparable.
2. POST-VERDICT boundary: sb-5 = existing queued hardening (BENCH2 ranks 3-10, BENCH3 feature
   fold) + the PRODUCT tier + spec-build v2 + the gate-side render check; SCORER_VERSION bump;
   controls re-run (high/determinism/isolation) before any verdict is trusted.
3. Re-score the archived corpus under sb-5 → the node-ratio question gets re-read on product
   axes for free.
4. Weighting proposal (to tune at implementation): core functional 0.60, journey 0.15,
   perf 0.10, visual 0.05, hard-block 0.10 — functional correctness stays the majority; the
   product axes are large enough to move builds and small enough not to drown Tier B.
