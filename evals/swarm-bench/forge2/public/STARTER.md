# Starter and offline tools

There is no internet in this workspace. Everything you can use is already here.

**Workspace: Scope Ledger v1**, the app your v2 replaces, exactly as it runs on the site today. Every file is
editable.
- `manifest.yml`: v1's modules (issue trigger, queue consumer, hourly scheduled trigger, dashboards widget with
  `edit`, sprint action, Rovo action, skill, agent and MCP server, Forge LLM), its scopes and its storage entities.
- `src/`: the backend; `index.js` exports every handler. `config.js` finds the Sprint field, the scrum boards, their
  active sprints and each board's estimation field; `sync.js` is the event path, the backfill and the reconciliation;
  `ledger.js` the storage and the §1 totals; `jira.js` the Jira requests and their 429 handling; `views.js` what each
  surface and the Rovo action get; `explain.js` the Forge LLM explanation; `realtime.js` the live updates;
  `numbers.js` the formatting.
- `static/widget`, `static/widget-edit`, `static/sprint`: the Custom UI surfaces (React sources in `src/`, the
  committed build in `build/`); `static/shared/` holds what they share. `npm run build` rebuilds all three with
  esbuild (`build.mjs`).
- `skills/sprint-scope-analyst/SKILL.md`: the Rovo skill.

**v1's data layout.** The sites' storage is v1's, as of the upgrade:
- entity `scope-change` (declared in `manifest.yml`): v1's ledger, one row per change, key `<changeId>:<sprintId>`,
  value `{ sprintId, changeId, at, created, issueId, issueKey, kind, authorId, authorName, source }`: `at` is the
  change time in epoch milliseconds, `created` the same instant as ISO-8601, `kind` `added`/`removed`, `source`
  `event`/`reconcile`. Index `by-sprint`: partition `sprintId`, range `at`. It holds every change of each active
  sprint's first 2 days and nothing later.
- v1 also writes the KV key `config` (the Sprint field id and the active sprints, see `src/config.js`) and the entity
  `sprint-issue` (membership and estimate per sprint and issue). They are v1's working state: do not count on
  finding them.

**Installed packages** (their typings and sources are your API reference): `@forge/api` 8.2.0, `@forge/kvs` 2.0.7,
`@forge/events` 3.0.7, `@forge/resolver` 2.0.0, `@forge/bridge` 7.1.0, `@forge/dashboards-bridge` 2.0.0,
`@forge/hooks` 2.0.0, `@forge/llm` 1.0.7, `@forge/realtime` 1.0.1, `@forge/react` 12.3.0 (UI Kit), `react` and
`react-dom` 18.3.1, `esbuild` 0.28.2. `package.json` pins v1's packages; `node_modules/` already holds all of these and
nothing else can be installed.

**Reference material** under `$FORGE_KIT` (read-only): `schema/manifest-schema.json` (the Forge manifest schema),
`openapi/jira.json` and `openapi/jsw.json` (Jira Cloud REST and Jira Software REST; the other files in `openapi/` are
the linter's offline copies). These are single-line JSON files of several MB: query them with `node` or `grep -o`,
never print them whole.

**Dev site.** A seeded Jira Cloud site answers at `$FORGE_SITE_URL` for the dev tools below, with the same shape as
the scoring sites (contract, "Scale") and another seed. It behaves like Jira Cloud — REST v3 and Jira Software REST,
including the bulk endpoints, their pagination and errors, and the rate model of `RATE-MODEL.json` on every request;
dates are ISO-8601 strings as the OpenAPI types them. JQL: fields `project`, `key`, `sprint`, `updated`, `created`,
`status`, `statusCategory`, `issuetype`, `labels`, `assignee`, `reporter`, `cf[id]`; operators
`= != in not in > >= < <= is is not ~`, `AND OR NOT`, parentheses, `ORDER BY`; functions `openSprints()`,
`closedSprints()`, `futureSprints()`, `currentUser()`, `now()`, `startOfDay()`; relative dates like `-14d`. Its update
stream also carries the world changes of contract §12, each kind at least once. Invocations run on its virtual clock
with the contract's limits (§11). Forge LLM answers with a scripted model. You never call the site directly; your app
reaches it through the Forge runtime. `users` lists the dev users: the first line is the default viewer; it marks who
holds Jira's `ADMINISTER` permission and names the issue the default viewer may not comment on (Jira answers that
comment with a 400 "you do not have the permission to comment", no ADD_COMMENTS), as on the scoring site.

**Tools** (`npm run lint`, `npm run build` and `node $FORGE_KIT/bin/forge-dev.cjs <command>`; `forge-dev` with no
command prints every flag):

| command | does |
|---|---|
| `npm run lint` | Forge's own client-side linter, offline, plus the checks Forge's servers apply at deploy (a named index takes exactly one range attribute; index names 3–50 allowed characters; at most 20 entities; a supported Node runtime). It is staged: fix and rerun until it reports no errors. |
| `invoke <functionKey> [--module <key>] [--resolver <key>] [--payload <file>] [--as <accountId>]` | runs a manifest function in the Forge runtime against the dev site with the event shape of the module that references it (`--resolver` calls that resolver key with `--payload`); prints the result, logs and every Jira, storage and queue call. `--as` makes the invocation user-led. |
| `events [--limit N]` | delivers the dev site's next N updates to your trigger(s) and drains the queues |
| `scheduled <moduleKey>` | runs a scheduled trigger once, then drains the queues |
| `serve <moduleKey> [--edit] [--sprint <id>] [--config <json>] [--theme light\|dark] [--as <accountId>]` | serves a Custom UI module with the Forge bridge and the dashboard host emulated, prints its URL and runs until stopped (start it in the background); its log prints every bridge call the page makes. In a served edit surface, `window.__forgeHost.save()` performs the dashboard's Save. |
| `uikit <moduleKey>` | renders a UI Kit (`render: native`) module with the real `@forge/react` reconciler, its resolver calls going to the dev site, and prints the tree it produced |
| `ci send` | signs a deployment event the way CI does (contract §14), posts it to your web trigger and prints the answer |
| `llm` / `realtime` | the dev site's Forge LLM (model list, scripted answers, every call) / every Realtime publish and subscription |
| `kvs` / `users` / `reset` | dump stored keys and entities / list dev users / return the dev storage, queues and update stream to the upgrade instant (v1's rows in place) |

Screenshot a served Custom UI surface with the bundled browser (`BROWSER-TESTING.md`). Build Custom UI into
`static/<name>/build/` (an `index.html` plus assets) with the installed esbuild; the harness never builds it for you.
A UI Kit resource is different: point its `path` at the source file (for example `src/frontend/admin.jsx`) and the
harness bundles it the way `forge deploy` does.
