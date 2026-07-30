# swarm-bench ledger

## Phase 1 status — Opus BUILD archetype

| rep | total | A | B | C | D |
|---|---|---|---|---|---|
| 0 | 95.3% | 100% | 100% | 86% | 94% |
| 1 | 94.7% | 100% | 100% | 86% | 91% |

**mean 95.0%, spread 0.6 points.** The instrument is STABLE — that is the load-bearing finding, and
it is what makes any future gap between models readable rather than noise.

**These two reps were scored with the PRE-FIX scorer.** The C 86% in both is a grader bug, not a
model defect: `vendor_all_pages` keyed on a `page` trace field that offset-based paging stopped
emitting. Corrected since. Re-scoring needs a re-run (the runtime probes boot the app), so treat
94.7/95.3 as a FLOOR on artifact quality, not a final number.

Not yet exercised: the four meticulous Tier D detectors added after these runs
(`store_atomic_upsert`, `store_indexed`, `client_timeouts`, `ui_error_actionable`).

## Standing conclusion

Opus is at or near the ceiling on ARTIFACT quality. Per the operator's rule, the answer is sharper
detection rather than a harder app — and the remaining headroom is in the PROCESS axes this
archetype structurally cannot see: plan shape, confidence calibration, judge precision, and whether
the run claims done honestly. Those exist only in a swarm run.

## Fabricated deductions caught (all four went AGAINST the model)

1. retry_after graded the agent's scratch testing, not the delivered client — phase marker added
2. cursor_expiry flagged a legitimate post-restart request as a re-sent dead cursor
3. request_efficiency docked Opus 12% for sending `limit=100` to a mock that ignored `limit`
4. error_detail_quality punished a 404 body that was EXACTLY what the spec specifies — check deleted

Bias direction worth remembering: a grader's bugs invent defects rather than excuse them, because a
broken probe returns falsy. A benchmark that does not audit its own misses will systematically
understate whatever it measures.

## Next action

Task 5 — the six-axis process scorer over swarm runs. Everything it needs is mapped: event names,
fields, and gaps are in the plan; `research_completed{grounded}`, `plan_confidence_breakdown`,
`judge_verdict` and `complete_result{passed,verified}` are the primary sources.

## Swarm results, first full sweep (2026-07-30)

| entrant | score | wall | timed out | reading |
|---|---|---|---|---|
| swarm-3node | **90.0%** | 9000s | yes | finished the work before the cap bit |
| swarm-1node | 46.0% | 9000s | yes | truncated — discard and re-run at 16200s |
| swarm-2node | 9.2% | 2519s | **NO** | **a real failure, not a cap artifact** |

**swarm-2node is the interesting one.** It exited CLEANLY at 42 minutes with Tier A at 100% —
every named module present, all 8 interface methods declared, server healthy, page served — and
Tier B at 0%: `sync_completeness 0/247`, `total_field None`, `summary_accuracy None`.

It built a complete, correctly-structured application whose vendor integration does not work, then
stopped and declared itself done. That is exactly the failure class this benchmark was built to
detect and that an artifact-only grader would score as "well structured". The four-tier split is
what makes it legible: A 100 / B 0 is a precise diagnosis, not a number.

Worth noting the swarm is NOT uniformly weak — 3 nodes reached 90.0%, comfortably above the local
single-agent's 84.1%. The spread across node counts is real and needs untruncated reps to explain.
