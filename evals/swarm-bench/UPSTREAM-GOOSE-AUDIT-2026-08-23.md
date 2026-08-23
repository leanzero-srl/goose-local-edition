# Upstream Goose audit for the local swarm

Date: 2026-08-23  
Audit type: read-only source/history/runtime-evidence review  
Fork snapshot: `7b770ff00db475d0034f7635b320e574273e14af` (`codex/swarm-provider-boundary`)  
Upstream snapshot: `8d844eecbdfd65626a881c9e8784ae8dc6093f1d` (`upstream/main`)  
Merge base: `a0aed81f36076cfe48def4b21c04d7f0d33072e8`  
Upstream range inspected: 508 commits after the merge base

This audit did not edit the integration checkout, build Goose, launch a model, alter LM Studio, or
touch a running benchmark. Source inspection, `git` history, read-only SQLite queries, archived
request/publish artifacts, and primary LM Studio/llama.cpp sources are the evidence. The document
itself was created on a separate worktree and branch.

## Executive conclusion

The most important missing upstream idea is not one of the four named commits. It is typed provider
terminal state. The fork's agent still converts an exhausted `ProviderError` into ordinary assistant
text; `GooseAgentDispatcher` concatenates that text into `RunAgentOut`; and the task dispatcher can
then treat it as a successful task result. The fork already contains a physical-broker
`ProviderTerminalKind`, but the main Goose agent stream does not feed actual provider failures into
it. This is a correctness boundary, not a tuning preference, and should be fixed before more retry,
fan, or repair policy is trusted.

The existing cache adaptation is also not the final upstream design. Moving a volatile turn-context
block to the request tail makes two equal-shaped requests differ only at the tail, but it does not
make request N a prefix of request N+1 during a tool loop. Upstream subsequently replaced relocation
with a once-composed, persisted, append-only turn-context event and added a regression test that
specifically catches relocated tails. The current fork test does not exercise that relation.

Of the four named candidates:

- `85aac1940` should be ported together with its prerequisite `5751715df`, narrowly.
- `465269e5d` is directionally correct but superseded by `66051ec7d`; porting it alone does not repair
  the fork's current tail-relocation invariant.
- `bb539f7d6` is only partially adapted and remains unsafe around empty terminal chunks, truncated
  tool arguments, Responses API, Anthropic, and dispatcher consumption.
- `c13aa86a7` should not be ported for the local swarm. Its tool-label surface does not exist here,
  and `complete_fast` already forces `ThinkingEffort::Off` for both fast and fallback calls.

No upstream commit in this range solves the reported 191,000-character uncapped reasoning loop or
the one-detail-task tail directly. Output-limit metadata helps only when a provider actually reports
an output limit; shell streaming helps only during a tool call. The fork's stream fingerprint and
judge visibility work therefore remains a separate, fork-specific mechanism.

## Ranked findings

| Rank | Finding | Correctness/quality impact | Throughput impact | Confidence |
|---|---|---|---|---|
| P0 | Preserve typed provider terminal status through agent -> dispatcher -> scheduler | Critical: prevents crashes, invalid requests, and exhausted retries from becoming false-success text | High indirect: prevents poisoned dependencies and useless downstream work | High |
| P0 | Complete output-token-limit handling from `bb539f7d6` | Critical when hit: never execute truncated tool arguments or accept incomplete read-only output | High on affected calls; neutral otherwise | High |
| P1 | Port structured context-overflow classification from `5751715df` + `85aac1940` | High: routes real overflow into compaction instead of generic failure | Avoids whole-task retries/failures | High |
| P1 | Replace tail relocation with append-only turn-context assembly from the final `66051ec7d` design | High semantic/cache invariant; no intended answer change | Potentially meaningful TTFT reduction, but current magnitude must be remeasured | High on defect, medium on wall-clock magnitude |
| P2 | Add bounded live shell progress plus cancellation from `cf312f1d1` | High for long repair commands: avoids false stalls and orphaned work | Medium when repair spends time inside shell tools | Medium-high |
| P2 | Port the low-risk stream/context hardening set (`1f6c7524e`, `f3ab1557c`, pending `8d2166144`, `701e93ab4`) | Prevents pathological allocation/context amplification | Low to medium, workload-dependent | High on mechanics |
| P2 | Port full choice-less SSE classification from `1844d3fb4` for cloud/OpenAI-compatible gateways | Prevents metadata frames from aborting or silently truncating a tool call | Low for LM Studio; relevant to cloud benchmark providers | Medium-high |
| P3 | Finish owner-aware, pre-inspection tool-name canonicalization from `f2e6e9ed0` | Security/correctness improvement for GLM/Minimax-style mangling | No current fleet speed case proved | Medium |

Priority is by correctness and quality risk, not implementation size.

## P0: provider errors still cross the swarm boundary as successful text

### Current mechanism

`crates/goose/src/agents/agent.rs` sets `provider_errored`, but its terminal branches emit user-facing
messages rather than a typed terminal event:

- context handling begins around lines 2489-2547;
- credits around 2549-2573;
- refusal around 2574-2589;
- network errors around 2590-2601;
- every other provider error around 2602-2613, using `Ran into this error: ...`.

`AgentEvent` at lines 272-278 has only `Message`, `Usage`, `McpNotification`, and `HistoryReplaced`.
There is no provider terminal variant.

`crates/goose-cli/src/commands/swarm.rs` then:

- defines `RunAgentOut` at lines 14528-14535 with text, final output, session id, and tool calls only;
- concatenates every `MessageContent::Text` at lines 17661-17683;
- returns `Ok(RunAgentOut)` at lines 17842-17848.

The task dispatcher has one string predicate, `is_stream_decode_interrupt` at lines 3382-3393. It
requires the literal `Stream decode error` plus an exact trailing resend sentence. The main task path
uses it around lines 32032-32061. Generic network failures, server failures, rate limits,
authentication errors, request errors, invalid models, and refusals do not carry a typed outcome to
the scheduler. Several response-only paths repeat the same special-case predicate.

The source itself documents the false-green mechanism at `swarm.rs` lines 32033-32040 and the
provider error swallowing at lines 37571-37574. Those comments correctly identify one signature but
also prove the architectural problem: scheduler behavior depends on parsing prose that was written
for a human.

### Evidence in retained data

A read-only query of `~/.local/share/goose/sessions/sessions.db` at audit time found 319,753 messages
across 62,193 sessions. This database contains all Goose sessions, not only swarm sessions, so it is
mechanism evidence rather than a fleet incident rate.

- 55 user text blocks begin exactly with `Ran into this error:`; all 55 occur in distinct sessions.
- 675 user text blocks contain the phrase, so 620 contain it embedded inside a larger downstream
  prompt/specification.
- Of the 55 exact propagated texts: 45 mention invalid models, 6 peer keepalive timeout, 3 backend
  crash/fatal-exception conditions, and 1 another condition.

The role matters: these are not merely the original assistant display messages. They have been
reintroduced as user-side input in later sessions. Five retained benchmark publish payloads make the
swarm propagation concrete:

- `evals/swarm-bench/nodeloop/bench/payloads/9a4aef1eba97.json`
- `evals/swarm-bench/nodeloop/bench/payloads/74aa66c14873.json`
- `evals/swarm-bench/nodeloop/bench/payloads/3635561cbd07.json`

contain an invalid-model error under `Output of dependency verify::store`;
`e1851cd41754.json` contains an LM Link `peer_keepalive_timeout` as completed dependency `entry`; and
`9cff8d410673.json` contains another invalid-model response in verification context. The engine did
not merely display these failures: it carried them forward as task material and ultimately retained
them in benchmark payloads.

### What upstream provides, and why it is not sufficient unchanged

The large unrolled-loop commit [`ca52cce62`](https://github.com/block/goose/commit/ca52cce62)
introduced `MessageContent::Error(ErrorContent)`, `MessageErrorKind`,
`Message::from_provider_error`, and an `ExitOnErrorOperation`. Upstream current has
`Authentication`, `ContextLengthExceeded`, `CreditsExhausted`, and `Other` kinds.

That establishes the right discriminated boundary, but `Other` is too coarse for swarm routing:
network, server, rate-limit, invalid request/model, refusal, usage, and execution failures need
different retry and device-health semantics. The whole commit is 88 files and more than 11,000 added
lines, and conflicts deeply with the fork's custom agent loop. It must not be cherry-picked wholesale.

### Narrow fork design

Prefer a dedicated terminal event over immediately adding a public `MessageContent` variant across
all UI/OpenAPI exhaustive matches:

1. Add `AgentEvent::ProviderTerminal(ProviderTerminal)` and retain the existing human-facing message.
2. Emit it only when the provider incident is terminal to the current turn/run. A first context
   overflow that is successfully compacted is an incident, not a failed task; an overflow still
   present after recovery is terminal.
3. Preserve the full current `ProviderError` classification, including rate-limit retry delay, or
   define a lossless equivalent. Do not collapse it to upstream's `Other`.
4. Add `provider_terminal` to `RunAgentOut`; capture the event separately from text.
5. Map network/server/rate-limit to an infrastructure retry; invalid-model/endpoint to unavailable
   route/config; authentication/credits/refusal to provider/run terminal; context overflow to the
   explicit compaction/recovery outcome; execution/usage to a loud engine failure.
6. Extend `DispatchError` so a transient can carry typed class and `retry_after`, rather than
   re-parsing a string.
7. Feed this result into the existing physical broker. Its current `ProviderTerminalKind`
   (`Finished`, `Failed`, `Cancelled`) is useful lifecycle truth, but `GooseAgentDispatcher` does not
   populate it from the actual provider stream, and the kind has no failure reason.

Affected fork files:

- `crates/goose/src/agents/agent.rs`
- `crates/goose-cli/src/commands/swarm.rs`
- `crates/goose-swarm/src/dispatch.rs`
- `crates/goose-swarm/src/scheduler.rs`
- `crates/goose-swarm/src/broker.rs` and control-plane event serialization
- optionally `crates/goose-provider-types/src/conversation/message.rs` if persistence is chosen

Required regressions:

- every `ProviderError` variant before the first item and after one streamed item;
- retry-exhausted `b7ddf933c` first-item failure becomes a typed terminal;
- recovered context overflow does not poison the successful task;
- a read-only task, planner, reviewer, and sink cannot return `Ok(TaskRunOutput)` from provider prose;
- no scheduler path depends on the wording of a human-visible message.

## P0: `bb539f7d6` is only partially adapted

The fork's `349bf453c` adaptation adds a visible marker in
`crates/goose-provider-types/src/formats/openai.rs` around lines 1363-1373, but only inside the branch
where the same chunk contains text/reasoning content.

That leaves four concrete holes:

1. OpenAI-compatible servers commonly send `finish_reason: "length"` on an empty final delta. The
   current branch at lines 1383-1387 yields only usage or nothing, so no marker is emitted.
2. During tool-call accumulation, any non-empty finish reason ends the inner loop at lines
   1183-1185. `length` is not recorded, and accumulated arguments can still become an executable tool
   request.
3. `MessageMetadata` and `RunAgentOut` carry no output-limit flag, so the scheduler cannot
   distinguish a complete result from an incomplete one without parsing marker text.
4. OpenAI Responses `incomplete/max_output_tokens` and Anthropic `max_tokens` are not surfaced.

Upstream [`bb539f7d6`](https://github.com/block/goose/commit/bb539f7d6) covers OpenAI Chat, OpenAI
Responses, Anthropic, Google, persistence, and UI. It is a 17-file cross-layer change, so a wholesale
cherry-pick would collide with the fork's provider adaptations.

There is also already fork work on non-ancestor branch commit `610e48b2f` (and equivalent
`d59234f54`) named `Reject output-truncated provider tool calls`. It adds message metadata and rejects
truncated OpenAI/Google tool calls. Coordinate with that branch instead of duplicating it. Even if
merged unchanged, it still needs:

- dispatcher/scheduler consumption of the typed flag;
- OpenAI Responses and Anthropic coverage;
- a final-empty-delta regression;
- a policy for independently verified artifacts.

The correct policy is not an unconditional rerun. A truncated tool call must never execute, and a
read-only/planning/review output is incomplete and must retry. A file-producing worker whose files
already pass independent objective gates may be salvageable; the terminal should be classified as
incomplete and sent through those gates rather than discarded or blindly accepted.

This is provider-declared terminal truth, not a deterministic content judge and not a new generation
cap. It cannot stop the 191k uncapped loop if LM Studio never emits an output-limit terminal.

## P1: port `5751715df` and `85aac1940` together

Current `crates/goose-providers/src/http_status.rs` classifies a 400 as context overflow only from the
extracted message string. This misses valid structured and byte-limit errors:

- [`5751715df`](https://github.com/block/goose/commit/5751715df) adds request-body,
  Content-Length, payload-size, and byte-limit phrase classification.
- [`85aac1940`](https://github.com/block/goose/commit/85aac1940) checks
  `error.code == context_length_exceeded` case-insensitively and the structured relation
  `error.n_prompt_tokens > error.n_ctx`.

Without that classification, a generic 400 becomes `RequestFailed`, so the existing agent compaction
recovery is bypassed. LM Studio model-metadata probing and proactive context limits do not replace
response classification: allocation, schemas, image/tool payloads, and backend-specific limits can
still make the actual request fail.

This is a narrow, low-conflict port into `crates/goose-providers/src/http_status.rs`. Preserve the
fork's public `is_context_length_exceeded_message`, because
`crates/goose/src/providers/utils.rs` calls it. Add table-driven tests for:

- every byte-size phrase from `5751715df` and nearby false positives;
- structured code with empty/generic message;
- numeric `n_prompt_tokens/n_ctx`, including zero/equal/non-numeric cases;
- the existing rate-limit retry-delay and response-deadline behavior, which must not regress.

No retained fleet corpus proves how often these two missing shapes occur, so the correctness case is
strong but a wall-clock claim would be speculative.

## P1: the current cache optimization is not sequentially prefix-safe

### What the fork currently does

`crates/goose/src/agents/moim.rs` recomputes the turn context on every inner provider loop:

- session usage and context limit at lines 50-86;
- `turns_taken` and budget at lines 87-95;
- `chrono::Local::now()` at line 145.

It inserts that context into a clone and does not persist it. `agent.rs` calls `inject_moim` on each
loop at lines 2027-2043.

The `affd1cea1` adaptation in `crates/goose-provider-types/src/formats/openai.rs` extracts the block
and appends it to the request tail (lines 203-212 and 516-572). Its test at lines 4301-4364 compares
two conversations with the same shape and different timestamps. It proves that only the tail differs
between those two synthetic requests; it does not compare consecutive request states.

The counterexample is mechanical:

```text
persisted state before first call: [U0]
wire request N after relocation:    [U0, TC]

persisted state after a tool call:  [U0, A1, T1]
wire request N+1:                   [U0, A1, T1, TC]
```

Request N is not a prefix of request N+1. At the position where N had `TC`, N+1 has `A1`. Moving the
volatile block to the latest tail prevents timestamp churn within equal-shaped requests, but it also
moves previously sent bytes after newly appended tool history on the next request.

The Anthropic adaptation uses explicit cache breakpoints, so its provider-specific behavior is not
identical to LM Studio's implicit OpenAI-compatible prefix. The local fleet path is OpenAI Chat and
is the priority here.

### Upstream's final design

[`465269e5d`](https://github.com/block/goose/commit/465269e5d) freezes timestamp, compaction
information, and turn count at outer-turn start. It is a useful prerequisite but not sufficient with
the fork's current relocation.

[`66051ec7d`](https://github.com/block/goose/commit/66051ec7d) replaces the workaround with a
once-composed, agent-only, tagged user message persisted at turn start. It is never moved or edited.
Its `prefix_invariance.rs` builds truly sequential states with assistant/tool exchanges and tests:

- OpenAI Chat strict append-only request relation;
- Responses API strict append-only relation;
- Anthropic/OpenRouter/Databricks explicit-breakpoint compatibility;
- `seeded_regression_relocated_tail_is_caught`, which deliberately moves the context to the tail and
  asserts that the invariant checker rejects it.

That last test is a direct falsifier for the fork's current strategy.

### LM Studio/llama.cpp evidence and caveat

LM Studio logs explicitly report looking for prompt sequences and a `Cache reuse summary`; an
[LM Studio issue](https://github.com/lmstudio-ai/lmstudio-bug-tracker/issues/778) shows a repeated
prompt reusing 939/939 tokens and decoding only one prompt token. The
[llama.cpp server documentation](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md)
defines `cache_prompt` as reuse of a common prefix so only the differing suffix is processed.

Concurrency complicates the expected gain. llama.cpp documents similarity-based slot selection and
explicit `id_slot`; its [KV reuse tutorial](https://github.com/ggml-org/llama.cpp/discussions/13606)
shows that reuse is slot-sensitive. A 2026
[slot-local checkpoint issue](https://github.com/ggml-org/llama.cpp/issues/22942) demonstrates that a
matching checkpoint on another slot can still cold-prefill. Therefore, append-only request assembly
is necessary request hygiene but is not proof that production LM Studio scheduling will hit the
cache on every turn.

Do not add hard session affinity blindly: reserving a slot can reduce concurrency and increase idle
node time. Measure single-slot first to prove request-shape reuse, then the production slot setup to
measure routing loss.

### Narrow integration plan

Do not cherry-pick `66051ec7d`: it spans 39 files and the unrolled state machine, formatter rewrites,
context management, session code, and security projection, all of which conflict with this fork.

Port the invariant and lifecycle:

1. Compose the complete turn context once at user-turn start.
2. Persist it immediately as an agent-only, tagged user message before any assistant/tool output.
3. Never move or edit an older turn-context message.
4. Project it to providers while filtering it from user-only recipe/export views as upstream does.
5. Remove OpenAI/Anthropic relocation only after the sequential request tests pass.
6. Include the fork's assistant prefill and forced-tool request parameters in request-level
   invariance tests; those synthetic suffixes must not reorder the persisted prefix.
7. Instrument serialized-request longest common prefix, LM Studio cache-reuse logs, prompt tokens,
   and TTFT.

The retained F136 measurement is a pre-fix baseline, not proof of the current implementation's gain:
16 fleet worker transitions, 5 breaks (31%), median 12,291 characters re-prefilled, with an honest
estimated 1-3% wall-clock effect at the time. F803 later called tail append cache-safe, but it proved
wire presence and equal-shaped test order, not sequential prefix invariance. Remeasure the current
Qwen3.8 fleet rather than carrying either conclusion forward.

Affected files:

- `crates/goose/src/agents/agent.rs`
- `crates/goose/src/agents/moim.rs`
- `crates/goose-provider-types/src/conversation/message.rs`
- `crates/goose-provider-types/src/conversation.rs`
- `crates/goose-provider-types/src/formats/openai.rs`
- `crates/goose-provider-types/src/formats/anthropic.rs`
- new request-shape integration tests under `crates/goose-provider-types/tests/`

## P2: live shell progress and cancellation for repair

`GooseAgentDispatcher::run_agent_in` resets its productive watchdog on `AgentEvent::McpNotification`
(`swarm.rs` lines 17773-17778). The built-in developer shell cannot currently produce one:

- `crates/goose/src/agents/platform_extensions/developer/mod.rs` lines 190-201 ignores the supplied
  `CancellationToken` and calls `shell_with_cwd` without any notification emitter.
- `crates/goose/src/agents/platform_extensions/developer/shell.rs` waits for process completion and
  only then returns the collected output.

A legitimately long, active build/test/repair command can therefore look silent to the swarm, and a
dropped task future does not use the MCP cancellation token to stop the shell process.

[`cf312f1d1`](https://github.com/block/goose/commit/cf312f1d1) adds request-scoped tool notification
plumbing, batches shell output (bounded cadence and bytes), and wires cancellation. It spans 14 files
and assumes newer tool-call context/cancellation plumbing, so port only the engine/headless slice:

- request-scoped notification emitter through `ToolCallContext`/`ExtensionManager`;
- bounded stdout/stderr progress batches;
- cancellation token into shell execution and process-group cleanup;
- explicit cancellation when `run_agent_in` aborts, stalls, or is dropped.

The watchdog must reset only when new bytes/progress actually arrive. Repeated empty notifications
would otherwise create a new immortal-loop mechanism. Byte bounds here are memory-safety bounds, not
fixed model-thinking or task-time caps.

This is relevant to repair reliability, but retained evidence does not show that the historical 420s
stalls were predominantly long shell calls; several were generation or compaction silence. Its
throughput rank is therefore below the typed terminal and cache work.

## P2 hardening set

### `1f6c7524e`: coalesce thinking in `collect_stream`

`Conversation::push` already coalesces compatible single-block thinking deltas. However,
`crates/goose-provider-types/src/base.rs::collect_stream` lines 322-371 coalesces only text.
`complete_fast` and compaction use this collector, so a thinking-heavy auxiliary completion can
allocate one `MessageContent::Thinking` block per streamed delta. Port upstream's signature-aware,
single-block-only coalescing and tests. This reduces memory/object amplification; it does not stop a
model from generating repeated reasoning.

### `f3ab1557c`: bound partial `<think` tag candidates

Current `ThinkFilter` can retain an unterminated, quoted `<think ...` candidate without bound.
Upstream caps the candidate buffer at 8 KiB and releases oversized capacity while preserving the
bytes as ordinary content/thinking. Port as defensive stream parsing. This is not the 191k loop fix:
normal closed thinking content does not sit in this candidate buffer.

### `3b3719de4`, superseded locally by pending `8d2166144`

The current shell has a 50,000-byte full-output threshold but builds a head/tail preview from 50
selected lines without a byte limit. One giant line can therefore still inject a giant preview into
context. Upstream `3b3719de4` bounds a tail preview; the fork already has a stronger non-ancestor
commit `8d2166144`, `fix(developer): byte-bound both ends of shell previews`, which preserves both
header and tail under a 10 KiB UTF-8-safe budget. Integrate that existing fork commit rather than
duplicating the weaker upstream patch.

### `701e93ab4`: composite session-message index

Current schema version is 14 and the live database has separate indexes on `session_id`, timestamp,
and message id. A read-only `EXPLAIN QUERY PLAN` for the exact session load query reports:

```text
SEARCH messages USING INDEX idx_messages_session (session_id=?)
USE TEMP B-TREE FOR ORDER BY
```

The database currently has 319,753 messages; the largest session has 2,383 messages, and 83 sessions
have at least 100. Port the composite `(session_id, created_timestamp, id)` index as the fork's next
schema version, not upstream's hard-coded version 16. This is a proven local sort removal, but most
sessions are short (mean about five messages), so it is hygiene rather than a headline swarm gain.

### `1844d3fb4`: choice-less SSE frames

Current OpenAI streaming already handles a real `"choices": []` usage frame. It still deserializes a
JSON object with no `choices` field as a stream decode error, and simply defaulting it to an empty
choice list would be wrong inside tool accumulation because it would terminate argument collection.
Upstream distinguishes:

- metadata-only choice-less frames, which are skipped;
- choice-less frames carrying status/type/detail error signals, which become a loud server error;
- real `choices: []` usage frames, which remain real chunks.

This is unlikely to affect direct LM Studio Chat Completions, but it is relevant to Z.ai/DeepSeek or
other OpenAI-compatible cloud gateways used by the parallel benchmark campaign. Port the full
classifier and its mid-tool-call regression, not only an optional `choices` field.

## P3: owner-aware tool-name recovery

The fork already contains the basic `1b2f77f71` malformed-tool recovery and a custom
`recover_mangled_tool_name` in `extension_manager.rs`. It strips `functions.` and maps an advertised
`extension__tool` to `extension.tool`, but it does not use owner metadata for unprefixed platform
tools. It also canonicalizes at dispatch time, after categorization/permission inspection.

[`f2e6e9ed0`](https://github.com/block/goose/commit/f2e6e9ed0) adds two useful properties:

- `developer.shell` can map to advertised unprefixed `shell` via owner metadata;
- canonicalization occurs before permission inspection and hooks, so policy checks see the same
  canonical name that later executes.

Port those properties while preserving ambiguity refusal. Do not introduce blind hard-coded aliases
such as `cat -> shell` or `read -> shell`.

The retained database has 415 structured `Tool '...' not found` responses: `cat` 288,
`final_output` 36, `read` 34, `read_file` 23, `test_tool` 19, `resolve-library-id` 9, `web_search` 4,
and one each for `finish` and `bash`. This does not prove `f2e6e9ed0` is a current fleet speed fix:
after 2026-08-01, the recent matches are test-fixture `test_tool` failures, and most older names are
invented aliases rather than namespace mangling. The port is still a correctness/security
improvement, but its throughput effect must not be sold from these counts.

## Named commit that should not be ported: `c13aa86a7`

`c13aa86a7` is for upstream's generated tool-call title/chain-summary surface. This fork does not
have `crates/goose/src/tool_call_labels.rs`, and the swarm does not invoke that UI enrichment path.

The fork's `complete_fast` already applies `ThinkingEffort::Off` to the chosen fast model and to its
main-model fallback (`crates/goose/src/model_config.rs` lines 128-164). For Qwen-family configured
fast models it also injects the preclosed thinking assistant prefix. The LM Studio provider registry
does not supply a default fast model, and no `GOOSE_FAST_MODEL` was set in the audit environment.

Porting `c13aa86a7` therefore adds no swarm throughput value. Its related
`31d3ff2bd` disables paid prompt-cache writes for one-shot cloud calls; that is a cloud billing
optimization, not LM Studio KV-cache reuse.

## Compaction and repair candidates not recommended as direct ports

### `ad87dd4c3`: structured compaction summary

This could improve continuity after compaction, but it adds a JSON/template contract to a model class
whose structured-output reliability is precisely under evaluation. It also conflicts with the
fork's K4 tail retention/reasoning stripping. Treat it as an A/B arm with a plain-text fallback and
score post-compaction requirement retention, JSON failure rate, tokens, and recovery time. Do not
make it default from theory.

### `33fb29402`: compact with the main session model

In the current local setup it is inert because no default/configured fast model is active. If a fast
model is introduced, upstream's quality choice and the fork's K1 latency choice conflict. Decide
from an empirical compaction-retention cell, not by cherry-pick.

### `4de6fe206`: bound recipe retry diagnostics

It bounds stderr for recipe `retry/on_failure` commands. The custom swarm repair scheduler does not
use that recipe retry path (`retry_config` is absent for these tasks), so this does not address the
reported repair-phase wall. It is reasonable general hardening but outside the critical swarm path.

### Other low/no-value changes for this path

- `950575bcd` only compacts tool-schema JSON in toolshim mode; the swarm uses native tool calls.
- `36cb569e3` schema `oneOf` rewriting has no measured current tool-schema incompatibility here.
- `a3c20531e` truncates function names to provider limits; current names are small.
- `f47a9620d` handles signed Anthropic thinking after a mid-session model switch; the LM Studio fleet
  keeps one OpenAI-compatible model per task.
- `bc6804922` fixes a Linux stdio-MCP child lifetime; all three local nodes are macOS.
- ACP/UI/session-load changes (`6a1344ba4`, `7f1666abb`, `e6b12beb4`) do not sit on the headless swarm
  dispatch hot path.

## Upstream work already present or adapted: do not duplicate

The fork already contains the following relevant semantics, sometimes under fork commit ids:

- `b7ddf933c` first-item transient retry -> `48662f727` adaptation.
- `3c1fdd692` empty provider-turn retry -> `ba5d37f23`.
- `1b2f77f71` GLM/Minimax malformed tool recovery -> `99360a89b` plus later fork work.
- `824b167af` empty finish reason is non-terminal -> `349415eb3`/equivalent.
- `1e03bbb56` preserve reasoning across multi-turn tool calls -> `f1cce3b36`.
- `ee61c7c49` incremental CLI streaming render -> `885e5d05a`.
- `7e431ac6f` inactivity rather than total streaming timeout -> `a556679e9` adaptation.
- `97f150db6` default developer-shell timeout -> `8375fe45a`.
- `d5a8a3fb9` atomic inventory/schema creation -> `8ed0d99d7`.
- `affd1cea1` tail relocation -> `5abfbfe03`; present, but supersede it with the append-only design
  described above rather than treating it as final.
- `bb539f7d6` visible output marker -> `349bf453c`; present only as the incomplete slice described
  above.

## Conflict and ingestion risk

| Candidate | Direct cherry-pick risk | Required approach |
|---|---|---|
| `ca52cce62` | Extreme: 88 files, new state machine, deep custom-loop collision | Mine typed error semantics only |
| `66051ec7d` | Extreme: 39 files, formatter/context/session/state-machine rewrite | Port lifecycle + invariant tests narrowly |
| `bb539f7d6` | High: 17 files, UI/ACP/persistence plus existing fork/pending implementation | Reconcile pending `610e48b2f`, then fill provider and dispatcher gaps |
| `cf312f1d1` | High: 14 files and newer notification/cancellation plumbing | Port headless engine slice with process cleanup tests |
| `f2e6e9ed0` | Medium: overlaps custom recovery introduced after `1b2f77f71` | Manually add owner metadata and pre-inspection canonicalization |
| `5751715df` + `85aac1940` | Low | Manual narrow port preserving fork public helper/deadline/retry code |
| `1f6c7524e`, `f3ab1557c`, `701e93ab4` | Low | Small semantic ports with upstream tests; adapt schema version |
| `3b3719de4` | Do not duplicate | Use the already-authored stronger fork commit `8d2166144` |

## Evidence-gated implementation order

1. Land typed provider terminal propagation and tests. Until this is true, retry/fan/repair
   measurements can contain false-success tasks and poisoned dependency output.
2. Reconcile and complete output-limit handling. Never execute provider-declared truncated tool
   arguments.
3. Port structured context-overflow classification so recovery is invoked on all supported shapes.
4. Implement append-only turn context with sequential request tests, then A/B on one LM Studio slot
   and the real production slot setup. Keep only if request-prefix reuse and TTFT improve without
   quality regression.
5. Wire live shell progress/cancellation and test long active commands against the same swarm
   watchdog that repair uses.
6. Land the small hardening ports and composite index; verify each targeted regression.
7. Add owner-aware pre-inspection tool canonicalization, but do not claim a speed win without a
   current-model reproduction.

This sequence restores truth at the provider boundary first. It adds no fixed model-generation cap,
does not replace the semantic judge with a deterministic verdict, and does not assume that a busy
node is productive. It makes later fan, judge, and repair experiments measurable rather than allowing
transport/provider failures to masquerade as model output.
