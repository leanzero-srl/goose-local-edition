'use strict';
// The entrant's dev kit (DESIGN.md §6.5, STARTER.md, E6): forge-dev against a dev site process —
// invoke with a resolver key, events + reset (the update stream rewinds), and two backgrounded `serve`
// processes plus CLI invocations sharing .forge-dev/state.json without losing a KVS write.
const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');
const { spawn, execFile } = require('child_process');
const { FORGE, SPIKE, ensureKit, scratch, copyDir } = require('./helpers.cjs');

const run = (args, opts) => new Promise((resolve) => execFile(process.execPath, args, { ...opts, maxBuffer: 32 << 20 }, (err, stdout, stderr) => resolve({ code: err ? err.code ?? 1 : 0, stdout, stderr })));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

test('forge-dev: invoke, events/reset rewind, concurrent serve processes keep one consistent state', { timeout: 300_000 }, async () => {
  const kit = ensureKit();
  const dir = scratch('forgedev');
  const app = path.join(dir, 'app');
  copyDir(path.join(SPIKE, 'app'), app, new Set(['node_modules', 'build']));
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
    assert.match(users.stdout, /may not comment on [A-Z]+-\d+: .+ \(Jira answers 400/);

    // invoke with a resolver key (user-led, defaulting to the dev viewer) — the spike resolver reads payload.issueKey.
    const pack = require(path.join(FORGE, 'site', 'fixtures.cjs')).facts('feedfacefeedface');
    const visible = pack.issues.find((i) => !i.hiddenFrom.includes(pack.viewer));
    fs.writeFileSync(path.join(dir, 'p.json'), JSON.stringify({ issueKey: visible.key }));
    const inv = await run([devBin, 'invoke', 'resolver', '--resolver', 'summarise', '--payload', path.join(dir, 'p.json')], { cwd: app, env });
    assert.strictEqual(inv.code, 0, inv.stdout + inv.stderr);
    assert.match(inv.stdout, new RegExp(`as ${pack.viewer}`));
    assert.match(inv.stdout, /jira {3}app  GET \/rest\/api\/3\/issue\//);
    // The whole result is written to a file every time.
    const last = JSON.parse(fs.readFileSync(path.join(app, '.forge-dev', 'last-result.json'), 'utf8'));
    assert.strictEqual(last.summary, visible.fields.summary);

    // A stale lock left by a dead process is taken over; a malformed --config is an error that exits.
    fs.writeFileSync(path.join(app, '.forge-dev', 'state.lock'), '999999:dead');
    const afterStale = await run([devBin, 'invoke', 'resolver', '--resolver', 'summarise', '--payload', path.join(dir, 'p.json')], { cwd: app, env });
    assert.strictEqual(afterStale.code, 0, afterStale.stdout + afterStale.stderr);
    assert.ok(!fs.existsSync(path.join(app, '.forge-dev', 'state.lock')));
    const bad = await run([devBin, 'serve', 'spike-issue-panel', '--config', '{bad'], { cwd: app, env, timeout: 60_000 });
    assert.notStrictEqual(bad.code, 0);
    assert.match(bad.stderr + bad.stdout, /--config is not valid JSON/);

    // events, then reset rewinds the dev site's update stream and clears dev storage.
    const ev1 = await run([devBin, 'events', '--limit', '3'], { cwd: app, env });
    assert.strictEqual(ev1.code, 0, ev1.stderr);
    const ids1 = [...ev1.stdout.matchAll(/changelog (\d+)/g)].map((m) => m[1]);
    assert.strictEqual(ids1.length, 3);
    const kvs1 = JSON.parse((await run([devBin, 'kvs'], { cwd: app, env })).stdout);
    assert.deepStrictEqual(kvs1.kvs.seen, ids1);
    const st = JSON.parse(fs.readFileSync(path.join(app, '.forge-dev', 'state.json'), 'utf8'));
    fs.writeFileSync(path.join(app, '.forge-dev', 'state.json'), JSON.stringify({ ...st, widgetConfigs: { 'dev-widget': { boardId: 1 } } }));
    assert.deepStrictEqual(JSON.parse((await run([devBin, 'kvs'], { cwd: app, env })).stdout).widgetConfigs, { 'dev-widget': { boardId: 1 } });
    assert.strictEqual((await run([devBin, 'reset'], { cwd: app, env })).code, 0);
    const cleared = JSON.parse((await run([devBin, 'kvs'], { cwd: app, env })).stdout);
    assert.deepStrictEqual(cleared.kvs, {});
    assert.deepStrictEqual(cleared.widgetConfigs, {}, 'reset clears saved widget configs');
    const ev2 = await run([devBin, 'events', '--limit', '3'], { cwd: app, env });
    assert.deepStrictEqual([...ev2.stdout.matchAll(/changelog (\d+)/g)].map((m) => m[1]), ids1);

    // Four backgrounded serve processes started 0-300 ms apart (ALT-NOTES 1.7 lost one to a lock ENOENT) and the
    // CLI hitting the same storage at once.
    const serveErr = [];
    for (let i = 0; i < 4; i++) {
      const p = spawn(process.execPath, [devBin, 'serve', 'spike-issue-panel'], { cwd: app, env, stdio: ['ignore', 'pipe', 'pipe'] });
      p.stderr.on('data', (d) => serveErr.push(String(d)));
      p.on('exit', (code) => { if (code) serveErr.push(`serve ${p.pid} exited ${code}`); });
      serves.push(p);
      await sleep(100);
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
    for (let i = 0; i < 3; i++) for (const url of urls) jobs.push(invokeVia(url));
    for (let i = 0; i < 2; i++) jobs.push(run([devBin, 'invoke', 'resolver', '--resolver', 'summarise', '--payload', path.join(dir, 'p.json')], { cwd: app, env }));
    const results = await Promise.all(jobs);
    for (const r of results.slice(0, 12)) assert.ok(r.ok, JSON.stringify(r));
    for (const r of results.slice(12)) assert.strictEqual(r.code, 0, r.stdout + r.stderr);
    const state = JSON.parse(fs.readFileSync(path.join(app, '.forge-dev', 'state.json'), 'utf8'));
    const views = new Map(state.kvs.kv).get(`views:${visible.key}`).value;
    assert.strictEqual(views, 14, 'every one of the 14 concurrent invocations incremented the shared counter exactly once');
    assert.deepStrictEqual(serveErr, [], 'no serve process failed');
    // The top-level dev page's favicon request answers 204, not a console-error 404.
    const fav = await fetch(new URL('/favicon.ico', urls[0]));
    assert.strictEqual(fav.status, 204);
  } finally {
    for (const p of serves) { try { process.kill(p.pid, 'SIGTERM'); } catch { /* gone */ } }
    process.kill(site.pid, 'SIGTERM');
  }
});

// SPEC §2.8: `events` follows the dev site's plan batches — an update whose queue work the scoring drive leaves
// pending until a later, related update (here a redelivery three slots on: dev seed fbc67de4b35a45e8 opens its first
// batch at slot 3 and closes it at slot 6) is followed by the rest of the batch before the queues drain, past --limit,
// so the two consumer deliveries of that change run concurrently, as on the scoring site.
test('forge-dev: events completes an open batch before draining; the redelivery runs beside its original', { timeout: 300_000 }, async () => {
  const kit = ensureKit();
  const app = path.join(scratch('forgedev-batch'), 'app');
  fs.mkdirSync(path.join(app, 'src'), { recursive: true });
  fs.writeFileSync(path.join(app, 'src', 'index.js'), `import { Queue } from '@forge/events';
export const trig = async (event) => { await new Queue({ key: 'work' }).push({ body: { changelogId: String(event.changelog.id) } }); };
export const work = async (event) => ({ seen: event.body.changelogId });
`);
  fs.writeFileSync(path.join(app, 'manifest.yml'), `modules:
  trigger:
    - key: b-trigger
      function: b-trig
      events:
        - avi:jira:updated:issue
  consumer:
    - key: b-work
      queue: work
      function: b-work
  function:
    - key: b-trig
      handler: index.trig
    - key: b-work
      handler: index.work
permissions:
  scopes:
    - read:jira-work
app:
  id: ari:cloud:ecosystem::app/00000000-0000-0000-0000-000000000000
  runtime:
    name: nodejs22.x
`);
  const token = 'cd'.repeat(12);
  const site = spawn(process.execPath, [path.join(FORGE, 'site', 'site.cjs'), '--seed', 'fbc67de4b35a45e8', '--token', token], { stdio: ['ignore', 'pipe', 'inherit'] });
  const started = await new Promise((r) => site.stdout.once('data', (d) => r(JSON.parse(String(d)))));
  const env = { ...process.env, FORGE_KIT: kit.kit_dir, FORGE_SITE_URL: started.url.replace('http://', `http://admin:${token}@`) };
  try {
    const ev = await run([path.join(kit.kit_dir, 'bin', 'forge-dev.cjs'), 'events', '--limit', '4'], { cwd: app, env });
    assert.strictEqual(ev.code, 0, ev.stdout + ev.stderr);
    const updates = [...ev.stdout.matchAll(/^### update \S+ changelog (\d+)( \(redelivery\))?/gm)];
    assert.strictEqual(updates.length, 7, 'slots 0-3 asked for, 4-6 delivered to close the batch opened at slot 3');
    assert.strictEqual((ev.stdout.match(/^ {3}queued: /gm) ?? []).length, 3, 'slots 3, 4 and 5 leave their queue work pending');
    const redelivered = updates.find((m) => m[2])[1];
    assert.strictEqual(updates[3][1], redelivered, 'the batch opens on the change it ends by redelivering');
    const paired = [...ev.stdout.matchAll(/^ {3}ran at the same time as event \S+ \((same-event|same-issue)\)/gm)].map((m) => m[1]);
    assert.deepStrictEqual(paired, ['same-event', 'same-event'], 'the original and its redelivery, each naming the other');
  } finally {
    process.kill(site.pid, 'SIGTERM');
  }
});
