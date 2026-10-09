# Forge 2.0 research — STABILITY and ROBUSTNESS

Fetched 2026-10-09 (all URLs below fetched that day). Method: every developer.atlassian.com page was downloaded with
curl and its content read from the page's embedded `window.__DATA__` document (the same markdown the page renders,
including note/tip/warning boxes). Quotes are the rendered text; markdown emphasis (`**`, backticks) is removed, words
are untouched. Table rows are quoted cell by cell joined with " | "; " … " joins two verbatim fragments of one page. "Page date" is the page's own metadata date. Changelog entries were read from the DAC changelog API
that backs https://developer.atlassian.com/platform/forge/changelog/ (entries cited as CHANGE-NNNN, URL
`https://developer.atlassian.com/platform/forge/changelog/#CHANGE-NNNN`, platform-wide ones at
`https://developer.atlassian.com/changelog/#CHANGE-NNNN`). SDK facts come from the npm tarballs
`@forge/events@3.0.7`, `@forge/kvs@2.0.7`, `@forge/api@8.2.0` (all published/modified 2026-10-09), extracted under
`pkgs/_forge_events`, `pkgs/_forge_kvs`, `pkgs/_forge_api` (read only, nothing executed). Community posts are cited
only when the author is badged "Atlassian Staff" unless explicitly marked as a partner observation.

Raw extracted pages: `research/raw/*.md`; changelog JSON: `research/cl/*.json`; community threads:
`research/cdac/*.json`; public Atlassian bug tracker: `research/jac/*.json`.

---------------------------------------------------------------------------------------------------------------------

## 0. Sources (page dates)

| Page | URL | Page date |
|---|---|---|
| Invocation limits | https://developer.atlassian.com/platform/forge/limits-invocation/ | 2026-09-01 |
| Async events limits | https://developer.atlassian.com/platform/forge/limits-async-events/ | 2026-02-26 |
| KVS and Custom Entity Store limits | https://developer.atlassian.com/platform/forge/limits-kvs-ce/ | 2026-09-28 |
| Scheduled trigger limits | https://developer.atlassian.com/platform/forge/limits-scheduled-trigger/ | 2025-12-01 |
| Web trigger limits | https://developer.atlassian.com/platform/forge/limits-web-trigger/ | 2025-12-01 |
| SQL limits | https://developer.atlassian.com/platform/forge/limits-sql/ | 2025-12-01 |
| Forge LLM limits | https://developer.atlassian.com/platform/forge/limits-llm/ | 2026-09-30 |
| Realtime limits | https://developer.atlassian.com/platform/forge/limits-realtime/ | 2026-06-25 |
| Object Store limits | https://developer.atlassian.com/platform/forge/limits-object-store/ | 2026-09-28 |
| App and developer limits | https://developer.atlassian.com/platform/forge/limits-app-developer/ | 2025-12-01 |
| Exceeding limits and suspended apps | https://developer.atlassian.com/platform/forge/exceeding-limits-and-suspended-apps/ | 2025-12-01 |
| Platform quotas and limits (index) | https://developer.atlassian.com/platform/forge/platform-quotas-and-limits/ | 2026-07-13 |
| Async Events API | https://developer.atlassian.com/platform/forge/runtime-reference/async-events-api/ | 2026-09-01 |
| Async events error handling | https://developer.atlassian.com/platform/forge/runtime-reference/async-events-api-error-handling/ | 2026-09-01 |
| Upgrade to @forge/events 2 | https://developer.atlassian.com/platform/forge/runtime-reference/async-events-api-version-2-upgrade/ | 2025-06-06 |
| Consumer module | https://developer.atlassian.com/platform/forge/manifest-reference/modules/consumer/ | 2026-04-03 |
| Function module | https://developer.atlassian.com/platform/forge/manifest-reference/modules/function/ | 2026-09-11 |
| Manifest (runtime) | https://developer.atlassian.com/platform/forge/manifest-reference/ | 2026-09-24 |
| Scheduled trigger module | https://developer.atlassian.com/platform/forge/manifest-reference/modules/scheduled-trigger/ | 2026-06-17 |
| Scheduled triggers (function reference) | https://developer.atlassian.com/platform/forge/function-reference/scheduled-trigger/ | 2025-02-07 |
| Scheduled trigger events | https://developer.atlassian.com/platform/forge/events-reference/scheduled-trigger/ | 2024-11-11 |
| Extending your app with a scheduled trigger | https://developer.atlassian.com/platform/forge/add-scheduled-trigger/ | 2026-05-22 |
| Trigger module | https://developer.atlassian.com/platform/forge/manifest-reference/modules/trigger/ | 2026-07-29 |
| Atlassian app events (product events) | https://developer.atlassian.com/platform/forge/events-reference/product_events/ | 2026-08-06 |
| Life cycle events | https://developer.atlassian.com/platform/forge/events-reference/life-cycle/ | 2026-04-07 |
| Events overview | https://developer.atlassian.com/platform/forge/events/ | 2026-05-22 |
| Web trigger module | https://developer.atlassian.com/platform/forge/manifest-reference/modules/web-trigger/ | 2026-04-17 |
| Web triggers (runtime) | https://developer.atlassian.com/platform/forge/runtime-reference/web-trigger/ | 2026-04-17 |
| Native Node.js runtime | https://developer.atlassian.com/platform/forge/function-reference/nodejs-runtime/ | 2026-09-21 |
| AppContext API | https://developer.atlassian.com/platform/forge/runtime-reference/app-context-api/ | 2026-10-06 |
| Use a long-running function | https://developer.atlassian.com/platform/forge/use-a-long-running-function/ | 2025-10-16 |
| Queue app interactions with storage API | https://developer.atlassian.com/platform/forge/storage-api-limit-handling/ | 2026-08-21 |
| Key-Value Store | https://developer.atlassian.com/platform/forge/storage-reference/kvs/ | 2026-08-21 |
| KVS API | https://developer.atlassian.com/platform/forge/storage-reference/kvs-api/ | 2026-08-21 |
| KVS query | https://developer.atlassian.com/platform/forge/storage-reference/kvs-api-query/ | 2026-08-21 |
| KVS batch | https://developer.atlassian.com/platform/forge/storage-reference/kvs-batch/ | 2026-08-21 |
| KVS transactions | https://developer.atlassian.com/platform/forge/storage-reference/kvs-transactions/ | 2026-09-28 |
| KVS error handling | https://developer.atlassian.com/platform/forge/storage-reference/kvs-errorhandling/ | 2024-03-14 |
| Custom Entity Store | https://developer.atlassian.com/platform/forge/storage-reference/entities/ | 2026-08-21 |
| Custom Entity Store API | https://developer.atlassian.com/platform/forge/storage-reference/entities-api/ | 2026-08-21 |
| Complex query | https://developer.atlassian.com/platform/forge/storage-reference/entities-api-query/ | 2026-08-21 |
| CES transactions | https://developer.atlassian.com/platform/forge/storage-reference/entities-transactions/ | 2026-09-28 |
| CES batch | https://developer.atlassian.com/platform/forge/storage-reference/entities-batch/ | 2025-03-03 |
| CES error handling | https://developer.atlassian.com/platform/forge/storage-reference/entities-errorhandling/ | 2025-03-14 |
| Custom entities (manifest) | https://developer.atlassian.com/platform/forge/storage-reference/entities-manifest/ | 2026-05-19 |
| Hosted storage data lifecycle | https://developer.atlassian.com/platform/forge/storage-reference/hosted-storage-data-lifecycle/ | 2026-09-28 |
| Forge storage REST API (transaction group, OpenAPI schema embedded) | https://developer.atlassian.com/platform/forge/rest/api-group-transaction/ | (rendered from swagger) |
| Forge storage REST API (KVS group; same embedded schema, has KEY_CONFLICT) | https://developer.atlassian.com/platform/forge/rest/api-group-key-value-store/ | (rendered from swagger) |
| Debugging (log limits) | https://developer.atlassian.com/platform/forge/debugging/ | 2026-08-30 |
| Logging guidelines | https://developer.atlassian.com/platform/forge/logging-guidelines/ | 2026-05-22 |
| Monitor invocation metrics | https://developer.atlassian.com/platform/forge/monitor-invocation-metrics/ | 2026-07-13 |
| Shared responsibility model | https://developer.atlassian.com/platform/forge/shared-responsibility-model/ | 2026-08-01 |
| Optimise Forge platform costs | https://developer.atlassian.com/platform/forge/optimise-forge-costs/ | 2026-08-21 |
| Forge platform pricing | https://developer.atlassian.com/platform/forge/forge-platform-pricing/ | 2026-08-12 |
| Forge bridge invoke | https://developer.atlassian.com/platform/forge/apis-reference/ui-api-bridge/invoke/ | 2026-06-19 |
| Realtime error handling | https://developer.atlassian.com/platform/forge/realtime/error-handling-for-realtime-methods/ | 2026-06-25 |
| LLM streaming errors | https://developer.atlassian.com/platform/forge/runtime-reference/forge-llms-api-errors/ | 2026-07-23 |
| LLM + Realtime long-running tutorial | https://developer.atlassian.com/platform/forge/llm-long-running-process-with-forge-realtime | 2026-07-23 |
| Assets import tutorial (job chaining) | https://developer.atlassian.com/platform/forge/queue-events-with-async-events-api-to-import-assets/ | 2026-04-17 |
| Forge SQL query policy | https://developer.atlassian.com/platform/forge/storage-reference/sql-query-policy/ | 2026-06-17 |
| Jira Cloud rate limiting | https://developer.atlassian.com/cloud/jira/platform/rate-limiting/ | 2026-10-09 |
| Confluence Cloud rate limiting | https://developer.atlassian.com/cloud/confluence/rate-limiting/ | 2026-10-09 |

---------------------------------------------------------------------------------------------------------------------

## 1. Invocation time limits per module type

Source: Invocation limits (2026-09-01), table "Additional invocation limits". Intro: "An app can be invoked by users,
web triggers, or scheduled triggers. The following limits apply to all three:"

| Row (verbatim) | Limit | Description (verbatim) |
|---|---|---|
| Runtime seconds (also includes UI modules invoked by Forge Remote) | 25 | Maximum runtime permitted before the app is stopped. |
| Runtime seconds (events invoked by Forge Remote) | 5 | Maximum runtime permitted before the app is stopped. This applies to remote back ends receiving events from the Atlassian platform. |
| Runtime seconds (async events and scheduled trigger module) | 900 | This applies to function modules that are only referenced by consumer or scheduled trigger modules. Default timeout is 55 seconds. Use timeoutSeconds to extend it. |
| Runtime seconds (web trigger, action and rovo:agentConnector modules) | 55 | Maximum runtime permitted before the app is stopped. |
| Single outbound request timeout (async events) | 180 | Maximum time a single outbound request can take before being terminated. Outbound requests refer to fetch requests, including both Atlassian app REST API and external API requests. This limit can only be reached using long-running functions. |

- Async Events API page: "One use case for async events is an app with a function that requires more than the Forge
  function maximum of 25 seconds to run, such as an AI client or import function."
- Function module `timeoutSeconds` (integer, optional): "The maximum timeout, specified in seconds. This applies only to
  the scheduled triggers module and the consumer of an asynchronous event. If this parameter is set for a function used
  by multiple modules, the function timeout will be the lowest timeout value among all modules. This parameter does not
  affect functions that are not used as asynchronous event consumers or scheduled triggers. Valid range: 1 and 900
  seconds" — example: "the function will timeout after 120 seconds, because that’s the lowest value."
- Life cycle events: "The pre-uninstall invocation has a timeout of 55 seconds, during which the uninstallation process
  will be paused."
- History: CHANGE-487 (2021-11-15) runtime extended to 25 s; CHANGE-1199 (2023-10-10) 55 s for consumer-only
  functions; CHANGE-2288 (2025-01-28) "Forge functions that are handling async events can specify a timeout of up to 15
  mins (900 seconds)."; CHANGE-2635 (2025-06-26) "We have increased the default timeout for forge functions used in both
  the webtrigger and scheduledTrigger modules from 25 seconds to 55 seconds. Additionally, developers can now specify a
  timeoutSeconds value for functions used in the scheduledTrigger module, with a maximum limit of 900 seconds.";
  CHANGE-2534 (2025-06-03) outbound request timeout for long-running functions raised "from 55 seconds to 180 seconds".
- Long-running guide: "All long-running functions must be invoked by an async event consumer." (contradicted in practice
  by scheduled triggers also accepting timeoutSeconds — see §17).
- Remaining time: AppContext API `invocationRemainingTimeInMillis()` — "The number of milliseconds remaining before this
  function will time out. This information can be useful for long-running functions." (CHANGE-2277, 2025-01-28.)
  SDK (`@forge/api` 8.2.0 `out/api/runtime.js`): `const invocationRemainingTimeInMillis =
  runtime.lambdaContext.getRemainingTimeInMillis ??` — i.e. backed by the AWS Lambda context.
- Product-event `trigger` functions: NOT explicitly listed in the table. The first row (25 s) is the generic one; the
  `timeoutSeconds` doc says it does not affect them. Treat 25 s as the most likely value but UNVERIFIED (see §19).
- Timeout error text in logs (CHANGE-2474, 2025-04-16): "Function timed out. Limit of 25.02 seconds" and "Function was
  terminated with SIGKILL. This may indicate running out of memory."

## 2. Memory, CPU, disk, payloads

- Invocation limits: "Memory | 1,024MB | Available memory per invocation. Default memory limit is 512MB. To change it,
  use the memoryMB setting of the runtime property." Note box: "If your app is still running on the previous runtime
  version, it only has 128MB of available memory per invocation."
- Manifest `app.runtime.memoryMB`: "The default amount of memory available to all the functions at runtime. Increasing
  the function memory also increases its CPU allocation. The default value is 512 MB. The value can be between 128 MB and
  1,024 MB. You can override the memory available for individual functions by setting at the function module
  definition." Function module `runtime`: "Override Forge runtime configuration defined in app.runtime. For now, only
  memoryMB property is allowed." (CHANGE-2584, 2025-05-28, CLI 11.5.0.)
- "Ephemeral disk space | 512MB | Available space in the /tmp directory. Data is only guaranteed for the duration of a
  single invocation and may be cleared between executions. Don't write tenant-specific data to this directory in a way
  that persists after the invocation finishes."
- "Payload size | 5MB | The maximum request payload size for an invocation."
- "Front-end invocation request payload size | 500KB | The maximum request payload size for a front-end invocation (for
  example, invoke and invokeRemote via @forge/bridge)."
- "Front-end invocation response payload size | 5MB | The maximum response payload size from a front-end invocation".
- Runtime: Node.js 22 and 24 ("This runtime supports Node.js 22 and Node.js 24."); `nodejs24.x` recommended; Node 20
  "reached end-of-life on April 30, 2026" (note box) and CHANGE-3209 (2026-05-06) "Node.js 20 runtime is no longer
  supported for Forge apps".
- Architecture `arm64` | `x86_64` (default x86_64).

## 3. Cold starts, warm reuse, unawaited work

- No Forge page publishes a cold-start latency figure. Only mention: Monitor invocation metrics — "This invocation time is
  measured from inside the AWS lambda, and doesn't include cold start, but it includes the time it took for the lambda
  initialization phase to complete." Note: "Invocation response time doesn’t include code executing in a Custom UI
  iframe, but includes functions invoked by @forge/bridge."
- Warm reuse (Shared responsibility model, Tenant safety): "Keep data in memory only within an invocation context. Do not
  write tenant-specific data to module-level (global) variables — the Forge runtime may reuse a warm execution process
  across multiple tenant invocations without clearing module-level state. Data stored in global variables during one
  tenant's invocation can persist into the next invocation, which may belong to a different tenant." Also: "If you use
  in-memory caches, always partition them by a tenant identifier such as cloudId."
- Node.js runtime, Developer responsibilities: "Your app must not persist customer data or sensitive content in global
  state, in memory or on disk, between subsequent invocations." and "Your app must not persist tenant-related data in
  global variables. This includes registering callbacks to be executed later if they are stored in a global queue."
- Delayed code execution: "The latest Forge runtime might keep executing the code after the function returns." /
  "timers and other asynchronous code may continue executing even after the Forge function returns a response."
- WARNING box (Node.js runtime page): "Unawaited promises don't necessarily run to completion during the invocation that
  created them. The runtime can suspend them when the handler returns, and resume them only when the environment is
  reused for a later invocation, immediately before that invocation's handler runs. This means: Execution timing is
  non-deterministic. The unawaited work might run seconds or minutes later, or never, depending on when (or whether) the
  environment is reused. Logs from this resumed code can be lost or misattributed ... Any data the resumed code reads or
  writes can belong to a different tenant's invocation than the one that scheduled it, risking tenant data isolation.
  ... Always await asynchronous operations before your handler returns. For work that must continue after the response
  is sent, use the Async Events API instead of unawaited promises or timers."
- Atlassian Staff (KamilKozlowski, 2025-07-17, https://community.developer.atlassian.com/t/questions-about-the-new-concurrency-limits-async-events/93837):
  "We’ve seen cases before where unresolved promise led to function timeouts, triggering the retry."
- "Forge runtime metadata not found" -> wrap callbacks with `bindInvocationContext` (exported from `@forge/api`).

## 4. Invocation rate limits (user-led: UI resolvers and web triggers)

Invocation limits page: "User-led invocations have the following rate limits, which are applied on a fixed one-minute
window:"
- "Per user | 1,200 per minute | Maximum number of invocations per user on a single installation"
- "Per install | 7,000 per minute and 300 per second (whichever is hit first) | Maximum number of invocations across all
  users on a single installation"
- "If a request is rate limited (429), wait until the current window resets before retrying."
- UI: `invoke(functionKey, payload, { rateLimitProperties: true })` returns `{ body, metadata }`; metadata.
  rateLimitProperties = `rateLimitValue`, `rateLimitRemaining`, `rateLimitReset` ("The time (in seconds since epoch) when
  the rate limit window resets"). Without the third argument invoke returns the body directly (CHANGE-3314, 2026-08-13).
- Web triggers: response headers "X-Ratelimit-Limit", "X-Ratelimit-Remaining", "X-Ratelimit-Reset: The time (in seconds
  since epoch) when the rate limit window resets".
- CHANGE-3420 (2026-09-01): "Per app installation: The limit is changing from 5,000 requests per minute (RPM) to 300
  requests per second (RPS) or 7,000 RPM (whichever is hit first)." / "Per environment: The previous global limit of
  30,000 RPM is being removed" / "Per user: The limit remains unchanged at 1,200 RPM." / "If your app frequently performs
  high-burst activities, you may need to implement retry logic or optimize your function calls to stay within the 300
  RPS threshold."
- CHANGE-3006 (2026-01-13): planned 20 RPS per-user change cancelled; "Invocation rate limits will continue to be 1,200
  per one-minute fixed window." and these limits are "unrelated to the upcoming Jira/Confluence point-based rate limits".
- Older (superseded?) CHANGE-1596 (2025-03-01): web-trigger invocations "capped at 20000 requests per 60 seconds. This
  limit is based on the path in the WebTrigger URL." The 2026-09-01 limits page lists no such per-path limit (§17).
- Other per-app ceilings: "Egress requests | 50,000 requests per minute, per app for egress calls"; "Network requests |
  3,000,000 requests per minute, per app and 100,000 requests per minute, per app, per tenant" (includes
  requestJira/requestConfluence).
- Per-invocation egress: "Egress requests | 100 per runtime minute (rounded up) | Number of network requests per
  invocation, excluding those made using requestJira or requestConfluence. The limit is calculated based on the function
  timeout, specified by timeoutSeconds, rounded up per minute. A function without a timeout declared is limited to 100
  requests. A function with timeoutSeconds: 90 (a minute and a half) is limited to 200 requests."

## 5. Async events (Queue / consumer)

### 5.1 Push limits (per installation) — Async events limits page
"The limits listed below apply to the Async events API for each installation of your app."
- "Event per request | 50 | Maximum number of events pushed in a single request."
- "Event per minute | 500 | Maximum number of events pushed in one minute." (raised from 100 in CHANGE-574, 2022.)
- "Payload size | 200 KB | Maximum combined payload size of events in single request."
- "Retry data size | 4 KB | Maximum size of retryData. This will be enforced from Nov 13, 2025."
- "Payload size for long running functions | 100 KB | Maximum size of an individual event. This limit only applies to
  functions specifying a timeout greater than 55 seconds."
- "Retry data size for long running functions | 4 KB"
- "Cyclic invocation limit | 1000 | An event resolver can push more events to the queue, which may trigger further event
  handlers in a chain. This limit applies to the total number of async event push requests that can be made across all
  handlers originating from a single initial function invocation, including requests made by downstream handlers. Each
  push request can contain up to 50 events. For example, a function that calls push 1000 times in a loop, or a chain
  where each handler calls push once for 1000 hops, both reach this limit."
- Exceeding: "If your app needs higher storage capacity or a higher cyclic invocation limit, you can contact us through
  Developer and Marketplace support." (Exceeding limits page.)

### 5.2 Push API
- "The events processing can be delayed up to 15 minutes using the delayInSeconds setting. A maximum of 50 events can be
  pushed per request, up to a maximum combined payload of 200 KB."
- Shape: `PushEvent { body: Body; delayInSeconds?: number; concurrency?: { key: string; limit: number } }`,
  `PushResult { jobId }`. Errors "Event must be an object." / "Event body must be an object."
- Error classes (error-handling page): "PartialSuccessError | Some pushed events were not recorded for later processing.
  Each event can have a different reason for failure. To get error details for failed events, inspect the error's
  failedEvents property here." / "RateLimitError | The total number of events pushed per minute exceeds the defined
  limits. To overcome this, retry adding events after a minute." / "TooManyEventsError | More than 50 events were pushed
  to the queue in a single request." / "PayloadTooBigError | The combined payload of events pushed in a single request
  exceeded 200 KB." / "InvalidPushSettingsError | The delayInSeconds setting of a pushed event is outside the supported
  range of 0 to 900 seconds." / "InvalidQueueNameError | The queue name is invalid. A valid queue name is alphanumeric
  string, and can start with _." / "InvocationLimitReachedError | An event resolver can push more events to the queue,
  creating a cycle. This error means an event pushed another event into the queue more than 1000 times. To avoid this,
  process more events in parallel."
- SDK `@forge/events@3.0.7` (`out/validators.js`, `out/queue.js`, `out/text.js`) — verified from code:
  - `const MAXIMUM_EVENTS = 50;` `const MAXIMUM_PAYLOAD_SIZE_KB = 200;` payload measured as
    `Buffer.byteLength(JSON.stringify(events)) / 1024`.
  - delay check: `(event.delayInSeconds && event.delayInSeconds > 900) || (event.delayInSeconds && event.delayInSeconds < 0)`
    → non-integers (the docs' own example uses `delayInSeconds: 0.5`) pass client validation.
  - `VALID_QUEUE_NAME_PATTERN = /^[a-zA-Z0-9-_]+$/` with text "Queue names can only contain alphanumeric characters,
    dashes and underscores." (differs from the docs wording, §17).
  - `const jobId = (0, crypto_1.randomUUID)();` — the job id is minted CLIENT-SIDE per push call. A producer that is
    itself retried mints a NEW job id for the same logical work → jobId is not an idempotency key across producer retries.
  - HTTP mapping: 201 success; 202 → PartialSuccessError (with `failedEvents`); 413 → PayloadTooBigError; 429 →
    RateLimitError ("Too many requests."); 405 → InvocationLimitReachedError ("The limit on cyclic invocation has been
    reached."); getStats 200, cancel 204, 404 → JobDoesNotExistError.
  - Endpoints: `/webhook/queue/publish/{contextAri}/{environmentId}/{appId}/{appVersion}`, `/webhook/queue/stats/...`,
    `/webhook/queue/cancel/...`, called through `@forge/api` `__requestAtlassianAsApp`.
  - No client-side validation of `concurrency.limit` (no documented maximum).
  - 3.0.0 major change was only "Adds support for TypeScript 5" (package CHANGELOG.md) — API equals the 2.x docs.

### 5.3 Consumer
- Manifest: `consumer: [{ key, queue, function }]` + `function: [{ key, handler, timeoutSeconds }]`. `resolver` property
  is "Deprecated. Used with @forge/events version 1.x." Consumer receives one argument `AsyncEvent` with `body, jobId,
  eventId, queueName, delayInSeconds?, concurrency?, retryContext?`. "There is no payload property; if you read
  event.payload you get undefined."
- "The optional timeoutSeconds parameter of the function module specifies the maximum runtime for an async event consumer
  function, enabling it to run longer than the default of 55 seconds. You can specify up to 900 seconds (15 minutes)."
- crossVersion: consumer property "temporary, and will be removed after 21 November 2025". CHANGE-2526 (2025-05-21):
  "We've fixed an issue where Forge async events were not delivered if the app version linked to the pushed async event
  was upgraded before event processing began." ... "With this fix, when an async event processing starts, the event will
  no longer be checked against the app’s version." ... "Note: After 2025-11-21, the crossVersion: true property will be
  removed, and by default, the event will no longer be checked against the app’s version." ⇒ events pushed by version N
  can be consumed by version N+1 code.

### 5.4 Delivery guarantees, retention, retries
- "Async Events that are successfully enqueued are guaranteed to be delivered at least once within a defined retention
  window."
- "It begins when the Async Event is successfully enqueued and lasts for 24 hours."
- "Retention window is extended due to: processing overhead caused by performance degradation of the Forge platform,
  retries triggered by platform level errors. Retention window can be extended by another 72 hours, to a total of 96
  hours."
- "If the retention window is exceeded due to app level errors, the Async Event is dropped."
- "If the retention window is exceeded due to platform level errors, the Async Event is not dropped but will no longer be
  retried automatically. Atlassian may intervene manually, which could include raising an incident or filing a public
  bug."
- "Async Events are automatically retried within the retention window until they are successfully delivered. Retries use
  exponential backoff, with intervals reaching up to approximately 15 minutes between attempts."
- "An event is considered successfully delivered if the invocation does not result in a retry request, app level errors,
  or platform level errors."
- "App-level errors occur when event processing fails due to issues in the Forge app’s code or setup, such as: runtime
  error, function time out, function out of memory, remote endpoint network error, insufficient permissions, reaching
  invocation limits."
- "Platform-level errors are failures caused by internal issues within the Forge platform, such as internal server
  errors, internal platform rate limiting."
- RetryContext: `retryCount` — "The number of retry attempts for this event. Platform-level errors do not increment
  this."; `retryReason` (string); `retryData`; `retentionWindow { startTime (ISO-8601), remainingTimeMs }` — "Remaining
  time in milliseconds before the retention window expires, including any extensions." "The retryContext property is
  only populated when the event is being retried, so check that it exists before reading it. Otherwise, destructuring it
  on a first delivery throws a TypeError."
- Retry request: "You can request a retry of an Async Event by returning an InvocationError object." Note: "You can
  request Async Event retries for as long as the retention window is not exceeded." `retryAfter` — "The initial delay
  before the Async Event retry processing starts. The maximum retryAfter value is 900 seconds (15 minutes). Any
  retryAfter values exceeding this limit are lowered to 900 seconds." `retryData` — "The maximum retryData size is 4KB."
  `InvocationErrorCode` values documented for apps: FUNCTION_RETRY_REQUEST, FUNCTION_UPSTREAM_RATE_LIMITED. SDK enum also
  has FUNCTION_OUT_OF_MEMORY, FUNCTION_TIME_OUT, FUNCTION_PLATFORM_UNKNOWN_ERROR, FUNCTION_PLATFORM_RATE_LIMITED.
- SDK: `InvocationError` constructor `return this.toJSON();` → `{ _retry: this._retry, retryOptions: this.retryOptions }`
  with `_retry = true`; `retryAfter <= 0` is raised to `MIN_RETRY_AFTER = 1`; default `{ retryAfter: 1, retryReason:
  FUNCTION_RETRY_REQUEST }`. The platform therefore recognises a retry request by the RETURNED object shape, not by a
  thrown error.
- v2 upgrade page: 1.x "Async Events were retried 4 times." vs 2.0.0 "Async Events are retried until the retention window
  is exceeded." CHANGE-2642 (2025-07-01): "Previously, there were four async events retries available in total. Now,
  async events are automatically retried within the retention window until they are successfully delivered." CONFLICT:
  the storage-limit guide note still says "with a maximum of four retries" (§17).
- Public bugs: ECO-734 (fixed 2025-07-09) retryContext missing for non-object payloads; ECO-759 (status "Gathering
  Impact", created 2025-04-02): "the Async Event may not be retried if the total size of the Async Event payload and
  retryData exceeds 200 KB." / "We recommend that the total size of the event payload and retryData shouldn't exceed
  200 KB." (https://jira.atlassian.com/browse/ECO-759)

### 5.5 Concurrency control
- "You can control the concurrency of event processing by setting the concurrency field of the PushEvent argument to
  Queue.push(). This limits the number of events that can be processed concurrently."
- "Concurrency counters are implicitly scoped to an app installation, but not to a specific queue, so a specific key can
  be used to control concurrency across multiple queues."
- Note: "If the concurrency field is not provided then there is no concurrency control. Event processing will be
  unbounded and limited only by the general per-installation invocation limits."
- Tip: "We recommend setting limit to a fixed value for all events with the same concurrency key. This ensures that the
  concurrency limit is consistent across all events, which helps avoid unexpected behavior."
- CHANGE-2639 (2025-06-24): "a basic ability for consumers of async events to control the rate at which events are
  consumed by setting a limit of how many events can be processed concurrently for an app-defined key."
- Atlassian Staff (KamilKozlowski, 2025-07-16, thread 93837): "when Async Events cannot be delivered (in other words, the
  function cannot be invoked) we retry with an exponential backoff. We retry on multiple occasions, rate limiting due to
  configured concurrency is one of the scenarios." / "The maximum delay in-between the retries is 15 minutes." /
  "There’s currently no limit to the number of unprocessed events that use the concurrency setting. The one case you may
  run into when pushing too many events with a limit=1 is that some of the events from this batch may run out of
  retention window eventually. Reaching the invocation limits does not extend it."
  The same thread's opening post (partner observation, limit=1, 5 events) logged order "Event 4, 1, 0, 3, 2" with 4–16 s
  gaps.
- NOT documented anywhere: maximum `limit` value, maximum number of distinct keys.

### 5.6 Ordering
- No Forge doc states an ordering guarantee. Atlassian Staff RFC-107 (2025-09-16,
  https://community.developer.atlassian.com/t/rfc-107-forge-fifo-queues/95373): "Currently, the Async Events API allows
  apps to push events to a queue for later processing. This, however, doesn’t guarantee the order of events, making it
  unusable for workflows or integrations that require strict ordering." The proposed `fifoConsumer` does not appear in
  the changelog (searched "FIFO", "fifoConsumer") and `/manifest-reference/modules/fifo-consumer/` returned no document
  on 2026-10-09 ⇒ not shipped.
- Atlassian Staff (JoshuaHwang, 2023-11-05, https://community.developer.atlassian.com/t/the-async-events-queue-is-not-a-queue/74415):
  "This is expected behaviour. It works similar to an SQS without FiFo on."

### 5.7 Jobs / progress / cancel
- "When you push events to the queue, a new job is created. This job's id is returned from the push API."
  `queue.getJob(jobId).getStats()` → `{ success, inProgress, failed }` (v2: returns stats directly, not an APIResponse).
- "When a job is canceled, any events from that job that have not yet started processing, including retries, will not be
  processed. This does not affect function invocations that are already being executed at the time of cancellation."
- SDK also has `queue.cancel(jobId)` and `queue.getStats(jobId)` shortcuts; 404 → `JobDoesNotExistError` "The job
  ${jobId} was not found for the queue ${queueName}."
- NOT documented: what makes an event count as `failed` (exhausted retention? any failed attempt?), how long job stats
  are retained, whether stats are eventually consistent, whether getStats/cancel calls are rate-limited.

## 6. Long work: chaining, checkpointing, remaining time

- Long-running guide: "By default, async event consumers time out after 55 seconds. Use cases that required longer
  computation have previously had to rely on breaking the task into multiple steps or batches, and queuing multiple
  events to do the work. You can now configure a timeout of up to 900 seconds (15 minutes) which will allow many such use
  cases to be performed in a single invocation."
- Storage limit guide (the official pattern for bursts): "Instead of calling kvs.set directly inside the Atlassian app
  event handler, we will push the events to the queue." Sample consumer computes
  `retryDelay = (baseDelay * (2 ** event.retryContext.retryCount)) + randomJitter;` and returns `new InvocationError({
  retryAfter: retryDelay, retryReason: InvocationErrorCode.FUNCTION_RETRY_REQUEST, retryData: {...} })`. Strategy text:
  "This example's strategy uses exponential backoff with jitter retry to minimize retry collisions."
- KVS limits page: "If an installation of your app exceeds these limits due to bulk processing (for example, triggered by
  a bulk issue update in Jira), consider using the Async events API to queue your app's interactions with the KVS and
  Custom Entity Store."
- Assets import tutorial (job chaining): "The most notable limitation of the Async Events API in our use case is the
  1000-depth limit for cyclic invocations." Its consumer pushes the next page event then later waits for job completion
  with `getStats()` and re-pushes itself with `delayInSeconds` while `inProgress`. NB: the tutorial still uses the
  deprecated 1.x resolver consumer (`resolver.define(..., async ({ payload, context }) => ...)`, `context.jobId`),
  does not `await` its `push` calls, and keeps the job list with a read-modify-write `kvs.get` + `kvs.set` — i.e. the
  official tutorial contains the unawaited-push and lost-update shapes the rest of the docs warn against.
- LLM tutorial: "When building Forge apps with large language models (LLMs), some prompts or agentic workflows may exceed
  the default function timeout (55s). To handle these, offload work to a queue consumer (up to 15 minutes) and stream
  results back to the user interface (UI) using Realtime publish/subscribe. This pattern avoids storage-polling latency,
  reduces client/server round-trips, and keeps your macro responsive."
- Cost guide: "Spreading work across more invocations through batching may make each individual invocation cheaper, but
  the total compute consumed is the same — the real saving comes from doing less work in the first place."

## 7. Product event triggers (trigger module)

- Delivery delay (note box on trigger module): "Events for trigger modules may take up to 3 minutes to reach your app
  after the triggering action occurs. This delay is part of how the Atlassian platform processes events internally —
  events are placed into a queue and delivered asynchronously rather than in real time."
- Retries: app-level via returned `InvocationError` — note "You can only retry an event for a maximum of four times." and
  "retryCount: number of times (max 4) the app requested for retry". retryAfter max 900 s, retryData 4 KB ("Because of the
  bug (ECO-734), retryData should be of an object type, otherwise it is not sent on retry.").
- Platform-level: "Platform level errors cannot be captured by the app. Examples include timeouts and Out of memory(OOM)
  errors. If a platform error occurs, the Forge platform will automatically retry the event on behalf of you." retryReason
  values FUNCTION_OUT_OF_MEMORY, FUNCTION_TIME_OUT, FUNCTION_PLATFORM_RATE_LIMITED, FUNCTION_PLATFORM_UNKNOWN_ERROR.
  CHANGE-681 (2022-07-04): "The Forge platform will now retry product event triggers that did not execute successfully.
  This includes errors such as timeouts, out-of-memory errors and app-level errors."
- No "at least once" statement exists for product events; no ordering statement. Partner bug ECO-1403 (created
  2026-03-27, closed "Fixed" 2026-06-09, https://jira.atlassian.com/browse/ECO-1403): Confluence update events delivered
  "~70 minutes" late and "Event B, which was created ~28 seconds before Event A, was delivered ~3 minutes after Event A";
  the reporter commented on closure "this was not solved."
- Self events: "If you don't use the ignoreSelf filter, self-generated events will still be delivered to your app with a
  selfGenerated property set to true in the event payload." Cost guide: "A common source of wasted invocations is an app
  that modifies a Jira entity (for example, updating an issue field) and then receives and processes the event that its
  own update generated. This creates unnecessary work and can even cause feedback loops."
- Filters: `appIsLicensed`, `ignoreSelf`, `expression` (Jira-expression subset), `onError` (IGNORE_AND_LOG default,
  IGNORE, RECEIVE_AND_LOG, RECEIVE). Expression non-boolean result ⇒ "considered an error and result in the event being
  filtered out". Entity-property filtering: "5 paths total"; "Filters are evaluated against recently stored entity
  property values that could be up to a few seconds older than the event generation time".
- Cascading deletes (Events page): "We only emit a delete event for top-level entities." "When a project is deleted, we
  won't emit separate delete events for its child issues, associated boards, versions, components or issue types."
- Install race (Life cycle events): "This process is eventually consistent, and the installed event may be sent before
  the app is granted the permissions needed to call certain APIs using .asApp(). For large sites, it may take a very long
  time (minutes) for your app user to be fully granted the requisite permissions." Recommendation: "If you receive an
  unexpected authorisation error when calling an Atlassian API (typically a 401 or 403 HTTP status code), you should use
  Forge's retry handling to reattempt the request after a delay."
- Upgrade event: "An event with the name avi:forge:upgraded:app is sent when an installed app on a site has been upgraded
  to a new major version. This event is not sent for minor or patch version upgrades."

## 8. Scheduled triggers

- Limits: "Total number of scheduled trigger modules in an app | 5"; "Total number of scheduled trigger modules with
  fiveMinute intervals in an app | 1". Intervals `fiveMinute`, `hour`, `day`, `week` (fiveMinute GA CHANGE-2444,
  2025-03-31).
- Start: "Each trigger is scheduled to start shortly after it is created, about 5 minutes after app deployment."
- Reset: "Every time any changes are made to any scheduled triggers module, all scheduled triggers will be recreated and
  their start times reset."
- Errors: "If the function throws an error, nothing will happen, and the function invocation will not be retried. The
  function will be invoked the next time the schedule is due." (FAQ repeats; note box "Errors don't trigger retries").
- Duplicates: "There is a small chance of duplicated invocations, such scenarios should be handled in the apps code by the
  app developer." Atlassian Staff (Nir, 2021-05-13, https://community.developer.atlassian.com/t/forge-scheduled-triggers-have-been-upgraded/47905):
  "There is a small chance of duplicated invocations, your app should expect at least one invocation."
- Distribution: "invocations will be distributed in batches evenly across the interval specified on any given module.
  Distribution is done by installations, so not all installations of an app will have their triggers invoked together.
  This is however a consistent distribution, meaning that if an hourly trigger invokes at 1:10 for a particular
  installation, and at 1:20 for another, those installations will invoke again at 2:10 and 2:20 respectively."
- Context: "Scheduled triggers run without a user context"; "The event parameter does not have a value for scheduled
  triggers"; request carries `context.cloudId`, `context.moduleKey`, `contextToken`.
- Timeout: default 55 s, `timeoutSeconds` up to 900.
- License filter: `filter.appIsLicensed: true` skips unlicensed installations (CHANGE-3260, 2026-06-17).
- Return value: CONFLICT (§17) — module page "If a function invoked from a scheduled trigger returns a value, it is
  ignored." vs function reference "your function must return a response in the format expected by the platform" ... "If
  the response from your app does not follow that structure, the platform records an error with a status code of 424
  Failed dependency." / "The platform recognizes a status code of 204 as success, and status codes in the 500 series as
  errors."
- Missed runs: no documented catch-up. Partner reports (not staff-confirmed): hourly trigger "many are down to 2
  invocations per 6 hours" (https://community.developer.atlassian.com/t/missed-scheduled-trigger-invocation/78982,
  2024-04-08); a fiveMinute trigger "stopped invoking with no error, no log line" for ~36 h
  (https://community.developer.atlassian.com/t/scheduled-trigger-stops-firing-silently-survives-every-documented-remediation-then-resumes-on-its-own-36h-later/102177,
  2026-08-14; staff reply only asks for an ECOHELP ticket).
- Overlap (a run still going when the next is due, e.g. 900 s function on fiveMinute): NOT documented.

## 9. Web triggers

- Runtime 55 s (§1). Must return `statusCode` ("Despite being type: dynamic, the web trigger function response still
  requires a statusCode field."); "If the function result is not compatible with the JSON format, then an error response
  with status code 500 is sent."
- Auth: "Web trigger URLs are publicly available and are not authenticated by the Forge platform. Atlassian user
  information is not attached to invocations, which means asUser API calls will not work in web trigger functions."
- Static vs dynamic response types; only static eligible for Runs on Atlassian.
- Rate-limit headers X-Ratelimit-* (§4). Web trigger management limits (Get 1,000/min; Create 500/min; Delete 500/min).
- Cost guide: "there is no flagfall cost or network charges for invoking web triggers, but the web trigger function
  runtime is still billed as compute."

## 10. KVS and Custom Entity Store (CES)

### 10.1 Consistency and conflicts
- KVS page: "The kvs.query method used by the Key-Value Store is eventually consistent. This means that the method returns
  data that may be slightly out of date." / "The kvs.get method, on the other hand, is strictly consistent. It will always
  return current data." CES page: same for `entity().query` (eventual) and `entity().get` (strict).
- "By default, Forge writes to keys using set or delete use a last write wins conflict resolution strategy. You can
  override this behaviour using the keyPolicy option." / "Writes to individual keys are atomic. Values are either updated
  in full or not at all."
- keyPolicy: "FAIL_IF_EXISTS: if the key already exists, don't overwrite it." / "OVERRIDE: always write data, regardless
  of whether it already exists or not. This is identical to the default strategy, which is last-write-wins."
  returnValue PREVIOUS|LATEST, returnMetadataFields CREATED_AT|UPDATED_AT|EXPIRE_TIME (CHANGE-3038, 2026-02-05).
- REST (OpenAPI embedded in https://developer.atlassian.com/platform/forge/rest/api-group-transaction/):
  `/forge/storage/kvs/v1/set` and `/v1/entity/set` → 409 "KEY_CONFLICT - The provided key already exists and the key
  policy is set to FAIL_IF_EXISTS"; 400 includes "INVALID_KEY_POLICY_RETURN_VALUE_COMBINATION".
- `kvs.delete`: "Deletes a value by key, this succeeds whether the key exists or not." (idempotent delete).
- `kvs.get` of a missing key returns `undefined` (SDK maps `KEY_NOT_FOUND` → undefined).
- CHANGE-3038: "The following methods also support SetOptions, but only to set a TTL: kvs.batchset, transact.set" ⇒ no
  keyPolicy inside batches or transactions (REST schema `SetOptions` = `{ ttl }` only).
- Partner (not staff) in thread 101559: "Forge KVS uses eventually consistent, so it can result in data being out of
  date." — stated as a general claim; the docs limit eventual consistency to query.

### 10.2 TTL
- "Maximum TTL: The maximum supported TTL is 1 year from the time the expiry is set."
- "Expired data deletion is asynchronous: Expired data is not removed immediately upon expiry. Deletion may take up to 48
  hours. During this window, read operations may still return expired results. If your app requires strict expiry
  semantics, request EXPIRE_TIME metadata and ignore values where expireTime is in the past."

### 10.3 Transactions (GA CHANGE-2679, 2025-07-09)
- KVS: "Transactions allow you to perform multiple operations in a single transaction, ensuring that all operations are
  either committed or rolled back together."
- CES: "Transactions allow you to perform multiple conditional operations in a single transaction, ensuring that all
  operations are either committed if all conditions are met or rolled back together." / "Each condition is checked and
  must be true; if any condition is not met, the entire transaction will fail." Operations: set (conditions optional),
  delete (conditions optional), check (conditions mandatory). Conditions use `Filter` and(...)/or(...) with the complex-
  query FilterConditions.
- CHANGE-2679: "When used in the Custom Entity Store, you can add conditions to operations." ⇒ KVS-only (untyped) items
  cannot carry conditions (REST `SetTransactionUntypedItemSchema` = `{key, value, options}`, additionalProperties false).
- Limits: "Transactions are treated as a single Write operation, subject to the rate limits ... The transaction will fail
  if it exceeds these limits, returning a TOO_MANY_REQUESTS error." / "Each transaction can contain a maximum of 25
  operations." / "Each key can only be used once in a transaction." / "Each transaction is limited to a payload size of
  4MB."
- REST transaction 400 codes include "INVALID_TRANSACTION_REQUEST - Transaction request must include one of set, check or
  delete." and "CONDITIONAL_CHECK_FAILED - Request failed due to conditional check specified or optimistic locking." Success
  204 "Successfully completed all the node operations".
- SDK `@forge/kvs@2.0.7` signature (`out/interfaces/transaction.d.ts`): `set<T>(key: string, value: T, entity?:
  EntityConditions<T>, options?: SetOptions): this;` `delete<T>(key, entity?)`, `check<T>(key, entity:
  EntityRequiredConditions<T>)`. Implementation reads only `entity.entityName` and `entity.conditions` from the 3rd arg
  (`transaction-api.js`), TTL only from the 4th. The docs' examples put `ttl` in the 3rd-arg object
  (`.set('key1', 'value1', { ttl: { unit: 'DAYS', value: 7 } })`, `{ entityName: 'employee', ttl: {...} }`) and show
  `.check('employee5', filter, { entityName: 'author', conditions })` with 3 args — in plain JS the TTL is silently
  dropped; in TS it fails to type-check. A Filter with zero conditions throws `ForgeKvsError('Builder must have at least
  one condition set')`.

### 10.4 Batch (GA CHANGE-2813, 2025-11-11)
- "These methods execute all batched requests in parallel, and will successfully complete as many as possible." / "There
  is no guaranteed order of completion. Batch operations will report which operations succeeded or failed." Result
  `{ successfulKeys, failedKeys[{key, entityName?, error{code,message}}] }`.
- "A single kvs.batchSet operation can be a payload of 4MB, similar to transactions." / "Each batch operation can contain a
  maximum of 25 keys." / "A batch operation will return an error and fail entirely if: It doesn't contain any keys. There
  are multiple requests to set, delete, or get the same key or key plus entity."
- "Batch operation requests use rate limits more efficiently and can be 5 times faster than making the individual
  requests in parallel." CHANGE-2813: batched requests "will be completed on a “best effort” basis (unlike Transactions,
  where all requests are completed in an “all or nothing” basis)".

### 10.5 Rate and size limits (per installation)
- "Request rate (RPS) | 1000"; "Read (10KB request per min) | 4000"; "Write (10KB request per min)* | 4000" (the asterisk
  has no footnote on the page).
- "Request sizes are rounded up to the nearest 10KB. Requests that are 10KB or smaller are counted as 1 request. Requests
  sized between 10KB and 20KB are counted as 2 requests. For example, a request with a payload of 65KB will be rounded up
  to 70KB, resulting in a count of 7 requests." / "10 individual writes of 1KB each will be counted as 100KB of limit use.
  However, if set in a single batch operation, it will count as 10KB of use."
- Per key: "Reads | 12 MB/s per key"; "Writes | 1 MB/s per key"; "Query | 24 MB/s per index value" (documented
  CHANGE-2539, 2025-05-16, "limits that already exist and are enforced").
- Sizes: "Key length | 500"; key regex `/^(?!\s+$)[a-zA-Z0-9:._\s-#]+$/`; "Value size | 240 KiB | Maximum size of a single
  persisted value (in RAW)"; "Object depth | 31".
- Errors: "Any request that exceeds a quota or limit will return a 429 status with an error code of RATE_LIMIT_EXCEEDED."
  Table: "TOO_MANY_REQUESTS | The app installation exceeded the maximum number of allowed operations within the specified
  period." / "REQUEST_THROTTLED | The app installation exceeded the maximum storage capacity or transfer limit." SDK
  throws `ForgeKvsAPIError` with `code`, `message`, `context`, `responseDetails{status, traceId,...}`; unparseable error →
  code "UNKNOWN_ERROR".
- CHANGE-2538 (2025-05-16): old per-operation limits replaced from 16 Nov 2025 by the unified model above.
- Recommendations: "avoid employing data models with entities that grow in an unbounded manner (in terms of depth and/or
  size), as this may lead to exceeding storage limits." / "avoid storing files."

### 10.6 Query and pagination
- KVS `query.limit`: "The query API returns up to 10 values by default, this can be increased to a maximum of 100."
  CHANGE-1916 (2024-08-26): "now return up to a 100 query results per page compared to 20 previously. Please note that the
  default number of results returned will still stay the same at 10".
- KVS where: "Queries can only target the key field." / "There may only be a single where condition for a query." / "The
  only condition supported by the Key-value store is beginsWith." Keys "are lexicographically ordered".
- Cursors: warning "Cursors are derived from underlying storage identifiers, and hence are subject to change anytime there
  is any change in how these underlying storage identifiers are created. This means that cursors are not stable and should
  not be persisted." KVS query page: "When building a query, do not persist cursors, as they may not always be stable."
  "You will have to use the same parameters as the initial query." `nextCursor` absent = last page.
- CES complex query: one index per query; `where` conditions beginsWith, between, equalTo, greaterThan, lessThan,
  greaterThanEqualTo, lessThanEqualTo; `filters` add notEqualTo, exists, notExists, contains, notContains; "each complex
  query can only have a maximum of 100 conditions"; page limit error "COMPLEX_QUERY_PAGE_LIMIT_NOT_IN_RANGE | The page limit
  must be set between 1 and 100."; `sort` ASC/DESC.
- CES schema limits: "An app can have a maximum of 20 entities"; "Each entity can have a maximum of 7 custom indexes and 50
  attributes"; index `range` — "This parameter can only have one attribute."; "The Custom Entity Store strictly enforces
  attribute types."; integers 32-bit signed; key index `by-key` free. Index build after deploy "from a minimum of 5
  minutes" and "Until the indexing process completes, you won't be able to install your app on any sites."
- SDK names (`@forge/kvs` 2.0.7 `conditions.d.ts`): `equalTo`, `greaterThan`, `greaterThanEqualTo`, ...; sort enum `Sort`.
  Doc examples use `FilterConditions.equalsTo`, `WhereConditions.isGreaterThan`, `SortOrder.DESC` — none exported.

### 10.7 Data lifecycle (robustness of state)
- Reinstall: "If an app is reinstalled, it is treated as a new installation. However, if a request is made within 21 days
  of uninstallation, the new installation can be relinked to the old data."
- License suspension: "the app becomes inactive, but the system keeps all the stored data without changes."

## 11. Forge SQL (if the app uses it)

- Per install: "Total stored data | 1 GiB (production installs)"; "DML Requests per second (RPS) | 150"; "DDL Requests per
  minute (RPM) | 25"; "Size per row | 6Mib"; "Total query execution time for all current invocations | 62.5 seconds
  (within each minute)".
- Per query: "Memory usage per query | 16 MiB"; "Request size | 1 MiB"; "Response size | 4 MiB"; "Per-connection timeout
  for SELECT queries | 5 seconds"; "Per-connection timeout for INSERT, UPDATE, and DELETE queries | 10 seconds";
  "Per-connection query timeout for DDL queries | 20 seconds".
- "Foreign keys are not supported." / "Each SQL statement can only contain a single query." Query policy rejects with
  `SQL_POLICY_VIOLATION`; `SET autocommit` not permitted; `LOCK TABLES` triggers an audit. No multi-statement transaction
  API is documented (not verified either way).
- Adding SQL to an existing app = major version upgrade (admin consent).

## 12. Logging

- "Log lines per invocation | 100 per runtime minute (rounded up) | Maximum number of log entries for an invocation. The
  limit is calculated based on the function timeout, specified by timeoutSeconds, rounded up per minute. A function
  without a timeout declared is limited to 100 log lines. A function with timeoutSeconds: 90 (a minute and a half) is
  limited to 200 log lines."
- "Log size per invocation | 200 KB | Maximum size of all log line data generated per invocation."
- "Log file size per download | 100 MB"; "Log lines per download | 96,000".
- Retention: CHANGE-1624 (2024-04-29) download window "from 60 days to 30 days".
- Frontend logs (EAP): strings truncated at 1,024 characters; 5 extra args; objects > 10,000 chars omitted.
- Behaviour beyond the backend caps (truncate vs drop vs error) is NOT documented.
- Cost: "Logs: Writes | $/GB | 1 GB | 1.005" and "Every call to console.log() contributes to billable log write volume."
- Guidelines: "Minimize the logging data you collect." / "Avoid logging personal data when possible." / "Avoid logging
  any authorization data (e.g. secrets, keys)." Table says Name/Email/Username/Session ID/User Generated Content: "No".
- Customers can disable log access: "a user may disable log access to their site, which means their logs will no longer
  appear in the developer console."
- Metrics caveat: "Metrics may not always be accurate because undelivered metrics data isn’t back-filled and data
  sampling might be used for some metrics."

## 13. Realtime and Forge LLM (robust usage)

- Realtime limit: "Operations per second | 50 (3000 events per minute) | Maximum number of requests in one second for
  each installation. Once this limit is reached, requests after will fail with errors. Apps are required to handle
  retries. We recommend using a retry backoff strategy when re-attempting failed requests." Applies to combined
  subscribe/publish/signRealtimeToken.
- "publish — returns a PublishResult object. On error, eventId and eventTimestamp will be null and errors will return a
  list of errors." (does not throw); "subscribe — returns a rejected Promise on error."; signRealtimeToken returns
  `{ token, expiresAt, errors }`. Token pre-validation errors "are not counted towards rate limiting".
- LLM limits (per installation): "Requests per minute | 100"; "Tokens per minute | 500,000" (raised CHANGE-3497,
  2026-10-02); "Inference time in minutes | 5 | The maximum time a model can process and generate responses before a
  timeout occurs, assuming the Async events API is used with a specified timeout equal or greater than 5 minutes.
  Otherwise the specified or default timeouts apply." Context windows: Haiku 200K/64K, Sonnet 1M/128K, Opus 1M/128K.
- Streaming errors: "Platform interruptions can cause streaming to conclude before a complete response is delivered." Code
  comment: "Exceptions are not thrown for finishing streams with incomplete responses." Detect via missing
  `finish_reason`; recover by prompting with prior context rather than resubmitting.
- LLM billing: "LLM usage is billed per credit with no free usage allowance."

## 14. Jira/Confluence REST rate limits — "Tier 1" dosing (the platform side of burst robustness)

(Other researchers may own tiers in depth; captured here because retry/backoff semantics interlock with async retries.)
- Three systems: "1. Points-based quota (per-hour)" / "2. Burst API rate limits (per-second)" / "3. Per-issue write
  limits". "When any limit is exceeded, Jira returns an HTTP 429 Too Many Requests response. Your app should handle this
  gracefully by respecting the Retry-After header and implementing appropriate backoff strategies."
- Points: "Each request starts with a base cost of 1 point, and additional points are added for each object involved.
  Write requests are charged only the base cost, with no additional points." Core objects (GET) 1 point; "Identity &
  access" objects 2 points; writes 1; others 1. Example: "1 (base) + 8 users = 17 points (1 + 8 × 2)".
- "All quotas are measured in points per hour and reset at the top of each UTC hour."
- "Tier 1 – Global Pool (default): Your app shares a single 65,000 point hourly quota across all tenants. This is the
  default tier for all apps." Tier 2 – Per-Tenant Pool after review (Standard 100,000 + 10 × users, Premium 130,000 + 20 ×
  users, Enterprise 150,000 + 30 × users, capped 500,000). Confluence page has the same Tier 1/Tier 2 table.
- Burst: "Jira implements Burst API Rate Limit using the token bucket algorithm." "For each tenant, Jira maintains a
  separate token bucket for every API endpoint." Defaults "GET | 100", "POST | 100", "PUT | 50", "DELETE | 50" requests
  per second (plus endpoint overrides, e.g. "GET | /servicedeskapi/servicedesk/{servicedeskid}/customer | 5", "GET |
  /api/{version}/issue/{issueidorkey} | 150"). "Your app should be designed around the steady-state refill rate, not the
  burst buffer."
- Per-issue writes: "Short window: 20 write operations per 2 seconds" / "Long window: 100 write operations per 30
  seconds" → reason `jira-per-issue-on-write`.
- Reasons: `jira-quota-global-based`, `jira-quota-tenant-based`, `jira-burst-based`, `jira-per-issue-on-write`.
  Headers: `Retry-After` ("Only returned with 429 responses. Indicates how many seconds to wait before retrying."),
  `X-RateLimit-Limit`, `X-RateLimit-Remaining`, `X-RateLimit-Reset` (ISO 8601), `X-RateLimit-NearLimit` ("true when less
  than 20% of capacity remains"), structured `RateLimit-Policy` / `RateLimit` (`q`, `w`, `r`, `t`); `Beta-` prefix =
  informational only.
- Guidance: "Use exponential backoff with jitter ... Only retry if the API is idempotent and the response includes a
  Retry-After header." / "For jira-quota-global-based or jira-quota-tenant-based: Pause all API requests until the window
  resets." / "Distribute requests over time: Spread your requests evenly throughout the hour rather than sending large
  spikes at predictable times. Add random jitter to scheduled jobs" / "Avoid using excessive concurrency: While
  parallelism can improve performance, using it specifically to bypass rate limits will lead to more 429 responses and
  degraded performance overall." / "Do not perform rate limit testing against Atlassian cloud tenants".
  Pseudocode: maxRetries 4, delays doubling, jitter range [0.7, 1.3], cap 30 s.
- Enforcement: CHANGE-3080 (2026-03-02): "Effective March 2, 2026, we are starting the phased enforcement of points-based
  quota rate limits for Jira and Confluence Cloud REST APIs." / "All Forge, Connect, and OAuth 2.0 (3LO) apps are in
  scope." / "Quota-related headers with a Beta- prefix ... indicate enforcement has not yet begun for your app."
- Forge-level link: `InvocationErrorCode.FUNCTION_UPSTREAM_RATE_LIMITED` + `retryAfter = Retry-After` is the documented
  bridge from a Jira 429 into a queue retry (async and product-event pages both show it).

## 15. Cost-linked robustness facts

- Crashes: "When this happens, you are charged for the lower of: the function's configured timeout (set via
  timeoutSeconds), or the measured execution time, including platform overhead." ("when a function runs out of memory or
  the runtime crashes")
- Async billing: CHANGE-3120 (2026-03-26): billing for asynchronous invocations "will now commence on 2026-07-01" and "A
  Forge app invocation is classified as asynchronous when invoked from the following Forge modules: Scheduled triggers,
  Async Events API, Trigger". Free allowance doubled to 200,000 GB-seconds/month (CHANGE-3309).
- KVS billing: "Writes are ~20× more expensive than reads"; "Empty KVS reads count as 1KB towards your usage".
- Forge Cache EAP closed (CHANGE-2761, 2025-08-29): "The service will not progress to GA".
- Suspension: "An app may be temporarily suspended if it negatively impacts the Forge platform, regardless of whether it’s
  in breach of any quotas or limits."

## 16. Documented anti-patterns (verbatim)

1. Unawaited promises/timers after return — Node.js runtime warning (§3).
2. Tenant data in module-level globals — "Do not write tenant-specific data to module-level (global) variables".
3. Resolver called every render — "A common anti-pattern is fetching data inside a component that re-renders frequently.
   Instead, fetch once and store the result".
4. Resolver used to fetch context — "A surprisingly common anti-pattern is invoking a Forge resolver to look up contextual
   metadata that is already available in the frontend context".
5. Polling storage from a resolver — "a common pattern is to use a resolver to poll Forge storage until a certain value
   is updated. This can cause unnecessary compute usage and a lengthy wait for the user. A more efficient alternative is
   to use Forge Realtime to push events from your backend to your UI."
6. Scheduled polling instead of events — "If your scheduled trigger is polling for changes ... replace it with a product
   event trigger that fires only when the relevant event actually occurs."
7. N+1 — "A classic anti-pattern is the "N+1 problem": fetching a list of items and then making a separate API call for
   each item to retrieve its details".
8. Fetch-all-then-filter — "fetch only the items you actually need to process, not all items and then filter in code."
   / "Anti-pattern: read every entity, then filter in application code".
9. No maxResults — "Always pass an explicit maxResults limit matched to your actual need".
10. Self-event feedback loops — see §7.
11. Verbose logging of full payloads — "Logs the full event payload on every invocation — expensive at scale".
12. Over-provisioned memory/timeouts — "Avoid over-provisioning"; "Right-size timeoutSeconds".
13. Excessive concurrency to bypass rate limits — Jira rate-limiting page (§14).
14. Persisting storage cursors — "cursors are not stable and should not be persisted."
15. Unbounded entity growth; storing files in KVS — KVS recommendations.
16. Storing secrets outside encrypted env vars / kvs.setSecret — "only use encrypted environment variables and
    kvs.setSecret to store secrets or credentials in your app."

## 17. Documentation conflicts the benchmark contract MUST pin (never grade the ambiguous side)

| # | Topic | Side A (quote, URL) | Side B (quote, URL) | Evidence for resolution |
|---|---|---|---|---|
| 1 | Async retry cap | "Async Events are automatically retried within the retention window until they are successfully delivered." (async-events-api) | "with a maximum of four retries" (storage-api-limit-handling note) | CHANGE-2642: "Previously, there were four async events retries available in total. Now ... until they are successfully delivered." ⇒ A is current |
| 2 | Timeout/OOM class | async page lists "function time out, function out of memory" under app-level errors (retryCount increments) | product events page: "Platform level errors ... Examples include timeouts and Out of memory(OOM) errors" | Different subsystems; pin per event type |
| 3 | Scheduled trigger return | "If a function invoked from a scheduled trigger returns a value, it is ignored." (module page, 2026-06-17) | "must return a response ... 424 Failed dependency ... 204 as success" (function-reference 2025-02-07, events-reference 2024-11-11) | Module page newer; pin "ignored; throw = failed run" |
| 4 | KVS query max | "maximum of 100" (query.limit text) | code comment "up to a maximum of 20"; error "Limit for list query should be below 100" | CHANGE-1916 ⇒ 100 |
| 5 | Object depth | "Object depth | 31" | "MAX_DEPTH ... (32) limit" (error pages, REST) | pin one |
| 6 | Transaction set signature | docs: 3rd arg `{ entityName, ttl, conditions }`; `.check(key, filter, {...})` | SDK: `set(key, value, entity?, options?)`; `check(key, entity)` | SDK code wins at runtime |
| 7 | SDK names in examples | `equalsTo`, `isGreaterThan`, `SortOrder.DESC` | SDK exports `equalTo`, `greaterThan`, `Sort` | SDK |
| 8 | Memory key | manifest `runtime.memoryMB` (function module + app.runtime) | cost guide YAML `functions:` + `memoryMiB:` | manifest reference |
| 9 | Interval spelling | `fiveMinute` | cost guide comment "fiveMinutes" | manifest reference |
| 10 | Resolver default | "Forge function maximum of 25 seconds" | LLM tutorial "default function timeout (55s)" | limits page: 25 s for UI resolvers |
| 11 | Long-running entry | "All long-running functions must be invoked by an async event consumer." | scheduled triggers accept timeoutSeconds up to 900 (CHANGE-2635) | CHANGE-2635 |
| 12 | Queue name rule | "alphanumeric string, and can start with _" | SDK `/^[a-zA-Z0-9-_]+$/` | SDK |
| 13 | Web-trigger rate | CHANGE-1596 "20000 requests per 60 seconds" per URL path | limits page 2026-09-01: per install 7,000/min & 300/s, per user 1,200/min | pin the limits page |
| 14 | Burst example | "e.g., 10 requests/second for GET endpoints" | table "GET | 100" | pin the table |
| 15 | Node 20 | manifest lists `nodejs20.x` | CHANGE-3209 "no longer supported" | pin nodejs22.x/24.x |
| 16 | Consumer API | 2.x `function` consumer (async-events-api) | Assets tutorial uses deprecated 1.x `resolver.define` consumers | pin 2.x |

## 18. How an offline benchmark can test each concept objectively

Principles: (a) virtual time — the emulator owns the clock, so 25 s / 900 s deadlines, 15-min retry gaps, 24 h retention
and the 1-hour Tier-1 window run in seconds of wall time and are identical across runs; (b) seeded, declared fault classes
— the contract names every fault class the emulator injects (duplicates, reordering, 429s, kills at deadline, stale query
reads, partial batches, cursor invalidation, schedule duplicates/skips/overlap, deploy mid-flight) without revealing the
seed; (c) outcome grading — final state vs ground truth, budgets consumed, latency of user-facing calls, nothing graded
that is not stated; (d) any implementation that yields the right outcome passes (no "must use transactions").

Emulator hooks verified in SDK source (so the real SDKs can run unmodified offline):
- `@forge/api` `getAppContext()` reads `global.__forge_runtime__` and `runtime.lambdaContext.getRemainingTimeInMillis` →
  the emulator supplies virtual remaining time.
- `@forge/kvs` sends every call through `global.__forge_fetch__({ type: 'kvs', provider: 'app', remote: 'kvs' }, path,
  options)` to `/api/v1/get|set|delete|query|entity/*|batch/*|transaction` → one interception point for stale reads,
  409 KEY_CONFLICT, CONDITIONAL_CHECK_FAILED, 429 RATE_LIMIT_EXCEEDED, partial batch failures.
- `@forge/events` posts to `/webhook/queue/publish|stats|cancel/...` via `__requestAtlassianAsApp`; status codes map to
  RateLimitError(429)/InvocationLimitReachedError(405)/PayloadTooBigError(413)/PartialSuccessError(202); a retry request
  is the returned object `{ _retry: true, retryOptions }`.

| Concept | What the app must do | What the grader observes | What the contract must state |
|---|---|---|---|
| 25 s resolver limit | Return fast from UI resolvers; hand heavy work to a queue and return a job handle | Virtual duration of every resolver invocation; any resolver killed at 25 s = failure; user action completes via async path | 25 s hard kill; virtual cost per mocked API call / KVS op; that `invocationRemainingTimeInMillis` exists |
| 55/900 s consumer + scheduled limits | Declare `timeoutSeconds` where needed (≤ 900, integer); checkpoint before deadline | Emulator kills at `timeoutSeconds` (default 55); after a kill no further side effects; grader checks no item lost/double-applied | Default 55, max 900, lowest-wins rule for shared functions, kill semantics (partial writes persist) |
| Remaining-time chaining | Check remaining time, persist a watermark, push a continuation event | Work set with variable per-item virtual cost totalling > 900 s completes exactly once | Emulator honours `getRemainingTimeInMillis`; per-item costs are variable |
| Memory | Stream/paginate; set `memoryMB` deliberately | Run consumers in workers with resourceLimits = declared memoryMB; OOM = app-level error | Emulator enforces the declared memoryMB as heap cap; dataset sizes stated |
| Cold/warm reuse | No tenant data or correctness-critical state in module scope | Emulator randomly reuses or recycles module instances, interleaving two installations in one process; any cross-tenant value leak or stale-cache wrong answer fails | "Module state may persist across invocations and across installations, or be reset at any time" |
| Unawaited work | `await` everything; use the queue for post-response work | Emulator freezes pending timers/microtasks at handler return and only resumes them (or never) in a later invocation; grader checks every promised side effect landed before return | Quote the Node.js runtime warning as emulator semantics |
| At-least-once / duplicates | Idempotent consumers keyed on a business id (not jobId) | Seeded duplicate deliveries (incl. redelivery after a kill post-side-effect); counters, created issues/comments, emails sent must equal the exactly-once ground truth | "Each event may be delivered more than once"; jobId is per push, eventId per event |
| Ordering | No reliance on push order; use versions/timestamps | Seeded shuffles; final state must equal the ordered ground truth | "No ordering guarantee" |
| App-level errors + poison messages | Bounded retries via `retryContext.retryCount`; dead-letter to a visible failed list; job completes | One always-failing item per run; it must appear in the admin failed list with reason while all other items complete; no retry storm | Thrown error / timeout / OOM = retry with backoff until 24 h virtual retention, then dropped; retryCount semantics |
| Upstream 429 (Tier 1 / burst) | Return `InvocationError` with `retryAfter ≥ Retry-After` and `FUNCTION_UPSTREAM_RATE_LIMITED`, or pace via concurrency; never busy-wait inside the function | Count of 429s, retries issued earlier than Retry-After (each = violation), virtual seconds burned sleeping in-function, total points vs 65,000/h budget, completion time | Points model table, bucket sizes/refill, per-issue windows, header formats, the hourly budget the emulator applies |
| Push limits | Chunk ≤ 50 events / ≤ 200 KB; pace ≤ 500 events/min; handle PartialSuccessError | Rejected pushes and permanently lost work items; any item never processed = failure | The four limits and the error classes the emulator raises |
| Cyclic limit | Fan out in batches of 50, re-seed from a scheduled trigger for very large work | Dataset sized so one-push-per-item chaining hits 1000 push requests from one origin while batched pushes fit | Exact counting rule (push requests per origin invocation, across descendants) |
| Concurrency key | Use `concurrency {key, limit}` to protect fragile upstreams | Mock upstream with declared max parallelism (e.g. 2); grader records peak concurrent calls and its 429s | Upstream's cap; that keys are per installation across queues |
| Job progress + cancel | Admin UI shows progress from getStats or own counters; cancel stops queued work | Progress shown at checkpoints vs ground truth; no event starts after cancel (in-flight may finish) | Emulator's definitions of success/inProgress/failed (undocumented upstream) |
| Retention expiry | Surface dropped/expired work; reconcile | Virtual 24 h passes with a failing dependency; app must show the items as expired/failed, not "done" | 24 h, +72 h only for platform errors, drop on app-level errors |
| Deploy mid-flight | Version event payloads; new code accepts old payloads | Emulator upgrades the app while events are queued and delivers old-shape bodies to new code | "In-flight events survive deploys and are processed by the new version" (CHANGE-2526) |
| Lost updates (KVS) | Conditional writes (CES transaction conditions / version attribute) or sharded keys; retry on CONDITIONAL_CHECK_FAILED / 409 | Emulator interleaves concurrent get→set on one aggregate; final aggregate must equal ground truth | Last-write-wins default; available primitives; conflict error codes |
| Idempotency marker | `kvs.set(..., { keyPolicy: 'FAIL_IF_EXISTS' })` or a CES conditional check | Duplicate deliveries do not double-apply | keyPolicy exists on set/entity().set only (not batch/transact) → 409 KEY_CONFLICT |
| Query eventual consistency | Never use query-after-write for uniqueness/correctness; use get | Emulator returns query results lagging recent writes by a stated virtual delay; uniqueness violations counted | Query lag model; get is strict |
| Batch partial failure | Retry `failedKeys` | Emulator fails a seeded subset; all keys must end written | 25 keys, 4 MB, best-effort semantics |
| Pagination + cursor instability | Page with limit ≤ 100; resume long scans from a key/range watermark, not a stored cursor | > 100 items everywhere; emulator invalidates cursors across invocations; items missed = failure | "Cursors are valid only within the invocation that produced them" (emulator choice aligned with the doc warning) |
| Storage throughput | Batch writes; queue bulk storage work; avoid single hot counter key | Emulator enforces 1000 RPS, 4000 10KB-units/min, 1 MB/s per key → 429 RATE_LIMIT_EXCEEDED | The per-installation and per-key numbers and the 10 KB rounding |
| TTL | Treat expireTime in the past as absent | Emulator delays physical deletion; reading an expired value as live = failure | Max 1 year; deletion lag up to 48 h |
| Scheduled duplicates/overlap/skips | Lease with TTL (FAIL_IF_EXISTS or conditional) + watermark catch-up | Emulator fires duplicate concurrent runs, skips ticks, and lets a long run overlap the next tick; grader checks no double processing, no gap after skipped ticks, stale lease expires | 5 triggers / 1 fiveMinute; no retries on error; duplicates possible; skips and overlaps will be injected |
| Web trigger burst | Authenticate, enqueue, respond fast (≤ 55 s, typically ack), dedupe by delivery id | Inbound burst at stated RPS; response latency; exactly-once processing; 429 handling via X-Ratelimit-* | 300 RPS / 7,000 per min / 1,200 per user, 55 s, unauthenticated URLs |
| Product-event semantics | ignoreSelf or selfGenerated check; idempotent; tolerate 3-min delay | Emulator re-emits app-caused updates; writes per issue must stay bounded; per-issue write limit 20/2 s | Delay bound, max 4 app-requested retries, self events, top-level-only deletes |
| Cascading deletes | Clean app records when only the project delete arrives | Orphaned records after a project delete = failure | "Only top-level delete events" |
| Install race | Retry 401/403 on install with backoff | First N asApp calls after install return 403 | The window length in virtual time |
| Logging | ≤ 100 lines per runtime minute, ≤ 200 KB; no PII/secrets; one summary per batch | Emulator caps lines; grader scans logs for seeded secrets/emails; counts lines per invocation | Caps and the PII/secret rule |
| UI invocation limits | Debounce/coalesce; no tight polling; use Realtime for progress | Resolver invocations per user per virtual minute; > 1,200 = failure; storage-polling resolvers flagged | Limits; Realtime available |
| Realtime | Coalesce progress publishes (≤ 50 ops/s per installation); check `result.errors` | Emulator rate-limits publish and returns errors without throwing; UI must still reach final state | 50 ops/s; publish returns errors |
| LLM | Run LLM calls in consumers with timeoutSeconds ≥ 300; pace ≤ 100 RPM / 500k TPM; detect truncated streams | Mock LLM 429s above the RPM, truncates some streams (no finish_reason); outputs must be complete | Limits; truncation model; 5-min inference only in async |

## 19. Could not verify / open questions

1. Maximum `concurrency.limit` and maximum number of concurrency keys — not in docs, not validated in the SDK.
2. Exact automatic-retry schedule (first delay, multiplier) — only "exponential backoff ... up to approximately 15 minutes".
3. Async events ordering — no guarantee documented; FIFO (RFC-107) not shipped per changelog search on 2026-10-09.
4. Scheduled triggers: overlap behaviour when a run outlasts its interval; whether missed ticks are ever caught up (no
   doc; partner reports of missed/silent stops are not staff-confirmed).
5. Cold-start latency — Atlassian publishes no figure.
6. Product-event `trigger` function timeout — not stated explicitly (25 s generic row is the likely value).
7. What happens when backend log caps are exceeded (truncation vs drop vs failure).
8. Job stats semantics (`failed` definition, retention, consistency) and whether getStats/cancel are rate-limited.
9. Behaviour of a CES conditional `set` on a key that does not exist (condition evaluates false? passes?).
10. Magnitude of query eventual-consistency lag ("slightly out of date" only).
11. Whether Forge SQL supports multi-statement transactions (not documented; `SET autocommit` is blocked).
12. Whether the user-led 300 RPS/7,000 RPM install limit also covers async (non-user-led) invocations — the page lists
    only user-led limits; async pacing is governed by the 500 events/min push limit and concurrency keys.
13. Points-based quota enforcement status for any specific app — phased from 2026-03-02; determined per app by header
    prefix, so an offline benchmark must simply declare enforcement on.
14. Community search API rate-limited anonymous queries; threads were read by id instead (no impact on cited facts).

## Verification

An independent fact-check ran on 2026-10-09 at about 20:10 EEST. The checker did not reuse this researcher's cached files.

**How it was checked:**

- Every cited URL was fetched again with curl into `scratchpad/forge2/verify-robustness/`.
- Each quote was matched word for word against the page text. Before matching, whitespace, table pipes and curly quotes were normalised. A quote containing "…" was matched one segment at a time.
- The `#CHANGE-…` anchors are filled in by the changelog service, so they were read from `dac-changelogs.services.atlassian.com/changes?apiGroups=…&match=CHANGE-n`.
- The npm tarballs were downloaded and extracted again.
- Community threads were read through Discourse's `/t/<id>.json` and `/raw/<id>` endpoints. The poster's title ("Atlassian Staff") was checked on each one.

**Where newer material was looked for:**

- The 400 newest changelog entries (2025-07-21 → 2026-10-09) across these groups: forge-core-platform, forge-jira, forge-confluence, rest-jira-cloud-platform, rest-confluence-cloud and adopting-forge-from-connect.
- The newest SDK pre-releases: @forge/kvs 2.1.0-next.2, @forge/events 3.0.8-next.2 and @forge/api 8.3.0-next.2, all published 2026-10-09.
- Two web searches, on cold starts and on staff statements about scheduled-trigger duplicates.

**Result:**

- All 74 quotes were found word for word at their source. For claim 17, the second half is on the error-handling page, not the cited API page.
- No claim is refuted or outdated.
- 20 claims need a correction, a narrower scope, or a contract pin. Those are in the last column of the table and are summarised after it.

| # | Claim (short) | Verdict | Evidence (fresh fetch; page "Last updated") | Correction / contract pin |
|---|---|---|---|---|
| 1 | UI resolvers 25 s; longer work → async events | confirmed | async-events-api (2026-09-01) verbatim. limits-invocation (2026-09-01) row "Runtime seconds … 25". function page: timeoutSeconds "does not affect" other functions | — |
| 2 | Consumer/scheduled-only functions: default 55 s, max 900 | confirmed | limits-invocation row verbatim. async-events-api says "default of 55 seconds … up to 900" | — |
| 3 | timeoutSeconds is an integer 1–900; lowest wins | confirmed | function module (2026-09-11) verbatim. Type `integer`. Worked example: 300 vs 120 → 120 | — |
| 4 | Web trigger / action / rovo:agentConnector 55 s | confirmed | limits-invocation row verbatim | — |
| 5 | 180 s single outbound request (async) | confirmed | limits-invocation row verbatim | — |
| 6 | `invocationRemainingTimeInMillis()` | confirmed | app-context-api (2026-10-06) verbatim. Not available on the legacy runtime | — |
| 7 | @forge/api reads `runtime.lambdaContext.getRemainingTimeInMillis` | confirmed | api-8.2.0 `out/api/runtime.js:118` verbatim. Unchanged in 8.3.0-next.2 | `getAppContext()` also destructures `runtime.appContext`, so the emulator must supply the whole `global.__forge_runtime__`. The function is called unbound, so it must be a closure |
| 8 | Memory 512 MB default, 128–1,024 MB, more memory = more CPU | confirmed | manifest-reference (2026-09-24) verbatim, plus a per-function override. limits page says "Default memory limit is 512MB" | — |
| 9 | 500 KB front-end request / 5 MB response / 5 MB backend | confirmed | limits-invocation: three rows verbatim | — |
| 10 | No published cold-start figure; Atlassian's metric excludes cold start but includes init | confirmed | monitor-invocation-metrics (2026-07-13) verbatim. It is the only one of 40 fetched pages that mentions cold start. Two web searches found no official figure. This is a negative claim, so it cannot be proven absolutely | — |
| 11 | Warm process reused across tenants | confirmed | shared-responsibility-model (2026-08-01) verbatim | The doc says "**may** reuse". The contract should say state may persist or reset |
| 12 | Unawaited promises suspended or resumed later, or never | confirmed | nodejs-runtime (2026-09-21) verbatim, including "or never" | — |
| 13 | 1,200 per user per min; 7,000 per min and 300 per s per install; fixed window | confirmed | limits-invocation verbatim. CHANGE-3420 (2026-09-01) set these values and removed the 30,000 RPM per-environment cap. CHANGE-3006 (2026-01-13) cancelled a planned 20 RPS per-user change | — |
| 14 | 100 egress requests per runtime minute | confirmed | limits-invocation verbatim. The same table adds 50,000 egress/min per app, 3,000,000 network/min per app and 100,000/min per app per tenant | — |
| 15 | Push limits: 50 events / 200 KB / 500 per min | confirmed | limits-async-events (2026-02-26) verbatim. SDK 3.0.7 checks 50 events and 200 KB on the client side (bytes of `JSON.stringify(events)` / 1024 > 200) | — |
| 16 | 100 KB per event for consumers with timeout > 55 s | confirmed | limits-async-events verbatim | The docs do not say where this is enforced (at push or at delivery), and the SDK has no client-side check. The contract must state it |
| 17 | delayInSeconds 0–900 | confirmed | The first sentence is on async-events-api. The InvalidPushSettingsError row is on async-events-api-error-handling (2026-09-01), not the cited page. The SDK throws on the client side and accepts fractions | Cite the error-handling page |
| 18 | Cyclic limit: 1,000 push requests | confirmed | limits-async-events verbatim. The SDK maps HTTP 405 → InvocationLimitReachedError | — |
| 19 | PartialSuccessError / RateLimitError recovery | confirmed | error-handling page verbatim. In the SDK, HTTP 202 → PartialSuccessError with `failedEvents[i] = {errorMessage, payload: <original event>}`, and 429 → RateLimitError | — |
| 20 | jobId minted on the client per push() | confirmed | events-3.0.7 `out/queue.js:20` `const jobId = (0, crypto_1.randomUUID)();`. 3.0.8-next.2 is identical | — |
| 21 | At least once within 24 h, extendable by 72 h | confirmed | async-events-api verbatim. CHANGE-2642 (2025-07-01) | Only platform degradation or retries after platform-level errors extend the window. The app cannot extend it |
| 22 | Backoff gaps up to ~15 min | confirmed | async-events-api verbatim | — |
| 23 | App-level errors (incl. timeout and OOM); dropped on expiry | confirmed | async-events-api verbatim | — |
| 24 | Retry by returning InvocationError; retryAfter ≤ 900 s; retryData ≤ 4 KB | confirmed | async-events-api verbatim. 4 KB enforced since 2025-11-13 (CHANGE-2508) | — |
| 25 | SDK InvocationError is the object `{_retry, retryOptions}` | confirmed | events-3.0.7 `out/invocationError.js`: the constructor returns `toJSON()`. retryAfter ≤ 0 → 1. Default is `{1, FUNCTION_RETRY_REQUEST}` | An omitted retryAfter stays `undefined` |
| 26 | retryContext only on retries; retryCount ignores platform errors; remainingTimeMs | confirmed | async-events-api verbatim | `retentionWindow` is **Required: false**, so `remainingTimeMs` can be absent. The doc's own example destructures it without a check. Dead-letter logic needs its own counter |
| 27 | DOC CONFLICT: four retries | confirmed | storage-api-limit-handling (2026-08-21) still says "maximum of four retries". The CHANGE-2642 sentence is verbatim | CHANGE-2642 applies the new model to "@forge/events with major version 2" only. Pin v2+ (the current SDK is 3.0.7) |
| 28 | Concurrency keys | confirmed | async-events-api verbatim | — |
| 29 | Staff: throttled events retried, 15 min gaps, no cap, can expire | confirmed | Thread 93837 post #2, KamilKozlowski ("Atlassian Staff"), 2025-07-16, verbatim | — |
| 30 | No ordering guarantee; FIFO not shipped | confirmed | RFC-107 post #1, RashiChandola ("Atlassian Staff"), 2025-09-16, verbatim. No `FifoQueue` in @forge/events 3.0.7 or 3.0.8-next.2. No FIFO in the docs or in the 400 newest changelog entries | — |
| 31 | Job progress and cancel | confirmed | async-events-api verbatim. `getStats()` → `{success, inProgress, failed}` | One partner said cancel did not stop processing (93837 post #4). This is anecdotal; the contract defines the semantics |
| 32 | crossVersion removed; no version check after 2025-11-21 | confirmed | CHANGE-2526 (2025-05-21), read from the changelog service: both sentences verbatim, with the date 2025-11-21. No later entry reverses it | — |
| 33 | Trigger delivery delay up to 3 min | confirmed | trigger module (2026-07-29) verbatim | — |
| 34 | Product events: four retries; timeout and OOM are platform-level | confirmed | product_events (2026-08-06) verbatim. retryAfter ≤ 900 s also applies | retryData must be an object or it is not sent (bug ECO-734) |
| 35 | ignoreSelf / selfGenerated | confirmed | product_events verbatim. The page says filtering is needed "to prevent infinite loops" | — |
| 36 | Only top-level delete events | confirmed | events (2026-05-22) verbatim, for both Jira and Confluence | — |
| 37 | Installed event can arrive before permissions | confirmed | life-cycle (2026-04-07) verbatim. It recommends retrying on 401/403 | A trigger gets at most 4 retries of ≤ 900 s each, about 60 min in total. The emulator's lag window must fit in that, or the app must hand off to a queue |
| 38 | 5 scheduled triggers, 1 fiveMinute | confirmed | limits-scheduled-trigger (2025-12-01) verbatim | — |
| 39 | A failing scheduled run is not retried | confirmed | scheduled-trigger module (2026-06-17) verbatim | — |
| 40 | Scheduled duplicates; staff says "at least one" | confirmed | Module page verbatim. Staff thread 47905 (Nir, "Atlassian Staff", 2021-05-13): "your app should expect at least one invocation" | — |
| 41 | DOC CONFLICT: scheduled-trigger return value | confirmed | function-reference (2025-02-07): statusCode required, 204 = success, 5xx = error, otherwise 424. Module page (2026-06-17): "it is ignored" | Returning `{statusCode: 204}` satisfies both |
| 42 | Web trigger URLs public; statusCode required | confirmed | web-trigger module (2026-04-17) verbatim: "response requires a statusCode field", for both static and dynamic | — |
| 43 | get strictly consistent, query eventually consistent | confirmed | kvs (2026-08-21) verbatim. storage-api-custom-entities says the same for `entity().get` / `entity().query` | — |
| 44 | Last write wins; atomic per key; keyPolicy | confirmed | kvs verbatim. kvs-api lists FAIL_IF_EXISTS / OVERRIDE | — |
| 45 | FAIL_IF_EXISTS → 409 KEY_CONFLICT | confirmed | The REST spec embedded in the api-group-key-value-store page has 409 on set and entity set. The SDK exports `ForgeKvsAPIError` | — |
| 46 | No keyPolicy in batch or transaction (TTL only) | confirmed | CHANGE-3038 (2026-02-05) verbatim. REST schema: `BatchSet*Item` and `SetTransaction*Item` use `SetOptions{ttl}`, and only single set uses `ExtendedSetOptions` (keyPolicy) | — |
| 47 | TTL max 1 year; expired values readable up to 48 h | confirmed | kvs-api (2026-08-21) verbatim | — |
| 48 | Transactions: 25 ops, unique keys, 4 MB, one write, TOO_MANY_REQUESTS | confirmed | kvs-transactions (2026-09-28) verbatim | — |
| 49 | Conditions only in CES transactions; CONDITIONAL_CHECK_FAILED | confirmed | entities-transactions (2026-09-28) verbatim. The KVS transactions page has no conditions. REST text verbatim | — |
| 50 | transact().set SDK signature vs docs | confirmed | kvs-2.0.7 `interfaces/transaction.d.ts` verbatim. `transaction-api.js` copies only entityName and conditions from the third argument, so TTL is dropped silently. Both doc pages (2026-09-28, the same day 2.0.7 shipped) still pass ttl in the third argument, for KVS and CES. Unchanged in 2.1.0-next.2 | — |
| 51 | Batch is best-effort, parallel, unordered | confirmed | kvs-batch (2026-08-21) verbatim. The 4 MB limit is stated for batchSet. The whole batch fails on zero keys or a duplicate key. batchGet and batchDelete arrived in CHANGE-3050 (2026-03-04) | — |
| 52 | 1,000 RPS; 4,000 + 4,000 10 KB units per min | confirmed | limits-kvs-ce (2026-09-28) verbatim | — |
| 53 | 1 MB/s writes per key | confirmed | limits-kvs-ce verbatim. Also reads 12 MB/s per key and queries 24 MB/s per index value | — |
| 54 | 429 on limits; queue through async events | confirmed | limits-kvs-ce verbatim. entities-errorhandling: "429 status with an error code of RATE_LIMIT_EXCEEDED". Transactions return TOO_MANY_REQUESTS | — |
| 55 | 240 KiB value, depth 31, key 500, regex | confirmed | limits-kvs-ce verbatim. The REST error text says MAX_DEPTH (32) | Pin 31 |
| 56 | Default 10 / max 100 results; one beginsWith | confirmed | kvs-api-query (2026-08-21) verbatim: "single where condition", beginsWith only | An example comment on the same page says "up to a maximum of 20". The contract pins 100 |
| 57 | Cursors are unstable | confirmed | kvs verbatim | — |
| 58 | CES 20 entities / 7 indexes / 50 attributes; range takes one attribute; strict types | confirmed | limits-kvs-ce verbatim. entities-manifest: "This parameter can only have one attribute." storage-api-custom-entities: "strictly enforces attribute types" | — |
| 59 | Doc examples use names the SDK does not export | confirmed | The entities-api-query (2026-08-21) examples use `isGreaterThan`, `SortOrder.DESC` and `equalsTo`. @forge/kvs 2.0.7 exports `equalTo`, `greaterThan` and `enum Sort`. The page's own reference tables use the correct names | — |
| 60 | SQL timeouts 5/10/20 s, 150 DML RPS, 62.5 s | confirmed | limits-sql (2025-12-01) verbatim | CHANGE-3254 (2026-06-17) adds SQL_POLICY_VIOLATION rejections (SLEEP(), multi-statement and others). The limits did not change |
| 61 | Logs: 100 lines per runtime minute, 200 KB | confirmed | limits-invocation verbatim | — |
| 62 | Logging guidance | confirmed | logging-guidelines (2026-05-22) verbatim | The doc says "avoid", not "never" |
| 63 | Realtime 50 ops/s; publish returns errors | confirmed | limits-realtime (2026-06-25) verbatim. The Realtime error-handling page: `PublishResult.errors`, RATE_LIMIT_EXCEEDED. Enforced from 2026-06-26 (CHANGE-3246) | The 50/s is shared by subscribe, publish and signRealtimeToken, across both the events API and the bridge API. UI subscriptions count |
| 64 | LLM: 100 RPM, 500k TPM, 5-min inference only in async | confirmed, with a scope correction | limits-llm (2026-09-30) verbatim. CHANGE-3497 (2026-10-02) raised TPM from 50k to 500k | TPM is **per installation per model** ("a single model"). RPM covers all models |
| 65 | Truncated LLM streams do not throw | confirmed | forge-llms-api-errors (2026-07-23) verbatim, including finish_reason detection and the resume prompt | — |
| 66 | Tier 1: 65,000 points per hour shared by all tenants; resets each UTC hour | confirmed | jira/platform/rate-limiting (updated **2026-10-09**) verbatim | — |
| 67 | Points model | confirmed | rate-limiting verbatim. Identity & access GETs cost 2 points | — |
| 68 | Burst token buckets; design for steady state | confirmed | rate-limiting verbatim. Table: GET 100, POST 100, PUT 50, DELETE 50, plus per-endpoint overrides | The same page also says "e.g., 10 requests/second for GET endpoints". The bucket is "per tenant and per API/resource path", and the page does not say whether apps share it. The contract must pin both |
| 69 | Per-issue writes: 20 per 2 s and 100 per 30 s | confirmed | rate-limiting verbatim. Reason header `jira-per-issue-on-write` | — |
| 70 | On 429: honour Retry-After, back off with jitter, pause on quota | confirmed | rate-limiting verbatim. For quota reasons: "Pause all API requests until the window resets" | — |
| 71 | Concurrency to get around limits is an anti-pattern | confirmed | rate-limiting verbatim | — |
| 72 | Enforcement from 2026-03-02; Beta- prefix means not yet enforced | confirmed | CHANGE-3080 verbatim. CHANGE-3003 moved the start from 2026-02-02. CHANGE-3045 introduced the Beta headers. No later entry | The rollout was phased "over several weeks", and no completion notice was found |
| 73 | Polling storage from a resolver is an anti-pattern | confirmed | optimise-forge-costs (2026-08-21) verbatim | — |
| 74 | N+1 is an anti-pattern | confirmed | optimise-forge-costs verbatim | — |

**Corrections and new contract pins this verification added:**

1. **LLM tokens per minute** are counted per installation **per model** (CHANGE-3497, 2026-10-02). Requests per minute (100) are counted across all models.
2. **`retryContext.retentionWindow` is optional**, so `remainingTimeMs` may be missing. Dead-letter logic needs its own attempt counter.
3. **Retention-window extension.** Only platform degradation or platform-level errors extend the window to 96 h. App retries never extend it.
4. **The 100 KB per-event limit for long-running consumers.** The docs do not say whether it is enforced at push or at delivery, and the SDK does not check it. The contract must choose.
5. **The four-retry doc conflict.** CHANGE-2642 applies unlimited retries within the retention window to @forge/events v2+ only. Pin v2+.
6. **Install permission lag.** A trigger can be retried at most 4 times, at ≤ 900 s each, so about 60 min in total. Keep the emulator's 403 window inside that. Product-event `retryData` must be an object (bug ECO-734).
7. **Remaining-time hook in the emulator.** It must provide all of `global.__forge_runtime__` (`appContext` and `lambdaContext`). Because `getRemainingTimeInMillis` is called unbound, it must be a closure.
8. **Realtime's 50 ops/s** includes subscribe and signRealtimeToken calls from the UI, not just publish.
9. **Pin these doc contradictions:**
   - KVS query maximum: 100 in the reference, 20 in an example comment.
   - Jira GET steady-state rate: 100 RPS in the table, "e.g., 10 requests/second" in the prose.
   - Object depth: 31 on the limits page, 32 in the REST error text.
   - Scheduled-trigger return value.
10. **Burst buckets are per tenant and per path.** The docs do not say whether apps on the same tenant share a bucket.
11. **Useful SDK facts for the emulator** (@forge/events 3.0.7):
    - HTTP 202 → PartialSuccessError, and `failedEvents` carries the original events.
    - 429 → RateLimitError.
    - 405 → InvocationLimitReachedError.
    - 413 → PayloadTooBigError.
    - The 200 KB check runs on the client over the serialised array. The delayInSeconds check also runs on the client and accepts fractions.
12. **Forge SQL** now rejects restricted functions and statements with SQL_POLICY_VIOLATION (CHANGE-3254). No numeric limit changed.
