'use strict';
// The entrant exercises Forge LLM and Realtime offline (DESIGN §17.2, §6.5): a widget served by `forge-dev serve`
// (one process) receives the publishGlobal of a consumer run by `forge-dev invoke` (another process) through the dev
// site's broker; `forge-dev realtime` lists the publishes, the serve log prints the delivery; `forge-dev llm` shows
// the model list, the script and every call; invoke prints what the scripted model answered.
// Run: node --test forge/kit/test/forge-dev-live.test.cjs
const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');
const { spawn, execFile } = require('child_process');
const { FORGE, ensureKit, playwright, scratch } = require('./helpers.cjs');

const run = (args, opts) => new Promise((resolve) => execFile(process.execPath, args, { ...opts, maxBuffer: 32 << 20 }, (err, stdout, stderr) => resolve({ code: err ? err.code ?? 1 : 0, stdout, stderr })));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

test('forge-dev: realtime across serve and invoke processes, llm visible offline', { timeout: 300_000 }, async () => {
  const kit = ensureKit();
  const app = path.join(scratch('devlive'), 'app');
  fs.mkdirSync(path.join(app, 'src'), { recursive: true });
  fs.writeFileSync(path.join(app, 'src', 'index.js'), `import Resolver from '@forge/resolver';
import { publishGlobal } from '@forge/realtime';
import { chat, list } from '@forge/llm';
const resolver = new Resolver();
resolver.define('ask', async () => {
  const { models } = await list();
  return chat({ model: models.find((m) => m.status === 'active').model, messages: [{ role: 'user', content: 'Explain.' }],
    tools: [{ type: 'function', function: { name: 'report_scope', description: 'd', parameters: { type: 'object' } } }], tool_choice: 'required' });
});
export const handler = resolver.getDefinitions();
export const consume = async (event) => publishGlobal('scope', { sprintId: event.body.sprintId });
`);
  fs.writeFileSync(path.join(app, 'manifest.yml'), `modules:
  dashboards:widget:
    - key: live
      title: Live
      description: Live widget
      thumbnail: https://developer.atlassian.com/platform/forge/images/icons/issue-panel-icon.svg
      resource: view
      resolver:
        function: resolver
  consumer:
    - key: updates
      queue: updates
      function: consume
  llm:
    - key: llm
      model:
        - claude
  function:
    - key: resolver
      handler: index.handler
    - key: consume
      handler: index.consume
resources:
  - key: view
    path: static/view
app:
  id: ari:cloud:ecosystem::app/00000000-0000-0000-0000-000000000000
  runtime:
    name: nodejs22.x
`);
  const { kitPaths } = require(path.join(FORGE, 'kit', 'lib', 'kitpaths.cjs'));
  const paths = kitPaths(path.join(FORGE, 'kit'));
  const view = path.join(app, 'static', 'view');
  fs.mkdirSync(view, { recursive: true });
  fs.writeFileSync(path.join(app, 'page.js'), `import { realtime } from '@forge/bridge';
realtime.subscribeGlobal('scope', (p) => { const li = document.createElement('li'); li.textContent = String(p); document.getElementById('g').appendChild(li); })
  .then(() => { document.getElementById('ready').textContent = 'ready'; });`);
  await paths.require('esbuild').build({ entryPoints: [path.join(app, 'page.js')], bundle: true, format: 'iife', platform: 'browser', outfile: path.join(view, 'main.js'), nodePaths: [paths.appModules], logLevel: 'silent' });
  fs.writeFileSync(path.join(view, 'index.html'), '<!doctype html><html><body><span id="ready"></span><ul id="g"></ul><script src="./main.js"></script></body></html>');
  fs.writeFileSync(path.join(app, 'ev.json'), JSON.stringify({ sprintId: 42 }));

  const token = 'cd'.repeat(12);
  const site = spawn(process.execPath, [path.join(FORGE, 'site', 'site.cjs'), '--seed', 'feedfacefeedface', '--token', token], { stdio: ['ignore', 'pipe', 'inherit'] });
  const started = await new Promise((r) => site.stdout.once('data', (d) => r(JSON.parse(String(d)))));
  const env = { ...process.env, FORGE_KIT: kit.kit_dir, FORGE_SITE_URL: started.url.replace('http://', `http://admin:${token}@`) };
  const dev = path.join(kit.kit_dir, 'bin', 'forge-dev.cjs');
  const serveOut = [];
  const serve = spawn(process.execPath, [dev, 'serve', 'live'], { cwd: app, env, stdio: ['ignore', 'pipe', 'pipe'] });
  serve.stdout.on('data', (d) => serveOut.push(String(d)));
  serve.stderr.on('data', (d) => serveOut.push(String(d)));
  const browser = await playwright().chromium.launch({ headless: true, executablePath: process.env.BENCH_BROWSER_EXECUTABLE || undefined });
  try {
    const rec = path.join(app, '.forge-dev', `serve.${serve.pid}.json`);
    for (let i = 0; i < 600 && !fs.existsSync(rec); i++) await sleep(100);
    const page = await browser.newPage();
    await page.goto(JSON.parse(fs.readFileSync(rec, 'utf8')).url);
    await page.locator('#ready').filter({ hasText: 'ready' }).waitFor({ timeout: 30_000 });

    const inv = await run([dev, 'invoke', 'consume', '--payload', path.join(app, 'ev.json')], { cwd: app, env });
    assert.strictEqual(inv.code, 0, inv.stdout + inv.stderr);
    assert.match(inv.stdout, /publishGlobal 'scope' payload \{"sprintId":42\} -> delivered/);
    await page.waitForFunction(() => [...document.querySelectorAll('#g li')].some((li) => li.textContent === '{"sprintId":42}'), null, { timeout: 20_000 });
    for (let i = 0; i < 50 && !serveOut.join('').includes('realtimeEvent'); i++) await sleep(100);
    assert.match(serveOut.join(''), /bridge realtimeEvent 'scope' delivered to the page: \{"sprintId":42\}/);

    const rt = await run([dev, 'realtime'], { cwd: app, env });
    assert.strictEqual(rt.code, 0, rt.stderr);
    assert.match(rt.stdout, /publishGlobal 'scope' from function updates \(consumer\) payload \{"sprintId":42\} -> delivered to 1 subscription/);

    const ask = await run([dev, 'invoke', 'resolver', '--module', 'live', '--resolver', 'ask'], { cwd: app, env });
    assert.strictEqual(ask.code, 0, ask.stdout + ask.stderr);
    assert.match(ask.stdout, /models: .*claude-opus-4-6 \(deprecated\)/);
    assert.match(ask.stdout, /finish_reason tool_use, tool report_scope\(/);
    const llm = await run([dev, 'llm'], { cwd: app, env });
    assert.match(llm.stdout, /next chat call gets: digits/);
    assert.match(llm.stdout, /2 call\(s\); every prompt and answer in full: \.forge-dev\/llm-log\.json/);
    assert.strictEqual(JSON.parse(fs.readFileSync(path.join(app, '.forge-dev', 'llm-log.json'), 'utf8'))[1].request.messages[0].content, 'Explain.');
    const restarted = await run([dev, 'llm', '--phase', 'again'], { cwd: app, env });
    assert.match(restarted.stdout, /next chat call gets: clean/);
  } finally {
    await browser.close();
    try { process.kill(serve.pid, 'SIGTERM'); } catch { /* gone */ }
    process.kill(site.pid, 'SIGTERM');
  }
});
