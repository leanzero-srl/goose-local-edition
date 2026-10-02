# Forge benchmark — research fact sheet (as of 2026-10-02)

This fact sheet supports a new agentic benchmark, **Forge**. The question it answers: can one model inside
goose, in about 150 calls, build a real Atlassian Forge app that works when we RUN it? It is graded offline,
without `forge deploy`.

**How the facts were gathered:**
- **Packages.** The npm tarballs were unpacked and read: @forge/api 8.2.0, kvs 2.0.7, events 3.0.7,
  resolver 2.0.0, bridge 7.1.0, sql 4.0.7, manifest 13.6.0, lint 6.3.0, cli 14.1.0 and react 12.3.0.
- **Docs.** developer.atlassian.com pages were fetched on 2026-10-02, plus the Forge changelog from 2026-03-19
  to 2026-10-02.
- **Local skills.** The LeanZero skills were read: `leanzero-forge-app-baseline`, `atlassian-jira-forge-skill`,
  `atlassian-confluence-forge-skill` and `forge-security-review`.
- **Measured probes.**
  - The offline lint, run three times.
  - The offline runtime shim, driving the REAL @forge packages.
  - Read-only GETs against the wolfaenpak test site.

**How claims are marked:**
- **[measured]** means a command was run here.
- **[doc]** means a docs page, quoted.
- **[pkg]** means the package source or types.
- **⚠** means a conflict or something unconfirmed.

Doc quotes came through a fetch summariser; re-check a quote before publishing it.

---

## 0. The five load-bearing facts (read these first)

1. **The REAL `@forge/*` runtime packages run in plain Node. Two globals are needed: `global.__forge_runtime__`
   and `global.__forge_fetch__(target, path, init)`.** [pkg + measured]
   - Every Jira/Confluence call, every KVS call and every queue push funnels through `__forge_fetch__`.
   - The target object tells you who is calling and what is being called:
     - `{type:'fpp', provider:'app'|'user'|'none', remote:'jira'|'confluence'|'stargate'}`
     - `{type:'kvs', provider:'app', remote:'kvs'}` (KVS: `POST /api/v1/get|set|query|batch/*|transaction|entity/*`)
     - `{type:'sql', remote:'sql'}`
     - queue pushes are `POST /webhook/queue/publish/...` and need a **201** reply.
   - So a scorer can see asApp vs asUser, the method, the path and the body. It can also inject 429s,
     `NEEDS_AUTHENTICATION_ERR` and KVS errors.
   - Sources: `@forge/api/out/api/runtime.js:109` (`const runtime = global.__forge_runtime__`),
     `out/api/fetch.js:23`, `@forge/kvs/out/index.js` and `@forge/events/out/queries.js`.
2. **`forge lint` itself needs a login and an app id. The client-side linters in `@forge/lint` do not, and they
   ran with the network DENIED (`sandbox-exec … (deny network*)`).** [pkg + measured]
   - The CLI runs `linterMode: 'both'` (`@forge/cli/out/command-line/controller/lint-controller.js`). Its
     server-side half calls Atlassian GraphQL (`getAppPreDeploymentCheck`).
   - `lint(files, manifest, env, logger, statsig, {linter:{mode:'client-side'}})` needs only a statsig stub with
     `getDeprecatedRuntimes`.
3. **`/rest/api/3/search` is gone.** Today it returns **410**: "The requested API has been removed. Please
   migrate to the /rest/api/3/search/jql API." [measured on wolfaenpak]
   - `/search/jql` pages with `nextPageToken`. The token is "not included in the response for the last page".
     The response has no `total`. Fields default to `id` only ("By default, this resource returns IDs only").
     Unbounded JQL gets a 400. [doc: Jira v3 OpenAPI; CHANGE-2046 https://developer.atlassian.com/changelog/#CHANGE-2046]
   - `forge lint` still treats `GET /rest/api/3/search` as a valid endpoint, so only a mock can catch it.
     [measured]
4. **`@forge/api` 8.0.0 (2026-06-22) REMOVED `storage`.** CHANGELOG: `561f8f4: Remove "storage" module from
   "@forge/api"`. KVS is `import { kvs } from '@forge/kvs'`. [pkg]
   - Lint flags any import named `storage`, but it does NOT flag `@forge/kvs` use when the `storage:app` scope is
     missing (`storage-api-node-visitor.js` matches only the identifier `storage`). [measured]
5. **Newest module status, per the changelog and module pages:** [doc]
   - **GA:** `dashboards:widget`, 2026-09-22, which deprecates `jira:dashboardGadget`; Forge LLM, 2026-07-30.
   - **Preview:** `rovo:skill`, 2026-10-02; `objectStore`, 2026-06-22; `global:fullPage`, 2026-08-03, which
     deprecates `jira:fullPage` and `confluence:fullPage`; `jira:command`; `apiRoute`; app-managed permissions.
   - **EAP:** `global:ui` and `dashboards:filter`.
   - **Schema-only:** `fifoConsumer` has no public page. It is in the 13.6.0 schema but has no SDK.
   - Release phases are defined at https://developer.atlassian.com/platform/forge/whats-coming/. EAP is
     "Only to EAP participants, and can only be enabled on development environments". Preview is "All Forge
     users, and can be enabled on production environments".

---

## 1. Manifest shape (from the @forge/manifest 13.6.0 schema, `out/schema/manifest-schema.json`) [pkg]

```yaml
modules:
  <moduleType>:            # 211 module types in the schema
    - key: my-key          # ^[a-zA-Z0-9_-]+$ ; must be unique across ALL modules incl. function keys [measured: 'found duplicate module key']
      function: fn-key     # or resource:+resolver:{function|endpoint} for UI modules
  function:
    - key: fn-key
      handler: index.exportName     # file src/index.(js|ts) export
      timeoutSeconds: 900           # only honoured on consumer / scheduledTrigger (webtrigger ≤ 55); lint errors otherwise
resources:
  - key: panel             # ≤ 23 chars [doc resources page]
    path: static/panel/build   # Custom UI: a directory containing index.html; UI Kit: a FILE (lint: "Client Side UI Kit resource … cannot be a directory")
permissions:
  enforcement: app-managed # Preview, optional
  scopes: [read:jira-work, write:jira-work, storage:app]
  content: { scripts: [...], styles: [unsafe-inline] }
  external:
    fetch:
      backend: [ { address: "https://api.example.com" } ]   # bare string still accepted but lint WARNS "deprecated egress permission entries" [measured] ⚠ docs still show the string form
      client:  [ ... ]
    images: [...]    # also fonts, styles, frames, media, scripts, navigation
app:
  id: ari:cloud:ecosystem::app/<uuid>
  runtime: { name: nodejs22.x }     # enum nodejs20.x | nodejs22.x | nodejs24.x ; nodejs18.x is a lint ERROR [measured]
  storage:
    entities:                       # KVS custom entities live HERE (app.storage.entities)
      - name: scope-change          # ^[a-z0-9:\-_.]*$, 3–60 chars
        attributes: { sprintId: {type: integer}, issueKey: {type: string}, addedAt: {type: integer} }
        indexes: [ { name: by-sprint, partition: [sprintId], range: [addedAt] } ]
```

**Egress object form.** It is `{address (required), category?: 'analytics', inScopeEUD?: boolean}`. The
permissions page says: "If inScopeEUD is set to true, then the app is ineligible for Runs on Atlassian."
(https://developer.atlassian.com/platform/forge/manifest-reference/permissions/)

### Module shapes that matter (schema, required fields first)

| module | shape (schema 13.6.0) | status |
|---|---|---|
| `scheduledTrigger` | `{key, function, interval: fiveMinute\|hour\|day\|week, filter?:{appIsLicensed}}` | GA; ≤ 5 per app, ≤ 1 at fiveMinute [doc limits-scheduled-trigger] |
| `webtrigger` | `{key, function, request?:{authentication: hmacSharedSecret\|none}, response?:{type: static\|dynamic, outputs[]}, urlFormat?: v1\|v2}` | GA; `hmacSharedSecret` is in the schema ⚠ not confirmed on a docs page |
| `trigger` | `{key, function, events: [string] \| [{eventType, filter?, payload?}], filter?:{ignoreSelf, expression, onError, appIsLicensed}}` | GA |
| `consumer` | `{key, queue, function}` (the old `resolver:` variant is still in the schema) | GA |
| `fifoConsumer` | `{key, queue, function}` | ⚠ no docs page; RFC-107 only; no `FifoQueue` in @forge/events 3.0.7 |
| `jira:jqlFunction` | `{key, function, name, arguments[{name,required}], types[...], operators[...]}` | GA (no banner) |
| `jira:actionValidator` | `{key, action: workItemTypeChanged\|workItemMoved\|…, expression, errorMessage}` | Preview; Jira expressions only |
| `jira:sprintAction` / `jira:boardAction` / `jira:backlogAction` | `{key, resource, title, resolver?, actionType: dynamic\|modal, render?, renderRuntimeType?: iframe\|webworker}` | GA (no banner) |
| `jira:command` | `{key, title, target{page\|resource…}, shortcut?, keywords?}` | Preview |
| `jira:issuePanel` | `{key, resource, title, icon (REQUIRED), resolver?}` or `{key, function, title, icon}` | GA; omitting `icon` is a lint ERROR [measured] |
| `jira:issueContext` | `{key, resource, title, label, …}` | GA; replaces `jira:issueGlance` (deprecated 2026-05-05) |
| `dashboards:widget` | `{key, title, description, thumbnail, resource, edit{…}, resolver?, render?}` | **GA 2026-09-22**; `jira:dashboardGadget` deprecated, "removed on May 17, 2027" |
| `dashboards:filter` | — | EAP 2026-09-30 (do not demand it) |
| `global:fullPage` | `{key, resource, routePrefix, resolver?, render?}` + app `compatibility` | Preview 2026-08-03; `jira:fullPage`/`confluence:fullPage` deprecated 2026-09-30 |
| `global:ui` | `{key, resource, render: native}` | EAP (barred from production) |
| `apiRoute` | `{key, path, operation, function, scopes, accept?}` | Preview |
| `rovo:agent` | `{key, name, prompt, actions?, skills?, conversationStarters?}` | GA |
| `action` | `{key, function, description, actionVerb: GET\|CREATE\|UPDATE\|DELETE\|TRIGGER, inputs?}` | GA |
| `rovo:skill` | `{key, source:{dir}, dependencies?:{tools:[action keys]}}`; `SKILL.md` frontmatter `name` (1–64 chars, = dir name), `description`, `allowed-tools` | **Preview 2026-10-02** (EAP 2026-09-10) |
| `rovo:mcp` | `{key, name, tools:[{action}]}`, at most one per app | Preview 2026-08-14; ⚠ the page still labels external clients EAP, while the 2026-10-01 changelog says Preview |
| `rovo:agentConnector` | A2A 1.0 | GA 2026-09-28 |
| `event` (app events) | `{key, name, allowedRecipients}`; `appEvents.publish` from @forge/events | Preview (2025) |
| `objectStore` | `{key, storageClass: standard}`; `@forge/object-store` | Preview 2026-06-22 |
| `sql` | `{key, engine: mysql}` | GA (pre-window) |
| `llm` | `{key, model:[claude]}`, once per app | GA 2026-07-30 |

**Deprecated or removed.**
- UI Kit 1 (`@forge/ui`) "will stop working from Feb 28, 2025" (https://developer.atlassian.com/changelog/#CHANGE-1932).
  UI Kit now means `@forge/react` with `render: native`.
- Custom UI `layout: basic` is deprecated (jira-global-page page: "blank, basic (deprecated)").
- `consumer` using `resolver:` is the old shape.
- `jira:issueGlance` → `jira:issueContext`; `jira:dashboardGadget` → `dashboards:widget`.

---

## 2. Exact current APIs

### @forge/api 8.2.0 [pkg `out/index.d.ts`]
- Exports: `default API`, `asApp`, `asUser(userId?)`, `fetch`, `requestJira`, `requestConfluence`,
  `requestBitbucket`, `route`, `assumeTrustedRoute`, `routeFromAbsolute`, `webTrigger`, `getAppContext`,
  `permissions`, `i18n`, `privacy`, `invokeRemote`, `invokeService`, plus the error classes
  (`HttpError`, `NeedsAuthenticationError`, `ProxyRequestError`, …). **There is no `storage`.**
- The product request type is `(url: Route, init?) => Promise<Response>`. A plain string throws: "You must
  create your route using the 'route' export from '@forge/api'" (`safeUrl.js`, `requireSafeUrl`).
- How `route` handles parameters (`safeUrl.js`):
  - **Path-position parameters are NOT encoded.** They are REJECTED if they contain `/`, `\`, `?`, `#` or `..`:
    "Disallowing path manipulation attempt".
  - **Query-position parameters are `encodeURIComponent`-ed.** A `URLSearchParams` is inlined.
  - Trap: interpolating a whole query string, as in `` route`/x?${'a=b&c=d'}` ``, encodes the `&` and `=`.
- `asUser()` with no user in the invocation fails. In the shim, `NEEDS_AUTHENTICATION_ERR` becomes
  `NeedsAuthenticationError` [measured]. Staff wording: "`AUTH_TYPE_UNAVAILABLE` error is thrown when `asUser`
  is called and there is no user involved … calling `asUser` in a product event or async event"
  (https://community.developer.atlassian.com/t/frequent-auth-type-unavailable-errors-when-using-api-asuser/82502).
  The web-trigger module page says: "asUser API calls will not work in web trigger functions".
- `webTrigger.getUrl(moduleKey, boolean | {forceCreate?, secretKeyConfig?})`, plus `deleteUrl` and `queryUrls`.
- Web trigger handler, `(request) => response` [pkg webTrigger.d.ts; doc events-reference/web-trigger]:
  - The request is `{method, headers: Record<string,string[]>, body: string, path, userPath, queryParameters: Record<string,string[]>}`.
  - The response is `{statusCode, headers?: {k: string[]}, body?: string, statusText?}`.
  - "If the function result is not compatible with the JSON format, then an error response with status code
    500 is sent."
  - Static triggers return `{outputKey}`.

### @forge/resolver 2.0.0 [pkg `out/index.js`]
- `new Resolver().define(key, fn)` throws on a duplicate key. `getDefinitions()` returns
  `async ({call:{functionKey, payload, jobId}, context}, backendRuntimePayload)`.
- `req.context` is the frontend context, spread, plus these fields:
  - `installContext`
  - `accountId = backendRuntimePayload.principal.accountId`
  - `license`
  - `jobId`
  - `installation`
- The result passes through `JSON.parse(JSON.stringify())`, so Dates become strings and `undefined` fields are
  dropped.
- An unknown key throws: "Resolver has no definition for '<key>'".
- The documented context (https://developer.atlassian.com/platform/forge/runtime-reference/forge-resolver/)
  also has `accountType`, `cloudId`, `localId`, `environmentId/Type` and `extension`. The page warns: "Not all of
  the values in the context parameter are guaranteed to be secure".

### @forge/bridge 7.1.0 (Custom UI frontend) [pkg]
- **Transport.** Calls go through `globalThis.__bridge.callBridge(cmd, payload)`. `getCallBridge()` runs at
  MODULE LOAD, so a missing bridge throws on import: "Unable to establish a connection with the Custom UI
  bridge". The cmd strings in 7.1.0 are:
  - Core: `invoke`, `getContext`, `fetchProduct`, `fetchRemote`, `submit`, `close`, `refresh`, `open`,
    `navigate`, `getUrl`, `reload`, `createHistory`.
  - UI chrome: `showFlag`, `closeFlag`, `openModal`, `enableTheming`, `changeWindowTitle`, `emitReadyEvent`,
    `onClose`, `getFrameId`, `getFrameDispatch`.
  - Events and realtime: `emit`, `on`, `emitPublic`, `onPublic`, `subscribeRealtimeChannel`,
    `publishRealtimeChannel`.
  - Rovo, object store and other: `openRovo`, `isRovoEnabled`, `trackObjectStoreAction`, `requestTeamworkGraph`,
    `fetchUserRecommendations`, `initFeatureFlags`, `emitFrontendCustomMetric`.
- `invoke(functionKey: string, payload?, metadata?)`:
  - The client-side rate limit is "Resolver calls are rate limited at 500req/25s".
  - A function in the payload throws.
  - An `undefined` result arrives as `{}` (ECO-277).
- `requestJira(restPath: string, init?)` takes a **plain string, not a `route`**. It always runs as the user:
  "There is no equivalent of asApp() on the @forge/bridge package"
  (https://developer.atlassian.com/platform/forge/apis-reference/ui-api-bridge/requestJira/).
  It sends `fetchProduct {product, restPath, fetchRequestInit{headers:[...entries]}}` and adds
  `X-Atlassian-Token: no-check`.
- `view`: `{submit, close, onClose, open, refresh, createHistory, getContext, getFrameDispatch, theme:{enable}, changeWindowTitle, emitReadyEvent, createAdfRendererIframeProps}`
- `router`: `{getUrl, navigate, open, reload}`
- `events`: `{emit, on, emitPublic, onPublic}`
- `showFlag({id, title, description, appearance, actions, isAutoDismiss})`
- `new Modal({resource, size, context, onClose}).open()`. Context reaches the modal under `extension.modal`.
- `FullContext` has: `accountId?, cloudId?, workspaceId?, extension, environmentId, environmentType, license?, localId, locale, moduleKey, siteUrl, timezone, theme?, surfaceColor?, userAccess?, permissions?`. For an issue panel, `extension` carries
  "type, issue.id, issue.key, issue.type, issue.typeId, project.id, project.key, project.type, isNewToIssue, location".
- **Theming.** "call view.theme.enable()", then read only `data-color-mode` (light / dark / auto); "data-theme
  … should not be read or modified". Tokens are `var(--ds-…)`.
  (https://developer.atlassian.com/platform/forge/design-tokens-and-theming/)
- **CSP** (https://developer.atlassian.com/platform/forge/extend-ui-with-custom-options/):
  - "All scripts and assets used in your Custom UI app must come from the same resource directory"
  - "you cannot fetch APIs from your static assets. Instead, you must use the invoke method"
  - Relative asset paths: use `./assets/x.png`, not `/assets/x.png`.
  - Inline `<script>`, `eval` and external CSS stay blocked unless the matching `permissions.content.*` or
    `external.*` entry is declared.
  - Iframe sandbox: "allow-scripts … but not create pop-up windows".
- **UI Kit (`@forge/react` 12.3.0).** The reconciler sends `callBridge('reconcile', {forgeDoc})` on every
  commit (`out/reconciler.js`). Offline, a UI Kit surface can therefore be graded as a forgeDoc TREE, not as
  Atlassian-rendered pixels.

### @forge/kvs 2.0.7 [pkg]
- `kvs.get/set/delete`; `set(key, value, {ttl?:{value, unit}, keyPolicy?: 'OVERRIDE'|'FAIL_IF_EXISTS', returnValue?})`.
- `batchGet/batchSet/batchDelete` return `{successfulKeys, failedKeys}`. They do not throw on partial failure.
- `getSecret/setSecret/deleteSecret`.
- `kvs.query().where('key', WhereConditions.beginsWith(x)).cursor(c).limit(n).getMany()` returns `{results, nextCursor}`.
- `kvs.entity(name).query().index(name, {partition:[…]}).where(…).filters(new Filter().and(attr, FilterConditions.x)).sort(Sort.DESC).getMany()`.
- `kvs.transact().set(k, v, entity?).delete(k).check(k, {entityName, conditions}).execute()`.
- ⚠ The package's `WhereConditions` also exposes `between`, `equalTo`, `greaterThan` and the like. The query
  docs say "The only condition supported … is `beginsWith`" for plain-key queries, so demand only `beginsWith`.
- ⚠ The query page size is inconsistent on the docs page: the prose says "maximum of 100", the sample says
  "maximum of 20".
- Limits (https://developer.atlassian.com/platform/forge/limits-kvs-ce/):
  - Keys: "Key length 500".
  - Values: "Value size 240 KiB" (bytes, not characters), "Object depth 31".
  - Throughput: "Writes 1 MB/s per key".
  - Transactions: "maximum of 25 operations".
  - Entities: "maximum of 20 entities"; "7 custom indexes and 50 attributes".
- Errors: `ForgeKvsAPIError{code}`. `KEY_NOT_FOUND` is turned into `undefined` by `get`/`delete`.

### @forge/events 3.0.7 [pkg]
- `new Queue({key})`; the key matches `^[a-zA-Z0-9-_]+$`.
- `push(event | event[])`, with each event `{body: object, delayInSeconds? 0..900, concurrency?:{key, limit}}`.
  It returns `{jobId}`.
- Client-side validation: 1–50 events and ≤ 200 KB combined (`validators.js`: `MAXIMUM_EVENTS = 50`,
  `MAXIMUM_PAYLOAD_SIZE_KB = 200`).
- Server answers: 201 ok; 202 → `PartialSuccessError{failedEvents}`; 413 → `PayloadTooBigError`;
  429 → `RateLimitError`; 405 → `InvocationLimitReachedError`.
- A consumer receives `AsyncEvent {body, queueName, jobId, eventId, retryContext?{retryCount, retryReason, retryData}}`.
- To retry, return `new InvocationError({retryAfter (≤ 900), retryReason: InvocationErrorCode.*, retryData (≤ 4 KB)})`.
- `getJob(jobId).getStats()` / `cancel()`; `appEvents.publish(...)`.
- Limits (https://developer.atlassian.com/platform/forge/limits-async-events/):
  - Rate: "Event per minute 500".
  - Payload: "Payload size for long running functions 100 KB" (when the timeout is above 55 s).
  - Retention: "lasts for 24 hours".
  - Loops: "Cyclic invocation limit 1000".

### @forge/sql 4.0.7 [pkg]
- `sql.prepare(q).bindParams(...).execute()`, `sql.executeRaw(q)`, `sql.executeDDL(q)`, and
  `migrationRunner.enqueue(name, stmt).run()`. Results are `{rows, metadata}`.
- It is MySQL-flavoured (TiDB). "Foreign keys are not supported". "Each SQL statement can only contain a single
  query". SELECT has a 5 s timeout; it is limited to 150 DML requests per second.
  (https://developer.atlassian.com/platform/forge/limits-sql/)
- **Offline cost:** it needs a real MySQL-compatible engine behind the shim. That is heavier than KVS, so keep
  it out of the first task.

### Scheduled trigger and product trigger
- Scheduled trigger handler: `({context: {cloudId, moduleKey}, contextToken}) => …`.
  ⚠ The two docs pages disagree on the return value (204/424 contract vs "returns a value, it is ignored … not
  retried"). Never grade the return value. "a small chance of duplicated invocations" exists, so jobs must be
  idempotent.
- Product trigger handler: `(event, context)`. "Events … may take up to 3 minutes".
  - `avi:jira:updated:issue` carries `changelog{id, items[{field, fieldId, from, fromString, to, toString}]}`,
    `jiraEventTypeName?` and `associatedStatuses?`.
  - The filter needs at least one of `ignoreSelf`, `expression` or `appIsLicensed`.
  - (https://developer.atlassian.com/platform/forge/events-reference/jira/,
    https://developer.atlassian.com/platform/forge/manifest-reference/modules/trigger/)

### Runtime limits (https://developer.atlassian.com/platform/forge/limits-invocation/) [doc]
- **Timeouts.** Standard functions (resolvers, triggers, validators) **25 s**. Web trigger, action and
  agentConnector **55 s**. Async consumer and scheduledTrigger default 55 s, up to **900 s** via
  `timeoutSeconds`.
  - lint enforces the same numbers: `MAX_TIMEOUT_SECONDS = 25`, `WEBTRIGGER_MAX_TIMEOUT_SECONDS = 55`,
    `LONG_RUNNING_MAX_TIMEOUT_SECONDS = 900` [pkg].
  - A function shared between a consumer and a short module gets the shortest timeout (CHANGE-1199 thread).
- **Memory and disk.** Memory is 512 MB by default and up to 1,024 MB (`memoryMB`). `/tmp` is 512 MB.
- **Payloads.** 5 MB general. "Front-end invocation request payload size 500KB"; response 5 MB.
- **Egress.** "100 per runtime minute … excluding … requestJira or requestConfluence".
- **User-led rate.** "Per user 1,200 per minute"; "Per install 7,000 per minute and 300 per second".
- **Jira points-based rate limiting** (https://developer.atlassian.com/cloud/jira/platform/rate-limiting/):
  - Enforcement "will begin on March 2, 2026". "Your app shares a single 65,000 point hourly quota across all
    tenants".
  - Per-issue writes: "20 write operations per 2 seconds" and "100 write operations per 30 seconds".
  - 429 replies carry `Retry-After`: "Only returned with 429 responses. Indicates how many seconds to wait
    before retrying".
  - `RateLimit-Reason` takes `jira-quota-global-based`, `jira-quota-tenant-based`, `jira-burst-based` or
    `jira-per-issue-on-write`.

### Jira REST specifics the mock must enforce [doc: v3 OpenAPI + measured]
- **`GET|POST /rest/api/3/search/jql`:**
  - `fields` default `id`.
  - `maxResults` default 50.
  - `nextPageToken` is absent on the last page, and "This token will expire in 7 days".
  - `isLast`.
  - Bounded JQL is required: 400 "Unbounded JQL queries are not allowed here." [measured]
  - At most 7 ORDER BY fields.
- **`POST /rest/api/3/search/approximate-count`** takes `{jql}` and returns `{count}`.
- **`POST /rest/api/3/issue/bulkfetch`** returns ≤ 100 issues by default, or ≤ 1000 when `fields` is named.
- **ADF bodies.**
  - Comment `body`, issue `description`, `environment` and textarea custom fields must be ADF:
    `{"version":1,"type":"doc","content":[…]}`.
  - A plain string gets 400 "Operation value must be an Atlassian Document".
  - (https://developer.atlassian.com/cloud/jira/platform/rest/v3/intro/,
    https://developer.atlassian.com/cloud/jira/platform/apis/document/structure/)

---

## 3. What runs OFFLINE (no login, no site) — and how a scorer uses it

| capability | offline? | how | proof |
|---|---|---|---|
| `forge lint` (CLI) | **No** | `requireAppId()` + server-side `getAppPreDeploymentCheck` | `@forge/cli` lint-controller.js; skill note "`forge lint` needs a login so it is SKIP in CI" |
| `@forge/lint` client-side linters | **Yes** | `lint(files, manifest, 'development', logger, {getDeprecatedRuntimes: async()=>[]}, {linter:{mode:'client-side'}})`, manifest via `new ConfigFile(new FileSystemReader()).readConfig()` from `@forge/cli-shared` | [measured, network denied] |
| `@forge/manifest` schema validation | **Yes** | runs inside the lint above (`FullManifestLinter`), or `ProcessorBuilder.instance().withValidation(ValidationTypes.FULL)` | [pkg README] |
| @forge/api, kvs, events, resolver backend code | **Yes** | define `global.__forge_runtime__` + `global.__forge_fetch__`, `require()` the app's bundle, call handlers directly | [measured: paginated search/jql with an injected 429, KVS set+query, queue push, asUser rejected] |
| Custom UI frontend | **Yes** | serve `resources[].path` statically; Playwright `addInitScript` defines `globalThis.__bridge = {callBridge}` BEFORE the bundle loads; `invoke` → resolver handler in Node → shim → mock Jira | [pkg bridge.js; design] |
| UI Kit frontend | **Partly** | capture `reconcile {forgeDoc}`; assert the component tree; no Atlassian pixels | [pkg reconciler.js] |
| Forge SQL | Partly | needs a MySQL-compatible engine behind `{type:'sql'}` | [pkg] |
| official local emulator | **No** | `forge tunnel` needs a login and a deployed app: "You must deploy your app at least once before running `forge tunnel`" (https://developer.atlassian.com/platform/forge/tunneling/) | [doc] |

### Everything client-side lint checks (16 linters, `@forge/lint/out/lint/lint.js`) [pkg + measured messages]
1. **PermissionLinter.** It parses source ASTs and maps each `requestJira`/`requestConfluence` call (method +
   path, against the bundled OpenAPI) to the scope it needs, for example "Jira endpoint: PUT
   /rest/api/3/issue/{issueIdOrKey} requires "write:jira-work" scope".
   - It also flags an external `fetch` host missing from `external.fetch` (`egress-permission-required`), image
     URLs, the notification API and UI hooks.
   - It recommends CLASSIC scopes even when a granular one is present (`read:issue:jira` → "requires
     read:jira-work") [measured].
2. **PermissionsManifestLinter.** Invalid scopes; deprecated bare-string egress (WARNING); invalid
   `content.scripts`.
3. **AgentProductContextPermissionsManifestLinter.**
4. **DeprecatedCspPermissionsManifestLinter.**
5. **FullManifestLinter.** JSON-schema validation and duplicate keys. The messages seen:
   - "invalid value 'jira:issuePannel' in modules"
   - "jira:issuePanel required properties are 'function, icon, title, key' or 'icon, resource, title, key'"
   - "interval 'hourly' allowed values are …"
   - "app runtime property name 'nodejs18.x' allowed values are …"
   - "found duplicate module key 'hook'"
   - "Client Side UI Kit resource … cannot be a directory"
6. **HandlerLinter.** "Cannot find exported function "X", which is required by module "Y"". This is only a
   WARNING.
7. **DynamicPropertiesPermissionsLinter.** Icon URLs.
8. **InvokeEndpointLinter.** Remote keys must exist in `remotes`.
9. **StorageModulesLinter.** "SQL package is used but 'sql' module is not defined" is an ERROR. The same check
   covers `@forge/os` and `@forge/object-store` against `objectStore`.
10. **FrameComponentLinter.** The `resource` of a Frame component.
11. **LlmModuleLinter.** "LLM package is used but 'llm' module is not defined" is only a WARNING.
12. **DeprecatedApiModuleLinter.** "The "storage" export from "@forge/api" is deprecated" (WARNING, plus the
    storage:app scope ERROR).
13. **DeprecatedEgressPermissionsManifestLinter.**
14. **AppManagedPermissionsSdkLinter.** With `enforcement: app-managed`, scope-needing calls must have
    Permissions-SDK checks.
15. **TeamworkGraphLinter.**
16. **FunctionTimeoutLinter.** It rejects `timeoutSeconds` on non-long-running modules and a consumer function
    shared with a short module.

**Lint is STAGED.** Schema errors hide the later linters. In the probe, fixing `interval`/`runtime` surfaced
the module-key typo, and fixing that surfaced the missing `icon` [measured]. A scorer must report the FINAL
lint state, not the first pass.

**Lint BLIND SPOTS that the runtime shim must catch instead [measured]:**
- Missing `storage:app` scope while using `@forge/kvs`.
- The removed `/rest/api/3/search`.
- Unknown or made-up endpoints, which lint passes silently.
- A plain-string path given to `requestJira`, which throws at runtime.
- `asUser` in triggers.
- Unpaginated search.
- Missing `fields` on `/search/jql`.
- ADF-less comment bodies.
- 429 handling.
- `layout: basic`.
- A resource path pointing at `src` instead of `build`.
- Lint passes duplicate extension keys across module, function and resource in some shapes: "forge lint
  reported 'No issues found'" while deploy failed
  (https://community.developer.atlassian.com/t/forge-lint-is-not-checking-extension-keys-correctly/47534).
  It did catch the webtrigger/function clash in our probe.

### Minimal shim (verified, scratchpad `harness/shim.js`)
```js
global.__forge_runtime__ = { proxy:{token:'t',url:'http://x'}, contextAri:'ari:cloud:jira::site/abc',
  appContext:{appId:'a',appVersion:'1',environmentId:'e',environmentType:'DEVELOPMENT',invocationId:'i',
    installationId:'in',functionKey:'sweep',moduleType:'scheduledTrigger',moduleKey:'nightly'},
  lambdaContext:{awsRequestId:'r',getRemainingTimeInMillis:()=>55000}, tracing:{traceId:'t',spanId:'s'},
  realtime:{}, featureFlags:()=>false, aaid: undefined /* set for user-led invocations */,
  metrics:{counter:()=>({incr(){},incrBy(){},decr(){},decrBy(){}}),timing:()=>({measure:()=>({stop(){}})}),gauge:()=>({set(){}})} };
global.__forge_fetch__ = async (target, path, init) => { /* route by target.type/provider/remote → mock Jira/Confluence/KVS/queue; return a WHATWG Response */ };
```
- The probe ran 4 search/jql calls (one injected 429 with `Retry-After`, then 3 pages of 2), then
  `kvs.set` + `kvs.query(beginsWith)` + `Queue.push` (needs 201).
- `asUser()` in a trigger context gave `NeedsAuthenticationError` [measured].
- The probe's OWN first draft looped with `do { … if (429) continue; … } while (token)`. `continue` jumps to the
  condition, where `token` is still undefined, so it exited after the 429 with ZERO issues. A natural trap for
  models too.

---

## 4. Where models fail (sourced; these are the probes a tough task should contain)

| # | pitfall | evidence |
|---|---|---|
| 1 | `import { storage } from '@forge/api'` — removed in 8.0.0 | @forge/api CHANGELOG `561f8f4`; "As of March 17, 2025, we stopped applying feature updates" https://developer.atlassian.com/platform/forge/storage-reference/kvs-migration-from-legacy/ |
| 2 | `startsWith` (legacy) instead of `WhereConditions.beginsWith` | same migration page |
| 3 | `/rest/api/3/search` + `startAt`/`total` | 410 measured; CHANGE-2046 "startAt parameter will be replaced with nextPageToken" |
| 4 | `/search/jql` without `fields` → issues come back as `{id}` only | OpenAPI "By default, this resource returns IDs only" + measured |
| 5 | unbounded JQL (`order by created`) → 400 | measured "Unbounded JQL queries are not allowed here." |
| 6 | missing `route` / string path | `safeUrl.js` error text; https://developer.atlassian.com/platform/forge/apis-reference/fetch-api-product.requestjira/ |
| 7 | whole query string interpolated into `route` (gets encoded) | `safeUrl.js` query mode `encodeURIComponent` |
| 8 | `asUser()` in scheduledTrigger / consumer / webtrigger / product trigger | staff quote (#AUTH_TYPE_UNAVAILABLE thread), web-trigger module page |
| 9 | comment/description as a plain string | "Operation value must be an Atlassian Document" (community thread title) |
| 10 | no 429 / `Retry-After` handling; 2026 points regime; per-issue write limit | rate-limiting page quotes above |
| 11 | scope missing (`write:jira-work`, `storage:app`, `read:page:confluence`) | lint messages measured; "Unauthorized; scope does not match" https://community.developer.atlassian.com/t/unauthorized-scope-does-not-match-forge-app/49459 |
| 12 | Confluence: granular scope on v1 path (or vice versa) | https://community.developer.atlassian.com/t/unauthorized-scope-does-not-match-confluence/73795 |
| 13 | Custom UI CSP: external CSS/script, absolute `/assets` paths, fetch from frontend | CSP thread https://community.developer.atlassian.com/t/content-security-policy-problem-with-jira-issue-panel-with-custom-ui/75376 ; custom-options page |
| 14 | resolver key mismatch / manifest handler not the exported name → "Resolver has no definition for …" | https://community.developer.atlassian.com/t/forge-custom-ui-resolver-has-no-definition-for-getpreviousstatus-error-when-using-data-in-react-app-edit-js/89962 |
| 15 | resolver returns `undefined` → frontend gets `{}` | https://jira.atlassian.com/browse/ECO-277 |
| 16 | UI Kit 1 (`@forge/ui`, `function:` UI) instead of `@forge/react` + `render: native` + `resource` | https://developer.atlassian.com/platform/forge/ui-kit/upgrade-to-ui-kit-latest/ |
| 17 | webtrigger response shape (headers not `string[]`, body not string, missing statusCode) → 500 / 424 | web-trigger events page; https://community.developer.atlassian.com/t/getting-status-as-424-and-failed-dependency-error-when-hitting-the-web-trigger-of-forge-app/92845 |
| 18 | >50 events / >200 KB per push; consumer with old `resolver:`; `timeoutSeconds` on a resolver | validators.js; lint FunctionTimeoutLinter; https://community.developer.atlassian.com/t/the-async-event-times-out-after-25-seconds/86377 |
| 19 | long work inside a 25 s resolver | https://community.developer.atlassian.com/t/forge-timeout-issue-after-25-seconds/85097 |
| 20 | KVS: value > 240 KiB (bytes!), query on non-key fields, custom entity without manifest index, attribute-type mismatch | limits-kvs-ce; entities page "queries require an index declared in the manifest" |
| 21 | check-then-set lock races in KVS (no CAS; use `keyPolicy:'FAIL_IF_EXISTS'` or `transact().check`) | skill `gotchas.md` (se-ppm write-lock), kvs types |
| 22 | newest-module ignorance: `jira:dashboardGadget`/`jira:fullPage`/`jira:issueGlance` instead of `dashboards:widget`/`global:fullPage`/`jira:issueContext`; `nodejs18.x` | changelog 2026-05-05/08-06/09-23; lint runtime enum |

No published benchmark or paper scores LLMs on Forge. The closest artefacts are Atlassian's Forge MCP server
(https://developer.atlassian.com/platform/forge/forge-mcp/, "may become out-of-date… always verify") and the
`atlassian/forge-skills` repo.

---

## 5. Newest modules (Preview / EAP / new GA) — what a benchmark may demand

**The rule:** demand only what has a public docs page. EAP needs sign-up and runs only in development, so never
demand it. Preview and freshly-GA modules are the "how current is the model" probe.

| module / capability | status (date) | manifest key + shape | API / handler | demandable? |
|---|---|---|---|---|
| **Rovo skill** | Preview 2026-10-02 | `rovo:skill: {key, source:{dir}, dependencies:{tools:[<action key>]}}`, wired via `rovo:agent.skills`; `SKILL.md` frontmatter `name` = dir name (1–64), `description`, `allowed-tools` | no code; instructions + references orchestrating `action`s | **Yes**, structural grade + the action functions run offline |
| **Rovo action** | GA | `action: {key, function, actionVerb, description, inputs}` | `(payload, context)`, where "The payload and context are passed into the function as arguments" | **Yes**, invoke offline with an inputs payload |
| **dashboards:widget** | GA 2026-09-22 (gadget deprecated 2026-09-23) | `{key, title, description, thumbnail, resource, edit, resolver?, render?}` | Custom UI or UI Kit | **Yes**, the strongest "is it current" probe with a full docs page |
| **jira:command** | Preview | `{key, title, target{page\|resource}, shortcut?, keywords?}` | opens a page or modal | Yes (structural + modal resource screenshot) |
| **global:fullPage** | Preview 2026-08-03 | `{key, resource, routePrefix, render?}` + `app.compatibility` | URL `https://<tenant>/apps/full-page/<installationId>/<route-prefix>/<app-route>` | Yes, but the compatibility block is fiddly ⚠ |
| **apiRoute** | Preview | `{key, path, operation, function, scopes, accept}` | ⚠ handler request/response shape not captured here | Only after reading its page |
| **App-managed permissions** | Preview 2026-07-01 | `permissions.enforcement: app-managed` | `permissions.hasScope / canFetchFrom / hasPermission` (@forge/api), `checkPermissions` (@forge/bridge), `usePermissions` (@forge/react); lint linter #14 enforces it | Yes; lint grades it offline |
| **objectStore** | Preview 2026-06-22 | `{key, storageClass: standard}` | `import fos from '@forge/object-store'`; `fos.get/delete/createUploadUrl/createDownloadUrl/…` | Possible (mock `{type:?}` transport ⚠ unmeasured) |
| **jira:actionValidator** | Preview | `{key, action, expression, errorMessage}` | Jira expression only | Hard to grade offline (needs a Jira-expression evaluator) |
| **app events** | Preview | `event: {key, name, allowedRecipients}` | `appEvents.publish` (@forge/events) | Yes (shim sees the publish) |
| **rovo:mcp** | Preview 2026-08-14 / ⚠ page says EAP for external clients | `{key, name, tools:[{action}]}` | — | Avoid (status inconsistent) |
| **Forge LLM** | GA 2026-07-30 | `llm: {key, model:[claude]}` | `chat({model, messages, …})` | Possible, but adds model-in-the-loop cost and nondeterminism; avoid |
| **global:ui**, **dashboards:filter** | EAP | — | — | **No** |
| **fifoConsumer** | ⚠ schema only, no docs, no SDK | — | — | **No** |

---

## 6. Candidate tasks (realistic, non-tutorial, graded by running the app against a mock)

Every candidate uses the same harness:
- A seeded mock Jira/Confluence behind the shim.
- The real `@forge/*` packages, pinned in the STARTER with an offline npm cache, so no installs are needed
  during the run.
- Client-side lint to its fixpoint.
- Direct handler invocation for triggers, consumers, webtriggers and actions.
- A Playwright `__bridge` shim for every Custom UI resource, with screenshots in light AND dark
  (`view.theme.enable` + `data-color-mode`).

The mock enforces the runtime rules lint cannot see:
- scope per endpoint, including `storage:app` for KVS
- 410 on the old `/search`
- ids-only without `fields`
- 400 on unbounded JQL
- ADF bodies
- scripted 429s with `Retry-After` / `RateLimit-Reason`
- the per-issue write limit
- asUser rejected when no `aaid` is set
- the queue push limits

### A. Sprint Scope-Creep Ledger (RECOMMENDED; detail in §7)
Product trigger on issue updates, a queue consumer, KVS custom entities, `search/jql` reconciliation,
`dashboards:widget` + `jira:sprintAction` Custom UI, a Rovo `action` + `rovo:skill`, and a JQL function.

### B. Blocker-Chain Radar
- **What it does.** An hourly `scheduledTrigger` walks `issueLinkType = "is blocked by" AND statusCategory !=
  Done AND project in (…)`. It builds blocker DAGs across projects (cycles included), stores per-project
  snapshots in a KVS entity indexed by risk, and fans each project out to a queue (≤ 50 events per push).
  `jira:issueContext` (the issueGlance replacement) shows a chain-depth badge, with a Custom UI panel for the
  graph.
- **Graded on.** Cycle detection on seeded loops; chain depth exactly matching a reference; correct
  pagination over 237 seeded issues at `maxResults` 50; fan-out batching.
- **Traps.** Link direction (`inwardIssue` vs `outwardIssue`), ids-only search, and the 25 s resolver limit.

### C. Confluence Runbook Freshness Steward
- **What it does.** A daily trigger lists pages under a space via Confluence v2 cursor pagination
  (`_links.next`) with the label `runbook`. It flags pages whose last version is older than their owner's
  stated SLA in a page property. It writes an ADF/storage-format footer comment, and a `confluence:pageBanner`
  (Custom UI) shows staleness.
- **Traps.** v1/v2 path vs granular scopes; storage vs ADF body formats; cursor links being RELATIVE; the
  `inScopeEUD` egress form if a webhook out is required.
- **Why not first:** it needs a second mock product, which costs more fixtures.

### D. Release Freeze Gatekeeper
- **What it does.** A `jira:actionValidator` (Preview) blocks `workItemMoved` during a freeze window, using a
  Jira expression. A web trigger (`response.type: dynamic`, HMAC per the schema) lets CI open and close
  freezes. A `global:fullPage` (Preview) admin page edits the windows.
- **Why it is held back:** grading needs a Jira-expression evaluator, and `hmacSharedSecret`'s docs page is
  unconfirmed. ⚠ Lower confidence that the grade can be made fair.

### E. Customer-Escalation Cost Ledger on Forge SQL
- **What it does.** `sql` migrations through `migrationRunner`, per-org escalation costs, and a resolver doing
  paged SQL (LIMIT/OFFSET with a deterministic ORDER BY).
- **Why it is held back:** it needs a MySQL/TiDB engine in the scorer. Keep it for tier 2.

---

## 7. Recommended task — **Sprint Scope-Creep Ledger** (tier 1)

**The pitch to the model:** an Agile coach wants to know what was ADDED to a sprint after it started, by whom,
and with how many story points, plus a reconciliation for anything the event stream missed.

### Required surface
At least five module types, including one Preview and one GA-since-September module.

1. **`trigger`** on `avi:jira:updated:issue`. It uses `filter: {ignoreSelf: true}` and an `expression` that keeps
   only events whose changelog touches the Sprint field. It reads `changelog.items[]` where `fieldId` is the
   sprint field and `to`/`from` are sprint ids. Work must be under 25 s, so it pushes to a queue.
2. **`consumer`** (`function:` shape) on queue `scope-ledger`.
   - Idempotent per `eventId`: `kvs.set(..., {keyPolicy:'FAIL_IF_EXISTS'})`.
   - Writes a `scope-change` **custom entity** (index `by-sprint`: partition `[sprintId]`, range `[addedAt]`).
   - Computes the story points via `GET /rest/api/3/issue/{key}?fields=<points field>`.
   - Handles a scripted 429 with `Retry-After` by returning `InvocationError({retryAfter})`. Sleeping past the
     limit is the wrong answer.
3. **`scheduledTrigger`** (`interval: hour`). It reconciles via `POST /rest/api/3/search/jql` with
   `jql: sprint in openSprints() AND project = OPS`, `fields:[…]` and `nextPageToken` pagination over 237
   seeded issues. Additions the trigger missed are backfilled, marked `source: reconcile`. Pushes are ≤ 50
   events.
4. **`dashboards:widget`** (GA 2026-09-22; `jira:dashboardGadget` is the stale answer). It is a Custom UI that
   charts committed vs added points per open sprint and must render in light and dark.
5. **`jira:sprintAction`** (`actionType: modal`, Custom UI). It is a per-sprint ledger table: who added what and
   when, sortable. It posts an ADF comment summarising creep on a chosen issue through the resolver, `asUser()`
   because it is user-led.
6. **`action`** (`actionVerb: GET`, `inputs: {sprintId}`) + **`rovo:skill`** (Preview 2026-10-02) +
   **`rovo:agent`** listing the skill. `skills/scope-creep/SKILL.md` is named after its dir, and
   `dependencies.tools: [<action key>]`.
7. **`jira:jqlFunction`** `addedAfterStart(sprintId)`. ⚠ Its handler contract (what it returns: a JQL clause or
   an issue list) was NOT captured in this research. Read the module page before demanding it; drop it if the
   contract is thin.

### Gradeable behaviours (all by running)
- **Lint.** It is clean at fixpoint, using client-side lint. The task carries lint-traps in its starter:
  - a bare-string egress (warning)
  - a `nodejs18.x` runtime
  - a consumer sharing a function with the widget resolver
- **Trigger.** Replay 40 seeded `avi:jira:updated:issue` events: sprint adds, removes, re-adds, non-sprint edits
  and self-generated events. The expected rows must be exact, ignoring self-events and non-sprint edits.
- **Consumer.**
  - Duplicate `eventId` gives no double row.
  - A 429 gives an `InvocationError` with `retryAfter` ≥ the header value.
  - A 202 partial push is handled.
- **Reconcile.**
  - 237 issues over 5 pages; the mock serves ids-only when `fields` is missing. The ledger must equal the
    reference set exactly.
  - No call may hit `/rest/api/3/search` (410).
  - `asApp()` only, since the shim rejects `asUser` with no user.
- **KVS.**
  - `storage:app` declared, since the mock 403s without it.
  - Values ≤ 240 KiB.
  - Entity queries by partition + range return the right order.
- **Custom UI.**
  - The served `build/` loads with the shim bridge, with no CSP-violating assets (Playwright CSP set to the
    Forge default).
  - `view.theme.enable()` is called.
  - Screenshots in light and dark, plus DOM assertions: totals equal the reference per sprint, and the table
    sort is right.
  - `invoke` keys exist in the resolver.
- **ADF.** The comment POST body is valid ADF (a schema check), sent `asUser`, and stays inside the per-issue
  write limit.
- **Rovo.** Invoke the action with `{sprintId}` and check the JSON against the reference. The skill dir
  structure and frontmatter are valid; the agent lists the skill.
- **Currency.** `dashboards:widget`, not `jira:dashboardGadget`; `@forge/kvs`, not `storage`; `render: native`
  or Custom UI, never `@forge/ui`.

### Why it is not benchmaxxed
- No hello-world panel, no todo app, and none of the Forge tutorials' examples.
- The sprint-changelog domain (sprint field ids in `changelog.items`) and the reconcile-vs-event-stream design
  are rare in training data.
- The newest modules make up about 30% of the surface.

### Economics
- The fixtures are JSON: 237 issues, 3 sprints, 40 events.
- The scorer is Node + Playwright only: no Atlassian login, no deploy, no SQL engine.
- The two Custom UI builds are the only heavy step. Pin a bundler in the STARTER with an offline cache.

### Confidence (stated honestly)
- **HIGH** on the backend grading path. The shim was measured driving the real packages.
- **HIGH** on lint. It was measured offline.
- **MEDIUM** on the Custom UI shim. The design follows bridge.js, but no Custom UI build was run through it
  here.
- **MEDIUM-LOW** on the jqlFunction and apiRoute contracts. They were not researched to handler level; read
  their pages before demanding them.
- **LOW** on the status of `rovo:mcp`, `fifoConsumer` and `hmacSharedSecret`. Excluded.

---

## 8. Local LeanZero skill facts used

- `leanzero-forge-app-baseline`:
  - "`forge lint` needs a login so it is SKIP in CI" (2026-09-26).
  - "Forge caps scheduledTrigger modules at 5". This is confirmed by
    https://developer.atlassian.com/platform/forge/limits-scheduled-trigger/ ("Total number of scheduled
    trigger modules in an app 5").
  - "Forge storage comes back EMPTY after a reinstall".
- `atlassian-jira-forge-skill/docs/gotchas.md`:
  - A stubbed `node_modules/@forge/*` (version `0.0.0`) deploys silently, and lint, webpack and unit tests all
    pass. Our scorer must therefore assert the REAL package versions are what got loaded: check
    `require('@forge/kvs/package.json').version` against the pin.
  - KVS has no CAS.
  - `POST /issue/bulk` returns only the successes in `body.issues`.
  - The 240 KiB limit is bytes, not characters.
  - `no-use-before-define` TDZ crashes appear only at runtime.
- `19-rate-limit-handling.md`: POST-that-reads is charged per object ("search/jql measured at ~11.4 pts/call"),
  and UI `@forge/bridge` reads are exempt from app points.
