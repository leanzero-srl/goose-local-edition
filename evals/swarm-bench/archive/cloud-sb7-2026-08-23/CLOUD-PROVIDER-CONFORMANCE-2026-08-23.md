# Cloud provider conformance ledger — 2026-08-23

Status: offline adapter conformance complete; paid/live entitlement verification not performed. This ledger covers only `glm-5.3`, `gemini-3.7-flash`, `gemini-3.1-pro-preview`, `deepseek-v4-flash`, and `deepseek-v4-pro`. It does not alter SB7, its scorer, benchmark outputs, or campaign state.

## Pinned contracts

| Campaign model | Native provider | Canonical context | Canonical max output | Request contract |
| --- | --- | ---: | ---: | --- |
| `gemini-3.7-flash` | `google` | 1,048,576 | 65,536 | Native Gemini request; medium thinking by default; `includeThoughts=true`; sampling fields omitted. |
| `gemini-3.1-pro-preview` | `google` | 1,048,576 | 65,536 | Native Gemini request; high thinking by default; `includeThoughts=true`. |
| `deepseek-v4-flash` | `custom_deepseek` | 1,000,000 | 384,000 | Native `thinking.type=enabled`; campaign must explicitly request `reasoning_effort=max`; omit `temperature`, `top_p`, penalties, and `tool_choice` while thinking. |
| `deepseek-v4-pro` | `custom_deepseek` | 1,000,000 | 384,000 | Same as Flash. |
| `glm-5.3` | `zai_api` at `https://api.z.ai/api/paas/v4` | 1,000,000 operational admission value | 131,072 | Native `thinking.type=enabled`; default/max maps to `reasoning_effort=max`; preserved history sets `clear_thinking=false`. |

`zai_api` is deliberately separate from the existing Anthropic-shaped `zai` integration. The campaign must not silently substitute `zai`, another router, or a legacy GLM model.

## Response and accounting evidence

- Google native tool calls preserve the server `functionCall.id`, exact function name, and the signature only on the part that carried it. Matching `functionResponse.id` and `functionResponse.name` are replayed without copying a thought signature onto tool responses.
- The Google fixture reports `promptTokenCount=100`, `candidatesTokenCount=7`, `thoughtsTokenCount=13`, and `totalTokenCount=120`. Goose records input 100, output 20, total 120. Thinking is added to candidate output once and the provider total is not recomputed.
- The DeepSeek fixture reports `completion_tokens=50` and `completion_tokens_details.reasoning_tokens=30`. Goose records output 50, not 80. The detail is a breakdown of the completion total, not an additional billable field.
- The DeepSeek fixture rebuilds one assistant turn containing its exact `reasoning_content`, two parallel tool calls, and both paired tool responses. This satisfies DeepSeek's requirement to replay reasoning history on every subsequent request carrying tools.
- The Z.AI fixture reports `completion_tokens=60`; Goose records output 60 exactly and preserves the response-reported model sentinel. Z.AI defines `completion_tokens` as output tokens and prices output tokens, so no independent reasoning-token addition is made.
- Response-reported model strings are retained verbatim in `ProviderUsage`. The DeepSeek/Z.AI values ending in `fixture-revision` are deliberate offline sentinels, not claims about a live vendor revision.

## Terminal evidence

- DeepSeek `insufficient_system_resource` becomes a typed server error.
- Z.AI `sensitive`, `model_context_window_exceeded`, and `network_error` become refusal, context-length, and network errors respectively.
- OpenAI-compatible `content_filter` becomes a refusal.
- `length` remains a completed but explicitly truncated response. The truncation marker is emitted even when the provider puts `length` on an empty terminal delta.
- Each typed error includes the response-reported model string so campaign logs retain provider evidence.

## Primary sources

- Google model contracts: [Gemini 3.7 Flash](https://ai.google.dev/gemini-api/docs/models/gemini-3.7-flash), [latest Gemini models](https://ai.google.dev/gemini-api/docs/latest-model), [function calling](https://ai.google.dev/gemini-api/docs/function-calling), and [thought signatures](https://ai.google.dev/gemini-api/docs/thought-signatures).
- DeepSeek contracts: [chat completion schema](https://api-docs.deepseek.com/api/create-chat-completion/), [thinking mode and reasoning replay](https://api-docs.deepseek.com/guides/thinking_mode/), and [pricing/model limits](https://api-docs.deepseek.com/quick_start/pricing/).
- Z.AI contracts: [native chat completion schema](https://docs.z.ai/api-reference/llm/chat-completion), [streaming usage](https://docs.z.ai/guides/capabilities/streaming), [pricing](https://docs.z.ai/guides/overview/pricing), and [GLM-5.3 release/effort guidance](https://z.ai/blog/glm-5.3).

## Known blockers before paid admission

1. Z.AI's official sources publish the GLM-5.3 context as `1M`, but do not expand it to an exact integer. Neither 1,000,000 nor 1,048,576 is vendor-proven. Goose uses 1,000,000 as a conservative operational admission value; the campaign must not describe it as an exact vendor limit. Resolve this from an official model descriptor or an approved live metadata probe before claiming exactness.
2. Offline fixtures prove serialization, replay, classification, and accounting logic. They do not prove that the supplied accounts are entitled to the exact model IDs, what live `model` revision each provider reports, or how a provider settles a real invoice. The campaign preflight must capture these facts before admitting paid benchmark work.
3. DeepSeek's generic provider default remains the vendor-native `high`. The campaign must explicitly set `max`; relying on tool auto-detection is not conformant.

## Implementation commits

- `0b50ee2b7` — native DeepSeek V4 and Z.AI provider request profiles.
- `4c158b4f5` — campaign model context/output metadata and regression table.
- `14e684483` — Google tool identity, thought-signature, thinking, sampling, and usage contracts.
- `e002de712` — DeepSeek/Z.AI reasoning replay, usage fixtures, and typed terminal evidence.

Offline gates at this point: `cargo test -p goose-provider-types` (378 passed) and `cargo test -p goose-providers` (88 passed). No credentials were read and no paid calls were made.
