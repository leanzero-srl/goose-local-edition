# PLAN OF RECORD — the closing measurement schedule (set with Mihai, Fri 2026-08-14 ~09:45)

Mihai: counts first ("how many of how many"), time estimates second; longer than Sunday is fine
if beneficial; keep the task list current; hold this plan judiciously.

## Phases, in queue order (counts live in the session task board, updated at every unit end)

1. **TODAY'S FINAL BUILD** (one batch at the next unit end, then BINARY FREEZE):
   defer-expansion (F804 — subsplit expansion moves after contracts freeze) + grammar-forced
   testgen landing. BENCH3 harness work is sweep/scorer-side and never breaks the freeze.
2. **MECHANISM SINGLES** — 7 runs (~11h): fill_fan + testgen (now testing the FIXED mechanisms),
   judge_nudge, fix_sched, probe_post (the F806 conditional proof), scout_doc_urls, doc_fetch.
3. **AUX_SLIM FLIP DECISION** — after the first 3 baseline-n3 rows (they lead the curve): the
   proposal goes to Mihai with PROS/CONS; no self-directed default flips.
4. **NODE-RATIO VERDICT** — 16 runs (n1×8 + n3×8, the F327 sign-test minimum): the campaign's
   core question on the final binary. ETA ~Sun early morning.
5. **TREATMENT ARMS n=5** — 50 runs (~80h): the per-lever verdicts the golden-formula bake-in
   needs. Concludes ~Tuesday. n2×5 curve refinement rides in here if slack allows.
6. **THEN**: the bake-in phase — winning tuning baked as defaults, desktop levers stripped to the
   two-sided ones with PROS/CONS (Mihai's end goal), headless tests against the desktop app.

## Standing rules for this stretch
- NO rebuilds after today's batch until the ratio verdict lands — every rebuild resets all
  current-binary rows (measured: the F794 starvation). A defect fix that cannot wait must be
  weighed against the reset cost EXPLICITLY in the tick report.
- Kill artifacts: void=True rows + the F784b set never enter any count or aggregate.
- Task-board counts refresh at every unit end; the tick report carries the current "X of Y".

## The POST-VERDICT boundary batch (held commits inventory — ships when the ratio verdict lands)
- F809 fix pair: stub-derived slots (SlotMissingInSkeleton closed by construction) + the
  soft-extraction splice (foreign edits recorded, owned slots salvaged) — the S3 unlock's
  second half; its registered check is the next fill_fan single after that boundary:
  join_spliced with spliced>0.
- (add future held engine commits here as they land — this list IS the boundary's contents)

## THE SHIPPING PHASES (Mihai, Fri ~18:40 — after the verdict, in order)
7. **WEBSITE READY-TO-POST** (parallel-safe now, own repo ~/Projects/LeanZero-website): the
   agentic-benchmarks pipeline exists end-to-end (desktop benchmark-publish → /api/benchmark-runs
   draft → promote → page); extend both ends for sb-4 (HARD tier, excellent band, wall, engine
   build), prove with a marked test draft, page polish. Honor that repo's own CLAUDE.md.
8. **DESKTOP PARITY** (post-verdict, ships the FINAL engine): same binary + every fix in the
   desktop app, benchmark flow exercised IN THE RUNNING APP, levers per the golden-formula strip.
9. **FIRST STABLE RELEASE** (gated on the clear-and-undeniable verdict + Mihai's explicit go):
   tag + GitHub release on the fork, notes distilled from FINDINGS, release-fork build stamped.
   Then Mihai swaps models and benchmarks on the stable base.
