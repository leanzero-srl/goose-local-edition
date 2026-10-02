'use strict';
// The entrant's dev kit (DESIGN.md §6.5, STARTER.md, E6): forge-dev against a dev site process —
// invoke with a resolver key, events + reset (the update stream rewinds), and two backgrounded `serve`
// processes plus CLI invocations sharing .forge-dev/state.json without losing a KVS write.
const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');
const { spawn, execFile } = require('child_process');
const { FORGE, ensureKit, scratch, copyDir } = require('./helpers.cjs');

const run = (args, opts) => new Promise((resolve) => execFile(process.execPath, args, { ...opts, maxBuffer: 32 << 20 }, (err, stdout, stderr) => resolve({ code: err ? err.code ?? 1 : 0, stdout, stderr })));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

test('forge-dev: invoke, events/reset rewind, concurrent serve processes keep one consistent state', { timeout: 300_000 }, async () => {
  const kit = ensureKit();
  const dir = scratch('forgedev');
  const app = path.join(dir, 'app');
  copyDir(path.join(FORGE, 'spike', 'app'), app, new Set(['node_modules', 'build']));
  // A trigger that records every Sprint change it is handed, so `events` has visible effects.
  fs.appendFileSync(path.join(app, 'src', 'index.js'), `
export const onUpdate = async (event) => {
  const seen = (await kvs.get('seen')) ?? [];
  seen.push(event.changelog.id);
  await kvs.set('seen', seen);
};
`);
  const manifest = fs.readFileSync(path.join(app, 'manifest.yml'), 'utf8')
    .replace('  scheduledTrigger:', '  trigger:\n    - key: on-update\n      function: on-update\n      events:\n        - avi:jira:updated:issue\n  scheduledTrigger:')
    .replace('  function:\n', '  function:\n    - key: on-update\n      handler: index.onUpdate\n');
  fs.writeFileSync(path.join(app, 'manifest.yml'), manifest);
  const out = path.join(app, 'static', 'panel', 'build');
  fs.mkdirSync(out, { recursive: true });
  fs.writeFileSync(path.join(out, 'index.html'), '<!doctype html><html><head></head><body>panel</body></html>');

  const token = 'ab'.repeat(12);
  const site = spawn(process.execPath, [path.join(FORGE, 'site', 'site.cjs'), '--seed', 'feedfacefeedface', '--token', token], { stdio: ['ignore', 'pipe', 'inherit'] });
  const started = await new Promise((r) => site.stdout.once('data', (d) => r(JSON.parse(String(d)))));
  const env = { ...process.env, FORGE_KIT: kit.kit_dir, FORGE_SITE_URL: started.url.replace('http://', `http://admin:${token}@`) };
  const devBin = path.join(kit.kit_dir, 'bin', 'forge-dev.cjs');
  const serves = [];
  try {
    const users = await run([devBin, 'users'], { cwd: app, env });
    assert.strictEqual(users.code, 0, users.stderr);
    assert.match(users.stdout, /\(default viewer\)/);

    // invoke with a resolver key (user-led, defaulting to the dev viewer) — the spike resolver reads payload.issueKey.
    const pack = require(path.join(FORGE, 'site', 'fixtures.cjs')).facts('feedfacefeedface');
    const visible = pack.issues.find((i) => !i.hiddenFrom.includes(pack.viewer));
    fs.writeFileSync(path.join(dir, 'p.json'), JSON.stringify({ issueKey: visible.key }));
    const inv = await run([devBin, 'invoke', 'resolver', '--resolver', 'summarise', '--payload', path.join(dir, 'p.json')], { cwd: app, env });
    assert.strictEqual(inv.code, 0, inv.stdout + inv.stderr);
    assert.match(inv.stdout, new RegExp(`as ${pack.viewer}`));
    assert.match(inv.stdout, /jira {3}app  GET \/rest\/api\/3\/issue\//);

    // events, then reset rewinds the dev site's update stream and clears dev storage.
    const ev1 = await run([devBin, 'events', '--limit', '3'], { cwd: app, env });
    assert.strictEqual(ev1.code, 0, ev1.stderr);
    const ids1 = [...ev1.stdout.matchAll(/changelog (\d+)/g)].map((m) => m[1]);
    assert.strictEqual(ids1.length, 3);
    const kvs1 = JSON.parse((await run([devBin, 'kvs'], { cwd: app, env })).stdout);
    assert.deepStrictEqual(kvs1.kvs.seen, ids1);
    assert.strictEqual((await run([devBin, 'reset'], { cwd: app, env })).code, 0);
    assert.deepStrictEqual(JSON.parse((await run([devBin, 'kvs'], { cwd: app, env })).stdout).kvs, {});
    const ev2 = await run([devBin, 'events', '--limit', '3'], { cwd: app, env });
    assert.deepStrictEqual([...ev2.stdout.matchAll(/changelog (\d+)/g)].map((m) => m[1]), ids1);

    // Two backgrounded serve processes and the CLI hitting the same storage at once.
    for (let i = 0; i < 2; i++) {
      const p = spawn(process.execPath, [devBin, 'serve', 'spike-issue-panel'], { cwd: app, env, stdio: ['ignore', 'pipe', 'pipe'] });
      serves.push(p);
    }
    const urls = [];
    for (const p of serves) {
      const deadline = Date.now() + 60_000;
      while (!fs.existsSync(path.join(app, '.forge-dev', `serve.${p.pid}.json`)) && Date.now() < deadline) await sleep(100);
      urls.push(JSON.parse(fs.readFileSync(path.join(app, '.forge-dev', `serve.${p.pid}.json`), 'utf8')).url);
    }
    const invokeVia = async (url) => {
      const u = new URL(url);
      const surfaceId = u.pathname.split('/')[2];
      const r = await fetch(`${u.origin}/__forge/op`, { method: 'POST', headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ surfaceId, op: 'invoke', payload: { functionKey: 'summarise', payload: { issueKey: visible.key } } }) });
      return r.json();
    };
    const jobs = [];
    for (let i = 0; i < 5; i++) for (const url of urls) jobs.push(invokeVia(url));
    for (let i = 0; i < 2; i++) jobs.push(run([devBin, 'invoke', 'resolver', '--resolver', 'summarise', '--payload', path.join(dir, 'p.json')], { cwd: app, env }));
    const results = await Promise.all(jobs);
    for (const r of results.slice(0, 10)) assert.ok(r.ok, JSON.stringify(r));
    for (const r of results.slice(10)) assert.strictEqual(r.code, 0, r.stdout + r.stderr);
    const state = JSON.parse(fs.readFileSync(path.join(app, '.forge-dev', 'state.json'), 'utf8'));
    const views = new Map(state.kvs.kv).get(`views:${visible.key}`).value;
    assert.strictEqual(views, 12, 'every one of the 12 concurrent invocations incremented the shared counter exactly once');
  } finally {
    for (const p of serves) { try { process.kill(p.pid, 'SIGTERM'); } catch { /* gone */ } }
    process.kill(site.pid, 'SIGTERM');
  }
});
