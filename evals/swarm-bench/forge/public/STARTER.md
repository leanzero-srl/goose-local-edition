# Starter and offline tools

There is no internet in this workspace. Everything you can use is already here.

**Workspace.** `manifest.yml` holds the app id and runtime and no modules. `package.json` pins the
installed packages; `node_modules/` already contains them and nothing else can be installed.
`src/index.js` is empty; `static/` and `skills/` are empty. Every file is editable.

**Installed packages** (their typings and sources are your API reference): `@forge/api` 8.2.0,
`@forge/kvs` 2.0.7, `@forge/events` 3.0.7, `@forge/resolver` 2.0.0, `@forge/bridge` 7.1.0,
`@forge/dashboards-bridge` 2.0.0, `@forge/hooks` 2.0.0, `@forge/llm` 1.0.7, `@forge/realtime`
1.0.1, `react` and `react-dom` 18.3.1, `esbuild`
0.28.2.

**Reference material** under `$FORGE_KIT` (read-only): `schema/manifest-schema.json` (the Forge
manifest schema), `openapi/jira.json` and `openapi/jsw.json` (Jira Cloud REST and
Jira Software REST; the other files in `openapi/` are the linter's offline copies). These are single-line JSON files of several MB: query them with `node` or
`grep -o`, never print them whole.

**Dev site.** A seeded Jira Cloud site answers at `$FORGE_SITE_URL` for the dev tools below. It
behaves like Jira Cloud — REST v3 and Jira Software REST, including the bulk endpoints, their
pagination, errors and rate limits; dates are ISO-8601 strings as the OpenAPI types them. JQL:
fields `project`, `key`, `sprint`, `updated`, `created`, `status`, `statusCategory`, `issuetype`,
`labels`, `assignee`, `reporter`, `cf[id]`; operators `= != in not in > >= < <= is is not ~`,
`AND OR NOT`, parentheses, `ORDER BY`; functions `openSprints()`, `closedSprints()`,
`futureSprints()`, `currentUser()`, `now()`, `startOfDay()`; relative dates like `-14d`. Forge LLM answers with a
scripted model on the dev site. You
never call the site directly; your app reaches it through the Forge runtime. As the default viewer
(`users`, first line) one dev issue refuses comments with 403, as on the scoring site.

**Tools** (`npm run lint` and `node $FORGE_KIT/bin/forge-dev.cjs <command>`):

| command | does |
|---|---|
| `npm run lint` | Forge's own client-side linter, offline. It is staged: fix and rerun until it reports no errors. |
| `invoke <functionKey> [--module <key>] [--resolver <key>] [--payload <file>] [--as <accountId>]` | runs a manifest function in the Forge runtime against the dev site with the event shape of the module that references it (`--resolver` calls that resolver key with `--payload`); prints the result, logs and every Jira, KVS and queue call. `--as` makes the invocation user-led. |
| `events [--limit N]` | delivers the dev site's next N issue updates to your trigger and drains the queues |
| `scheduled <moduleKey>` | runs a scheduled trigger once, then drains the queues |
| `serve <moduleKey> [--edit] [--sprint <id>] [--config <json>] [--theme light\|dark] [--as <accountId>]` | serves a Custom UI module with the Forge bridge and the dashboard host emulated, prints its URL and runs until stopped (start it in the background). In a served edit surface, `window.__forgeHost.save()` performs the dashboard's Save. |
| `kvs` / `users` / `reset` | dump stored keys and entities / list dev users / clear dev storage and queues and rewind the dev site's update stream |

Screenshot a served surface with the bundled browser (`BROWSER-TESTING.md`). Build Custom UI into
`static/<name>/build/` (an `index.html` plus assets) with the installed esbuild; the harness never
builds it for you.
