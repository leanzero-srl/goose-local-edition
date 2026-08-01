# ENGINE FREEZE — in effect from 2026-08-01 21:30, engine_build 1785609208-235593584

Tonight produced eleven findings and ZERO measurements. The instrumentation is now good and has
nothing to instrument, because the engine was changing faster than the loop could produce data — and
the harness re-runs any unit whose `engine_build` does not match the current binary, so every unit
finished before a boundary is discarded no matter how valid it was.

**No engine change until the baseline replicate set and the node curve are complete** — roughly 12
units, ~24h of fleet time. Concretely: `baseline` at n=3 for nodes 3, 1 and 2.

## The ONLY thing that reopens the engine early

A defect that makes a measurement MEAN THE WRONG THING. The bar is F38: a cell labelled 1-node that
actually ran two workers. Not "the swarm builds worse apps than it should" — that is the thing being
measured, and stopping to fix it mid-campaign is how a campaign never produces a number.

Everything else — quality defects, dead levers, missing instrumentation on mechanisms that are not
under measurement — goes in the queue below and ships at the NEXT boundary, after the data lands.

## Queued for the boundary after the freeze lifts

(nothing yet — F41 through F44 all shipped in 1785609208)

## What the freeze is protecting

- the replicate spread on THIS engine, which every arm verdict is measured against
- the node curve at 1 / 2 / 3 workers, now that a cell measures the worker count it is labelled with
- first observation of `retarget_discarded` (the detail-reuse hit rate), `pool_resolved`,
  `complete_verify.finding_texts`, `task_split` and `speculated` in a completed run
