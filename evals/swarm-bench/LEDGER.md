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

## Node scaling, first VALID measurement (2026-07-31)

Enforced with the new `GOOSE_SWARM_MAX_NODES` engine lever; every verdict records `actual_pool` from
`run_started` and all three matched their label. No truncation — every run finished under its cap.

| entrant | build | process | A | B | C | D | wall |
|---|---|---|---|---|---|---|---|
| local-single | **84.1%** | 92.0% | 83 | 88 | 86 | 78 | ~40 min |
| swarm-1node | 45.0% | 81.9% | 96 | 33 | 14 | 37 | 155 min |
| swarm-2node | 37.0% | 79.5% | 73 | 19 | 14 | 47 | 112 min |
| swarm-3node | 33.1% | 76.2% | 25 | 25 | 43 | 43 | 192 min |

**The swarm loses to a single agent using the same models, and MORE NODES MAKE IT WORSE.** That is
the opposite of the expected result and it is now measured rather than assumed.

**The 3-node failure is diagnosed, not just scored.** The app crashes on startup:

    TypeError: Store.__init__() got an unexpected keyword argument 'path'

`__main__.py` calls `Store(path=...)`; `store.py` defines a different signature. Two workers built
against different assumptions and nothing reconciled them — a parallel-work integration failure,
which is exactly the risk that grows with node count. Tier A 25% for 3 nodes vs 96% for 1 node says
the same thing structurally: more parallelism, less coherence.

**The process axes are healthy, and that is the useful part.** JUDGE 98-99%, DELIVERY 100% (the run
finished AND told the truth — no false green), RESEARCH 100%. The machinery works; the coordination
of parallel authorship does not. PLANNING falls with node count (66 → 62 → 50), driven by
confidence calibration: the run declares high confidence and earns a low score.

**This is the finding the whole benchmark existed to produce.** An artifact-only grader would say
"the swarm is bad". The tier split plus the process axes say something actionable instead: planning,
judging and honesty are fine, and the defect is cross-worker interface agreement.

## Root-cause dig (2026-07-31) — three corrections to my own first diagnosis

**1. `drop_unparseable_stubs` is NOT missing — it is wired and working.** `swarm.rs:21069` already
drops a stub that fails to parse, with a comment describing this exact failure class. My first read
("the broken stub is injected") was WRONG. The worker gets *nothing* rather than garbage.

That still loses the run, because nothing fills the hole. The comment claims "a module with NO frozen
self-interface is an ALREADY-HANDLED case: it builds without one and **integrate-verify
reconciles**." It does not reconcile — see below.

**2. The integrator role ALREADY EXISTS. It just did not run.**

`integrate-verify` is in the plan for every run. In swarm-3node:

    planned=17  dispatched=18  completed=17
    PLANNED BUT NEVER DISPATCHED: integrate-verify
    dispatched but never completed: test-meridian

    integrate-verify deps: [verify-e2e::0, verify-e2e::1, verify-e2e::2]  -> ALL THREE COMPLETED

So it was unblocked and simply never got its turn: the run went into the fix loop over a failing
sibling (`complete_failed_tasks: ["test-meridian-client"]`), burned three `complete_verify` rounds,
and ended with `integrate-verify` still unscheduled.

**This is the structural defect.** The task that reconciles cross-module interfaces sits at the TAIL
of the DAG, so it is the first casualty of round exhaustion or a failing sibling — it is skipped
precisely when the run is in trouble, which is exactly when it is needed. A safety net gated behind
everything else succeeding is not a safety net.

**3. The gates are healthy and honest.** `complete_verify` ran 3 rounds, `passed:false` every time,
and the run refused to claim green (DELIVERY 100%). Detection works. What fails is REPAIR — and the
fix loop runs with implementer-role constraints ("read AT MOST the ONE file you will edit"), so a
cross-module signature mismatch is structurally invisible to the loop meant to fix it.

### Revised fix, much smaller than "build an integrator"

- Make integration verification **unconditional**: run it before the completion gate regardless of
  sibling failures or round exhaustion. It is a safety net; it must not be schedulable-away.
- When a contract is dropped for not parsing, **derive** the interface from the plan's declared
  signatures instead of leaving the module with no contract at all.
- In a FIX round, lift the single-file reading restriction — the defect being fixed is by definition
  not in one file.
