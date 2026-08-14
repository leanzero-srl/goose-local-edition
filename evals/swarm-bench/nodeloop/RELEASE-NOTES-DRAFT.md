# goose local-edition — first stable release (DRAFT, awaiting the verdict + Mihai's go)

> Status: SKELETON. The bracketed numbers land when the node-ratio verdict (curve.py) and the
> final replicate counts are in. Nothing ships from this file without Mihai's explicit go.

## What this release is

The first stable build of goose local-edition's **swarm**: a multi-node orchestrator that turns a
fleet of local LM Studio models into a single software-building system — planned, supervised,
verified, and graded by running the software it builds.

Evidence line (to fill from curve-verdict.json): across [N] paired runs on the final binary,
3 nodes beat 1 node on quality in [k/n] pairs (p = [..]) and on wall-clock in [k/n] pairs
(p = [..]); recent 3-node runs average [~0.7x] on the hardened sb-4 scorer with [x] runs at
0.94+.

## The mechanisms this release ships (each proven live during the closing campaign)

- **Repair as orchestration**: the completion gate turns findings into real scheduled fix tasks
  (disjoint per file), raced or fanned across the fleet, promoted only when a shadow verifies
  strictly better than baseline. Observed live: 6 findings → a 6-task fix DAG → strictly-better
  promotions → a clean tree (F799).
- **Supervision everywhere**: the judge watches every phase, redirects looping calls in-session
  with its own hint instead of killing them (F790-1), cites deterministic instrument readings
  (failed tool calls, import health) in its verdicts (F790-2), and idle nodes review/testgen
  during any tail (F779).
- **Ask the run questions**: drop a text file into a running swarm's `.swarm/questions/` and an
  idle node answers with the run's own state — measured at 57 seconds question-to-answer (F801).
- **Resume, not restart**: a stopped run resumes its plan and re-runs against the warm tree
  (F811); node counts are ground-truthed against LM Studio's own activity, independent of the
  engine's self-report (F812).
- **Findings with prescriptions**: the conditional-request family (the most persistent measured
  loss) now reaches repair workers with the exact fix named (per-page ETag keying) and a shape
  finding when the documented counters are missing (F806/F797).
- **Skeleton-fill parallelism**: hard modules expand into skeleton → parallel slot-fills → an
  AST-splice join, gated on stub parseability so the fan fires exactly when it can succeed
  (F804/F809) — with a byte-fence that has never let a foreign edit reach the real tree.
- **Local-model economics**: prefix-cache-safe assembly, keep-tail compaction, and the aux-call
  slimming lever ([aux_slim replicate summary]).

## The harness (ships in-tree)

sb-4: a hardened, control-trusted grader (run-twice determinism, defect-isolation controls,
hard-block aggregation) + the brownfield amend mode (seed an existing app, grade regression AND
feature) + the node-curve reporter with a pre-registered sign test.

## Benchmarks site

Results post from the desktop app to leanzero.net/agentic-benchmarks (draft → promote →
ranked leaderboard with tiers, hard-block mean, and wall-clock).

## Known limits (honest)

- The skeleton-fill splice has not yet had a clean live exercise: its first firing hit two
  real defects (slot naming, foreign-edit refusal) — both fixed and held for the next boundary
  (F809); the byte-fence held throughout.
- Idle-slot test generation shipped and ran a full unit without firing once (an honest null —
  the idle windows it targets were absorbed by review work that run); firing rate unproven.
- The conditional-request loss family is fixed for missing counters and prescribed for honest
  ones, but an app that self-reports FALSE counters still passes the engine gate — the
  vendor-stub gate that closes this is designed and queued (F816), not yet shipped.
- [treatment-arm verdicts at n=5 — fill when the extended phase concludes]
