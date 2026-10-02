// Proof 3b: UI Kit (render: native). @forge/react's reconciler (out/reconciler.js:56-59, 127)
// emits the whole tree as `self.__bridge.callBridge('reconcile', { forgeDoc })` after every
// commit; the product's host renderer (Atlassian-internal, NOT on npm) maps ForgeDoc -> Atlaskit.
// The emulator stands in for that host: it captures every ForgeDoc (gradable as a structured tree)
// and paints it with a small generic renderer so there is something to screenshot and click.
// Function props stay live because the app runs in the same realm (in the product they are proxied).
const path = require('path');
const fs = require('fs');
const esbuild = require('esbuild');
const { chromium } = require(process.env.PLAYWRIGHT_PATH ?? path.join(process.env.HOME, '.nvm/versions/node/v22.22.0/lib/node_modules/playwright'));
const { startMockJira, calls } = require('./mock-jira.cjs');
const { createForgeProxy } = require('./forge-proxy.cjs');
const E = require('./emulator.cjs');

const appDir = path.join(__dirname, '..', 'app');
const SCRATCH = process.env.SPIKE_SCRATCH ?? path.join(__dirname, '..', '.out');

const HOST_RENDERER = () => {
  const tags = { Heading: (p) => p.as || 'h2', Text: () => 'p', Stack: () => 'div', Inline: () => 'span', Box: () => 'div', Lozenge: () => 'span', Button: () => 'button', Link: () => 'a', Strong: () => 'b', Em: () => 'i', Code: () => 'code' };
  const paint = (node) => {
    if (node.type === 'String') return document.createTextNode(node.props.text);
    const make = tags[node.type];
    const el = document.createElement(make ? make(node.props) : 'div');
    el.dataset.forgeType = node.type;
    if (!make) el.dataset.unmodelled = 'true';
    if (node.props.appearance) el.dataset.appearance = node.props.appearance;
    if (typeof node.props.onClick === 'function') el.addEventListener('click', () => node.props.onClick());
    for (const c of node.children) el.appendChild(paint(c));
    return el;
  };
  window.__forgeDocs = [];
  window.__paintForgeDoc = (doc) => {
    window.__forgeDocs.push(JSON.parse(JSON.stringify(doc, (k, v) => (typeof v === 'function' ? '[fn]' : k === 'key' ? undefined : v))));
    const root = document.getElementById('root');
    root.replaceChildren(paint(doc));
  };
};

(async () => {
  const manifest = E.loadManifest(appDir);
  await startMockJira(18997);
  const proxy = createForgeProxy({ jiraUrl: 'http://127.0.0.1:18997', manifest });
  const proxyUrl = await proxy.listen();
  const bundleDir = await E.bundle(appDir, path.join(SCRATCH, 'uikit-backend'), manifest);
  E.prepareRealRuntimeDir(bundleDir);
  const ectx = { bundleDir, manifest, proxyUrl };
  const mod = manifest.modules['jira:issuePanel'].find((e) => e.render === 'native');
  const resource = manifest.resources.find((r) => r.key === mod.resource);
  const frontJs = path.join(SCRATCH, 'uikit', 'uikit.js');
  await esbuild.build({ entryPoints: [path.join(appDir, resource.path)], bundle: true, format: 'iife', platform: 'browser', outfile: frontJs, jsx: 'automatic', loader: { '.jsx': 'jsx' }, define: { 'process.env.NODE_ENV': '"production"', global: 'globalThis' }, logLevel: 'error', nodePaths: [path.join(__dirname, '..', 'node_modules')] });

  const seeded = { accountId: '5b10ac8d82e05b22cc7d4ef5', cloudId: E.CLOUD_ID, siteUrl: 'https://emulator.atlassian.net', moduleKey: mod.key, localId: 'l1', environmentType: 'DEVELOPMENT', extension: { type: 'jira:issuePanel', issue: { key: 'SPK-1', id: '10001' }, project: { key: 'SPK' } } };
  const ops = [];
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 560, height: 260 } });
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  page.on('console', (m) => errors.push('console.' + m.type() + ': ' + m.text()));
  await page.exposeFunction('__forgeEmulatorCall', async (op, payload) => {
    ops.push(op === 'invoke' ? `invoke:${payload.functionKey}` : op);
    if (op === 'getContext') return seeded;
    if (op === 'invoke') {
      const r = await E.invokeB(ectx, mod.resolver.function, mod.key, { call: { functionKey: payload.functionKey, payload: payload.payload }, context: seeded });
      if (!r.success) throw new Error(r.error.errorMessage);
      return r.body;
    }
    return undefined;
  });
  await page.setContent('<!doctype html><html><head><style>body{font-family:-apple-system,sans-serif;margin:16px}[data-forge-type=Lozenge]{background:#0052CC;color:#fff;padding:1px 6px;border-radius:3px;font-weight:600;font-size:12px}button{background:#0052CC;color:#fff;border:0;padding:8px 14px;border-radius:4px}[data-unmodelled]{outline:2px dashed #DE350B}</style></head><body><div id="root"></div></body></html>');
  // setContent reuses the existing document, so init scripts would not fire: install the host
  // renderer and the bridge directly, BEFORE the UI Kit bundle (invoke.js captures __bridge at load).
  await page.evaluate(HOST_RENDERER);
  await page.evaluate(() => {
    globalThis.__bridge = { callBridge: (op, payload) => (op === 'reconcile' ? window.__paintForgeDoc(payload.forgeDoc) : window.__forgeEmulatorCall(op, payload)) };
  });
  await page.addScriptTag({ path: frontJs });
  await page.getByText('Login button misaligned').waitFor({ timeout: 15000 }).catch(async (e) => { console.log('ROOT:', await page.locator('#root').innerHTML().catch(() => '?'), 'OPS:', ops, 'ERR:', errors, 'DOCS:', await page.evaluate(() => (window.__forgeDocs || []).length)); throw e; });
  await page.getByRole('button', { name: 'Add comment' }).click();
  await page.getByText('Comment status 201').waitFor({ timeout: 15000 });
  const shot = path.join(SCRATCH, 'uikit-after-click.png');
  await page.screenshot({ path: shot });
  const docs = await page.evaluate(() => window.__forgeDocs);
  console.log('UI KIT rendered text:\n' + (await page.locator('#root').innerText()));
  console.log('reconcile count:', docs.length, 'bridge ops:', ops.join(', '));
  console.log('last ForgeDoc types:', JSON.stringify(docs.at(-1), (k, v) => (k === 'props' ? Object.keys(v).filter((x) => x !== 'children') : v)).slice(0, 700));
  console.log('jira calls:', calls.map((c) => `${c.as} ${c.method} ${c.path}`).join(' | '), 'page errors:', errors);
  console.log('screenshot:', shot);
  await browser.close();
  process.exit(0);
})().catch((e) => { console.error('FAILED', e); process.exit(1); });
