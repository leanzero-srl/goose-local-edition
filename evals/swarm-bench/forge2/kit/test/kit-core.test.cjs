'use strict';
// Forge 2.0 kit-core (P1) guarantees, each against the real thing it claims (Atlassian's pinned runtime wrapper, the
// sandbox fence, the forge2 site and lint):
//   virtual time  waits and proxied requests cost virtual time, never wall time; the limit kills; a timer and a
//                 request are ordered by virtual time; a returned invocation is never killed by work it left behind;
//   N1            a trigger starts at the site's time after its change was applied (the stale 1.0 clock is gone);
//   defect C      a consumer declared `resolver: {function, method}` is invoked, as @forge/resolver dispatches;
//   redelivery    InvocationError retryAfter (consumer and product trigger) and the trigger retry cap;
//   probe door    callResolver builds the platform context for asUser; a forged payload cannot change it;
//   webtrigger    getUrl answers the emulator's public route; without the ingress that route is a loud 501;
//   KVS           the measured real-Forge codes (SPEC §2.4);
//   lint          the six measured server refusals at manifest.yml 0:0, after the client half;
//   defect A      host flags let clicks through.
// Run: node --test evals/swarm-bench/forge2/kit/test/kit-core.test.cjs
const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');
const { ensureKit, scratch } = require('./helpers.cjs');

const KIT = path.resolve(__dirname, '..');
const SITE = path.resolve(__dirname, '..', '..', 'site', 'site.cjs');
const APP_ID = 'ari:cloud:ecosystem::app/00000000-0000-0000-0000-000000000000';

const SOURCE = `import api, { route, webTrigger } from '@forge/api';
import { kvs } from '@forge/kvs';
import Resolver from '@forge/resolver';
import { InvocationError, Queue } from '@forge/events';
import { list, stream } from '@forge/llm';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
export const sched = async (event) => {
  const t0 = Date.now();
  switch (event.op) {
    case 'sleep': await sleep(event.ms); return { waited: Date.now() - t0 };
    case 'requests':
      for (let i = 0; i < 3; i++) await api.asApp().requestJira(route\`/rest/api/3/myself\`);
      await api.asApp().requestJira(route\`/rest/api/3/search/jql?jql=\${'project = ' + event.project + ' order by key'}&maxResults=10\`);
      return { elapsed: Date.now() - t0 };
    case 'race': {
      const winner = await Promise.race([api.asApp().requestJira(route\`/rest/api/3/myself\`).then(() => 'request'), sleep(event.ms).then(() => 'timer')]);
      return { winner, elapsed: Date.now() - t0 };
    }
    case 'kvs': await kvs.set('vt-key', 1); await kvs.get('vt-key'); return { elapsed: Date.now() - t0 };
    case 'leftover': setInterval(() => {}, 1000); setTimeout(() => {}, 60000); return { returned: true };
    case 'subtle': await crypto.subtle.digest('SHA-256', new TextEncoder().encode('x')); return { elapsed: Date.now() - t0 };
    case 'push': await new Queue({ key: event.queue }).push({ body: event.body }); return { pushed: true };
    case 'url': return { url: await webTrigger.getUrl('vt-ci') };
    case 'poll': {
      await kvs.set('vt-poll', 0);
      let ticks = 0;
      await new Promise((resolve) => { const h = setInterval(() => { ticks++; if (ticks === 5) { clearInterval(h); resolve(); } }, 2000); });
      return { ticks, elapsed: Date.now() - t0 };
    }
    case 'stream': {
      const models = await list();
      const s = await stream({ model: models.models.find((m) => m.status === 'active').model, messages: [{ role: 'user', content: 'x' }] });
      const chunks = [];
      for await (const c of s) chunks.push(c);
      s.close();
      return { chunks: chunks.length, finish: chunks.map((c) => c.choices[0].finish_reason ?? null).filter(Boolean), elapsed: Date.now() - t0 };
    }
    default: throw new Error('unknown op ' + event.op);
  }
};
export const consume = async (event) => {
  console.log('CONSUME ' + JSON.stringify({ body: event.body, retryCount: event.retryContext?.retryCount ?? 0, at: Date.now() }));
  if (event.body?.retry && !event.retryContext) return new InvocationError({ retryAfter: 30 });
  return { ok: true };
};
const r = new Resolver();
r.define('consume-it', async ({ payload, context }) => { console.log('RCONSUME ' + JSON.stringify({ payload, jobId: context.jobId })); return { ok: true }; });
export const rconsume = r.getDefinitions();
export const trig = async (event) => {
  console.log('TRIG ' + JSON.stringify({ at: Date.now(), retryCount: event.retryContext?.retryCount ?? 0 }));
  if (!event.retryContext) return new InvocationError({ retryAfter: 30 });
  return { ok: true };
};
const ui = new Resolver();
ui.define('whoami', ({ payload, context }) => ({ accountId: context.accountId, claimed: payload.accountId ?? null, ext: context.extension?.type ?? null }));
ui.define('sleep', async ({ payload }) => { await sleep(payload.ms); return { slept: payload.ms }; });
export const resolve = ui.getDefinitions();
export const web = async (req) => ({ statusCode: 200, body: JSON.stringify({ got: req.body }) });
`;

function writeApp(dir) {
  fs.mkdirSync(path.join(dir, 'src'), { recursive: true });
  fs.mkdirSync(path.join(dir, 'static'), { recursive: true });
  fs.writeFileSync(path.join(dir, 'src', 'index.js'), SOURCE);
  fs.writeFileSync(path.join(dir, 'manifest.yml'), `modules:
  scheduledTrigger:
    - key: vt-hourly
      function: vt-sched
      interval: hour
  consumer:
    - key: vt-consumer
      queue: vt-queue
      function: vt-consume
    - key: vt-resolver-consumer
      queue: vt-rqueue
      resolver:
        function: vt-rconsume
        method: consume-it
  trigger:
    - key: vt-trigger
      function: vt-trig
      events:
        - avi:jira:updated:issue
  jira:adminPage:
    - key: vt-admin
      title: VT admin
      resource: main
      render: native
      resolver:
        function: vt-resolve
  webtrigger:
    - key: vt-ci
      function: vt-web
  llm:
    - key: llm
      model:
        - claude
  function:
    - key: vt-sched
      handler: index.sched
    - key: vt-consume
      handler: index.consume
    - key: vt-rconsume
      handler: index.rconsume
    - key: vt-trig
      handler: index.trig
    - key: vt-resolve
      handler: index.resolve
    - key: vt-web
      handler: index.web
resources:
  - key: main
    path: static
permissions:
  scopes:
    - read:jira-work
    - read:jira-user
    - storage:app
app:
  id: ${APP_ID}
  runtime:
    name: nodejs22.x
`);
}

let shared = null;
async function world() {
  if (shared) return shared;
  ensureKit();
  const { createSite } = require(SITE);
  const { createEmulator } = require(path.join(KIT, 'lib', 'emulator.cjs'));
  const appDir = path.join(scratch('kitcore'), 'app');
  writeApp(appDir);
  const site = await createSite({ seed: '0123456789abcdef' });
  const emu = await createEmulator({ appDir, site, runtime: 'wrapper' });
  const built = await emu.build();
  assert.ok(built.functions.every((f) => f.loaded), JSON.stringify(built.functions));
  shared = { site, emu };
  return shared;
}
test.after(async () => { if (shared) { await shared.emu.close(); await shared.site.stop(); } });

const sched = (emu, event) => emu.invoke('vt-sched', { moduleKey: 'vt-hourly', event });
const logLines = (r, tag) => (r.logs ?? []).flatMap((l) => l.logArguments ?? []).filter((x) => String(x).startsWith(`${tag} `)).map((x) => JSON.parse(String(x).slice(tag.length + 1)));

test('virtual time: a 30 s wait costs 30 virtual s and no wall time; the clock the app reads agrees', { timeout: 120_000 }, async () => {
  const { emu } = await world();
  const r = await sched(emu, { op: 'sleep', ms: 30_000 });
  assert.ok(r.ok, JSON.stringify(r.error));
  assert.strictEqual(r.result.waited, 30_000);
  assert.strictEqual(r.vms, 30_000, `vms ${r.vms}`);
  assert.ok(r.ms < 15_000, `real ms ${r.ms}`);
  assert.strictEqual(Date.parse(r.t1) - Date.parse(r.t0), 30_000);
});

test('virtual time under the dev fence (node --permission, what forge-dev uses inside a workspace sandbox)', { timeout: 120_000 }, async () => {
  const { site } = await world();
  const { createEmulator } = require(path.join(KIT, 'lib', 'emulator.cjs'));
  const appDir = path.join(scratch('kitcore-dev'), 'app');
  writeApp(appDir);
  const emu = await createEmulator({ appDir, site, runtime: 'wrapper', fence: 'node-permission' });
  try {
    assert.ok((await emu.build()).functions.every((f) => f.loaded));
    const r = await sched(emu, { op: 'sleep', ms: 30_000 });
    assert.ok(r.ok, JSON.stringify(r.error));
    assert.strictEqual(r.vms, 30_000);
    const k = await sched(emu, { op: 'sleep', ms: 60_000 });
    assert.strictEqual(k.timedOut, true);
    assert.strictEqual(k.vms, 55_000);
  } finally { await emu.close(); }
});

test('virtual time: proxied requests cost their class (3 GET + 1 search page = 660 ms), stamped per call', { timeout: 120_000 }, async () => {
  const { emu } = await world();
  const r = await sched(emu, { op: 'requests', project: emu.siteInfo.projects[0].key });
  assert.ok(r.ok, JSON.stringify(r.error));
  assert.strictEqual(r.result.elapsed, 3 * 120 + 300);
  assert.strictEqual(r.vms, 660);
  const jira = r.calls.filter((c) => c.service === 'jira');
  assert.deepStrictEqual(jira.map((c) => c.status), [200, 200, 200, 200], JSON.stringify(jira.map((c) => [c.path, c.status])));
  assert.deepStrictEqual(jira.map((c) => Date.parse(c.t_virtual) - Date.parse(r.t0)), [0, 120, 240, 360]);
  assert.deepStrictEqual(jira.map((c) => c.vcostMs), [120, 120, 120, 300]);
});

test('virtual time: a 50 ms timer beats a 120 ms GET; KVS read 120 + write 200', { timeout: 120_000 }, async () => {
  const { emu } = await world();
  const race = await sched(emu, { op: 'race', ms: 50 });
  assert.ok(race.ok, JSON.stringify(race.error));
  assert.strictEqual(race.result.winner, 'timer');
  assert.strictEqual(race.result.elapsed, 50);
  const slow = await sched(emu, { op: 'race', ms: 500 });
  assert.strictEqual(slow.result.winner, 'request');
  assert.strictEqual(slow.result.elapsed, 120);
  const k = await sched(emu, { op: 'kvs' });
  assert.ok(k.ok, JSON.stringify(k.error));
  assert.strictEqual(k.result.elapsed, 200 + 120);
});

test('virtual time: an app interval it waits on moves time (5 x 2 s after one KVS write = 10.2 s)', { timeout: 120_000 }, async () => {
  const { emu } = await world();
  const r = await sched(emu, { op: 'poll' });
  assert.ok(r.ok, JSON.stringify(r.error));
  assert.deepStrictEqual(r.result, { ticks: 5, elapsed: 200 + 10_000 });
});

test('virtual time: a Forge LLM stream read through the agent keeps every chunk (list GET 120 + stream POST 200)', { timeout: 120_000 }, async () => {
  const { emu } = await world();
  const r = await sched(emu, { op: 'stream' });
  assert.ok(r.ok, JSON.stringify(r.error));
  assert.ok(r.result.chunks >= 2, JSON.stringify(r.result));
  assert.strictEqual(r.result.finish.length, 1, JSON.stringify(r.result));
  assert.strictEqual(r.result.elapsed, 120 + 200);
});

test('limits: past the module limit the invocation is killed in virtual time (55 s scheduled, 25 s resolver)', { timeout: 120_000 }, async () => {
  const { emu } = await world();
  const r = await sched(emu, { op: 'sleep', ms: 60_000 });
  assert.strictEqual(r.ok, false);
  assert.strictEqual(r.timedOut, true);
  assert.strictEqual(r.realTimeout, false);
  assert.strictEqual(r.vms, 55_000);
  assert.ok(r.ms < 15_000, `real ms ${r.ms}`);
  const ok = await emu.callResolver({ moduleKey: 'vt-admin', functionKey: 'sleep', payload: { ms: 24_000 }, asUser: emu.siteInfo.viewer });
  assert.ok(ok.ok, JSON.stringify(ok.error));
  const killed = await emu.callResolver({ moduleKey: 'vt-admin', functionKey: 'sleep', payload: { ms: 26_000 }, asUser: emu.siteInfo.viewer });
  assert.strictEqual(killed.timedOut, true);
  assert.strictEqual(killed.vms, 25_000);
});

test('limits: work left behind after the handler returned neither moves the clock nor kills it; the wrapper log interval does not move time', { timeout: 120_000 }, async () => {
  const { emu } = await world();
  const r = await sched(emu, { op: 'leftover' });
  assert.ok(r.ok, JSON.stringify(r.error));
  assert.strictEqual(r.vms, 0);
  const s = await sched(emu, { op: 'subtle' });
  assert.ok(s.ok, JSON.stringify(s.error));
  assert.strictEqual(s.result.elapsed, 0);
  assert.strictEqual(s.vms, 0);
});

test('N1: a trigger starts at the site time after its change was applied; it is short in virtual time', { timeout: 180_000 }, async () => {
  const { emu, site } = await world();
  const d = await emu.deliverNext();
  assert.ok(d, 'the site has a live change to deliver');
  const t = d.triggers[0];
  const created = Date.parse(site.pack.live.find((c) => c.changelogId === d.changelogId).created);
  assert.ok(Date.parse(t.t0) >= created, `trigger t0 ${t.t0} is before its change ${new Date(created).toISOString()}`);
  assert.ok(t.vms < 1000, `trigger vms ${t.vms}`);
});

test('redelivery: a product trigger InvocationError is retried after retryAfter with retryContext (≤ 4 retries)', { timeout: 180_000 }, async () => {
  const { emu } = await world();
  const d = await emu.deliverNext();
  const first = d.triggers[0];
  assert.strictEqual(first.retry?.kind, 'retry');
  assert.strictEqual(first.retry.waitS, 30);
  const ds = await emu.drainQueues();
  const retried = ds.find((x) => x.eventId === `trigger:vt-trigger:${d.changelogId}:1`);
  assert.ok(retried, JSON.stringify(ds.map((x) => [x.kind, x.eventId, x.outcome])));
  assert.strictEqual(retried.outcome, 'ok');
  const [line] = logLines(retried, 'TRIG');
  assert.strictEqual(line.retryCount, 1);
  // The 1.0-forked site clock also runs on wall time, so a gap BETWEEN invocations carries a few ms of it.
  const gap = Date.parse(retried.t) - Date.parse(first.t1);
  assert.ok(gap >= 30_000 && gap < 32_000, `retry gap ${gap}`);
});

test('redelivery: consumer InvocationError comes back after 30 virtual s; defect C: the resolver-form consumer runs', { timeout: 180_000 }, async () => {
  const { emu } = await world();
  assert.ok((await sched(emu, { op: 'push', queue: 'vt-queue', body: { retry: true } })).ok);
  assert.ok((await sched(emu, { op: 'push', queue: 'vt-rqueue', body: { hello: 'resolver' } })).ok);
  const ds = await emu.drainQueues();
  const c = ds.filter((x) => x.queueName === 'vt-queue');
  assert.deepStrictEqual(c.map((x) => x.outcome), ['retry', 'ok'], JSON.stringify(ds.map((x) => [x.queueName, x.outcome, x.error?.message])));
  const [a] = logLines(c[0], 'CONSUME');
  const [b] = logLines(c[1], 'CONSUME');
  assert.strictEqual(b.retryCount, 1);
  assert.ok(b.at - a.at >= 30_000 && b.at - a.at < 32_000, `redelivery gap ${b.at - a.at}`);
  const rc = ds.find((x) => x.queueName === 'vt-rqueue');
  assert.strictEqual(rc?.outcome, 'ok', JSON.stringify(rc));
  const [line] = logLines(rc, 'RCONSUME');
  assert.deepStrictEqual(line.payload, { hello: 'resolver' });
  assert.ok(line.jobId, 'jobId reaches the resolver context');
});

test('probe door: callResolver builds the platform context for asUser; a forged payload identity stays in the payload', { timeout: 120_000 }, async () => {
  const { emu } = await world();
  const users = emu.siteInfo.users.map((u) => u.accountId);
  const [me, other] = [emu.siteInfo.viewer, users.find((u) => u !== emu.siteInfo.viewer)];
  const r = await emu.callResolver({ moduleKey: 'vt-admin', functionKey: 'whoami', payload: { accountId: other }, asUser: me });
  assert.ok(r.ok, JSON.stringify(r.error));
  assert.deepStrictEqual(r.result, { accountId: me, claimed: other, ext: 'jira:adminPage' });
  assert.strictEqual(r.asUser, me);
});

test('webtrigger: getUrl answers the public route; with no ingress installed the route answers a loud 501', { timeout: 120_000 }, async () => {
  const { emu } = await world();
  const u = await sched(emu, { op: 'url' });
  assert.ok(u.ok, JSON.stringify(u.error));
  assert.strictEqual(u.result.url, emu.webtriggerUrl('vt-ci'));
  const direct = await emu.invokeWebtrigger('vt-ci', { method: 'POST', path: '/', headers: {}, queryParameters: {}, body: '{"a":1}' });
  assert.ok(direct.ok, JSON.stringify(direct.error));
  assert.strictEqual(direct.timeoutSec, 55);
  assert.deepStrictEqual(JSON.parse(direct.result.body), { got: '{"a":1}' });
  if (!fs.existsSync(path.join(KIT, 'lib', 'webtrigger.cjs'))) {
    const res = await fetch(u.result.url, { method: 'POST', body: '{}' });
    assert.strictEqual(res.status, 501);
    assert.strictEqual((await res.json()).code, 'EMULATOR_NOT_MODELLED');
    assert.ok(emu.harnessMissing.some((m) => /webtrigger ingress/.test(m.what)));
  }
});

test('KVS: the measured real-Forge codes (SPEC §2.4)', () => {
  const { createKvs } = require(path.join(KIT, 'lib', 'kvs.cjs'));
  const kvs = createKvs({ entities: [{ name: 'row', attributes: { n: { type: 'integer' } } }] });
  const code = (op, body) => { const r = kvs.handle(op, body); return r.status < 300 ? 'ok' : `${r.status} ${r.body.code}`; };
  assert.strictEqual(code('/api/v1/set', { key: 'a', value: 1 }), 'ok');
  assert.strictEqual(code('/api/v1/set', { key: 'a', value: 2, options: { keyPolicy: 'FAIL_IF_EXISTS' } }), '409 KEY_CONFLICT');
  assert.strictEqual(code('/api/v1/transaction', { check: [{ entityName: 'row', key: 'missing', conditions: { and: [{ property: 'n', condition: 'EQUAL_TO', values: [1] }] } }] }), '400 CONDITIONAL_CHECK_FAILED');
  assert.strictEqual(code('/api/v1/transaction', { set: Array.from({ length: 26 }, (_, i) => ({ key: `t${i}`, value: i })) }), '422 UNPROCESSABLE_ENTITY');
  assert.strictEqual(code('/api/v1/entity/set', { entityName: 'nope', key: 'x', value: {} }), '404 SCHEMA_NOT_FOUND');
  assert.strictEqual(code('/api/v1/transaction', { set: [{ key: 'd', value: 1 }, { key: 'd', value: 2 }] }), '400 KEY_DUPLICATION_ERROR');
  assert.strictEqual(code('/api/v1/batch/set', Array.from({ length: 26 }, (_, i) => ({ key: `b${i}`, value: i }))), '400 TOO_MANY_BATCH_ENTITIES');
  assert.strictEqual(code('/api/v1/entity/set', { entityName: 'row', key: 'x', value: { n: 2 ** 31 } }), '400 INCORRECT_PROPERTY_TYPE');
  assert.strictEqual(code('/api/v1/set', { key: 'z', value: 1, options: { ttl: { value: 0, unit: 'SECONDS' } } }), '400 INVALID_TTL');
});

const LINT_BASE = {
  modules: { webtrigger: [{ key: 'wt', function: 'fn' }], function: [{ key: 'fn', handler: 'index.run' }] },
  permissions: { scopes: ['storage:app'] },
  app: { id: APP_ID, runtime: { name: 'nodejs22.x' },
    storage: { entities: [{ name: 'scope-change', attributes: { sprintId: { type: 'string' }, at: { type: 'float' }, changeId: { type: 'string' } },
      indexes: [{ name: 'by-sprint', partition: ['sprintId'], range: ['at'] }] }] } },
};
const variant = (fn) => { const m = JSON.parse(JSON.stringify(LINT_BASE)); fn(m); return m; };
const ix = (m) => m.app.storage.entities[0].indexes[0];
const VARIANTS = {
  v01: variant((m) => { ix(m).range = ['at', 'changeId']; }),
  v02: variant((m) => { ix(m).name = 'bs'; }),
  v03: variant((m) => { ix(m).name = 'by sprint'; }),
  v07: variant((m) => { for (let i = 0; i < 20; i++) m.app.storage.entities.push({ name: `e-${i}`, attributes: { a: { type: 'string' } } }); }),
  v08: variant((m) => { m.app.runtime.name = 'nodejs20.x'; }),
  v10: variant((m) => { ix(m).name = 'x'.repeat(51); }),
};
const PASSING = {
  v00: LINT_BASE,
  v04: variant((m) => { m.app.storage.entities[0].indexes.push({ name: 'by-sprint', range: ['at'] }); }),
  v05: variant((m) => { m.app.storage.entities[0].attributes.blob = { type: 'any' }; ix(m).range = ['blob']; }),
  v09: variant((m) => { m.app.storage.entities[0].indexes = ['at']; }),
};

test('lint pack: exactly the six measured server refusals; the documented-but-unenforced constraints pass', () => {
  const { rules, MESSAGES } = require(path.join(KIT, 'lint-pack', 'server-rules.cjs'));
  const expected = { v01: MESSAGES.rangeCount, v02: MESSAGES.nameShort, v03: MESSAGES.nameChars, v07: MESSAGES.entityCount, v08: MESSAGES.node20, v10: MESSAGES.nameLong };
  for (const [v, m] of Object.entries(VARIANTS)) {
    const f = rules(m);
    assert.deepStrictEqual(f.map((x) => [x.variant, x.reason, x.rule, x.category]), [[v, expected[v], 'MANIFEST_INVALID_RULE', 'ERROR']], v);
  }
  for (const [v, m] of Object.entries(PASSING)) assert.deepStrictEqual(rules(m), [], v);
});

test('lint: bin/lint.cjs reports the server refusals at manifest.yml 0:0 after the client half (exit 1); a clean app exits 0', { timeout: 180_000 }, () => {
  ensureKit();
  const YAML = require(require.resolve('yaml', { paths: [path.join(process.env.FORGE_KIT_MODULES, 'lint-modules', 'node_modules')] }));
  const lint = (manifest) => {
    const dir = path.join(scratch('lintpack'), 'app');
    fs.mkdirSync(path.join(dir, 'src'), { recursive: true });
    fs.writeFileSync(path.join(dir, 'src', 'index.js'), 'export const run = async () => ({ statusCode: 204 });\n');
    fs.writeFileSync(path.join(dir, 'manifest.yml'), YAML.stringify(manifest));
    const r = spawnSync(process.execPath, [path.join(KIT, 'bin', 'lint.cjs'), '--json', dir], { encoding: 'utf8', env: { ...process.env, FORGE_KIT: '' } });
    const line = r.stdout.split('\n').find((l) => l.startsWith('LINT_JSON '));
    assert.ok(line, `no LINT_JSON (exit ${r.status}): ${r.stderr.slice(-800)}`);
    return { status: r.status, out: JSON.parse(line.slice('LINT_JSON '.length)) };
  };
  const clean = lint(LINT_BASE);
  assert.strictEqual(clean.status, 0, JSON.stringify(clean.out.problems));
  const two = lint(VARIANTS.v01);
  assert.strictEqual(two.status, 1);
  const server = two.out.problems.filter((p) => p.reference === 'MANIFEST_INVALID_RULE');
  assert.deepStrictEqual(server.map((p) => [p.file, p.line, p.column, p.sev, p.message]),
    [['manifest.yml', 0, 0, 'error', 'Storage entity named index must include exactly one range attribute.']]);
  const node20 = lint(VARIANTS.v08);
  assert.strictEqual(node20.status, 1);
  assert.ok(node20.out.problems.some((p) => p.reference === 'MANIFEST_INVALID_RULE' && /nodejs20\.x runtime is deprecated\. Migrate/.test(p.message)), JSON.stringify(node20.out.problems));
  assert.ok(node20.out.problems.some((p) => p.sev === 'warning' && /nodejs20\.x/.test(p.message)), 'the pinned deprecated-runtimes flag gives the client warning too');
});

test('defect A: the host flag layer lets clicks through to the app', () => {
  const src = fs.readFileSync(path.join(KIT, 'lib', 'bridge-page.cjs'), 'utf8');
  assert.match(src, /:host\{position:fixed;[^}]*pointer-events:none/);
});
