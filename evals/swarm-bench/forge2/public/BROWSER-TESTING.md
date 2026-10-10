# Browser self-testing

Node, Playwright and a headless Chromium browser are supplied. They work inside the same isolation boundary as your
app; this file contains no private tests.

**Custom UI surfaces.** Serve one with the dev tools (`STARTER.md`), in the background:

```
node $FORGE_KIT/bin/forge-dev.cjs serve <moduleKey> [--sprint <id>] [--theme dark] > .forge-dev/serve.log 2>&1 &
```

The first line of the log is the surface's URL (also in `.forge-dev/serve.json`); the rest of the log lists every
bridge call the page makes (`bridge invoke <key>`, …). Then:

```
node browser-self-test.mjs <url> screenshot.png
```

It loads the page, waits up to 10 s for a table row, saves a full-page screenshot and prints JSON with the page
errors and console errors. On a surface with no table it reports a `readinessError` and still saves the screenshot.
You may modify it or write your own Playwright tests: load Playwright with `require(process.env.BENCH_BROWSER_MODULE)`
and launch Chromium with `executablePath: process.env.BENCH_BROWSER_EXECUTABLE`. Playwright's network events give the
bytes and origins a surface loads before its first data paint (contract §17); the serve log gives its invokes.

**The UI Kit admin page** has no HTML of its own: `node $FORGE_KIT/bin/forge-dev.cjs uikit <moduleKey>` prints the
tree your `@forge/react` code renders.
