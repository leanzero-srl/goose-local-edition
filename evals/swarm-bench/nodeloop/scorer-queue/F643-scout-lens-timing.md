QUEUED ENGINE INSTRUMENT — specified, deliberately NOT coded while the binary is frozen.

WHAT WAS MEASURED. The research phase does **not** get faster with the fleet. Three-node runs take
377.2s at the median against one-node's 395.4s — a **4.6% reduction**, +39.1s on the mean at **0.64
SE**, over 11 one-node and 17 three-node runs (F642). The lens fan is also structurally fleet-blind:
`select_lenses` (swarm.rs:10800) drops amendment-only lenses and clamps to `max_research_questions`,
and node count never enters, so a greenfield build gets exactly three lenses whatever the hardware.

**That is roughly seven minutes of every run in which extra hardware provably buys no time**, on the
pillar that currently looks worse (speed: 3 nodes +7.4 min slower, 0.69 SE).

## Three candidate explanations were named; two are now dead

- **NOT serial dispatch, NOT planner-pinned.** `swarm.rs:13436` fans the lenses through
  `fanout_over_fleet_straggler`, commented *"One scout per device (work-stealing): a weight-1 node
  never has a second scout queued."*
- **NOT a lens-drop confound** (F643). Lens-drop rate is 0.0% at one node (n=17) and 4.3% at three
  (n=23) — a 4.3-point gap against a 20-point bar, so the arms are balanced across that divide.
- **The Amdahl reading does not survive its own arithmetic.** Pool weight is 2, so one node runs two
  lenses concurrently then the third while three nodes run all three at once. If the phase were
  mostly lens work, roughly-equal lenses would make one node take **twice** as long as three. The
  observed ratio is **1.05**. Rescuing Amdahl would require two lenses finishing in ~18s while the
  third ran ~377s, which is not credible.

**Leading remaining explanation, marked as INFERENCE from one ratio and NOT measured:** the phase's
duration is dominated by a serial component that is not the parallel lens work at all — prompt
assembly, model warm-up, or the synthesis that turns findings into the planner's input.

## Why it cannot be settled from any existing log

Between `scouts_planned` and `research_completed` **the event log is completely silent**. Measured on
`baseline-n3-r3`: 17:11:36 to 17:17:43, **six minutes and seven seconds with no event of any kind**.
`research_completed` carries `findings`, `grounded`, `looked_nothing_up`, `lenses_returned` and
`finding_texts` — lens *names*, never lens *timings*, and no device attribution.

So the phase that provably wastes the fleet is also the least-instrumented phase in the engine.

## The change — per-lens spans, the same shape as F578

At the scout closure (`swarm.rs:13436`, where `started`, `lens` and `model` are already in scope and
`started` is already an `Instant`), emit on completion:

    "event": "scout_completed",
    "lens": lens.id,
    "model": model,
    "secs": started.elapsed().as_secs_f64(),
    "chars": <finding length>

Then `research_secs` decomposes into concurrent lens work versus whatever is left, and the serial
remainder becomes a number instead of an inference.

⚠️ NOT WRITTEN AS CODE, ON PURPOSE — the same call as F578/F591/F603. `model` is moved into an async
closure here and the emit needs the run's `EventSink` threaded into that scope; that is a signature
question, not an additive JSON value, and the binary is frozen so it cannot be compiled. F560/F567
were queued as commits only because they are single additive values that cannot break a build.

## The evidence AGAINST prioritising this, stated because it is real

- **The prize is bounded and small.** The whole phase is ~7 min of a ~120 min run, so even perfect
  parallelisation returns at most ~3 min — against an arm difference of 7.4 min that is itself only
  0.69 SE. Compare F628's ladder projection at ~9.1 min/run.
- **It is an instrument, not a fix.** It tells you where the seven minutes go; it does not shorten
  them, and the serial-component hypothesis might turn out to name something irreducible (a model
  load).
- **F642's own registration was compromised** — the write was refused by the three-field guard while
  the measurement ran anyway, so the 4.6% is disclosed as not-cleanly-pre-registered. The source
  observation about `select_lenses` was read before measuring and is unaffected.

**One finding here needs no instrument and should be recorded regardless:** the author's straggler
fix WORKS. 39 of 40 gradable runs return the full lens set (97.5%), across six builds, against the
author's own measured 6-of-6 loss before the gate. Their written falsifier did not fire.
