# forge-1.0 kit (WP1)

The Forge platform the benchmark runs apps on: Atlassian's own runtime wrapper, a private seeded Jira
Cloud site, the Custom UI / dashboards host, and the offline dev tools. The scorer and the entrant's
`forge-dev` use the same emulator; only the site's seed and the oracle differ.

## Materialise

`python3 bench/forge_kit.py ensure` prints `{kit_dir, modules_dir, kit_lock_sha256, kit_code_sha256,
wrapper_sha256}`. It runs `npm ci --ignore-scripts` for `package-lock.json` (app-modules: the entrant's
`node_modules`) and `lint/package-lock.json` (lint-modules), fetches the runtime wrapper and loader and
refuses unless both match `runtime-pin.json` by sha256 and size, copies the manifest schema out of
`@forge/manifest`, and stages `kit-<code_sha256[:16]>/` (`bin lib openapi runtime-pin.json` plus links).
Concurrent calls are serialised by an flock. Any change under `bin/ lib/ openapi/` makes a new kit dir:
re-run `ensure` and use its `kit_dir` as `$FORGE_KIT`. `install-modules <workdir>` clones app-modules
into a workspace (APFS clone; refuses on other filesystems).

The wrapper is never committed. `forge_kit.py repin` re-reads the CDN index the way `@forge/bundler`'s
NetworkWrapperProvider does (first `<script src>` containing `wrapper` / `loader`) and rewrites the pin;
re-pinning is deliberate and means rescoring the golden.

## Emulator (I2) — `lib/emulator.cjs`

```js
const emu = await createEmulator({ appDir, site, runtime: 'wrapper' });   // 'shim' only with an explicit
await emu.build();               // -> {functions:[{key,handler,bundled,loaded,error}], files}  flag; unpublishable
await emu.invoke(fnKey, { moduleKey, event, asUser });
await emu.invokeResolver(moduleKey, functionKey, payload, context, asUser);
await emu.invokeAction(actionKey, inputs, { asUser });       // Rovo: {...inputs, context:{cloudId, moduleKey}}
await emu.deliverProductEvent(change);  // avi:jira:updated:issue to every trigger; does NOT drain
await emu.deliverNext();                // the site's next scheduled delivery (duplicates/permutations/drops)
await emu.drainQueues();                // [{eventId, attempt, outcome: ok|retry|throw|timeout|no_consumer, retryAfter, redeliverAt, ...}]
await emu.runScheduled(moduleKey);      // {run, invocation, deliveries}
await emu.openSurface(page, { moduleKey, entry: 'view'|'edit', theme, layout, asUser, extension, widgetId });
await emu.hostSave(page);               // dashboard Save -> {config, stored, via}
await emu.resizeSurface(page, layout);  // LAYOUT_CHANGED / EDIT_LAYOUT_CHANGED
emu.log; emu.bridgeLog; emu.kvs.snapshot(); emu.harnessMissing; emu.cspReports(); emu.widgetConfigs();
emu.publishable; emu.fence; emu.wrapper; await emu.close();
```

Handlers follow `@forge/cli-shared`: exactly `<file>.<export>` resolved under `src/` (`src/index.fn` is
rejected, as `forge lint` does). Every invocation is a fresh process running the pinned wrapper through its
loader, under a deny-default `sandbox-exec` profile: reads only the bundle, no exec, network only to the
per-invocation proxy port (kit.test.cjs measures EPERM/EPERM/200/EPERM/ENOTFOUND). The proxy speaks the
platform's `/fpp/...` protocol: product calls go to the site with the app's scopes, KVS to `lib/kvs.cjs`,
queue pushes to the emulator; platform `fetch` outside `permissions.external.fetch` answers 403.
Async events retry at 1, 2, 4, 8 then every 15 minutes inside 24 h; `InvocationError.retryAfter` is
clamped to 1..900 s. The real wrapper throws on an `InvocationError` without `retryData` when
`timeoutSeconds > 55` — kept (platform fidelity).

## Forge LLM and Realtime

Both live on the site, so every emulator attached to it (the scorer's; the dev kit's `serve` and CLI
processes) shares them. MEASURED on the pinned wrapper: `@forge/llm` calls GET `${proxy}/llm/` (list) and
POST `${proxy}/llm/<model>` (chat/stream, body + `stream`); `@forge/realtime` POSTs GraphQL
(`publishRealtimeChannel`, `signRealtimeToken`) to `${proxy}/fpp/as/app/provider/atlassian/capability/realtime`
with `x-forge-context-token` = the invocation body's `contextToken` (the string "undefined" for a consumer).
The proxy forwards both; without an `llm` module an LLM call is refused 403.

- `site/llm.cjs`: `list()` = 2 active + 2 deprecated (incl. the docs' `claude-opus-4-6`), seeded order;
  each chat takes the next scripted answer — clean tool call, digits + a hidden and an unknown change id,
  refusal, malformed arguments, 429 `ForgeLlmAPIError` — then clean; `emu.llm.phase(name)` restarts it;
  `emu.llm.log()` holds every prompt and answer. stream() returns the answer as one ChatResponse chunk.
- `site/realtime.cjs`: channels, module/product-context scoping, signed tokens (claims must match),
  `replaySeconds`. A resolver call carries a frontend context token, so its `publish()` reaches that
  surface's `subscribe`; `publish()` without one (consumer, scheduled, trigger) is REJECTED with an error
  result and logged; `publishGlobal` works anywhere. `emu.realtime.log()` lists every publish
  (delivered / no subscriber / rejected); `emu.realtime.deliveries()` what reached a page.
- forge-dev: `llm [--phase]`, `realtime [--follow]`; invoke prints each LLM answer and publish outcome,
  `serve` prints each realtime event delivered to its page.

## Site — `../site/`

`createSite({seed, port})` serves REST v3 + Jira Software REST for the ops in the shipped OpenAPI that
the handlers model; anything else valid is 501 + `harness_missing`, anything not in the OpenAPI 404.
Scopes are checked per operation (OAuth2 scopes; a mismatch is the measured 401). Fixture packs come from
`fixtures.cjs` (`facts(seed)`, byte-identical per seed). Every limit in `limits.cjs` carries a receipt.

## Dev tools

`bin/lint.cjs [--json] [appDir]` runs `@forge/lint` + the manifest's FullValidationProcessor offline and
reports the stage it reached. It needs all six files in `openapi/` (`@forge/lint`'s LOCAL_*_SWAGGER
copies); the site models `jira.json` and `jsw.json` (Jira Software).

`bin/forge-dev.cjs` is the entrant's CLI (STARTER.md, `--help`). `FORGE_SITE_URL` is
`http://admin:<control token>@127.0.0.1:<port>`: the token is the URL password (`forge_site.serve()` and
`createSite()` print/return it as `url` with credentials and `adminUrl`). Its state lives in
`.forge-dev/state.json`, shared by every forge-dev process in the workspace under a pid lock (a dead
holder's lock is taken over by atomic rename; a corrupt state file is an error, never an empty
substitute). It holds dev KVS, pending queue events and saved widget configs: a widget Save persists
across `serve` runs as on a dashboard, `--config` overrides it for one run, `reset` clears all of it.
Every `invoke` writes the full result to `.forge-dev/last-result.json` (the terminal shows 4,000 chars).

## Deliberate deviations from Jira Cloud

1. `/changelog/bulkfetch` `created` is ISO-8601 (the OpenAPI type, STARTER). Jira Cloud measured
   2026-10-02 returns epoch milliseconds there.
2. Inside the entrant's own sandbox a nested `sandbox-exec` is refused (exit 71), so `forge-dev` falls back
   to `node --permission` (file reads to the bundle, no child processes, network NOT fenced) and says so.
   Only `fence: 'dev-auto'` (forge-dev) allows that; the default fence REFUSES when sandbox-exec cannot
   apply, so scoring never runs app code unfenced.
3. Keys Jira Cloud answers beyond the OpenAPI text stay where measured (sprint `createdDate`, issue
   bulkfetch `expand`); the deprecated agile sprint-issue list answers the agile page although its doc
   example is a single issue. Paging follows each operation's documented parameters (token vs offset).
4. Realtime: what the platform answers to `publish()` without a frontend context, the token lifetime, the
   payload type a subscriber receives (here: the published string) and Forge LLM's finish_reason values and
   error codes are not documented; they are the harness's choices, recorded in site/realtime.cjs and llm.cjs.
5. Rovo actions get the user in both documented places: `payload.context.accountId` (rovo-action page) and
   the second argument's `principal.accountId` (function arguments page). Not measured on a live Rovo call.
6. KVS codes no docs page names (409 `CONDITIONAL_CHECK_FAILED`, `MAX_BATCH_SIZE`, `TOO_MANY_OPERATIONS`,
   `DUPLICATE_KEY`, ...) are listed in `lib/kvs.cjs`.

## Tests

`node --test forge/kit/test/*.test.cjs` (fixtures, site, kit, widget, forge-dev, forge-dev-live, llm-realtime,
smoke-spike) and
`python3 -m unittest forge/kit/test/test_entrant_fence.py`.
