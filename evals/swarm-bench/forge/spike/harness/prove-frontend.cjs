// Proof 3: render the app's Custom UI resource in Chromium with a fake @forge/bridge.
// Seam: @forge/bridge/out/bridge.js:9-15 reads globalThis.__bridge.callBridge(op, payload) — and
// invoke/invoke.js:8 captures it at MODULE LOAD, so it must exist before the app bundle runs.
// page.addInitScript installs it before any page script; page.exposeFunction carries each call to
// this Node process, which answers getContext from a seeded context, invoke by running the
// resolver function through the emulator, and fetchProduct through the forge proxy -> mock Jira.
const path = require('path');
const fs = require('fs');
const http = require('http');
const { chromium } = require(process.env.PLAYWRIGHT_PATH ?? path.join(process.env.HOME, '.nvm/versions/node/v22.22.0/lib/node_modules/playwright'));
const { startMockJira, calls } = require('./mock-jira.cjs');
const { createForgeProxy } = require('./forge-proxy.cjs');
const E = require('./emulator.cjs');
// Captured before tier A's in-process shim swaps globalThis.fetch for the egress door — the
// in-process hazard that makes a forked child (tier B's shape) the right production layout.
const nativeFetch = globalThis.fetch;

const tier = process.argv[2] ?? 'A';
const appDir = path.join(__dirname, '..', 'app');
const SCRATCH = process.env.SPIKE_SCRATCH ?? path.join(__dirname, '..', '.out');

function serveStatic(dir) {
  const types = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.png': 'image/png', '.json': 'application/json' };
  const srv = http.createServer((req, res) => {
    const p = path.join(dir, decodeURIComponent(new URL(req.url, 'http://x').pathname));
    const f = fs.existsSync(p) && fs.statSync(p).isDirectory() ? path.join(p, 'index.html') : p;
    if (!f.startsWith(dir) || !fs.existsSync(f)) { res.writeHead(404); return res.end(); }
    res.writeHead(200, { 'content-type': types[path.extname(f)] ?? 'application/octet-stream' });
    fs.createReadStream(f).pipe(res);
  });
  return new Promise((ok) => srv.listen(0, '127.0.0.1', () => ok(`http://127.0.0.1:${srv.address().port}/`)));
}

(async () => {
  const manifest = E.loadManifest(appDir);
  await startMockJira(18995);
  const proxy = createForgeProxy({ jiraUrl: 'http://127.0.0.1:18995', manifest });
  const proxyUrl = await proxy.listen();
  const bundleDir = await E.bundle(appDir, path.join(SCRATCH, `fe-bundle-${tier}`), manifest);
  if (tier === 'B') E.prepareRealRuntimeDir(bundleDir);
  const ectx = { bundleDir, manifest, proxyUrl };

  const [moduleType, entries] = Object.entries(manifest.modules).find(([, es]) => es.some((e) => e.resource && !e.render));
  const mod = entries.find((e) => e.resource && !e.render);
  const resource = manifest.resources.find((r) => r.key === mod.resource);
  const seeded = {
    accountId: '5b10ac8d82e05b22cc7d4ef5', cloudId: E.CLOUD_ID, siteUrl: 'https://emulator.atlassian.net', localId: `ari:cloud:ecosystem::extension/emulator/${mod.key}`,
    moduleKey: mod.key, environmentType: 'DEVELOPMENT', environmentId: 'emulator-env', locale: 'en-US', timezone: 'Europe/Bucharest', theme: { colorMode: 'light' },
    extension: { type: moduleType, issue: { key: 'SPK-1', id: '10001', type: 'Bug', typeId: '10004' }, project: { key: 'SPK', id: '10000', type: 'software' } },
  };
  const bridgeLog = [];
  const unmodelled = [];
  async function callBridge(op, payload) {
    bridgeLog.push(op + (op === 'invoke' ? `:${payload.functionKey}` : op === 'fetchProduct' ? `:${payload.restPath}` : ''));
    switch (op) {
      case 'getContext': return seeded;
      case 'invoke': {
        const event = { call: { functionKey: payload.functionKey, payload: payload.payload }, context: seeded };
        const fn = mod.resolver.function;
        if (tier === 'A') return E.invokeA(ectx, fn, mod.key, event, { principal: { accountId: seeded.accountId }, installContext: E.CONTEXT_ARI });
        const r = await E.invokeB(ectx, fn, mod.key, event);
        if (!r.success) throw new Error(r.error.errorMessage);
        return r.body;
      }
      case 'fetchProduct': {
        const init = payload.fetchRequestInit;
        const r = await nativeFetch(new URL(`/fpp/provider/user/remote/${payload.product}`, proxyUrl), { method: init.method ?? 'GET', headers: [...init.headers, ['forge-proxy-target', payload.restPath], ['forge-proxy-authorization', 'Bearer emulator']], body: init.body });
        return { body: await r.text(), headers: Object.fromEntries(r.headers), status: r.status, statusText: r.statusText, isAttachment: false };
      }
      case 'enableTheming': case 'emitReadyEvent': case 'emitFrontendCustomMetric': case 'initFeatureFlags': return undefined;
      default: unmodelled.push(op); throw new Error(`emulator: bridge op '${op}' not modelled`);
    }
  }

  const url = await serveStatic(path.join(appDir, resource.path));
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 640, height: 320 } });
  const consoleErrors = [];
  page.on('console', (m) => m.type() === 'error' && consoleErrors.push(m.text()));
  page.on('pageerror', (e) => consoleErrors.push(String(e)));
  await page.exposeFunction('__forgeEmulatorCall', callBridge);
  await page.addInitScript(() => { globalThis.__bridge = { callBridge: (op, payload) => window.__forgeEmulatorCall(op, payload) }; });
  await page.goto(url);
  await page.getByText('Login button misaligned').waitFor({ timeout: 15000 }).catch(async (e) => { console.log('BODY:', await page.locator('body').innerText(), 'LOG:', bridgeLog, consoleErrors); throw e; });
  const shot1 = path.join(SCRATCH, `panel-${tier}-loaded.png`);
  await page.screenshot({ path: shot1 });
  await page.getByRole('button', { name: 'Add comment' }).click();
  await page.locator('#posted').waitFor({ timeout: 15000 });
  const shot2 = path.join(SCRATCH, `panel-${tier}-after-click.png`);
  await page.screenshot({ path: shot2 });
  console.log(`TIER ${tier} rendered text:\n` + (await page.locator('body').innerText()));
  console.log('bridge ops:', bridgeLog.join(', '));
  console.log('jira calls:', calls.map((c) => `${c.as} ${c.method} ${c.path}`).join(' | '));
  console.log('console errors:', consoleErrors, 'unmodelled bridge ops:', unmodelled, 'proxy unmodelled:', proxy.unmodelled);
  console.log('screenshots:', shot1, shot2);
  await browser.close();
  process.exit(0);
})().catch((e) => { console.error('FAILED', e); process.exit(1); });
