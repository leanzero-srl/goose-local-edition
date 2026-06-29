# M6 — Confidence-Driven Planning (design, research-grounded)

## Research takeaways (2025 LLM confidence literature)
- **Verbalized confidence alone is systematically OVERCONFIDENT** — LLMs cluster self-reported scores at
  80–100%, RLHF makes it worse. So "rate your confidence 0-100" by itself is unreliable.
- **Self-consistency across samples is a CALIBRATED signal**: agreement among independently sampled answers
  tracks correctness far better than a single self-report. Just ~2 samples already give a usable estimate.
- **Best combo (CoCoA / confidence-informed self-consistency)**: blend output-consistency with verbalized
  confidence, weighting consistency higher.
- **Two-step verbalization** (ask confidence in a SEPARATE query, with an anti-overconfidence instruction)
  calibrates better than one-step.
Sources: openreview 66D3rZrNjV; arxiv 2506.03723; arxiv 2412.05563; LLM-Honesty-Survey (TMLR 2025).

## The key fit: the swarm ALREADY self-samples
`best_of_n_skeletons` drafts N plan candidates in parallel and picks the best via `score_skeleton`. That is
self-consistency for FREE — we just aren't reading the AGREEMENT among the N drafts as a confidence signal.

## Design
Confidence = blend of two signals (weight consistency higher, per research):
1. **PLAN AGREEMENT (primary, calibrated)** — across the N best-of-n skeletons, measure structural
   agreement: same subtask count (±1), overlapping file partition, same layout convention, similar
   dependency shape. High agreement -> high confidence; the N drafts diverging -> LOW confidence (the model
   doesn't actually know how to decompose this -> a real signal to research more). Computed from the
   candidates `score_skeleton` already has in hand.
2. **VERBALIZED (secondary, discounted)** — two-step: after the winning skeleton, a separate harsh prompt:
   "Rate 0-100 how confident this plan is COMPLETE and CORRECT. You are KNOWN to be overconfident — be
   brutally harsh, subtract for anything unverified. List the 1-3 biggest uncertainties." Parse the number
   + the uncertainties.
final_confidence = 0.7*agreement_score + 0.3*verbalized (then clamp).

## Confidence-gated research loop (the user's core ask: research to RAISE confidence before committing)
If final_confidence < threshold (config `plan_confidence_floor`, default ~60) AND research rounds <
max (e.g. 2): run another research-planning pass targeting the listed uncertainties (context7/web-search
MCP), then RE-DRAFT the skeletons and RE-MEASURE. Loop until >= floor OR max rounds (then proceed but
SURFACE the low confidence loudly). Never silently ship a shaky plan.

## Surface the meter (user wants to SEE it)
Emit plan confidence to progress.log + the run report: e.g.
`plan confidence: 72/100 (agreement 0.80, verbalized 65; uncertainties: <list>)`.
Later: per-worker confidence on its output -> low-confidence files get priority for the M5 idle-node
correctness pre-review. Confidence ties the whole system together: low confidence anywhere -> spend idle
compute (M5) + research to raise it.

## Implementation (incremental, flag confidence)
1. Agreement metric over the N candidates in the best-of-n path (swarm.rs parallel_plan / score_skeleton).
   **Confidence: MEDIUM-HIGH** (compares structures already in memory).
2. Two-step verbalized-confidence call + harsh prompt + parse. **Confidence: MEDIUM**.
3. Confidence-gated extra research round before finalize. **Confidence: MEDIUM** (loop bound + reuses
   research-planning). Risk: extra wall-clock on the slow fleet — bound rounds, gate by config.
4. Surface in EventSink/progress/report. **Confidence: HIGH**.
Gate behind config `confidence_planning` (default on). Build/test/commit per increment.
