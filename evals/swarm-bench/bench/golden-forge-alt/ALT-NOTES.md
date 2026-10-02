# golden-forge-alt: notes from a second, independent implementation

This app was built only from what an entrant gets: `forge/public/{spec-build-forge.md,
FORGE-CONTRACT.md, STARTER.md}`, `forge/kit/README.md`, the installed `@forge/*` packages, the kit's
OpenAPI and manifest schema, `forge-dev`, and public docs on developer.atlassian.com and
support.atlassian.com. The reference app, the scorer, the oracle, the probe, the mutants, DESIGN.md
and `forge/site/**` source were not read. The dev site was started through `createSite({seed, port})`
as the kit README describes, with seed `a1b2c3d4e5f60718`.

Its purpose is to show whether the mock Jira and the emulator accept a *different* valid
implementation. Section 1 lists the places where they did not, or where the contract or kit made
the choice ambiguous. Section 2 lists the alternatives they did accept.

**Revision 2 (contract 2006de559).** Section 4 covers Forge LLM, Realtime and `rovo:mcp`.
Earlier findings were re-checked against the fixed kit and are marked **FIXED**. The kit used was
`825be630e817fabf/kit-9c8a0fbf6acb97c6`, from WP1's uncommitted working tree, which adds `@forge/llm`
and `@forge/realtime`.

## 0. What this app does differently

| concern | this app |
|---|---|
| changelog source | `GET /rest/api/3/issue/{id}?expand=changelog` for events; `expand=changelog` on `/rest/software/1.0/sprint/{id}/issue` and `/rest/api/3/search/jql` for reconcile. When `changelog.total` exceeds the embedded page it falls back to `/issue/{id}/changelog`. It never calls `/changelog/bulkfetch`. |
| sprint members | `/rest/software/1.0/sprint/{id}/issue`, the current replacement for the deprecated agile 1.0 path |
| leavers | one JQL: `updated >= "<earliest start − 1 d>" AND (sprint not in (<active ids>) OR sprint is EMPTY)` |
| visibility | `asApp()` plus `POST /rest/api/3/permissions/check` with the viewer's `accountId` and `BROWSE_PROJECTS` on the issue ids. Nothing is read `asUser()`. |
| comment | posted from the Custom UI with `@forge/bridge` `requestJira`, which is user-led. The 429 / `Retry-After` retry runs in the browser. |
| issue link | `router.navigate({ target: 'issue', issueKey })`, a `NavigationLocation`, not a `/browse/` string |
| explain (LLM) | `stream()` (not `chat()`). The model is the lexicographically last `list()` model with status `active`. Tool calls are folded across chunks (object arguments merged; string fragments concatenated, or replaced when a chunk resends the whole string), then read strictly with hand-written type guards: exactly one call, named `report_scope`, an object argument, a non-empty `summary` string and a string array of `changeIds`. No JSON-schema validator. |
| live widget | One global channel per shown sprint (`scope-ledger-sprint-<id>`), published with `publishGlobal` by the consumer only when that sprint's ledger rows changed. The payload is `{sprintId}`. There are **no realtime tokens**. The widget makes one `subscribeGlobal` per sprint and coalesces reloads. |
| widget save | no `onProductSave` handler. Only `widgetEdit.updateConfig`, so the host stores the last value. |
| scheduled job | Reads Jira and the ledger, computes the diff, and pushes only the missing rows to the queue. The consumer performs every write. A run with nothing new pushes nothing. |
| consumer form | `consumer.function` (direct function), not `resolver: {function, method}` |
| KVS | Two entities. `scope-change` has key `<sprint>#<changelogId>` and index `sprint-time` (partition `sprintId`, range `atMs` **float**). `scope-member` has key `<sprint>#<issueId>` and index `sprint-issue` (partition `sprintId`, range `issueNum`); it stores each issue's flags per sprint (`atStart`, `inNow`, `everIn`) and its current points. A plain `registry` key holds the sprint field, the estimation field per board, and the sprints. Exactly-once comes from the deterministic key plus `keyPolicy: 'FAIL_IF_EXISTS'`, not from a transaction. |
| Custom UI | Plain DOM and TypeScript bundled with esbuild, with no React and no inline styles. The chart is SVG with `x` and `width` in percent and `height` in px. |
| 429 | Each invocation gets a wait budget: 20 s for the consumer and resolvers, 600 s for the hourly job (`timeoutSeconds: 900`). A wait that does not fit becomes `InvocationError({retryAfter, retryData})`. |

## 1. Where a valid alternative failed, or the contract or kit was ambiguous

### 1.1 The site's `openSprints()` also matched FUTURE sprints. This was the defect with real impact. **FIXED** in 9aa46664e.

Public docs (support.atlassian.com, *JQL functions*, `openSprints()`): *"Search for work items that
are assigned to a sprint that was started, but has not yet been completed."* `futureSprints()`:
*"…assigned to a sprint that hasn't been started yet."* STARTER lists `openSprints()` and says the
site "behaves like Jira Cloud".

Measured on the dev site with four issues whose only sprints are future (885/603), or closed plus future (143, 885):

```
key in (PAY-199, PAY-203, PAY-224, SRCH-319) AND sprint in openSprints()      -> all 4
key in (...) AND sprint not in openSprints()                                    -> []
key in (...) AND NOT sprint in openSprints()                                    -> []
key in (...) AND sprint in futureSprints()                                      -> all 4
key in (...) AND sprint not in closedSprints()                                  -> 3 (PAY-203 excluded, correct)
```

The first version used the recipe the public docs imply for "issues that left the active sprint":
`… AND (sprint not in openSprints() OR sprint is EMPTY)`. Its first backfill missed exactly the
issues moved from an active sprint into a future one: 4 issues and 5 changes (`321#30897`,
`365#30852`, `365#29913`, `365#30221`, `365#30022`). Those are "removed" rows and "removed"
points. A brute-force recompute over every issue on the site found the gap. The app now names
the active sprint ids explicitly.

Re-verified on the fixed kit: `sprint in openSprints()` now returns `[]` for those four issues, and `not in` returns all four. The app still lists explicit sprint ids.

Caveat: community threads disagree about whether production Cloud includes future sprints in
`openSprints()`. The public doc text does not include them. Either the mock follows the doc, or
STARTER should state the site's semantics.

### 1.2 `/rest/software/1.0/sprint/{id}/issue` answered in the agile 1.0 shape, not the documented one. **FIXED** in 9aa46664e.

In the shipped `openapi/jsw.json`, this operation has the parameters `nextPageToken`, `maxResults`,
`reconcileIssues`, `jql`, `validateQuery`, `fields` and `expand`. It has **no `startAt`**, and the
response example carries `isLast`. The same file marks `/rest/agile/1.0/sprint/{id}/issue` as
`deprecated`.

The dev site answers the software path with `{expand, startAt, maxResults, total, issues}`. It has no
`isLast` and no `nextPageToken`, and it honours the undocumented `startAt`
(`maxResults=5&startAt=5` returned issues 6 to 10). An app written to the OpenAPI loops while
`!isLast` and follows `nextPageToken`, so it reads exactly one page. With no `maxResults` the site
returned 21 of 21, so dev-size sprints hide the problem. The scoring seed may not. This app follows
both shapes.

Re-verified: the software path now answers `{expand, issues, nextPageToken, isLast}`, and the agile path keeps `startAt/total`. The app now pages the software path by token only.

### 1.3 The scope for `POST /rest/api/3/permissions/check` is ambiguous under the contract's scope rule

Contract §2: *"per call, the OAuth2 scopes the shipped OpenAPI lists for it (the classic scope where
one exists, else the whole granular set)"*. In `openapi/jira.json` this operation has
`security: [{basicAuth}, {OAuth2: []}]` and `x-atlassian-oauth2-scopes` with `Current: []` (empty)
and `Beta: ["read:permission:jira"]`. Its description says *"Classic: `read:jira-work`, Granular:
`read:permission:jira`"*.

Under that rule an entrant cannot tell whether the call needs `read:jira-work`, nothing, or
`read:permission:jira`. This app needs `read:jira-work` for other calls and does not request
`read:permission:jira`. The site answered 200. An app that used `permissions/check` but no other
`read:jira-work` call would not know which scope is "needed and nothing more".

### 1.4 `permissions/check` for another user as the app: the mock allows it, and the docs only exempt Connect

The OpenAPI text says *"Administer Jira global permission to check the permissions for other users
… Connect apps can make a call from the app server … for any user"*. It says nothing about Forge
`asApp()`. The site granted the app the check for any `accountId`. For 3 users × 3 sprints the result matched
`asUser().requestJira(GET /issue/{id})` exactly (0 mismatches, 69 issues per user). Contract §3
explicitly allows "the app with an explicit permission check", so the behaviour is accepted. It is
recorded here because the public docs do not say a Forge app may do this.

### 1.5 The KVS `FAIL_IF_EXISTS` failure code is undocumented

`kvs.entity(...).set(key, v, { keyPolicy: 'FAIL_IF_EXISTS' })` on an existing key throws
`code: "CONDITIONAL_CHECK_FAILED"` with message `Key 'scope-change\u00001#9' already exists.`. The kit
README says this code is one "no docs page names". The `keyPolicy` option itself is in the
`@forge/kvs` 2.0.7 typings. An app that relies on first-writer-wins has to guess the code or match
the message. This app checks the code and falls back to matching "already exists".

### 1.6 Unspecified behaviours where this app had to choose

- Rovo `get-sprint-scope` for a **future** sprint: §6 defines only unknown or missing ids. This app
  returns `{error: "… has not started yet …"}`. For a closed sprint it returns whatever the
  ledger holds.
- The Rovo function received the person as `context.principal.accountId`, while the public Rovo
  action page says *"The `context` contains the user's `accountId`"*. **FIXED** in ad1f180b3: the kit
  README now says both are sent. Re-verified: as Elif, `hiddenChanges` is 5 for sprint 365, the same
  as before.
- JQL date literals: `"2026-10-22 07:25"` works, and an ISO literal with `T…Z` returns 400, as in real
  JQL. STARTER does not say which timezone the literal is read in (the app user's?). This app
  subtracts a day of margin.
- The function clock is the site's clock (`new Date()` returned `2026-10-29…` on 2026-10-02). That is
  correct, but STARTER does not mention it. Any `-Nd` computed from wall time would be wrong.

### 1.7 Dev-kit friction, which an entrant would hit while testing

- **FIXED** (README and `--help` now document it): `FORGE_SITE_URL` must carry the control token as the URL **password**. With the token in the path
  (`/__site/<token>`) forge-dev fails with `FORGE_SITE_URL carries no dev-site control token`.
  Neither the README nor STARTER gives the format. The harness sets it for entrants, so this matters
  only when running forge-dev yourself.
- **FIXED** (STARTER now names `jsw.json`): STARTER named `openapi/jira.json` and `openapi/jira-software.json`. The kit ships
  `openapi/jsw.json`, which is the Jira Software file, plus bb/conf/confv2/jsm.
- **FIXED** per ad1f180b3, which made the lock race-free (two serves started 1 s apart both came up; four were not retried): four `forge-dev serve` processes started 0 to 4 s apart, and one died with
  `forge-dev: ENOENT: no such file or directory, open '…/.forge-dev/state.lock'`. The README says
  *"its state lives in `.forge-dev/` under a lock so several processes can share a workspace"*.
- `serve` with a malformed `--config` printed `Expected property name or '}' in JSON` but **kept
  running**. A later `kill` was needed.
- Now documented as intended, like a dashboard: widget config persists across `serve` runs. After an edit-surface Save stored `boardId 138`, a
  later `serve scope-widget` with no `--config` rendered board 138, not `needs-config`.
  `--config '{}'` is needed to see the empty state. This is not documented.
- **FIXED** (`.forge-dev/last-result.json` now holds the full result): `invoke` truncated large results (`… (9067 chars)`) and had no `--json`, so a resolver's whole
  result cannot be checked from the CLI.
- `invoke … --resolver` for a `jira:sprintAction` requires `--sprint` even when the payload carries
  the id (`--sprint (missing): not a sprint on the dev site`).
- The serve log prints `bridge navigate` without the location, so an entrant cannot see which URL a
  `NavigationLocation` resolved to. It does map `{target:'issue', issueKey}` to `/browse/KEY`.
- **FIXED**: the served page is top level, so the browser requests `/favicon.ico` on the serve origin,
  which gave a 404 console **error** on every surface's first load. Re-verified: it now answers 204.
  The app's `icon.svg` workaround is removed, saving one request per surface.

## 2. Alternatives the mock and emulator ACCEPTED

The following were confirmed, with measured results.

- `GET /rest/api/3/issue/{id}?expand=changelog`, and `expand=changelog` on `search/jql` and on the
  software sprint-issue endpoint. All return complete histories, with `changelog.total` present.
- `consumer.function` (direct form), `InvocationError` with `retryData` and `retryAfter: 30` after a
  `429 Retry-After 30` (redelivered after 30 s), and in-invocation waits for `Retry-After 2`.
- Writes made only in the consumer, from row batches the scheduled job pushed. Backfill: 57 changes
  and 79 members in 4 queue events. After the full update stream (36 deliveries, including redeliveries,
  permutations and 2 drops): 29 `event` rows. The next scheduled run healed the 2 dropped changes
  with 4 writes. The run after that made 0 writes and 0 queue pushes. All of this matches a
  brute-force recompute over all 238 issues (0 diffs, 0 missing). That check validates the app's
  own model. The oracle was not consulted.
- An entity range attribute of type `float` holding epoch ms. `integer` is rejected as non-32-bit,
  which matches the public doc *"Must be a 32-bit signed integer"*.
- `keyPolicy: 'FAIL_IF_EXISTS'` (first writer wins, `source` kept).
- A dashboards widget with **no** `onProductSave`. `__forgeHost.save()` returned
  `{config:{boardId:"138"}, stored:{boardId:"138"}, via:"updateConfig"}`, and reopening the edit surface showed it pressed.
- `router.navigate({target:'issue', issueKey})`, which the bridge host maps to `/browse/<KEY>`.
- A comment posted from the frontend with `requestJira`. A double click gave exactly one comment,
  authored by the viewer (Elif Yilmaz), with two `fetchProduct` calls (429 then retry) and one success
  flag. Revision 2 keeps the browser-side retry, although the contract dropped the 429 path.
  The points sort is removed, and clicking `th[data-col=points]` now leaves `at` as the active sort.
- Plain-DOM surfaces: lint gave 0 errors and 0 warnings, and every surface ran in light and dark. In
  each case: 0 text elements below 4.5:1 (the selected ledger row measured 4.61), 0 `style`
  attributes, no horizontal scroll at 380 and 1180 px, and rendered bar heights proportional to their
  values (39 / 98.5 × 140 = 55.43 px measured). The console was clean apart from the host's own
  sandbox warning.

## 4. Forge LLM, Realtime and `rovo:mcp` (contract 2006de559)

### 4.1 What could be run on forge-dev, and what could not

The kit in WP1's working tree installs `@forge/llm` 1.0.7 and `@forge/realtime` 1.0.1, and the
functions bundle and load. Neither capability is emulated yet:

- `@forge/llm` answers `GET /llm/ -> 501`. The resolver returns `{error:"error", message:"Forge LLM
  failed: proxy route /llm/"}`, the modal shows an error flag (serve log: `bridge invoke explain`,
  `bridge showFlag`), and the modal keeps working: sorting still reverses, and the button is
  re-enabled.
- `publishGlobal` answers `POST …/capability/realtime -> 501`. The consumer logs
  `realtime publish failed for 3 channel(s): …` and does **not** throw, because the rows are already
  written and a redelivery would only republish nothing. The backend results are unchanged from
  revision 1: 88 changes and 95 members, 0 diffs against the brute-force recompute, and the idle run
  makes 0 writes, 0 queue pushes and 0 publishes.
- Bridge `subscribeGlobal` gives `bridge op 'subscribeRealtimeChannel' is not modelled`. The first
  build let that rejection destroy the rendered widget (no `[data-metric]`). It now uses
  `Promise.allSettled`, keeps the numbers on screen, and logs one console error.
- The `report_scope` parsing was tested offline. `explain.js` was bundled with `@forge/llm` stubbed,
  across 9 cases: object arguments, delta strings, cumulative strings, digits in the summary (the
  ledger sentence is used instead), refusal (no tool call), invalid JSON, `changeIds` as numbers,
  the wrong tool, and a thrown LLM error. Unknown ids were dropped, `tool_choice` was
  `{type:'function', function:{name:'report_scope'}}`, the active model was picked over a deprecated
  one, and the prompt held no hidden-change data.

**Unverified until WP1 emulates them:** a real explanation rendered in `[data-testid="explanation"]`
(light and dark contrast included), and a widget that updates live after `fd events`.

### 4.2 Ambiguities, and places where the docs and the contract pull apart

- **Realtime from the consumer.** Contract §4: live updates *"after ledger rows are written"*.
  Contract §2: the consumer *"performs the ledger writes"*. The Realtime events API page says
  *"`publish` API is only supported for functions invoked from the app frontend. This is **not**
  currently available for async events and web triggers."* No page says in so many words that
  `publishGlobal` works from an async event (the search summary says it must be used there). This
  app relies on that reading.
- **`signRealtimeToken` outside a resolver.** The Authorizing Realtime channels page says
  `signRealtimeToken` is *"exclusively available in a resolver"*, and that *"the claims object must
  match exactly between the publisher and subscriber"*. A token-scoped channel published from the
  consumer (a non-resolver) therefore contradicts the docs. This app uses tokenless global
  channels, and safety rests on the contract's "sprint ids only" payload rule. That is exactly the
  risk the docs name for `publishGlobal`: *"any users with access to your app may receive events
  from a private Jira issue"*. If the grader expects a token, the contract does not say so.
- **Channel name syntax** (allowed characters, length) is documented nowhere an entrant can see.
  `scope-ledger-sprint-<id>` is a guess.
- **A sprint that starts while the widget is open.** It has no channel subscription, so it appears
  only on reload. The contract's "after ledger rows are written, an open widget shows the new
  numbers" does not say whether a newly active sprint must appear.
- **Streamed tool calls are undocumented.** The `@forge/llm` README's stream example carries only
  text. The SDK reference says only `StreamResponse extends AsyncIterable<LlmResponse>`. The
  typings give `ToolCall.function.arguments: object` with an `index`, while the chat README example
  shows an object. Whether a stream sends tool arguments whole, as delta strings or as cumulative
  strings, and whether `index` is stable, is not stated. A stream with neither a stable `index` nor
  a stable `id` would be split into two calls here and graded `malformed`.
- **"A model that `list()` reports `active`"**: which active model is not specified, and the
  manifest only names the family (`model: [claude]`). This app takes the last active name in
  sorted order.
- **"Refusal (no tool call)" with a forced `tool_choice`.** A compliant backend always calls the
  tool, so a refusal can only come from the scripted model. Text alongside a valid call is accepted
  here. Two calls, or a different tool, count as `malformed`.
- **"Send it only what the viewer may see".** This app sends the team totals (contract §1: *"the same
  for everyone"*, although they include hidden issues' points) and the visible changes, including
  author display names. It does not send the hidden count. The contract does not say whether totals
  or authors count as "what the viewer may see".
- **`rovo:mcp`**: lint accepts `tools: [get-sprint-scope]` as action-key strings, matching the schema.

## 5. What was not verified

- That the totals match the benchmark's oracle. Only the app's own model and a brute-force recompute
  were compared.
- Changelog paging beyond one embedded page: no dev issue had more histories than one page.
- Behaviour on the scoring seed.
- A live Forge LLM explanation, and a live Realtime update end to end (4.1): both wait on WP1's emulation.
