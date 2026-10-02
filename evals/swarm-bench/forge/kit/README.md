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

## Site — `../site/`

`createSite({seed, port})` serves REST v3 + Jira Software REST for the ops in the shipped OpenAPI that
the handlers model; anything else valid is 501 + `harness_missing`, anything not in the OpenAPI 404.
Scopes are checked per operation (OAuth2 scopes; a mismatch is the measured 401). Fixture packs come from
`fixtures.cjs` (`facts(seed)`, byte-identical per seed). Every limit in `limits.cjs` carries a receipt.

## Dev tools

`bin/lint.cjs [--json] [appDir]` runs `@forge/lint` + the manifest's FullValidationProcessor offline and
reports the stage it reached. `bin/forge-dev.cjs` is the entrant's CLI (STARTER.md); its state lives in
`.forge-dev/` under a lock so several processes can share a workspace.

## Deliberate deviations from Jira Cloud

1. `/changelog/bulkfetch` `created` is ISO-8601 (the OpenAPI type, STARTER). Jira Cloud measured
   2026-10-02 returns epoch milliseconds there.
2. Inside the entrant's own sandbox a nested `sandbox-exec` is refused (exit 71), so `forge-dev` falls back
   to `node --permission` (file reads to the bundle, no child processes, network NOT fenced) and says so.
   Only `fence: 'dev-auto'` (forge-dev) allows that; the default fence REFUSES when sandbox-exec cannot
   apply, so scoring never runs app code unfenced.
3. KVS codes no docs page names (409 `CONDITIONAL_CHECK_FAILED`, `MAX_BATCH_SIZE`, `TOO_MANY_OPERATIONS`,
   `DUPLICATE_KEY`, ...) are listed in `lib/kvs.cjs`.

## Tests

`node --test forge/kit/test/*.test.cjs` (fixtures, site, kit, widget, forge-dev, smoke-spike) and
`python3 -m unittest forge/kit/test/test_entrant_fence.py`.
