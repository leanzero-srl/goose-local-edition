'use strict';
// Smoke: the spike's fixture app (forge/spike/app) through the new kit end to end — bundled like
// `forge deploy`, every function invoked in Atlassian's runtime wrapper under the deny-default fence,
// against the seeded site; its Custom UI panel rendered by the host in light and dark with a click.
const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');
const { FORGE, SPIKE, ensureKit, playwright, scratch, copyDir } = require('./helpers.cjs');

test('spike fixture app runs through the kit (backend, queue, unmodelled policy, Custom UI light/dark)', { timeout: 240_000 }, async () => {
  const kit = ensureKit();
  const { createSite } = require(path.join(FORGE, 'site', 'site.cjs'));
  const { createEmulator } = require(path.join(FORGE, 'kit', 'lib', 'emulator.cjs'));
  const { kitPaths } = require(path.join(FORGE, 'kit', 'lib', 'kitpaths.cjs'));
  const appDir = path.join(scratch('spike'), 'app');
  copyDir(path.join(SPIKE, 'app'), appDir, new Set(['node_modules', 'build']));
  // The entrant builds Custom UI themselves; this is the spike panel's build with the kit's esbuild.
  const paths = kitPaths(path.join(FORGE, 'kit'));
  const out = path.join(appDir, 'static', 'panel', 'build');
  fs.mkdirSync(out, { recursive: true });
  await paths.require('esbuild').build({ entryPoints: [path.join(appDir, 'static', 'panel', 'src', 'index.jsx')], bundle: true, format: 'iife', platform: 'browser',
    outfile: path.join(out, 'main.js'), jsx: 'automatic', define: { 'process.env.NODE_ENV': '"production"' }, nodePaths: [paths.appModules], logLevel: 'silent' });
  fs.writeFileSync(path.join(out, 'index.html'), '<!doctype html><html><head><meta charset="utf-8"><link rel="stylesheet" href="./panel.css"></head><body><div id="root"></div><script src="./main.js"></script></body></html>');
  fs.writeFileSync(path.join(out, 'panel.css'), 'body{margin:16px;font-family:sans-serif;background:var(--ds-surface);color:var(--ds-text)}');

  const site = await createSite({ seed: '5eed5eed5eed5eed' });
  const emu = await createEmulator({ appDir, site, runtime: 'wrapper' });
  try {
    assert.strictEqual(emu.publishable, true);
    assert.strictEqual(emu.fence, 'sandbox');
    assert.strictEqual(emu.wrapper.wrapperSha256, kit.wrapper_sha256);
    const built = await emu.build();
    assert.deepStrictEqual(built.functions.map((f) => [f.key, f.bundled, f.loaded]), [['resolver', true, true], ['hourly', true, true], ['hook', true, true], ['consume', true, true]]);

    const issue = site.pack.issues.find((i) => !i.hiddenFrom.length);
    const ctx = { cloudId: site.pack.cloudId, moduleKey: 'spike-issue-panel', extension: { type: 'jira:issuePanel', issue: { key: issue.key } }, accountId: site.pack.viewer };
    const r = await emu.invokeResolver('spike-issue-panel', 'summarise', { issueKey: issue.key }, ctx, site.pack.viewer);
    assert.ok(r.ok, JSON.stringify(r.error));
    assert.strictEqual(r.result.summary, issue.fields.summary);
    assert.deepStrictEqual(r.calls.map((c) => `${c.service}:${c.provider}:${c.status}`), ['jira:app:200', 'kvs:app:404', 'kvs:app:204']);

    // Egress outside permissions.external.fetch is refused by the proxy, as on the platform.
    const egress = await emu.invokeResolver('spike-issue-panel', 'egressProbe', {}, ctx, site.pack.viewer);
    assert.strictEqual(egress.result.proxyError, 'REQUEST_EGRESS_ALLOWLIST_ERR');

    // Scheduled -> queue -> consumer. The spike consumer edits labels with PUT /issue, a real Jira operation
    // the site does not model: 501 + harness_missing on both sides, never a silent 200.
    const sched = await emu.runScheduled('spike-hourly');
    assert.ok(sched.invocation.ok);
    assert.strictEqual(sched.deliveries.length, sched.invocation.result.swept);
    assert.ok(sched.deliveries.every((d) => d.outcome === 'ok'));
    assert.ok(emu.harnessMissing.some((m) => /PUT \/rest\/api\/3\/issue\//.test(m.what)));
    assert.ok(site.harnessMissing.some((m) => m.what === 'REST PUT /rest/api/3/issue/{issueIdOrKey}'));

    // Custom UI in light and dark.
    const pw = playwright();
    const browser = await pw.chromium.launch({ headless: true, executablePath: process.env.BENCH_BROWSER_EXECUTABLE || undefined });
    const shots = scratch('shots');
    try {
      for (const theme of ['light', 'dark']) {
        const page = await browser.newPage();
        const errors = [];
        const csp = [];
        page.on('pageerror', (e) => errors.push(String(e)));
        page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });
        await page.addInitScript(() => document.addEventListener('securitypolicyviolation', (e) => window.__csp = [...(window.__csp ?? []), e.violatedDirective]));
        await emu.openSurface(page, { moduleKey: 'spike-issue-panel', entry: 'view', theme, layout: { width: 640, height: 320 }, asUser: site.pack.viewer,
          extension: { type: 'jira:issuePanel', issue: { key: issue.key, id: issue.id }, project: { key: issue.projectKey } } });
        await page.getByText(issue.fields.summary).waitFor({ timeout: 20_000 });
        assert.strictEqual(await page.evaluate(() => document.documentElement.getAttribute('data-color-mode')), theme);
        const bg = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);
        assert.strictEqual(bg, theme === 'light' ? 'rgb(255, 255, 255)' : 'rgb(29, 33, 37)', 'the --ds-surface token resolves per mode');
        await page.screenshot({ path: path.join(shots, `panel-${theme}.png`) });
        await page.getByRole('button', { name: 'Add comment' }).click();
        await page.locator('#posted').waitFor({ timeout: 20_000 });
        const posted = await page.locator('#posted').innerText();
        // No scripted comment-path 429 (DESIGN §17.2 E): both posts land.
        assert.match(posted, /Comment status 201/);
        assert.deepStrictEqual(errors, []);
        assert.deepStrictEqual(await page.evaluate(() => window.__csp ?? []), []);
        await page.close();
        await emu.advance(1500);
      }
      const ops = emu.bridgeLog.map((b) => b.op);
      for (const op of ['getContext', 'enableTheming', 'invoke', 'fetchProduct']) assert.ok(ops.includes(op), `bridge op ${op} observed`);
      assert.ok(fs.statSync(path.join(shots, 'panel-dark.png')).size > 1000);
      assert.strictEqual(emu.cspReports().length, 0);
      assert.ok(site.comments.length === 2 && site.comments.every((c) => c.authorId === site.pack.viewer && c.as === 'user'));
    } finally {
      await browser.close();
    }
  } finally {
    await emu.close();
    await site.stop();
  }
});
