---
paths:
  - "crates/goose/src/agents/agent.rs"
  - "crates/goose/src/agents/*.rs"
---

# agent.rs — steer, cancel, and the reply loop. What was measured, not assumed.

## A steer CANNOT interrupt a call in flight

`drain_pending_steers` runs only at the top of the loop, gated by a flag set AFTER the provider
stream closes. A steer lands at the NEXT turn boundary — so a pure-reasoning call (no tool calls
yet) structurally cannot receive one mid-stream. Measured: r1's looping reasoning-only call ignored
six steers; r5's opener and skeleton both ignored one each. The escalation that works is the swarm's
judge RESTREAM (cut + re-seed with ESTABLISHED + the verbatim tail since 63ebe140b) — it lives in
swarm.rs, not here. Do not "fix" steers to interrupt streams; the cancel token is the sanctioned
interrupter.

## Cancel keeps the partial

The CancellationToken breaks the stream loop at a chunk boundary and the cancelled path falls
through NORMAL persistence — the partial output is kept (pinned by tests/agent.rs). `fix_messages`
repairs an orphaned tool-request pairing. Whether LM Studio accepts the repaired history is NOT
settled from code — prefer cancel only when no tool call is in flight.

## The swarm's stake in this file

`run_agent_in_inner` (swarm.rs) consumes this stream in its own loop with two digest write sites;
the TOOL_FORMING_OBSERVER task-local crosses this file's await points — never `tokio::spawn` a
detached consumer of the stream here, or the task-local scope (and the forming sidecar with it)
silently detaches.

## No time may bound the stream

Gate 5: read/total windows were deleted (II-7); only connect-timeout remains (transport). Any new
`tokio::time::timeout` around provider work is rejected on sight.
