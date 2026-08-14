# BENCH3 — the AMEND mode: add a feature to an EXISTING app (Mihai's directive, 2026-08-13 ~23:35)

> "This has mostly built apps from scratch, correct? Consider adding a new feature to one of
> these already existing apps — add this to the backlog to test as part of the benchmark and
> improve goose swarm's proven ability to support implementing new features in existing code."

Every unit to date is greenfield: the workdir is wiped and vendorsync is born from the spec.
Brownfield is the more common real job and a DIFFERENT capability axis — this mode tests it.

## Design

**Base tree:** the controls' known-good app (runs/build/opus-5-r0) — already the trusted fixture
the GRADER controls inject defects into, so its score level is measured and its shape is exactly
what production units produce. Seeded into the workdir BEFORE the run (sweep.py gains a
`seed_tree` field on the arm; the wipe step copies the base in instead of leaving empty).

**Feature spec (v1):** "Add a status-summary view: a new GET /api/summary/by-status endpoint
returning {currency: {count, total_minor}} from the local store; a UI section showing it; and a
CSV export at /api/export.csv streaming all payments." Deliberately multi-file (api + store +
web) so the fan/ownership machinery is exercised on existing files, with deterministically
checkable outcomes.

**Scoring = regression half + feature half:**
- REGRESSION: the existing sb-4 check set on the amended tree — the base app scored ~0.90 as
  known-good; an amend run that drops it below (known-good − spread) FAILED the brownfield test
  regardless of the new feature. This is the half that measures "did the swarm break what
  worked" — the distinctive brownfield risk.
- FEATURE: new checks (by-status shape + count reconciliation, CSV row count + header,
  UI section presence) — folded into the BENCH2 ranks 3-10 program so both backlogs land in one
  scorer change.

**Engine questions this mode answers (each a potential finding):**
1. Does the planner plan a DELTA or re-plan the whole app? (plan_loaded task count on a seeded
   tree; the amendment flag exists — is_amendment via working_dir_has_sources — but nothing
   measures its effect.)
2. Do contracts freeze FROM existing code (read the real api.py) or invent conflicting stubs?
3. Does owned_files discipline hold when the "owner" is pre-existing code no task created?
4. Does the repair loop's promote-only-strictly-better protect the BASE functionality (the
   regression half is the registered check)?

**Harness work (working session, in order):** (a) sweep.py seed_tree support + the amend arm
unparked; (b) score_build feature checks + the regression-half readout in the [done] row;
(c) a first n=1 mechanism cell before any score comparison.

**Queue entry:** the `amend_feature` arm is REGISTERED but PARKED (reps 0) until (a)+(b) land —
the preflight rule (a question-less arm cannot run) plus reps-0 parking keep it visible in every
restart banner without burning fleet time before the harness can score it honestly.
