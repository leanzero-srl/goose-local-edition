---
paths:
  - "crates/goose/src/agents/agent.rs"
  - "crates/goose/src/agents/*.rs"
---

# agent.rs — steer, cancel, and the reply loop. What was measured, not assumed.

## A steer interrupts a generation only at a chunk boundary, and only before the first tool request of the turn

STALE until 2026-09-01: this section said a steer could not interrupt a call in flight. Since `eeda65809`/
`40c231152` the inner stream loop is forced out at the next chunk when a steer is queued:
`steer_may_interrupt = !saw_tool_request_in_turn && has_pending_steers(...)` and the
`tokio::select! { biased; … _ = std::future::ready(()), if steer_may_interrupt => break, _ = self.steer_arrived.notified(), if !saw_tool_request_in_turn => break, next = stream.next() … }`
(`agent.rs`, grep `steer_may_interrupt`); `steer()` ends with `steer_arrived.notify_waiters()`. Measured on
r6d: a judge steer queued at `thinking_chars: 24280` landed mid-generation — the lane's think.log pivots at
exactly that offset. Once a tool request has been seen in the turn, the steer waits for the turn boundary
(`can_drain_pending_steers`, set after the stream closes) so a tool call is never cut in half.

Since `65df1cd55` a steer-cut turn on a structured-output lane is NOT treated as a forgot-final-output turn:
the arm `Some(None) if self.has_pending_steers(&session_config.id).await => {}` precedes the bare `Some(None)`
arm that pushes `FINAL_OUTPUT_CONTINUATION_MESSAGE` — before it, r6d's q5 received "You MUST call the
final_output tool NOW" paired with the relay/steer in one delivery (`tests/agent.rs`
`test_steer_cut_turn_on_a_structured_lane_is_not_nudged_for_final_output`, proven failing on the old arm).

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
