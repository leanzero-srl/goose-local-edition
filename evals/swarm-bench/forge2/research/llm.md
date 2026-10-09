# Forge 2.0 research: Forge LLM (`@forge/llm`) and Rovo, as of 2026-10-09

All pages fetched 2026-10-09 (EEST) with curl and converted to text locally, so every quote below is
copied from the page text rather than paraphrased by a summariser. Local copies:
`research/pages/*.txt` (docs, blog, terms), `research/cdac/<topic>.txt` (developer community threads,
via the Discourse JSON API), `research/pkgs/` (npm tarballs, Bitbucket example). Wayback snapshots are
used only to date changes. Nothing below is from memory. "Staff" means the poster carries the
"Atlassian Staff" title in the Discourse JSON.

---------------------------------------------------------------------------------------------------

## 0. Timeline (what changed, and when)

| date | event | source |
|---|---|---|
| 2025-10-29 | RFC-117 (Forge LLMs) published | https://community.developer.atlassian.com/t/rfc-117-forge-llms/96506/1 |
| 2026-01-23 (page date) | limits page: EAP, "Context window size in tokens | 200000", RPM 100, inference 5 min, NO TPM row | Wayback 20260512045550 of /limits-llm/ |
| 2026-06-02 | Preview: "the Forge LLMs API is now available in Preview and can be enabled on production environments today" (Adam Moore, staff). Billing from June 1, 2026 | https://community.developer.atlassian.com/t/now-in-preview-build-ai-powered-forge-apps-with-atlassian-hosted-llms/101045/1 |
| 2026-06-17 (page date) | models page (Preview): 4 models (haiku-4-5, sonnet-4-5, sonnet-4-6, opus-4-6), no retirement dates | Wayback 20260624152952 of /forge-llms-models/ |
| 2026-07-29 | GA blog: "the Forge LLMs API is now generally available for all developers"; "With GA, we've upgraded support to Sonnet 5, and Opus 4.7 and 4.8 are now available." | https://www.atlassian.com/blog/development/forge-llms-api-is-now-ga-heres-how-you-can-build-ai-native-apps-on-atlassian |
| 2026-08-03 (page date) | models page lists 8 models incl. `claude-opus-5`, with tentative retirement dates; the API reference (same page date) carries the sampling "Validation rules" (no earlier snapshot of the reference exists on Wayback, so when the rules first appeared is unverified) | /forge-llms-models/, /forge-llms-api-reference/ |
| 2026-08-05 / 08-10 | batch processing (FRGE-2232) and structured outputs (FRGE-2237) logged as suggestions, "not something that we have planned yet" | cdac 101988/5, 102024/5 |
| 2026-08-09 | `rovo:mcp` page still "Rovo MCP (EAP)", "Last updated Jul 24, 2026" | Wayback 20260809102404 of /rovo-mcp/ |
| by 2026-08-20 | `rovo:mcp` in Preview scoped to Rovo Studio custom agents (partner: "now that `rovo:mcp` is in Preview"; staff 09-01: "The Preview release scoped `rovo:mcp` to custom Rovo agents.") | cdac 100683/24, 100683/25 |
| 2026-09-28 | `rovo:agentConnector` GA (A2A 1.0 only) | /platform/forge/changelog/ |
| 2026-09-30 (page date) | limits page: per-tier context windows; TPM 500,000 row | /limits-llm/ |
| 2026-10-01 | changelog: `rovo:mcp` tools reachable from third-party MCP clients, "now available in Preview" (CHANGE-3495) | /platform/forge/changelog/ |
| 2026-10-02 | changelog: Forge LLM TPM 50,000 -> 500,000; `rovo:skill` EAP -> Preview | /platform/forge/changelog/ |
| 2026-10-09 | `@forge/llm` latest = 1.0.7 (2026-09-28); 1.0.8-next.1 and the 2026-10-07 experimental build change only the `@forge/api` dependency | `npm view @forge/llm`, tarball diffs |

---------------------------------------------------------------------------------------------------

## 1. The current API (`@forge/llm` 1.0.7 + docs)

### 1.1 Methods
API reference (https://developer.atlassian.com/platform/forge/runtime-reference/forge-llms-api-reference/, "Last updated Aug 3, 2026"):
> "list() => Promise<ModelListResponse>
> chat(Prompt) => Promise<LlmResponse>
> stream(Prompt) => Promise<StreamResponse>"

> "The @forge/llm SDK gives you a lightweight, purpose-built client for invoking Atlassian-hosted LLMs directly from Forge runtime functions."

> "Use `chat()` for structured multi-turn exchanges. Use `stream()` to incrementally receive LLM responses as smaller chunks. Provide `tool` definitions so the model can call typed functions, and inspect returned usage to guide adaptive behaviour."

Backend only: Forge Remote is not supported. Adam Moore (staff, 2026-06-16, cdac 101251/3): "Forge Remote support for Forge LLMs is not something we're really considered so far". Containers: Adam Moore (staff, 2026-01-17, cdac 98220/9): "When Forge Containers are available they will also support Forge LLMs".

### 1.2 Request shape (docs = SDK `out/interfaces/internal.d.ts`, verified identical)
```
type Prompt = LlmRequest & { model: string; };
interface LlmRequest {
  messages: Message[];
  temperature?: number;
  max_completion_tokens?: number;
  top_p?: number;
  tools?: Tool[];
  tool_choice?: ToolChoice;
}
type Message = SystemMessage | UserMessage | AssistantMessage | ToolMessage;
interface ToolMessage { content: Content; role: "tool"; tool_call_id?: string; name?: string; }
interface AssistantMessage { content: Content; role: "assistant"; tool_calls?: ToolCall[]; }
interface ToolCall { id: string; type: "function"; index: number; function: { name: string; arguments: object; }; }
type ToolChoice = | "auto" | "none" | "required" | { type: "function"; function: { name: string; }; };
interface Tool { type: "function"; function: { name: string; description: string; parameters: Record<string, unknown>; }; }
type Content = string | ContentPart[];
type ContentPart = TextPart;
interface TextPart { type: "text"; text: string; }
```
NOT in the request type (1.0.7 and 1.0.8-next.1): `response_format`/`json_schema`, `strict` on tools,
`stop`/stop sequences, `cache_control`, `metadata`, images. The SDK spreads `...request` into the POST
body (`out/llm-api.js`), so an extra field WOULD be sent; whether the service accepts or rejects it is
undocumented (unverified).

Structured output is not supported; the documented route to machine-readable output is a forced tool
call (`tool_choice` = `"required"` or `{type:'function', function:{name}}`). Staff (Shobhit Sharma,
2026-08-10, cdac 102024/5): "I have added this as a suggestion under [FRGE here ](https://ecosystem.atlassian.net/browse/FRGE-2237)for now."
Tool invocation is the app's job. Adam Moore (staff, RFC-117 #26): "The function of tools in this sense is really about tool selection, it doesn't do the tool invocation (that's up to your app)."

Max tokens: the field is `max_completion_tokens` (not `max_tokens`), optional. Agentic tutorial:
"max_completion_tokens: Limits the maximum number of tokens in the generated response." The default
when omitted is NOT documented (unverified). Output ceiling per tier is on the limits page (§3).

Text only. Models page: "Only text input/output is currently supported; multimodal support may be considered later."

### 1.3 Response shape
```
interface LlmResponse { choices: Choice[]; usage?: Usage; }
interface Choice { finish_reason: string; index?: number; message: AssistantMessage; }
interface Usage { input_tokens?: number; output_tokens?: number; total_tokens?: number; }
interface StreamResponse extends AsyncIterable<LlmResponse> { close(): Promise<void> | undefined; }
interface ModelListResponse { models: { model: string, status: "active" | "deprecated", }[]; }
```
`finish_reason` uses Anthropic stop-reason names. Agentic tutorial
(https://developer.atlassian.com/platform/forge/create-an-agentic-llm-webtrigger-app/, Jul 23, 2026):
`"finish_reason": string, // "tool_use", "end_turn", "max_tokens", etc.` and the tutorial code checks
`toolUseAssistantChoice.finish_reason !== 'tool_use'`.
The package README's chat example returns `"finish_reason": "tool_use"` with `content` as a TextPart array
AND `tool_calls`, and `"arguments": { "location": "San Francisco, CA", "unit": "celsius" }` (an object).
`refusal` as a Forge `finish_reason` is NOT documented by Atlassian. On Anthropic's own API
(https://platform.claude.com/docs/en/build-with-claude/handling-stop-reasons): "`refusal` | Claude declined to respond." and
"Safety classifiers return this stop reason as a normal HTTP 200 response, not an error." Pass-through on
Forge is plausible (Forge already passes `end_turn`/`tool_use`/`max_tokens`) but unverified.
The same Anthropic page: "Sometimes Claude returns an empty response (exactly 2–3 tokens with no content) with `stop_reason: "end_turn"`."

Usage: overview page: "The Forge LLM API reports usage data per request (the number of input and output tokens consumed) in the API response." Note `usage` and each field are optional in the types.

### 1.4 Tools / agent loop (documented)
Agentic tutorial: the follow-up prompt "must include three types of messages:
 * 1. The original user and/or system messages from the initial prompt.
 * 2. The assistant's message indicating a tool should be used (`tool_use`).
 * 3. A tool message containing the function's result and context for the assistant."
The tutorial only handles `tool_calls?.[0]` and only one round: "(Optional) Repeat as needed: In more
complex agentic flows, the LLM may call multiple tools before producing a final response." Parallel
tool calls (several entries in `tool_calls`) are allowed by the types (`index`) but not exercised by
any Atlassian sample (gap: a contract must state how they are to be answered).
The tutorial parses arguments defensively: `typeof toolCall.function.arguments === 'string' ? JSON.parse(...) : ...`.

### 1.5 Streaming
Package `out/streaming/llm-stream-parser.js`: newline-delimited JSON, each line one `LlmResponse` chunk;
partial lines kept as fragments; a last line without newline is flushed. Errors page
(https://developer.atlassian.com/platform/forge/runtime-reference/forge-llms-api-errors/, Jul 23, 2026):
> "Platform interruptions can cause streaming to conclude before a complete response is delivered. In other words, not only can the LLM's text output cut off prematurely, but the client can also fail to receive finalising streaming messages. One way to detect incomplete responses, and therefore attempt a retry, is to check whether a completion choice object with a `finish_reason` property is missing when the stream ends"
> "// Exceptions are not thrown for finishing streams with incomplete responses."
> "Instead of resubmitting the original prompt, a better way to recover is to prompt the LLM with prior context."
Recovery prompt given: "You were interrupted in your previous attempt. Your original instruction was "${originalUserPrompt}". Continue from the following interrupted output: ${storedOutput}"
How tool-call arguments are split across stream chunks is NOT documented (unverified).

### 1.6 `list()`
SDK: GET `https://llm` via `global.__forge_fetch__({ type: 'llm' }, ...)` (`out/llm-client.js`); chat/stream
POST `https://llm/<encodeURIComponent(model)>` with body `{...request, stream: false|true}`.
Models page: "You can use the `list` method from the @forge/llm SDK to dynamically fetch the list of supported models and their current status."
`ModelDetails` = `{ model, status: 'active' | 'deprecated' }` only: no tier, no context window, no
retirement date. A partner asked for tier in `list()` (cdac 101045/7, 2026-06-11); no staff answer.
The models table writes status as `ACTIVE` (upper case); the SDK type is lower case `'active'`.

### 1.7 Validation
Server-side rules (API reference "Validation rules"):
> "`temperature` and `top_p` cannot be specified together. Provide only one, not both."
> "The following models do not support the `temperature` and `top_p` sampling parameters. Omit both parameters from requests to these models:" `claude-opus-4-7`, `claude-opus-4-8`, `claude-opus-5`, `claude-sonnet-5`.
The HTTP status / `code` of these rejections is NOT documented.
Client-side (SDK `out/validators.js` + `out/text.js`, thrown as `PromptValidationError` before any network call):
`Model is required.`; `Invalid temperature ${temp}: Temperature must be between 0 and 1.`;
`Invalid top_p ${topP}: top_p must be between 0 and 1.`;
`Invalid max_completion_tokens ...: max_completion_tokens must be a positive integer.`;
`No messages were provided. Provide at least one message.`; role must be one of `system, user, assistant, tool`.
SDK bug: `isValidChatContent` is `typeof msg.content !== undefined` (always true), so the "must be a
non-empty string" message is never produced client-side.

TRAP: the package README examples send `temperature: 0.7, max_completion_tokens: 1000, top_p: 0.9` together
and use model `claude-sonnet-4-20250514` (not on today's list). Atlassian's own example app
(bitbucket.org/atlassian/forge-llm-examples, confluence-ai-assistant, summaryInteractor.ts) sends
`model: 'claude-3-7-sonnet-20250219', temperature: 0.7, max_completion_tokens: 100, top_p: 1` and
`createJiraStoryInteractor.ts` imports `Tool` from `@forge/llm/out/interfaces/llm-api`, a path that does
not exist in 1.0.7. Models trained on samples will copy these.

---------------------------------------------------------------------------------------------------

## 2. Manifest `llm` module
https://developer.atlassian.com/platform/forge/manifest-reference/modules/llm/ (Jul 23, 2026):
> "Use the `llm` module to enable Forge Large Language Model (LLM) capabilities in your app. You can only define this module once per app."
> "Adding the `llm` module to your manifest will trigger a major version upgrade."
Properties: `key` (Regex: `^[a-zA-Z0-9_-]+$`), `model` `string[]`: "List of LLM model families to enable. The value must be an array. Currently only `claude` family (Anthropic Claude) is supported."
```
modules:
  llm:
    - key: main-llm
      model:
        - claude
```
API reference: "If the SDK is used without declaring this module, linting will fail with an error like: Error: LLM package is used but 'llm' module is not defined in the manifest"
CONTRADICTION: `@forge/lint` 6.3.0 (latest, 2026-10-09) `LlmVerifier.getLintClass()` returns
`LintClass.Warning`, and its auto-fix writes `llm: [{ key: 'llm', model: ['claude'] }]`
(https://unpkg.com/@forge/lint@6.3.0/out/lint/linters/llm-module-linter/llm-verifier.js).
Runtime refusal without the module/approved version: `FORGE_LLMS_MODEL_FORBIDDEN` 403 (§5).
Overview: "Adding Forge LLMs—or a new model family—to an existing app triggers a major version upgrade requiring admin approval."
Rolling releases: Michael Cooper (staff, 2026-06-05, cdac 101045/4): "The current iteration of Rolling Releases does not support Forge LLMs, so will currently block the upgrade."

---------------------------------------------------------------------------------------------------

## 3. Models, limits, timeouts

### 3.1 Models (https://developer.atlassian.com/platform/forge/runtime-reference/forge-llms-models/, Aug 3, 2026)
| Model ID | Tier | Status | Tentative retirement date |
|---|---|---|---|
| claude-haiku-4-5-20251001 | Haiku | ACTIVE | Not sooner than October 15, 2026 |
| claude-sonnet-4-5-20250929 | Sonnet | ACTIVE | Not sooner than September 29, 2026 |
| claude-sonnet-4-6 | Sonnet | ACTIVE | Not sooner than February 17, 2027 |
| claude-sonnet-5 | Sonnet | ACTIVE | Not sooner than June 30, 2027 |
| claude-opus-4-6 | Opus | ACTIVE | Not sooner than February 5, 2027 |
| claude-opus-4-7 | Opus | ACTIVE | Not sooner than April 16, 2027 |
| claude-opus-4-8 | Opus | ACTIVE | Not sooner than May 28, 2027 |
| claude-opus-5 | Opus | ACTIVE | Not sooner than July 24, 2027 |
> "Check model `status` and tentative retirement dates regularly, and update your app before a model reaches end-of-life."
> "Where possible, Forge provides at least six months' notice for model deprecations."
Note: sonnet-4-5's earliest retirement date has passed (today 2026-10-09) and the only Haiku model's is
6 days away; both still ACTIVE on the page. A realistic emulator may mark one `deprecated`.
Tier blurbs: Opus "Most capable ... Slowest ... Highest cost"; Sonnet "Balanced capability / Moderate speed / Moderate cost"; Haiku "Fast and efficient (best for lightweight or high‑volume tasks) / Lowest cost".

### 3.2 LLM limits (https://developer.atlassian.com/platform/forge/limits-llm/, Sep 30, 2026)
> "The following limits apply for each installation of your app when using the Forge LLMs API:"
> "Requests per minute | 100 | The number of prompts sent to any model in any given minute."
> "Tokens per minute | 500,000 | The maximum number of tokens that a single model can process each minute."
> "Inference time in minutes | 5 | The maximum time a model can process and generate responses before a timeout occurs, assuming the Async events API is used with a specified timeout equal or greater than 5 minutes. Otherwise the specified or default timeouts apply."
Context window (in / out): Haiku 200K / 64K; Sonnet 1M / 128K; Opus 1M / 128K.
Changelog 2 Oct 2026: "The tokens per minute (TPM) rate limit for Forge LLMs has been increased from 50,000 to 500,000." / "The limit applies per installation of your app for each model." / "If your app previously encountered rate limiting errors (429 Too Many Requests) due to token usage, you may now be able to increase the frequency or size of your LLM requests."
No LLM-specific "tier" exists in the docs; the only LLM tiers are the model tiers. (Tier 1 = the Jira/Confluence REST points "Global Pool", a different limit; the REST reads that build prompts count against it, LLM calls do not, per the separate limit tables. Not researched here.)

### 3.3 Function timeouts that bound an LLM call (https://developer.atlassian.com/platform/forge/limits-invocation/, Sep 1, 2026)
> "Runtime seconds (also includes UI modules invoked by Forge Remote) | 25 | Maximum runtime permitted before the app is stopped."
> "Runtime seconds (async events and scheduled trigger module) | 900 | This applies to function modules that are only referenced by consumer or scheduled trigger modules. Default timeout is 55 seconds. Use timeoutSeconds to extend it."
> "Runtime seconds (web trigger, action and rovo:agentConnector modules) | 55 | Maximum runtime permitted before the app is stopped."
Per-user invocation rate: "Per user | 1,200 per minute"; per install: "7,000 per minute and 300 per second (whichever is hit first)".
Realtime tutorial (Jul 23, 2026): "some prompts or agentic workflows may exceed the default function timeout (55s). To handle these, offload work to a queue consumer (up to 15 minutes) and stream results back to the user interface (UI) using Realtime publish/subscribe."
(Inconsistency: a resolver's limit is 25 s per the limits page; the tutorial says 55 s.)
Cost guide: "Long-running operations — tasks that would exceed Forge's function timeout (25 seconds for standard functions)."

---------------------------------------------------------------------------------------------------

## 4. Pricing and billing

https://developer.atlassian.com/platform/forge/runtime-reference/forge-llms-api-pricing/ (Jul 23, 2026):
> "No free usage allowance: The Forge LLMs API does not include a free monthly usage quota. All token usage is billed."
> "Forge LLMs usage is charged to the developer of the Forge app and counted toward your Forge monthly bill."
| Model | Credits per 1M tokens | $/1M input | $/1M output |
|---|---|---|---|
| Opus 4.6 | 50 | $5 | $25 |
| Sonnet 4.5 | 30 | $3 | $15 |
| Haiku 4.5 | 10 | $1 | $5 |
> "Input credits: $0.10 per credit" / "Output credits: $0.50 per credit"
Worked example: 0.5M/0.2M Opus + 5M/1M Haiku = "Total monthly cost: $7.50 + $10.00 = $17.50".
GAP: no rate is published for claude-sonnet-4-6, claude-sonnet-5, claude-opus-4-7/4-8/5 (page predates GA).
CONTRADICTION: /optimise-forge-costs/ (Aug 21, 2026): "Input credits and output credits are charged at different rates ($0.0000001/credit for input, $0.0000005/credit for output)." This conflicts by 10^6 with $0.10/$0.50; the pricing page and the platform-pricing worked example ($2.00 for 20 Haiku input credits) agree on $0.10/$0.50.
No caching or batch discount exists in the SDK. RFC-117 wrap-up (staff, #35) lists "More features like response streaming and caching" as a requested theme; 1.0.7 has no `cache_control`. Batch: Adam Moore (staff, 2026-08-05, cdac 101988/5): "It's not something that we have planned yet but happy to track interest."
Compute is billed on top. Danielle Larregui (staff, relaying the Forge LLM team, 2026-01-13, cdac 98220/5): "Compute usage (GB-seconds) is a different cost element to the LLM usage (token based). While LLM responses do take longer to respond, the running costs will therefore be twofold:" Adam Moore (staff, cdac 98220/9): "we need to account for the cost of the underlying lambda which waits for the response as well as the tokens used by the model."
Platform pricing (https://developer.atlassian.com/platform/forge/forge-platform-pricing/, Aug 12, 2026): "LLM: Input | $/credits | 0 credits | Credit pricing varies by model."; non-production environments are billed too: "Usage will be measured across all environments which include production, staging, development and all custom development environments."
Dosing is the app's job. Adam Moore (staff, RFC-117 #17, 2025-10-30): "Yes, early adopters of Forge LLMs will need to build user/tenant/edition based limits within their apps."
Usage alerts email at 50/75/90/100% thresholds (https://developer.atlassian.com/platform/forge/usage-alerts/).

Rovo billing (platform pricing page): "Rovo billing is managed at the customer organization level, so Marketplace partners are not responsible for the AI usage costs of their Rovo agents." / "Forge consumption-based pricing (compute, storage, and logs used to execute your action) is paid by the developer of the Forge app, just like for any other Forge module."

---------------------------------------------------------------------------------------------------

## 5. Errors

SDK classes (`out/errors.d.ts`): `ForgeLlmError extends Error`; `PromptValidationError extends ForgeLlmError`;
`ForgeLlmAPIError extends ForgeLlmError { code?: string; status: number; statusText: string; traceId?: string | null; }`; `StreamResponseError extends ForgeLlmError`.
Only `ForgeLlmError` and `PromptValidationError` are exported from the package entry
(`out/interfaces/types.js` defines exports for those two only). An app recognises an API error by
`err.status` / `err.name === 'ForgeLlmAPIError'` / `instanceof ForgeLlmError`.
`checkResponseError` (`out/utils/error-handling.js`): code/message from a JSON body `{code, message}`,
else `code` = header `forge-proxy-error` ?? `'UNKNOWN_ERROR'`; traceId from `x-b3-traceid`/`x-trace-id`/`atl-traceid`.
NO `Retry-After` or rate-limit header is surfaced on the error, and NO `context` property is set:
the docs' examples log `err.context?.responseText`, which is always `undefined` on a `ForgeLlmAPIError`
(1.0.7). The realtime tutorial's consumer therefore always publishes "Unknown error occurred".
Known codes/statuses (documented or observed):
- 403 `FORGE_LLMS_MODEL_FORBIDDEN`, message "Forge LLMs model not allowed: anthropic.claude-sonnet-5" (partner log, cdac 101982/1, 2026-08-02); staff reply (Shobhit Sharma): "This shouldn't be the case if your app manifest has Family defined." Fixed by updating the installed version from admin.
- 429 Too Many Requests from token usage (changelog 2 Oct 2026, quoted in §3.2).
- Sampling-rule violations: status/code undocumented.
- Moderation: overview: "Requests to Forge LLMs undergo the same moderation checks as Atlassian first‑party AI and Rovo features. High‑risk messages (per the Acceptable Use Policy) are blocked." Status/code of a block undocumented.
- Inference timeout beyond 5 min / function timeout: shape undocumented.

---------------------------------------------------------------------------------------------------

## 6. Data handling, moderation, responsible AI, observability

- No egress / Runs on Atlassian: overview: "Apps using this API are badged as Runs on Atlassian"; manifest: "The app retains its Runs on Atlassian eligibility after the module is added."
- Hosting: GA blog: "Under the hood, the Forge LLMs API runs on Amazon Bedrock"; "customer data never travels to a third-party AI service."
- Residency: Adam Moore (staff, RFC-117 #32): "Data residency - Cross region inference will be used, but no data will be stored out of region so it meets Atlassian's definition of data residency."
- Training: Adam Moore (staff, RFC-117 #27): "prompts and outputs won't be used by Atlassian or our sub processor (AWS in this case) for model training."
- Vanilla models: Adam Moore (staff, RFC-117 #25): "They will be vanilla models with an additional moderation check"
- Forge Terms 7.3 (https://developer.atlassian.com/platform/forge/developer-terms/, effective Dec 1, 2025): "If you elect to use Forge LLMs in your Forge App, you acknowledge and agree that you are instructing Atlassian to apply our moderation filters and other safeguards to your requests, and to submit any end user queries included in your requests to Forge LLMs for processing on your behalf. If we identify any issues with your use of Forge LLMs, we may suspend your App or your right to use Forge LLMs at any time in our sole discretion."
- Acceptable Use Policy, AI section (https://www.atlassian.com/legal/acceptable-use-policy): no use to "Make automated decisions with legal or similarly significant effects"; no use to "Mislead individuals into believing that they are communicating with a human when they are not, or claim that content generated through our artificial intelligence offerings and features was generated by a human"; a violation to "Seek to override or circumvent the technical or safety measures designed to safeguard our services, including, for example, by using techniques such as prompt injection and jailbreaking".
- Observability: Adam Moore (staff, 2026-02-27, cdac 99300/2): "our general principle is to not expose raw production prompts and responses." Recommends offline evals ("Build a robust set of "Golden Test Cases""), and "logging high-level outcome metrics (e.g., `llm_success_rate`, `tool_call_retry_count`)".
- Prompt-injection guidance specific to Forge LLM (the `chat()` path): NONE found on developer.atlassian.com. The only primary injection guidance is the Marketplace security requirement for Rovo actions (§7) and the AUP sentence above.

---------------------------------------------------------------------------------------------------

## 7. Security requirements that bind AI features (Marketplace, mandatory)
https://developer.atlassian.com/platform/marketplace/security-requirements/ ("Last updated Feb 19, 2026"), Forge table:
> "1. An application must default to using `asUser()` when performing an operation on behalf of the user."
> "2. Before making calls using `asApp()`, you must verify the expected permissions (for example, from product context) with the permissions REST APIs."
> "6. Any Atlassian End User Data written to application logs must exclude Personally Identifiable Information (PII), credentials, and sensitive data."
> "7. An application must ensure strict tenant isolation during runtime. Data or variables from one tenant must not be accessible to another, including via runtime artifacts."
> "13. An application using Forge Rovo actions must treat all action inputs as untrusted, except for context parameters. The application must validate `inputs` and verify permissions before executing sensitive actions or making network requests to mitigate prompt injection attacks and data exfiltration risks." Implementation detail: "1. An application using Forge Rovo agents that can perform admin level actions must implement its own permission based checks so that only authorized users can invoke those actions, as Rovo agents are accessible to all users."
> "14. An application using Forge Rovo actions must configure `actionVerb` values that accurately reflect whether an action only reads data or also performs mutable actions to preserve user consent and control over actions."
Permission-before-context (remote agents guide, https://developer.atlassian.com/platform/forge/remote-agents-in-jira/, Oct 9, 2026): "You must ensure that your agent only reasons about data that the user who assigned them to a work item has access to." / "agent memory must be specific to the context user — it is not safe to have shared memory for a Jira tenant / installation" / "you must not pass data to an agent working on a task without first checking that the assigning user has permission to view that data".
Realtime channel isolation (realtime tutorial): "You must use tokens with unique custom claims to secure your Realtime channels." and "Avoid cross-talk where one user receives another user's events".

---------------------------------------------------------------------------------------------------

## 8. Rovo (modules, changes since 2026-07)

Common banner on every Rovo module page: "For the protection of our customers, Atlassian performs safety screening on Agents at our sole discretion."
Index (https://developer.atlassian.com/platform/forge/manifest-reference/modules/rovo-index/, Oct 2, 2026): `rovo:agent`, `rovo:skill` (Preview), `action`, `rovo:mcp` (Preview), `rovo:agentConnector`.

### 8.1 `rovo:agent` (Oct 2, 2026)
- `name`: "Must not exceed 30 characters."; `prompt` mandatory, string or resource path ("you must have Forge CLI version 10.6.0 or above"); `skills`: "A list of `rovo:skill` module keys that the Agent can use. ... This property is available as part of the `rovo:skill` Preview."
- Data: "app-based Agents only have access to the data in the workspace that the app is installed in."
- Interaction points: bridge `rovo` API, chat side panel, `/ai` in Confluence and Jira editors, Automation.
- "Long inline `prompt` values are one of the largest contributors to manifest size".

### 8.2 `action` (Aug 3, 2026)
- `actionVerb`: "`GET`, `CREATE`, `UPDATE`, `DELETE`, `TRIGGER`." / "Agents triggered by automation rules will not invoke actions with actionVerb `CREATE`, `UPDATE`, `DELETE`, and `TRIGGER`."
- input `type`: "`string`, `integer`, `number`, or `boolean`."
- "The Rovo Agent action module can only handle data up to 5 MB due to a dependency size limit"
- "Input: The LMM extracts inputs from the Atlassian app context and user interactions (e.g., chat prompts). This makes it more flexible but also leaves it subject to hallucination. Your app should never rely on values passed as inputs to perform critical checks like authorization. If you need a user's `accountId` read it from the `context`."
- "Context: ... This is done deterministically in the same way the Atlassian app context is passed to other Forge modules."
- "Your function can return any string or JSON object, which the Agent will interpret and transform into a natural-language response to the customer."
- Confirmation semantics: Sushant Bista (staff, RFC-137-A #5, 2026-06-19): "This can be done now via `actionVerb` on your `action` and anything other than read requires confirmation." Ian Buchanan (staff, 2024-11-11, cdac 86069/2): non-GET verbs "are all effectively "not GET"".
- Counter-example in Atlassian's own tutorial (/build-a-jira-issue-analyst-rovo-agent/, Mar 3, 2025): `api.asApp().requestJira(route`/rest/api/3/search/jql?jql=${jql}`)` with `jql` built from an agent input — violates requirement 2 (asApp without permission check).

### 8.3 `rovo:skill` (Preview since 2026-10-02; page Oct 2, 2026)
- "A skill consists of a `SKILL.md` instruction file, optional supporting files, and optional action dependencies from the same Forge app."
- `source.dir` must contain `SKILL.md`; `dependencies.tools`: "A list of action module keys from the same app."
- Frontmatter: `name` "Must be 1–64 characters, contain only lowercase letters, numbers, and single hyphens, and match the parent directory name"; `description` "Must be 1–1,024 characters. Use at least 50 characters"; `allowed-tools` "If specified, it must include every action listed in `dependencies.tools`."
- "Keep the Markdown body of `SKILL.md` to 500 lines or fewer. Forge CLI warns when the body exceeds this length."
- "A skill directory must not exceed 100 MB uncompressed."
- "At runtime, the Agent selects a skill based on its description and the user's request. Explicit invocation by name isn't supported during Preview."
- Preview limits: "Skill-to-skill dependencies aren't supported." / "Executable skill sources, including scripts, aren't supported."
- "The `rovo:skill` module doesn't introduce additional scopes or a separate consent step."
- Partner report (no staff answer, cdac 101854, 2026-07-21): a Studio agent with one skill of an app could invoke every action of the app.

### 8.4 `rovo:mcp` (EAP Jul 24 -> Preview for Rovo Studio by Aug 20 -> external clients Oct 1)
Page (Oct 1, 2026): "The `rovo:mcp` module lets you expose actions as tools that agents can invoke. Tools are available to custom agents in Rovo Studio, and can also be connected to third-party, MCP-compatible AI clients (EAP). An app can have at most one `rovo:mcp` module."
- `name`: "Must not exceed 30 characters."; `tools`: "Each referenced action must have a unique key under 64 characters, and there can be at most 50 actions."
- External: "External MCP exposure is disabled by default." Endpoint `https://mcp.atlassian.com/forge/<appId>`; OAuth 2.1 consent; "subject to the invoking user's own permissions on that site."
- Status CONTRADICTION: changelog 1 Oct 2026 says "the `rovo:mcp` module, which is now available in Preview" for external clients; the module page and both tutorials still say the third-party-client capability "is available under Forge's Early Access Program (EAP)" (the hello-world tutorial's heading says "(Preview)" over an EAP paragraph).
- asUser caveat (Jira MCP tutorial, Oct 1, 2026): "For reliable `asUser()` execution from a third-party client, the tool's user must have an app consent entry for this app in their Connected apps. Without this consent entry, `asUser()` calls can fail when invoked remotely. ... use `asApp()` if you can't guarantee the user has this consent entry." (RFC-134 had promised "`asApp()` will be actively prevented for MCP-invoked actions"; the shipped docs say otherwise.)
- Dean Peach (staff, 2026-10-05, cdac 102929/6): "a customer's site does not need Rovo enabled or licensed for an external AI client to access an app's `rovo:mcp` tools." and per-tool admin choice "is planned for GA."
- Tool selection (Dean Peach, staff, RFC-134 #20/#22): "Tools are picked based on name + description from a flat list. Descriptions should be concise (roughly 40-100 tokens) and we will most likely enforce such a limit." / "We recommend individual tools. A single tool with an action parameter hides capability from the discovery layer and forces two-step reasoning."

### 8.5 `rovo:agentConnector` (GA 2026-09-28; page Oct 9, 2026)
- "Integrations must use A2A 1.0."; "Only `jira` is currently supported." (productContexts); `read:jira-work` "is required".
- Timeouts: sync 55s; streaming SSE 900s; chat interaction 30 min; work item 60 min.
- "The app system user will no longer be mentionable." (agent connector user replaces it).
- "Non-production apps might fail during the invocation if the user triggering the agent is not a contributor to the app."

### 8.6 Bridge `rovo` (Mar 23, 2026)
`rovo.open({type:'forge', agentName, agentKey, prompt?})`, `{type:'atlassian', ...}`, `{type:'default', prompt?}`;
"You can only open Forge agents that are created in the same app from where the method is called.";
`rovo.isEnabled()` "returns a boolean value indicating whether Rovo is enabled in the current tenant." Supported in "All Jira modules", "All Confluence modules", and two JSM modules.
No backend API to call Rovo from a Forge function was found (partner answers only, cdac 99784).

---------------------------------------------------------------------------------------------------

## 9. Doc/SDK contradictions a benchmark must not trip on (record, then decide)
1. Lint: docs "linting will fail with an error" vs `@forge/lint` 6.3.0 Warning + auto-fix.
2. Error detail: docs log `err.context?.responseText`; `ForgeLlmAPIError` has no `context` in 1.0.7.
3. Credit price: $0.10/$0.50 per credit (pricing page) vs "$0.0000001/credit ... $0.0000005/credit" (cost guide).
4. Credit rates exist only for Opus 4.6 / Sonnet 4.5 / Haiku 4.5; 5 of 8 listed models have no published rate.
5. `rovo:mcp` third-party clients: Preview (changelog Oct 1) vs EAP (module page, tutorials).
6. Resolver timeout: 25 s (limits page) vs "default function timeout (55s)" (realtime tutorial).
7. Model status case: `ACTIVE` (docs table) vs `'active'` (SDK type).
8. Atlassian samples use models no longer listed and send `temperature` + `top_p` together (rejected today).
9. `refusal` finish_reason: not documented on Forge.
10. RFC-134 said `asApp()` would be prevented for MCP-invoked actions; shipped tutorial recommends `asApp()` as a workaround.

---------------------------------------------------------------------------------------------------

## 10. How an offline benchmark can grade "good Forge LLM usage" objectively

Principle (same as Forge 1.0's R6): the model behind the emulator is a SCRIPTED fake. It never grades
model quality, only what the app does with each answer. Every graded behaviour below is observable in
the emulator's LLM log (method, model, body, caller invocation, asUser, timestamps, usage), the app's
logs, the mock Jira/Confluence write log, and the DOM.

G1 Model choice and lifecycle
- App must: pick a model `list()` returns `active` at call time; never a `deprecated` or unlisted id; honour the sampling rules.
- Grader: emulator `list()` may report one model `deprecated` (realistic: sonnet-4-5's earliest retirement date has passed); every chat/stream `model` must be an `active` id; sampling violations get a 400.
- Contract must state: "the model list changes; choose from `list()` at call time; a `deprecated` model must not be called". The sampling rules are documented (knowledge trap allowed, as in 1.0), but if graded beyond the docs (e.g. which tier for which job) the tier policy must be stated, including how a tier is recognised from an id (list() carries no tier).

G2 Grounding (numbers from data)
- App must: compute every metric itself; the LLM writes prose only; any number or issue key the AI text shows must exist in the app's own computation and be visible to the viewer.
- Grader: scripted answers carry invented numbers, a hidden issue key, an unknown key, and a correct one; DOM numbers inside the AI panel must be a subset of the oracle's numbers; only visible, existing keys may render.
- Contract must state the rule ("never display a number or key the model produced unless it equals one you computed / is visible").

G3 Structured output through a forced tool
- App must: request structured output via `tools` + forcing `tool_choice`; validate the arguments (types, enums, ranges) before use; treat `arguments` as an object but tolerate a JSON string.
- Grader: scripted malformed arguments (wrong types, missing required, extra keys) must produce the app's error state and no side effect.
- Contract must state the tool name and JSON schema, and that schema-invalid arguments are an error. (Structured outputs / `strict` are unavailable in 1.0.7, so the app cannot delegate validation.)

G4 Agentic tool loop
- App must: answer every `tool_calls` entry with a `role:'tool'` message carrying its `tool_call_id`, re-send the assistant message with its `tool_calls`, execute only allow-listed read tools, and bound the loop.
- Grader: scripted turn with two parallel tool calls; the follow-up must carry both tool messages with matching ids (the emulator answers 400 otherwise, mirroring the tutorial's three-message rule); a call to a non-allow-listed or write tool must not reach the mock Jira write log.
- Contract must state: the tool list per surface, "answer every tool call", and the loop bound. (Parallel calls are not in any Atlassian sample, so they must be stated.)

G5 Failure handling (refusal, malformed, 403, 429, 5xx, timeout, truncated stream, empty answer)
- App must: show a defined error state and keep working; non-AI features keep working; no retry storm.
- Grader: scripted sequence; the emulator returns `refusal` (shape stated), 403 `FORGE_LLMS_MODEL_FORBIDDEN`, 429, 500, a stream that ends without a `finish_reason` chunk, and an `end_turn` with empty content.
- Contract must state each shape the emulator uses (none of refusal/moderation/429 codes are documented by Atlassian), the expected UI state per class, and the retry rule. Because the SDK exposes no `Retry-After` on `ForgeLlmAPIError`, the retry rule cannot be "honour Retry-After"; state an explicit rule (e.g. "after a 429, no further LLM request from that installation for N seconds, at most K retries, then queue the job").
- Truncated stream: the documented detection is "a completion choice object with a `finish_reason` property is missing when the stream ends"; grade that the UI never shows the partial as final, and (optionally, if stated) that the retry carries the accumulated output rather than the bare original prompt.

G6 Rate dosing within the documented per-installation limits
- App must: keep ≤ 100 LLM requests per minute per installation across all models and ≤ 500,000 tokens per minute per model, even when a user or a scheduled job triggers a large batch.
- Grader: the emulator counts requests per installation in every 60 s window and tokens per model per window (token estimator stated); it returns 429 above the limit; score = zero windows over the limit + the batch still completes.
- Contract must state the two numbers (they are documented, but stating them keeps it fair) and the token estimator the emulator uses.

G7 Token economy and the developer's bill
- App must: bound output (`max_completion_tokens` on every call), keep input under a stated per-request budget (send only the fields needed, batch several items per prompt), cache results keyed by installation + viewer visibility scope + content hash + model, and re-process only changed items.
- Grader: LLM log = calls per action (second identical request = 0 new calls), max input tokens per request, presence of `max_completion_tokens`, calls after an unchanged rerun = 0.
- Contract must state the budgets, the cache key semantics, and the estimator.
- Admin panel (UI Kit 2) budgets: the staff-stated need ("build user/tenant/edition based limits within their apps") becomes: an admin sets a per-user and per-installation token budget; when exhausted the app makes no further LLM calls and says so. Grader: set budget via the admin form, drive usage, count calls after exhaustion (must be 0). Cost shown in the panel = oracle from the emulator usage log x the documented credit rates; contract must give the rates for every model the emulator lists (5 of 8 are not published; state the mapping by tier).

G8 Long calls run where they can finish (and boot speed)
- App must: never block a resolver (25 s) on a long LLM call; run long jobs in a queue consumer with `timeoutSeconds` ≥ 300 and push the result to the UI via Realtime with per-user token claims; make zero LLM calls while a page first renders.
- Grader: emulator injects latency above 25 s for the long job; resolver-initiated calls time out; the consumer path completes and the UI updates; a second user on the same channel name receives nothing; LLM log shows no call during page boot.
- Contract must state the latency envelope ("an AI job may take up to N minutes"), "opening a page makes no LLM call", and the realtime isolation rule (documented as "Important" in the tutorial, but state it).

G9 Permission filtering before prompting, and per-user caches
- App must: build every person-facing prompt only from data the viewer can see (`asUser()` reads, or `asApp()` + permission check per requirement 2); never serve a cached AI result computed for a wider-visibility user to a narrower one.
- Grader: canary strings in hidden issues must appear in no LLM request body from a person-facing invocation and in no DOM; user A (wide) then user B (narrow) trigger the same AI view: B gets no A-only content.
- Contract must state the rule. Primary anchors: requirements 1, 2, 7; remote-agents guide "not safe to have shared memory for a Jira tenant / installation".

G10 Injection resistance (defence in depth, model assumed compromised)
- App must: delimit untrusted content, never let model output trigger a write without an explicit user confirmation in the UI, never execute a tool or argument outside the allow-list or the viewer's permissions, and never let data-borne text change what the app displays as fact.
- Grader: issue text carries injected instructions; the scripted model "obeys" them (emits a write tool call, a hidden key, an exfiltration URL in text); pass = no mock-Jira write without a confirm click, no hidden canary in DOM/logs, no external fetch (egress log), injected URL not rendered as a link.
- Contract must state "treat model output as untrusted input" and the confirmation rule. Rovo side: requirement 13/14 (documented); still state it.

G11 Rovo surfaces
- App must: actions authorize from the invocation `context` (accountId) not from inputs; validate inputs (issue-key regex, ranges); accurate `actionVerb`; admin-level actions check admin permission; MCP tool count ≤ 50, keys < 64 chars, names ≤ 30; skill frontmatter rules; `rovo.isEnabled()` checked before offering `rovo.open`.
- Grader: invoke actions directly in the emulator with a forged `accountId` input, a key the caller cannot see, a non-admin caller on an admin action; static manifest checks; tenant with Rovo disabled must not call `rovo.open`.
- Contract must state input names/types and expected refusals; the platform limits are documented.

G12 Data handling and labelling
- App must: no prompt text, model output, or issue content in logs (requirement 6); label AI-generated content as AI-generated (AUP); a human confirms consequential writes (AUP "automated decisions").
- Grader: canary scan of captured app logs; DOM/ADF marker on AI text; write log empty before confirm.
- Contract must state the label text/marker and the confirm requirement.

---------------------------------------------------------------------------------------------------

## 11. Open questions / could not verify
- HTTP status and `code` for: sampling-rule violations, moderation blocks, RPM vs TPM 429s, inference timeouts, context-window overflow. Only `FORGE_LLMS_MODEL_FORBIDDEN`/403 is observed (partner log + staff reply).
- Whether Forge passes Anthropic's `refusal` (or `model_context_window_exceeded`, `pause_turn`) through as `finish_reason`.
- Default `max_completion_tokens` when omitted.
- Whether unknown request fields (`response_format`, `stop`, `cache_control`) are ignored or rejected server-side.
- How streamed tool-call arguments are chunked; whether `usage` arrives on every chunk or only the last.
- Whether a 429 response carries `Retry-After` at the HTTP level (the SDK drops headers other than the trace id either way).
- Credit rates for sonnet-4-6, sonnet-5, opus-4-7, opus-4-8, opus-5.
- Exact EAP date of `rovo:skill` and the Preview date of `rovo:mcp` for Rovo Studio (between 2026-08-09 and 2026-08-20 per Wayback + cdac); the changelog API (`/gateway/api/dac-changelogs/changes`) answers 401 to anonymous requests and the RSS URL 404s, so pre-2026-09-26 changelog entries could not be read directly.
- Whether the RPM limit (100) counts `list()` calls.
- Whether Forge LLM calls made from a Rovo action are allowed/billed differently (nothing documented; presumably developer-billed like any Forge LLM call).

## 11b. Platform limits that shape an LLM pipeline (fetched 2026-10-09)
- Async events (https://developer.atlassian.com/platform/forge/limits-async-events/, Feb 26, 2026), per installation:
  "Event per minute | 500 | Maximum number of events pushed in one minute."; "Event per request | 50";
  "Payload size for long running functions | 100 KB | Maximum size of an individual event. This limit only applies to functions specifying a timeout greater than 55 seconds."
  => an LLM job queued for a long consumer cannot carry a large prompt in its event (the realtime tutorial pushes the whole prompt in `queue.push([{ body: payload }])`); enqueue ids and re-read in the consumer.
- Realtime (https://developer.atlassian.com/platform/forge/limits-realtime/, Jun 25, 2026): "Operations per second | 50 (3000 events per minute) | Maximum number of requests in one second for each installation. Once this limit is reached, requests after will fail with errors. Apps are required to handle retries."
  => publishing every streamed token to the UI breaches it; coalesce chunks.
- Invocation (limits page): "Per user | 1,200 per minute"; per install "7,000 per minute and 300 per second (whichever is hit first)"; "Front-end invocation request payload size | 500KB"; "Front-end invocation response payload size | 5MB"; "Payload size | 5MB".
- SDK transport (https://unpkg.com/@forge/llm@1.0.7/out/llm-client.js): `static LLM_BASE_URL = 'https://llm';` and `global.__forge_fetch__({ type: 'llm', model: model }, path, ...)`; request body = `{ ...request, stream: false|true }` (out/llm-api.js) — unknown fields pass through unfiltered.

## 11c. Quote verification
`research/claims.json` holds the 83 claims returned to the orchestrator; `scripts/verify_quotes.py` checks
every quote is a substring of its local source copy after collapsing whitespace, dropping the backticks that
stand for code formatting, unwrapping markdown links, and rendering table cell boundaries as " | ".
Result: "83 claims, 0 quotes not found" (negative control: two altered quotes were caught). In quotes taken
from tables, " | " separates table cells.

## 12. Source index (local copies, all fetched 2026-10-09)
pages/llm-api-reference.txt, llm-overview.txt, llm-models.txt, llm-pricing.txt, llm-errors.txt, llm-limits.txt,
llm-manifest.txt, llm-webtrigger-tutorial.txt, llm-agentic-tutorial.txt, llm-realtime-tutorial.txt,
forge-changelog.txt, rovo-index.txt, rovo-agent.txt, rovo-action.txt, rovo-skill.txt, rovo-mcp.txt,
rovo-agent-connector.txt, rovo-bridge.txt, rovo-mcp-tutorial.txt, rovo-mcp-jira-tutorial.txt,
rovo-agent-tutorial.txt, rovo-issue-analyst.txt, remote-agents-jira.txt, limits-invocation.txt,
limits-async-events.txt, forge-platform-pricing.txt, optimise-costs.txt, usage-alerts.txt,
security-requirements.txt, forge-terms.txt, aup.txt, blog-forge-llms-preview.txt, blog-forge-llms-ga.txt,
anthropic-stop-reasons.txt, wb-models-20260624.txt, wb-limits-20260512.txt, wb-models-20260921.txt,
wb-rovo-mcp-20260809.txt.
cdac/: 96506 (RFC-117), 98220, 99300, 99784, 101045, 101251, 101982, 101988, 102024, 100683 (RFC-134),
100984 (RFC-137-A), 101854, 102254, 102510, 102929, 86069.
pkgs/: llm-1.0.7, llm-next (1.0.8-next.1), llm-exp (1.0.8-next.0-experimental-e03cc85), lint (6.3.0),
bb-confluence-ai-assistant (Bitbucket atlassian/forge-llm-examples).

---------------------------------------------------------------------------------------------------

## Verification

Independent fact-check of the 66 design-critical claims, 2026-10-09. Nothing below reuses the
researcher's local copies: every source URL was re-fetched with curl at 2026-10-09 17:10 UTC into
`forge2/verify-llm/fetched/`, converted to text with a stdlib script (`verify-llm/txt/`), and each quote
was checked as a substring by `verify-llm/scripts/checkquotes.py` (curly quotes, non-breaking hyphens,
backticks and whitespace normalised). Result: 63 of 66 quotes found verbatim (#31 after dropping the
researcher's "- " list bullets). The other 3 (#24, #30, #40) are multi-row table quotes that my converter
renders one row per line instead of " | "; each row was then confirmed by reading the page text.

Newer-source sweep: the Forge changelog's public API answers anonymously when given `apiGroups`
(`https://dac-changelogs.services.atlassian.com/changes?apiGroups=forge-core-platform,forge-jira-cloud-platform,forge-jsm-cloud,forge-jsw-cloud,forge-confluence-cloud&limit=100&offset=N`),
which gave 400 entries from 2025-05-12 to 2026-10-09. That closes §11's "pre-2026-09-26 changelog
could not be read" gap. The sweep also covered the npm registry, a file-by-file diff of unpkg 1.0.7
against 1.0.8-next.1, Anthropic's Model status page, FRGE-2237 over the ecosystem REST API, the
atlassian/forge-llm-examples tarball, and Wayback.

### Newer facts that change the design
1. MODELS: Anthropic's Model status page is the one the Forge models page links for "the latest
   values" (https://platform.claude.com/docs/en/about-claude/model-deprecations#model-status). It now
   shows:
   - `claude-sonnet-4-5-20250929` Deprecated on September 30, 2026, retiring November 30, 2026 (recommended replacement `claude-sonnet-5-5`).
   - `claude-haiku-4-5-20251001` still Active, "Not sooner than October 15, 2026".
   - `claude-haiku-5-5`, `claude-sonnet-5-5` and `claude-opus-5-5` listed Active.

   No Forge page or changelog entry mentions the three 5-5 models. The last Forge model addition is
   Opus 5 (CHANGE-3370, 2026-08-03), and no Forge entry deprecates sonnet-4-5. I did not check Forge's
   live `list()` status, because that needs a deployed app.
2. 180-SECOND OUTBOUND CAP: limits-invocation (Sep 1, 2026; the row is already in the Apr 24, 2026
   Wayback copy) says: "Single outbound request timeout (async events) | 180 | Maximum time a single
   outbound request can take before being terminated. Outbound requests refer to fetch requests,
   including both Atlassian app REST API and external API requests." The SDK sends LLM calls through
   `__forge_fetch__({type:'llm'})`. Atlassian does not say whether this cap applies to them. If it
   does, a single chat()/stream() call is cut at 180 s even inside a 900 s consumer, below the
   documented 5-minute inference window. Contract: keep injected single-call latency at or below 180 s,
   or state the rule.
3. CREDIT PRICE CONFLICT: /optimise-forge-costs/ ("Last updated Aug 21, 2026", newer than the pricing
   page) says: "Input credits and output credits are charged at different rates ($0.0000001/credit for
   input, $0.0000005/credit for output)". That is 10^6 below the pricing page's $0.10/$0.50.
   platform-pricing (Aug 12, 2026) works its example at $0.10/$0.50. Treat the cost guide's figure as
   a doc error, but state the rate in the contract.
4. DEPRECATION POLICY (GA changelog CHANGE-3365, 2026-07-30): "While we aim to provide a standard
   6-month deprecation notice for models, this may not always be possible."
5. ATLASSIAN'S REALTIME TUTORIAL IS EXPLOITABLE: `sendLLMPrompt` pushes the client `payload`
   unchanged, and the consumer signs its publish token from `event.body.customClaims` and calls
   `chat(prompt)` with the client's prompt. A caller can therefore:
   - put another user's accountId in the claims and publish into that user's channel;
   - pick any model and any prompt, at the developer's cost.

   A contract should require that claims and model are derived server-side (`context.accountId`).
   Realtime GA (CHANGE-3326, 2026-06-29) added `permissions` on `signRealtimeToken`, which allows
   publish-only and subscribe-only tokens.
6. Changelog dates the researcher could not read:

   | date | change | entry |
   |---|---|---|
   | 2026-02-04 | `claude-3-7-sonnet-20250219` removed from Forge LLMs | CHANGE-3055 |
   | 2026-06-01 | Preview begins (progressive rollout); `claude-sonnet-4-20250514` not included | CHANGE-3261 |
   | 2026-06-23 | Sonnet 4.6 added | |
   | 2026-07-08 | Opus 4.7 added | |
   | 2026-07-16 | Sonnet 5 added | |
   | 2026-07-20 | Opus 4.8 added | |
   | 2026-07-30 | GA | CHANGE-3365 |
   | 2026-08-03 | Opus 5 added | |
   | 2026-08-03 | `rovo:mcp` EAP | CHANGE-3362 |
   | 2026-08-14 | `rovo:mcp` Preview for Rovo Studio | CHANGE-3400 |
   | 2026-08-25 | Agent Connector Preview | CHANGE-3405 |
   | 2026-09-10 | `rovo:skill` EAP (development environments only) | CHANGE-3436 |
   | 2026-09-28 | Agent Connector GA | |
   | 2026-10-02 | `rovo:skill` Preview | CHANGE-3499 |
7. FRGE-2237 ("Support structured (JSON schema) outputs in Forge LLMs", type Suggestion): status
   "Reviewing", unresolved, last updated 2026-08-10.
8. `@forge/llm`: latest is 1.0.7 (2026-09-28); 1.0.8-next.1 was published 2026-10-09 00:02 UTC. Every
   `out/` file I compared is byte-identical between the two. Only package.json differs (version, and
   `@forge/api` ^8.2.0 to ^8.3.0-next.2).
9. atlassian/forge-llm-examples:
   - last commit 2026-01-12;
   - pins `@forge/llm` 0.2.0 / ^0.2.0;
   - four interactors hard-code `claude-3-7-sonnet-20250219`;
   - summary, explain and generateContent send `temperature: 0.7` with `top_p: 1`.

### Per-claim verdicts
| # | claim (short) | verdict | evidence / correction |
|---|---|---|---|
| 1 | LLMs API GA 2026-07-29; no Preview banner | confirmed | Blog text verbatim; API reference (Aug 3) carries no Preview label; changelog CHANGE-3365 is dated 2026-07-30 (published 07-29 22:31Z). Correction: Preview began 2026-06-01 (CHANGE-3261); 06-02 is the community announcement. |
| 2 | exactly list/chat/stream | confirmed | Quote verbatim; `out/index.js` exports only chat, stream and list, plus 2 error classes. |
| 3 | request fields; no response_format/strict/stop/cache_control/images | confirmed | Docs and `internal.d.ts` match; 1.0.8-next.1 `out/` is byte-identical; no fetched page documents a default for `max_completion_tokens`. |
| 4 | tool_choice values force a call | confirmed (values) | Types match. The FORCING meaning of 'required' and of a named function is not stated by Atlassian (the tutorial says only "Specifies which tool (if any) the LLM should use"). State it in the contract. |
| 5 | arguments typed as object | confirmed | As typed, and the README example has object args. The response mapper is a bare `response.json()` and Atlassian's tutorial parses defensively (`typeof ... === 'string' ? JSON.parse(...)`), so the wire type is not enforced. |
| 6 | usage and every field optional | confirmed | Docs and SDK types. |
| 7 | usage returned per request | confirmed | Overview quote verbatim. "Only runtime metering signal" is not stated by Atlassian; I found no runtime usage API, only the console and app-level usage-alert emails. |
| 8 | finish_reason tool_use/end_turn/max_tokens; refusal undocumented | confirmed | Tutorial quote verbatim; none of the ~30 fetched Forge pages contains "refusal". |
| 9 | three-message follow-up; samples answer tool_calls[0] only | confirmed | Tutorial and example app use `tool_calls?.[0]` for one round. createJiraStoryInteractor loops but breaks at the first match and sends no tool message. No Atlassian sample answers parallel calls. |
| 10 | no structured outputs; FRGE-2237 suggestion | confirmed | Post #5, Shobhit Sharma (Atlassian Staff), 2026-08-10. FRGE-2237 is still "Reviewing". Neither typing has response_format, json_schema or strict. |
| 11 | completeness = a finish_reason chunk arrived | confirmed | Errors page (Jul 23) verbatim. The doc's check is `finish_reason !== undefined`, so intermediate chunks carrying `finish_reason: null` would pass it early: the emulator should omit the key mid-stream, or the contract should say "non-null". |
| 12 | incomplete streams throw nothing | confirmed | Doc comment verbatim. SDK nuance: a transport read error mid-stream DOES throw `StreamResponseError` (stream-response-wrapper.js); only a clean early end is silent. |
| 13 | 8 model ids, all ACTIVE | OUTDATED | The Forge page (Aug 3) still says this, but Anthropic's Model status (Forge's named source for latest values) shows sonnet-4-5 Deprecated on 2026-09-30, retiring 2026-11-30, plus haiku/sonnet/opus 5-5 that Forge does not list. Forge's live `list()` was not checked. |
| 14 | list() = {model,status} only | confirmed | API reference and `ModelDetails`. The docs table writes ACTIVE; the SDK type is 'active'. |
| 15 | status/retirement dates change; sonnet-4-5 date passed, haiku 10-15 | confirmed | Forge page as stated. Newer support: Anthropic deprecated sonnet-4-5 on 2026-09-30; GA changelog says 6-month notice "may not always be possible". |
| 16 | temperature+top_p together rejected (all models) | confirmed | Validation rules, "All models". README sends 0.7/0.9; three example interactors send 0.7/1. The HTTP status of the rejection is undocumented. |
| 17 | opus-4-7/4-8/5, sonnet-5 reject both params | confirmed | Validation rules list. |
| 18 | SDK validates client-side, PromptValidationError | confirmed | validators.js runs before the POST in chat/stream; `list()` is unvalidated; the content check (`typeof msg.content !== undefined`) is a no-op. |
| 19 | one llm module, key regex, family claude | confirmed | Manifest page (Jul 23). The runtime answer for a missing module is not documented (docs: lint fails); the 403 shape comes from #38. |
| 20 | 100 RPM per install across models | confirmed | Limits page: "for each installation". "to any model", set against TPM's "a single model", reads as an aggregate limit but is not explicit; whether `list()` counts is undocumented. |
| 21 | 500k TPM per install per model | confirmed | Limits page, plus changelog "per installation of your app for each model". Whether input and output tokens both count is not stated. |
| 22 | TPM 50k to 500k on 2026-10-02; 429 | confirmed | CHANGE-3497 (2026-10-02) verbatim; the SDK surfaces no Retry-After. |
| 23 | 5-min inference needs a consumer with timeout >= 300 s | confirmed | Quote verbatim. Caveat: the 180 s single outbound request cap (newer fact 2) may cut a single call first. |
| 24 | context windows by tier | confirmed | Rows Haiku 200K/64K, Sonnet 1M/128K, Opus 1M/128K. |
| 25 | consumer/scheduled default 55 s, up to 900 s | confirmed | Verbatim; same 180 s caveat for a single LLM call. |
| 26 | resolver 25 s | confirmed | The 25 s row covers user-led invocations. The realtime tutorial's "default function timeout (55s)" is inconsistent with it. |
| 27 | resolver enqueues, consumer, publishGlobal, claims | confirmed | Tutorial (Jul 23) verbatim, but the sample is exploitable (newer fact 5). |
| 28 | channels need claim-bearing tokens | confirmed | "Important:" sentence verbatim; Realtime GA adds publish/subscribe-scoped tokens. |
| 29 | no free allowance, developer pays | confirmed | Pricing page and platform-pricing "0 credits"; all environments are billed. Open: CHANGE-3135 (2026-04-04) exempts "Forge usage" on the first five sandboxes per production site, and it is not stated whether that covers LLM tokens. |
| 30 | 50/30/10 credits per 1M | confirmed | Table rows verified. |
| 31 | $0.10 in / $0.50 out per credit | confirmed | Pricing page and the platform-pricing worked example. A NEWER page (/optimise-forge-costs/, Aug 21) contradicts it with $0.0000001/$0.0000005 (doc error). |
| 32 | rates published for 3 models only | confirmed | Still 3 rows; no rate in platform pricing (Aug 12), the changelog or the GA blog. |
| 33 | LLM call bills tokens + GB-seconds | confirmed | Danielle Larregui (Staff) 2026-01-13, relaying the Forge LLM team; Adam Moore (Staff, #9) says the same; async compute billed since July 2026 (CHANGE-3328). |
| 34 | apps build their own per-user/tenant limits | confirmed | Adam Moore (Staff), RFC-117 #17, 2025-10-30. No platform per-user quota appears in the docs since; usage alerts are app-level emails at 50/75/90/100%. |
| 35 | ForgeLlmAPIError fields | confirmed | errors.d.ts and errors.js. |
| 36 | ForgeLlmAPIError not exported | confirmed, overstated | Named import is undefined at runtime (a TS compile error), and `instanceof undefined` throws. "Must branch on status/name" is too strong: `instanceof ForgeLlmError` works (exported base class; Atlassian's own example uses it), and the deep import `@forge/llm/out/errors` resolves (package.json has no "exports" map). |
| 37 | error keeps status/statusText/traceId/code/message only | confirmed | error-handling.js; the constructor never sets `context`. Nuance: a non-JSON error body becomes `err.message`; `code` falls back to the `forge-proxy-error` header, then 'UNKNOWN_ERROR'. |
| 38 | 403 FORGE_LLMS_MODEL_FORBIDDEN; staff-confirmed cause | confirmed, overstated | Shape verbatim (partner log 2026-08-02, statusText 'Forbidden'). The cause is NOT staff-confirmed: staff said only "This shouldn't be the case if your app manifest has Family defined" and noted the call came from a tunnel; the PARTNER reported the fix (updating the version from admin). No other code is documented. |
| 39 | moderation; block shape undocumented | confirmed | Overview verbatim; no page documents a block's status or code. |
| 40 | asUser default; check permissions before asApp | confirmed | Implementation details 1 and 2 of requirement 1 in both Forge tables (page Feb 19, 2026; announced as 2026 additions in CHANGE-3041; nothing newer). |
| 41 | req 13 + admin-action detail | confirmed | Verbatim, in both Forge tables. |
| 42 | req 14 actionVerb | confirmed | Verbatim. |
| 43 | req 6 no PII in logs | confirmed | Verbatim. Atlassian's own action example logs the full payload and context. |
| 44 | req 7 tenant isolation | confirmed | Verbatim. |
| 45 | agent memory per user | confirmed | remote-agents guide (Oct 9, 2026), "Advanced method"; scope is remote agents. |
| 46 | example app stale | confirmed | Raw file on main. Repo last commit 2026-01-12; that model was removed from Forge 2026-02-04 and retired by Anthropic 2026-02-19. |
| 47 | rovo:skill Preview 2026-10-02 | confirmed | CHANGE-3499 verbatim (EAP since 2026-09-10, development only). |
| 48 | SKILL.md name rules | confirmed | Verbatim. Only 1–1,024 for the description is a "must"; "at least 50 characters" and the 500-line body are guidance (the CLI warns). |
| 49 | allowed-tools covers dependencies.tools | confirmed | Verbatim. |
| 50 | rovo:mcp: 50 tools, keys < 64, one module, name <= 30 | confirmed | Module page (Oct 1). |
| 51 | MCP asUser consent caveat, asApp workaround | confirmed | Tutorial (Oct 1) verbatim; its asApp snippet has no permission check. |
| 52 | inputs never authorize; context deterministic | confirmed | Action page (Aug 3) verbatim. |
| 53 | action data cap 5 MB | confirmed | Verbatim. |
| 54 | non-GET actions need confirmation | confirmed | Sushant Bista (Staff), RFC-137-A #5, 2026-06-19 ("current thinking"); no doc page states it. |
| 55 | agent name <= 30 | confirmed | Agent page (Oct 2). |
| 56 | Rovo AI billed to the customer | confirmed | Platform pricing (Aug 12) verbatim. |
| 57 | rovo.open + must check isEnabled | confirmed, overstated | API as stated (GA 2026-03-23, CHANGE-3113). "Must check isEnabled" is not stated; the check appears only in an example. Supported in all Jira and Confluence modules plus 2 JSM modules. |
| 58 | action/web trigger/agentConnector 55 s | confirmed | Verbatim. |
| 59 | body spread + stream flag | confirmed | llm-api.js; `model` is removed from the body and goes into the URL. |
| 60 | transport | confirmed | llm-client.js. `list()` passes `{type:'llm'}` without a model; headers are x-b3-traceid, x-b3-spanid and Content-Type. |
| 61 | NDJSON parser | confirmed | The wrapper calls `flush()` at the end. Because the scan is char by char, '\n\n' never matches and lines split on '\n' only; `flush` also splits "}{"-concatenated objects. |
| 62 | skill Preview limits | confirmed | Changelog and module page. |
| 63 | input types string/integer/number/boolean | confirmed | Verbatim. |
| 64 | async 500/min; 100 KB per event above 55 s | confirmed | Page Feb 26, 2026; also 200 KB combined per push. |
| 65 | 500 events/min, 50 per push | confirmed | Verbatim. |
| 66 | Realtime 50 ops/s | confirmed | Page Jun 25; enforced since GA (CHANGE-3326). |

Verification files: `forge2/verify-llm/{fetched,txt,scripts}`. `scripts/quotes.json` holds the 66 quotes;
`txt/changelog-api.txt` holds the 400 changelog entries.
