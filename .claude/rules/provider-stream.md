---
paths:
  - "crates/goose-provider-types/src/formats/*.rs"
  - "crates/goose-providers/src/**"
---

# The provider stream decode — the most load-bearing shared code in the repo

Every provider's bytes pass through here. A defect ships to every phase of every run at once.

## The MessageStream contract: partial text, COMPLETE tool calls

`base.rs:282-285`. Text and thinking may stream as deltas; a ToolRequest is yielded only whole.
The transient `ToolFormingEvent` observer seam (openai.rs:~44) is the ONLY sanctioned way to see a
tool call mid-formation — `Forming` → `ArgsDelta{id,delta}` (empty fragments never emitted; forwarded
verbatim, in order, never batched) → `Complete`. The observer no-ops outside a
`TOOL_FORMING_OBSERVER.scope` (swarm arms exactly ONE, in `run_agent_in`'s wrapper), and the
task-local's own doc states the law: it does not cross `tokio::spawn` — the scope must live in the
same task that drives the stream. TWO more contracts the enum's docs carry (openai.rs:~60): a
stream that dies mid-call never publishes `Complete`, so a consumer must ALSO clear forming state
when the provider-call future completes — that is what `FormingGuard`'s Drop exists for. Do not add
a persisted MessageContent variant for progress — codex's 1,100-line sink was refused for that.

## A length cut must NEVER read as completion

`finish_reason == "length"` appends the deterministic stamp `[OUTPUT TRUNCATED: the model hit its
output-token limit mid-generation — this response is INCOMPLETE]` (bb539f7d6, openai.rs:~1440).
VERIFIED 2026-08-30: no engine code greps that text — its readers are the MODEL itself (the stamp
sits in the history a retry re-reads), the judge reading the lane's words, tick.py's guillotine
counter, and humans. So it is a words-layer signal, not a code hook; the LMS 25k output cap
(Mihai, 2026-08-30) makes it live. Reword it only by updating tick.py's grep in the same commit —
nothing else matches it, and that is easy to break silently.

## LM Studio's routing, measured

The swarm reaches ALL fleet nodes through the LOCAL LM Studio daemon (engine → 127.0.0.1:1234 →
remote device). lmstudio.json declares `engine: openai` → `openai_compatible::stream_openai_compat`
→ `response_to_streaming_message`. Anthropic/google decoders never see ToolFormingEvent; ollama's
`response_to_streaming_message_ollama` (ollama.rs:159) wraps the standard OpenAI streaming handler,
so it inherits the events — but it BUFFERS tool-call messages until stream end (its own doc,
:153-155), so forming timing there differs; no observer is armed outside the swarm anyway.

## The test-module trap (refuter-caught, twice)

The formats test module holds its OWN exhaustive matches over ToolFormingEvent (shape formatter +
sequence asserts). A new variant compiles the run path and BREAKS the tests — extend them honestly
(the sequence assert must reflect the real new order, not be loosened). Fixtures are hand-written
to the OpenAI chunk grammar, not raw captures — say so in any claim about them.

## Retry only before the first item

`reply_parts.rs:324-353`: a provider stream that fails BEFORE its first item is retried (logged
"Provider stream failed before its first item, retrying (n/m)"); after the first item it is the
caller's problem (partials are kept). Do not widen retry past that line — replaying a
half-consumed stream duplicates content.
