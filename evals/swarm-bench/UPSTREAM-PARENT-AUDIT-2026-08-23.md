# Parent Goose audit for the local swarm engine — 2026-08-23

Status: read-only upstream audit. No upstream commit was merged, no source code was
changed, and no model/provider call was made.

Adversarial refresh at 2026-08-23: both official upstream refs still resolve to
`8d844eecbdfd65626a881c9e8784ae8dc6093f1d`; there is no newer parent delta and
parent still has no swarm scheduler, fan, or judge implementation. The two P0
behaviors below are now implemented on `codex/cloud-sb7-harness` by
`d59234f543a367f1eda705e48d745c458a3b950c` and
`8d216614413459007a737e8e6bfaa8c4f31943a9`, but neither is yet an ancestor of
the main `local-edition` branch. Conflict risk is branch-specific:
`1844d3fb` applies mechanically to current `local-edition` but conflicts with
the cloud branch's newer parser hardening. Finally, the archived
`developer.*`-name count is post-dispatch evidence, not a raw-wire negative;
it cannot prove the provider never emitted a mangled name. The exact-frame gate
for that candidate therefore remains mandatory.

This is a focused addendum to `SWARM-ENGINE-AUDIT-2026-08-22.md`. It answers a
narrow question: which changes made in the parent `block/goose` repository after
this fork diverged are useful to the LM Studio / LM Link execution path, and which
changes merely look relevant from their subject lines.

## Reproducible comparison boundary

The comparison was made in the isolated `codex/cloud-publish-pipeline` worktree.

- Parent remote: `upstream = https://github.com/block/goose.git`
- Local audited head: `b211ad8f5bb9cd770714a1337e630a0ea714034b`
- Fetched parent head: `8d844eecbdfd65626a881c9e8784ae8dc6093f1d`
  (`2026-08-21`, `fix(security): escape OAuth callback content (#11479)`)
- Merge base: `a0aed81f36076cfe48def4b21c04d7f0d33072e8`
  (`2026-07-03`, `fix(providers): treat unicode punctuation as image path
  terminators (#10106)`)
- Parent-only commits after the merge base: 508
- Fork-only commits after the merge base: 2,429

Commands used to pin and bound the audit:

```sh
git fetch --prune upstream
git rev-parse HEAD upstream/main
git merge-base HEAD upstream/main
git rev-list --count a0aed81f..upstream/main
git log --no-merges a0aed81f..upstream/main -- \
  crates/goose-provider-types crates/goose-providers \
  crates/goose/src/agents crates/goose/src/session \
  crates/goose/src/providers crates/goose/src/toolkit crates/goose/src/config
git ls-tree -r --name-only upstream/main | \
  rg '(^|/)(swarm|goose-swarm)(/|$)|commands/swarm\.rs'
```

The last command returns no files. Parent Goose has no `goose-swarm` crate and
no `commands/swarm.rs`. Therefore parent history contains no scheduler, fan-out,
fan-in, judge, task-splitting, node-idleness, or straggler policy that can be
ported. All applicable changes below improve one worker's provider/tool/session
path. Any claim that a parent commit directly fixes swarm fanning would be false.

## Decision summary

The parent contains two high-value gaps the earlier audit did not fully surface:

1. The fork's adaptation of `bb539f7d6` is incomplete. It marks truncated text,
   but it can still execute a tool call whose stream ended with
   `finish_reason=length` if the partial arguments happen to be valid JSON.
2. The fork's line-based shell preview can return an unbounded single line. A
   200 KB minified/base64/progress line passes through in full even though the
   complete output was already spilled to disk. `3b3719de4` fixes that class.

Three more narrow behavior ports are worth controlled fixtures: Thinking-block
coalescing in `collect_stream`, structured context-overflow classification, and
pre-first-item retry/cancellation. Five other changes remain conditional because
the archived local evidence does not show their input shape or because their
effect is outside the critical generation path.

No candidate below is a model-generation cap. The only proposed byte bound is
on a tool-output *preview after the complete output has already been saved*.

## Port candidates

### P0 — prevent execution of output-truncated tool calls

Parent commit:
`bb539f7d6f950b3d3b17a540c7dc954f3f03c554` —
`feat: surface output-token-limit info (#10831)`

Parent files/mechanism:

- `crates/goose-provider-types/src/formats/openai.rs`
  - records `finish_reason=length` for streaming and non-streaming responses;
  - turns every length-terminated tool request into `INVALID_PARAMS`, even when
    its accumulated argument string is empty or happens to parse as valid JSON;
  - preserves the response id and emits an empty metadata message when the
    length signal arrives in a terminal chunk with no content.
- `crates/goose-provider-types/src/conversation.rs`
  - folds the output-limit metadata into the matching response by id and retains
    an unmatched marker as user-visible/agent-hidden evidence.
- `crates/goose-provider-types/src/conversation/message.rs`
  - adds the stable `output_token_limit_reached` metadata field.

Fork state and concrete gap:

- Local `349bf453c980afa9c08a86f489ddbb833752cfe4` adapted the commit by
  appending `OUTPUT_TRUNCATION_MARKER` on the text path.
- Current `formats/openai.rs` treats only `finish_reason == "tool_calls"` as an
  immediately complete tool stream, but the inner accumulation loop stops on
  *any* finish reason. It then parses and yields the accumulated call normally.
- A call ending in `length` with `{"path":"valid-so-far"}` can therefore be
  executed. JSON validity cannot prove semantic completeness; the provider has
  explicitly said generation was guillotined.

Applicability: high. A wrong tool execution can corrupt a build, force repair,
or create misleading completion evidence. It is more serious than a malformed
call because a syntactically valid partial call may not fail loudly.

Port shape: selectively port the message metadata, conversation-by-id merge, and
the non-executable rule. Preserve the fork's loud text marker as an additional
model-visible signal. Do not import unrelated ACP/UI presentation hunks.

Conflict risk: high. `formats/openai.rs` also contains the fork's terminal-proof,
usage, reasoning, malformed-tool, and empty-finish adaptations, including recent
`506ae4baa4641c55877d409790a609fda662d44a`. A raw cherry-pick is unsafe.

Required tests:

- streaming, valid-object arguments followed by `finish_reason=length`;
- streaming, empty arguments followed by `length`;
- non-streaming equivalent;
- a length-only terminal chunk retains the response id and exactly one marker;
- normal `tool_calls` remains executable;
- captured LM Studio/LM Link frame replay before activation.

### P0 — byte-bound an already-spilled shell preview

Parent commit:
`3b3719de4e3b253c6d0235ddb09046c63b1dc0ad` —
`fix(developer): byte-bound the shell truncation preview (#10992)`

Parent file/mechanism:

- `crates/goose/src/agents/platform_extensions/developer/shell.rs`
- after saving the complete output, limits the returned tail preview to 10,000
  bytes and advances to a UTF-8 character boundary;
- covers newline-free progress output, minified JSON/JS, and base64 blobs that a
  line-count limit cannot constrain.

Fork state and concrete gap:

- The fork deliberately improved the preview to include both head and tail, with
  `OUTPUT_LIMIT_BYTES = 50_000` and `OUTPUT_PREVIEW_LINES = 50`.
- When a 200 KB output has one line, the output exceeds the byte threshold but
  `total_lines <= OUTPUT_PREVIEW_LINES`, so `lines.join("\n")` returns all 200 KB.
- The complete output was already saved. Sending the same 200 KB back to a local
  model consumes context and invites the observed re-read/re-cat failure pattern
  without adding information.

Applicability: high. This is a direct context, latency, and tool-reliability fix
for local models. It does not delete data or cap a model response.

Port shape: retain the fork's head-and-tail preview, split a byte budget across
both ends, snap each slice to a UTF-8 boundary, and keep the existing elision/path
notice. Do not regress to upstream's tail-only presentation.

Conflict risk: medium because the fork intentionally rewrote this exact preview.

Required tests:

- one 200 KB ASCII line;
- one multi-byte Unicode line;
- many lines whose selected head/tail still exceed the byte budget;
- both ends are present and the full spill file is byte-identical to input.

### P1 — coalesce Thinking deltas in complete-response collection

Parent commit:
`1f6c7524e1ad1b3b46f5653390af4b79614d17d8` —
`fix(providers): coalesce consecutive Thinking blocks in collect_stream (#11317)`

Parent file/mechanism:

- `crates/goose-provider-types/src/base.rs`
- teaches `collect_stream` the same signature-aware Thinking merge used by
  `Conversation::push`;
- only merges single-block deltas, adopts a closing signature, and preserves
  distinct signed blocks and multi-block provider units.

Fork state and concrete gap:

- The fork's `Conversation::push` already coalesces Thinking blocks.
- Its `collect_stream` only coalesces Text. Consumers of `Provider::complete`,
  including auxiliary/compaction paths, can therefore receive one Thinking block
  per token/delta before the message ever reaches `Conversation::push`.

Applicability: medium-high for thinking-heavy Qwen. This reduces message/block
count, serialization work, and retained context representation. It does not
reduce tokens generated by the model and must not be sold as a cure for a
191,000-character loop.

Port shape: narrow `collect_stream` hunk and the upstream signature/boundary
tests. Preserve the fork's usage-only behavior and provider metadata additions.

Conflict risk: medium; `base.rs` has substantial fork-only model/provider work.

### P1 — recognize structured local context overflow

Parent commits:

- `85aac194044aadbb58cfb62b1b927e919be89652` — structured error code and
  `n_prompt_tokens > n_ctx` detection;
- it subsumes the message-phrase additions in
  `5751715df` — byte-size request-limit classification.

Parent file/mechanism:

- `crates/goose-providers/src/http_status.rs`
- detects nested `error.code = context_length_exceeded` case-insensitively;
- detects numeric `error.n_prompt_tokens > error.n_ctx` with a positive context;
- keeps incomplete/top-level/equal-count shapes classified as ordinary bad
  requests;
- recognizes common byte-limit phrases without confusing output-token limits,
  tool-description limits, or quota errors.

Fork state and concrete gap:

- The fork has a broad message-text classifier but does not inspect structured
  nested fields.
- A terse llama.cpp/LM Studio-compatible 400 such as
  `{"error":{"code":400,"n_prompt_tokens":49202,"n_ctx":49152}}` is therefore
  a generic `RequestFailed`; it cannot enter the existing context-compaction path.

Applicability: medium-high, conditional on a captured local payload. The shape is
typical of llama.cpp-family servers, but the archive has not yet supplied a
positive LM Studio frame. Enable only after replaying a real response or a
contract fixture from the exact local endpoint.

Port shape: classifier and tests only. Preserve the fork's URL redaction,
Retry-After handling, terminal-safety policy, and error vocabulary.

Conflict risk: medium in a locally hardened `http_status.rs`.

### P1/P2 — split the first-item retry and cancellation behaviors

Parent commit:
`b7ddf933c429c2553713dc6d5e0347c1cec43872` —
`fix(provider): retry transient errors on first stream item before ending turn (#10968)`

Parent files/mechanism:

- `crates/goose/src/agents/reply_parts.rs`
  - retries a transient error only before the first yielded stream item;
  - reconstructs the same request under the session id;
  - never replays after an item has been admitted;
  - skips provider-managed-context sessions.
- `crates/goose/src/agents/agent.rs`
  - replaces cancellation polling between items with `tokio::select!`, so a
    permanently pending stream can be cancelled immediately.

Applicability:

- Cancellation select: high correctness value for local stuck streams, but it
  must be reconciled with the fork's request lifecycle/physical-terminal proof.
- First-item retry: medium for local interactive use. In the cloud campaign,
  `GOOSE_PROVIDER_TERMINAL_SAFE_RETRIES=true` makes `should_retry` false and must
  continue to prohibit replay of paid/ambiguous requests.

Port shape: two independent changes. First port cancellation with lifecycle
tests. Then consider pre-first-item retry only where a request is proven not to
have been admitted, or under the existing non-terminal-safe local policy. Do not
infer from “no decoded item” that the remote model did no work.

Conflict risk: high. `agent.rs` and `reply_parts.rs` are among the fork's most
divergent files and contain swarm-specific event, repair, judge, and terminal
safety behavior. Never raw-cherry-pick this commit.

Required tests:

- cancellation while the first item is permanently pending;
- no retry after any item, including usage-only/metadata evidence;
- terminal-safe mode performs one physical POST;
- local retry only for the configured transient classes;
- lifecycle log proves request-start/request-terminal ordering.

## Conditional candidates — keep fixture-gated

### Metadata-only SSE frames

`1844d3fb4aed0ec7f2e3806829cb887981f15ead` changes
`crates/goose-provider-types/src/formats/openai.rs` so a JSON object with no
`choices` key can be skipped as gateway metadata while `choices: []` remains a
usage frame. It also surfaces choice-less in-stream error shapes and avoids
ending a tool-argument stream when metadata appears between argument deltas.

The fork accepts `choices: []` already, but a frame with the key absent still
fails deserialization. This is useful for Portkey/Azure-like gateways. No such
frame was found in the archived direct-LM-Studio evidence, so this is conditional
for LM Studio and more immediately relevant to cloud-compatible gateways. Port
only with an exact captured frame. Conflict risk is high in the same OpenAI
stream parser as the P0 truncation fix.

### Bound an unterminated possible `<think>` tag

`f3ab1557c299e1c4d4fbd1b6a55cfd9ad3b6207e` changes
`crates/goose-provider-types/src/thinking.rs`. It flushes a malformed partial
candidate after 8 KiB instead of buffering forever and releases retained
capacity. It does not stop generation. This could keep malformed Qwen output
visible and memory-bounded, but it needs a captured malformed stream because
normal long reasoning inside a valid `<think>...</think>` pair is a different
case. Conflict risk is low.

### Canonicalize mangled tool names before inspection

`f2e6e9ed05ec22508f13403a52f654c11e395cfd` changes
`crates/goose/src/agents/extension_manager.rs` and `reply_parts.rs`. It resolves
recoverable names before permission/hooks inspect them, including owner-aware
forms such as `developer.shell`.

The fork already has late dispatch recovery from local
`99360a89ba269231a5690ea7d07bf7b5e6044e69`, but not all of the earlier
inspection canonicalization. The reproducible archive search below found zero
mangled `developer.*` names and 3,670 ordinary short `shell/write/edit/tree`
call names, so this is robustness/security hardening rather than a measured
local bottleneck:

```sh
rg -o 'developer\.(shell|write|edit|tree)' \
  /Users/mihaiperdum/goose-builds/*/.swarm/activity | wc -l
rg -o '"name"\s*:\s*"(shell|write|edit|tree)"' \
  /Users/mihaiperdum/goose-builds/*/.swarm/activity | wc -l
```

Port only after a captured failure. Conflict risk is high in the extension and
reply pipeline.

### Session message composite index

`701e93ab41ca993ee2e575bf36911832153ab115` adds a SQLite index on
`(session_id, created_timestamp, id)` in
`crates/goose/src/session/session_manager.rs`, preventing a temporary B-tree on
ordered session loads. The fork is at schema version 14 and lacks this index.
It can improve large-session resume and forensic reads, but workers primarily
append to in-memory conversations during live generation, so it is not a
scheduler or decoder-speed fix. Treat it as a separately measured session-store
change. Conflict risk is medium-high because schema migrations have diverged.

### Refine request deadlines only if a test exposes the residual gap

`7e431ac6f804fdc5a6fb9262fa2ca5b8b0fd6ce6` distinguishes streaming and
non-streaming requests: it bounds connect/headers/error bodies, uses inactivity
for a streaming body, and retains a total deadline for non-streaming responses.

The primary fix is already adapted by local
`a556679e9522823f3aa10774e34cd0f0623da424`: streaming uses inactivity timeout,
with `GOOSE_PROVIDER_READ_TIMEOUT_SECS` and an opt-in
`GOOSE_PROVIDER_TOTAL_TIMEOUT`. The residual difference is that a drip-fed
non-streaming body can exceed a total duration in the fork. That has not appeared
in local evidence. Do not replace the local policy without a failing test;
conflict risk is high in `api_client.rs` and its call sites.

### Request lifecycle vocabulary, not the OTEL implementation

`f45ccd46f700ef6e0c5143af10598277a390291d` adds request parameters,
response metadata, tool-call parity, and agent ids to OpenTelemetry. Raw OTEL
does not supervise or schedule a headless swarm, and the benchmark telemetry
filter does not make those spans the control plane. The useful part is its stable
request-id/timing vocabulary. The fork's provider lifecycle file and cloud
manager now emit actionable lifecycle evidence, so no broad OTEL port is
warranted.

## Already present or deliberately adapted

Do not re-port these parent changes:

- `3c1fdd692`: empty provider-turn retry exists in the fork; terminal-safe mode
  deliberately ends instead of replaying a paid ambiguous call.
- `824b167af`: empty-string `finish_reason` handling is local
  `349415eb399dd01bc3b78ded68268b350e84fd88`.
- `1b2f77f71`: malformed GLM/Minimax tool recovery is local
  `99360a89ba269231a5690ea7d07bf7b5e6044e69`.
- `affd1cea1`: OpenAI-compatible prefix isolation is local
  `5abfbfe03c7e6402fd3e810dc173a85e17eec1f2`.
- `7e431ac6f`: the important streaming inactivity behavior is local
  `a556679e9522823f3aa10774e34cd0f0623da424`; only the conditional refinement
  described above remains.
- `ee61c7c49`: incremental CLI rendering is local
  `885e5d05a0555efa94f0aa3b8f8414136dcb5eb2`.
- `d5a8a3fb9`: atomic inventory migration is local
  `8ed0d99d778b36f6f82abcda7b79f52583807d60`.
- `8f1590b75`: silent extension-skip fix is local
  `052005908e6d78a1c47136606af75705e8622dd0`.
- `d5785a367`: configured session manager for summaries is local
  `04b5ae430905658eed91ffbd41e77390f7d26f0b`.
- `bf332b983`: exact extension-owner matching is local
  `eb0671f6c1a3840b6752f182ef06be959b108358`.
- `dc7798483`: critical-command normalization is local
  `5fec0f3d24b4d2b31ae5facef4630d4388b476b4`.
- `97f150db6`: developer-shell default timeout behavior is present. The swarm
  intentionally overrides `GOOSE_DEFAULT_EXTENSION_TIMEOUT=1800` for legitimate
  long builds; importing another fixed timeout would reintroduce false kills.
- `1e03bbb56`: reasoning preservation has a fork-specific implementation and is
  a semantic conflict, not a missing cherry-pick.
- `bb539f7d6`: only the text-marker slice is present. It remains listed as P0
  because the tool-call/metadata safety behavior is missing.

## Explicit rejections

These are not recommendations for the local swarm even though keywords such as
“agent”, “context”, “stream”, “retry”, or a provider name make them appear in a
mechanical search.

### No parent scheduler or fan-out work exists

Parent has no swarm implementation. Commits in `summon`, subrecipes, ACP
delegation, or the unrolled single-agent loop do not operate the custom
`crates/goose-swarm` DAG/scheduler. In particular reject raw adoption of:

- `23c2e2824`, `935a37a68`, `3e15ccb88`, `9fec4152a`: upstream Summon/subagent
  routing or notifications;
- `ca52cce62`, `d2389ef4a`, `5f4b7cc10`: the unrolled single-agent loop refactor.

The latter is a broad architectural rewrite with extreme conflict and no
mechanism for task fanning, node utilization, judge work stealing, or straggler
handling.

### Do not replace measured local compaction policy

- Reject raw `33fb29402` (main-session-model compaction). The fork deliberately
  routes compaction to `GOOSE_FAST_MODEL`, uses a think-suppressed prefill in
  `9ba20a51141a61b54a0cc54c5fd74d5d50334287`, and preserves verbatim tail turns
  via `467da87ee54fd3cfd5719527070db1d6729b51b3` and
  `f21642b806e4081d5ff0d251819597893ed774a5`. Parent behavior would overwrite a
  measured local design and trade latency for unknown quality.
- Reject raw `ad87dd4c3` structured compaction for the same reason; it conflicts
  with the fork's K4 keep-tail invariant.
- Reject broad cache/context restructuring `66051ec7d` and `dafdbb736`; the
  applicable prefix issue was already adapted narrowly in `5abfbfe03`.

Only a role-specific A/B run can change the local compaction decision.

### Tool schema changes are empirically inert for this benchmark

- `950575bcd` compact toolshim schemas;
- `ca6ba6c44` const-union normalization;
- `36cb569e3` oneOf-to-anyOf rewrite;
- `a3c20531e` overlong function-name handling.

The measured SB7 request has four fixed tools, about 2,064 schema characters,
and no `oneOf`, `anyOf`, `$defs`, or `$ref` target shapes. Archived calls use
short tool names. These commits solve real general-Goose problems but do not
explain local node time or benchmark quality.

### Wrong provider or wire path

- `f3c3563c6`: DeepSeek/Alibaba OpenAI **Responses** API fixes; the swarm and LM
  Studio use the OpenAI-compatible chat path.
- `cdc150f87`: Z.AI Anthropic-only `clear_thinking` behavior.
- `f47a9620d`: stale signed-Thinking cleanup for Anthropic, Bedrock, and
  Databricks after a mid-session model switch; it does not touch the selected
  local OpenAI-compatible formatter.
- `12f80edee`: Ollama Cloud provider.
- `1bced6616` and later SafeMLX/local-inference work: the built-in MLX provider,
  not LM Studio/LM Link.
- `31d3ff2bd`: cloud prompt-cache write pricing. LM Studio does not use those
  explicit billing/cache-control semantics, and forcing the policy can reduce
  local prefix reuse.
- Bedrock, OpenRouter catalog/pricing, OAuth, Foundry, Google, Copilot, Codex,
  declarative-provider inventory, and model-picker commits do not execute in the
  selected local OpenAI-compatible path.

### Observability/UI and inactive control planes

- `cf312f1d1` streams live shell output to CLI/ACP/UI notification consumers. It
  does not expose partial shell output to the model, split a task, free a node,
  or change tool completion. The benchmark now has its own live harness log; do
  not import a 1,000-line UI/event feature as a scheduler fix.
- ACP, hooks, permissions UI, Code Mode, Apps, recipe UI, cost display, model
  picker, onboarding, auth, and desktop changes are outside the headless swarm
  control path. Hooks are not the swarm judge and ACP is not its scheduler.
- `bc6804922` uses Linux `PR_SET_PDEATHSIG` to keep stdio extensions alive. The
  target M4/M3 nodes are macOS, so the mechanism has no effect there.

### Security-only changes are valuable but not performance evidence

Changes such as `e7c33077c` (secure spill files), Unicode prompt sanitization,
OAuth hardening, socket rejection, and bounded diagnostics should remain in the
normal security-upstream review. They do not reduce generation time, improve
fanning, or cure local model loops and should not be mixed into a performance
arm where attribution matters.

### Structurally absent mechanisms

`8343c4a16` decouples source-file and tool-response limits through supporting-file
machinery that this fork does not have in the same form. The fork is already
structurally decoupled on this path. Importing it would drag newer skill/security
machinery without changing the measured four-tool request.

## Safe adoption order and evidence gates

No group should be bulk-cherry-picked. Each behavior should be implemented as a
fork-native patch, committed separately, and measured as one lever:

1. Length-terminated tool calls become non-executable, while retaining the
   existing loud marker. Gate on streaming/non-streaming fixtures and physical
   request lifecycle proof.
2. Byte-bound the shell preview while preserving both ends and the complete
   spill file. Gate on exact byte/Unicode tests.
3. Coalesce Thinking in `collect_stream`. Gate on signature and multi-block
   boundary tests, then measure serialized block count and wall time separately.
4. Add structured context-overflow classification only after capturing the exact
   LM Studio/LM Link 400 shape. Gate on positive and false-positive fixtures.
5. Add cancellation select independently. Gate on a permanently pending stream
   and lifecycle terminal evidence.
6. Consider first-item retry for non-terminal-safe local lanes only. Prove no
   retry after any admitted item; keep cloud terminal-safe mode at one physical
   POST.
7. Promote any conditional candidate only when an archived frame/session proves
   its shape occurs. Absence of evidence is not a reason to turn on a generic
   compatibility patch in the critical path.

The first two items close static correctness holes and should be validated before
the next benchmark arm. The remaining items need one-change A/B attribution;
none substitutes for work in the fork-only swarm scheduler, task decomposition,
judge supervision, or straggler recovery.
