# Next Swarm Liveness: Deferred Authority Work

The scheduler-side nudge and broker changes deliberately stop at boundaries that the current
authority model can prove. The following work must not be approximated with task-name heuristics,
synthetic artifacts, or relaxed lifecycle checks.

## Typed unavailable obligations

An exhausted task currently becomes `Failed`. Independent DAG branches continue, and terminalizing
its blocked descendants prevents `scheduler_stuck`, but there is no typed way to carry the missing
core obligation to a repair worker or final ruler. Adding `Unavailable` safely requires one coherent
schema change across `TaskState`, dependency release, checkpoint replay, `RunReport`, event receipts,
repair admission, and final-ruler input. A missing artifact must remain missing; an unavailable
receipt must never masquerade as artifact evidence. This slice therefore does not claim ordinary
exhaustion recovery.

## Bonus priority provenance

Physical execution rejects the legacy dynamic replanner, so it cannot currently enqueue bonus work.
If physical bonus work is introduced, its origin must be sealed into `WorkOpportunity`, admission
receipts, and journal replay so the broker can order it below judge/review work. Role or task-id
inference is not sufficient authority.

## Provider-turn delivery boundary

A semantic nudge is queued into the exact agent session, atomically reserves cancellation of the
captured provider turn, waits for that exact request's accepted `Cancelled` terminal, and is consumed
by the next provider call in the same session. The engine does not inject a message into an active
HTTP response stream. Any future mid-stream protocol needs explicit provider support and equivalent
request/terminal evidence; it must not bypass this boundary.
