---
paths:
  - "crates/goose/src/agents/agent.rs"
  - "crates/goose/src/agents/*.rs"
---

# agent.rs — steer, cancel, and the reply loop. What was measured, not assumed.

## A steer CANNOT interrupt a call in flight

`drain_pending_steers` (agent.rs:573) runs only at the top of the reply loop
(`can_drain_pending_steers` checked at :2000), and that flag is set in exactly one place — AFTER
the provider stream closes (:2685; initialized false at :1993). A steer lands at the NEXT turn boundary — so a pure-reasoning call (no tool calls
yet) structurally cannot receive one mid-stream. Measured: r1's looping reasoning-only call ignored
six steers; r5's opener and skeleton both ignored one each. The escalation that works is the swarm's
judge RESTREAM (cut + re-seed with ESTABLISHED + the verbatim tail since 63ebe140b) — it lives in
swarm.rs, not here. Do not "fix" steers to interrupt streams; the cancel token is the sanctioned
interrupter.

## Cancel keeps the partial

The CancellationToken breaks the stream loop at a chunk boundary and the cancelled path falls
through NORMAL persistence — the partial output is kept, pinned by
`test_cancel_midstream_preserves_partial_then_reply_continues` (tests/agent.rs:3632), whose asserts
require every delta received before the cancel and nothing past it. `fix_messages`
repairs an orphaned tool-request pairing. Whether LM Studio accepts the repaired history is NOT
settled from code — prefer cancel only when no tool call is in flight.

## The swarm's stake in this file

`run_agent_in_inner` (swarm.rs) consumes this stream in its own loop with two digest write sites;
the TOOL_FORMING_OBSERVER task-local crosses this file's await points, and its own doc
(provider-types openai.rs:~66) states the law: task-locals do not cross `tokio::spawn` — never
spawn a detached consumer of the stream here, or the forming sidecar silently detaches.

## No time may bound the stream

Gate 5: read/total windows were deleted (II-7); only connect-timeout remains (transport). Any new
`tokio::time::timeout` around provider work is rejected on sight.
