# Swarm GOALS / PILLARS — design (user direction, 2026-07-03)

DO THIS AFTER the current exploratory run finishes (do NOT interrupt it). This is the user's
top architectural direction for the swarm. Capture is faithful; design below is my read.

## The user's vision (verbatim intent)
1. **Clarify the road to implementation, and drive confidence.** When the AI asks questions, the
   GOAL is to *clarify its road to the implementation*. The clearer the road, the easier it is to
   get something done correctly. So the swarm's questions should be about building a clear,
   confident implementation path — not incidental.
2. **Pillars of the app.** A main reason modules don't connect well enough is **context size**: the
   swarm **forgets the pillars**. It isn't SETTING them or DISTILLING them from the requirements +
   the user's vision. Fix:
   - **Distill pillars** from the requirements/vision up front.
   - **If the detail is too small, it should ASK** (clarify) in order to CREATE its pillars.
   - **On REVIEW, check the pillars** and **mutate the result** accordingly **until it's functional**.
3. **GOALS.** Unsure if goose has goals or if we must implement them for local models — **we need
   goals.** The **judge can judge much better based on goals.**

## Current state (what the swarm has today — grounds the design)
- Research (parallel scouts) → architect decomposes the spec into a DAG of subtasks with file
  ownership → workers build in parallel (CONTRACTS injects per-module *signature stubs* so siblings
  see each other's APIs) → semantic judge watches activity → integrate-verify sink wires + smoke →
  deterministic verify. Per-turn compaction (`GOOSE_LOCAL_CONTEXT_CAP`) can make a worker forget.
- What's MISSING: there is **no persistent, app-level GOALS/PILLARS artifact.** Requirements are
  per-task; CONTRACTS is signature-level; nothing carries the *vision/north-star* through the whole
  run, survives compaction, anchors cross-module coherence, gates the review, or drives the judge.

## Proposed feature: `GOOSE_SWARM_GOALS` (default OFF, gated flag) — 5 parts
1. **Distill pillars (plan time).** After research, before/with the architect, produce a SMALL set
   of PILLARS = the app's core goals + load-bearing cross-module invariants + user vision. Small +
   stable (e.g. spendlog: P1 expenses persist to the --db JSON store; P2 money is 2-decimal
   everywhere; P3 total/by-category/monthly all read the SAME store; P4 runs as `python -m spendlog`).
   Persist to `.swarm/pillars.json` (+ a pillars event).
2. **Clarify-if-thin (confidence gate).** If the spec is terse/underspecified (low confidence the
   pillars are complete/correct), ASK clarifying questions FIRST (operator/ASK gate) and refine the
   pillars until confident enough — THEN dispatch. "Clearer road → easier." Bounded round cap (no
   thrash). Ties into the existing CONFIDENCE_GATE idea.
3. **Persist + inject into every worker.** Re-inject the pillars block into EVERY worker prompt each
   dispatch (survives compaction). This is the level ABOVE signature stubs — the shared *vision*
   contract that makes modules connect even after the details are compacted away. Small token cost,
   high coherence payoff.
4. **Review against pillars + mutate.** The review / integrate-verify phase checks the built app
   against EACH pillar (deterministic where a pillar maps to a runnable check; judged otherwise) and
   re-dispatches targeted fixes until every pillar is satisfied AND the app is functional.
5. **Judge on goals.** Give the judge a goal-oriented rubric: "does the app achieve pillar Pi?" per
   pillar, not generic quality. Sharper, more actionable verdicts.

## Confidence / risk (honest, per the user's rule)
- HIGH value, MED-to-larger surface: touches plan + worker-context + review + judge. The riskiest
  swarm change since it threads a new artifact through the whole pipeline.
- Correctness risks to verify: (a) a WEAK model distilling GOOD pillars (garbage-in → garbage
  anchor) — mitigate: distill via the fleet/best-of-N + keep pillars few + concrete; (b) the
  clarify-gate thrashing — mandatory round cap; (c) injection context budget (pillars must stay
  small); (d) review-mutate looping — bound the fix rounds (connect to the integrate-verify/sink
  work + the sink-cap). Ship env-gated default-OFF; validate in exploratory (spendlog-class apps)
  before any default.

## Build order (after the run)
1. Investigate: does goose/`crates/goose-swarm` have any goal/vision primitive? (grep goals/vision/
   objective). If not, design the pillars artifact + event.
2. Implement part 1 (distill) + 3 (inject) first — the coherence core — behind `GOOSE_SWARM_GOALS`.
   GATE (build+clippy+test), validate in an exploratory run (do the modules connect better?).
3. Then part 4 (review checks pillars) + 5 (judge on goals) + 2 (clarify-if-thin confidence gate).
4. A/B in exploratory: pillars ON vs OFF on the same spec — do modules connect + does functional
   quality rise? Turn findings into the next iteration.

Related: [[commit-every-change]] discipline; connects to CONTRACTS, CONFIDENCE_GATE, REVIEW,
integrate-verify in the v8 plan + BENCH-FIXES-BACKLOG.md.
