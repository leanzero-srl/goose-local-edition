'use strict';
// Forge 2.0 hardening, the kit's two platform behaviours (contract S1, S2), each against the real thing (Atlassian's
// pinned runtime wrapper, the sandbox fence, the forge2 site) plus the pure schedule and KVS index underneath:
//   S1  consumer deliveries that carry the same event or name the same issue run at the same time under the proxy's
//       READS-FIRST schedule: a check-then-act consumer double-posts, a FAIL_IF_EXISTS claim posts once; the pair is
//       visible in the delivery records, the invocation results, emu.concurrency and the KVS call log; unrelated
//       deliveries keep one at a time, a concurrency key exempts no pair; the interleaving is the same on every run;
//       the site's plan marks the batches the drives leave queued together (deliverNext's batchEnd).
//   S2  kvs.query / entity queries miss a write younger than 5 virtual s (get, transaction conditions and FAIL_IF_EXISTS
//       see it at once); after 5 virtual s the query sees it.
// Run: node --test evals/swarm-bench/forge2/kit/test/concurrency.test.cjs
const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');
const { ensureKit, scratch } = require('./helpers.cjs');

const KIT = path.resolve(__dirname, '..');
const SITE_DIR = path.resolve(__dirname, '..', '..', 'site');
const SITE = path.join(SITE_DIR, 'site.cjs');
const SEED = '0123456789abcdef';
const APP_ID = 'ari:cloud:ecosystem::app/00000000-0000-0000-0000-000000000000';
const { createKvs, QUERY_LAG_MS } = require(path.join(KIT, 'lib', 'kvs.cjs'));
const { createConcurrentGroup } = require(path.join(KIT, 'lib', 'proxy.cjs'));

// ── S2: the query index, pure ────────────────────────────────────────────────────────────────────────────────────

const ENTITIES = [{ name: 'note', attributes: { issueId: { type: 'string' }, at: { type: 'float' }, n: { type: 'integer' } },
  indexes: [{ name: 'by-issue', partition: ['issueId'], range: ['at'] }] }];
const T0 = Date.parse('2026-10-30T09:00:00Z');
function store({ floor } = {}) {
  const kvs = createKvs({ entities: ENTITIES, now: () => T0, ...(floor ? { floor } : {}) });
  const h = (op, body, t) => kvs.handle(op, body, t === undefined ? {} : { t });
  const keys = (t, prefix = 'job:') => h('/api/v1/query', { where: [{ property: 'key', condition: 'BEGINS_WITH', values: [prefix] }] }, t).body.data;
  const notes = (t, issueId = 'I1') => h('/api/v1/entity/query', { entityName: 'note', indexName: 'by-issue', partition: [issueId] }, t).body.data;
  return { kvs, h, keys, notes };
}

test('S2: a kvs.query misses a write younger than 5 virtual s; get sees it at once; at 5 s the query sees it', () => {
  assert.strictEqual(QUERY_LAG_MS, 5000);
  const { h, keys } = store();
  assert.strictEqual(h('/api/v1/set', { key: 'job:1', value: 'v1' }, T0).status, 204);
  assert.deepStrictEqual(keys(T0 + 200), []);
  assert.strictEqual(h('/api/v1/get', { key: 'job:1' }, T0 + 200).body.value, 'v1');
  assert.deepStrictEqual(keys(T0 + QUERY_LAG_MS - 1), []);
  assert.deepStrictEqual(keys(T0 + QUERY_LAG_MS), [{ key: 'job:1', value: 'v1' }]);
});

test('S2: inside the lag a query sees the value before the write, a deleted key still listed, an entity row missing', () => {
  const { h, keys, notes } = store();
  h('/api/v1/set', { key: 'job:a', value: 'old' }, T0);
  h('/api/v1/set', { key: 'job:a', value: 'new' }, T0 + 10_000);
  h('/api/v1/set', { key: 'job:b', value: 'gone' }, T0);
  h('/api/v1/delete', { key: 'job:b' }, T0 + 10_000);
  assert.deepStrictEqual(keys(T0 + 10_100), [{ key: 'job:a', value: 'old' }, { key: 'job:b', value: 'gone' }]);
  assert.strictEqual(h('/api/v1/get', { key: 'job:a' }, T0 + 10_100).body.value, 'new');
  assert.strictEqual(h('/api/v1/get', { key: 'job:b' }, T0 + 10_100).status, 404);
  assert.deepStrictEqual(keys(T0 + 15_000), [{ key: 'job:a', value: 'new' }]);
  h('/api/v1/entity/set', { entityName: 'note', key: 'n1', value: { issueId: 'I1', at: 1, n: 1 } }, T0);
  assert.deepStrictEqual(notes(T0 + 120), []);
  assert.strictEqual(h('/api/v1/entity/get', { entityName: 'note', key: 'n1' }, T0 + 120).body.value.n, 1);
  assert.deepStrictEqual(notes(T0 + 5000).map((r) => r.key), ['n1']);
  // An update that moves the row to another partition: the index keeps the old partition until the lag passes.
  h('/api/v1/entity/set', { entityName: 'note', key: 'n1', value: { issueId: 'I2', at: 1, n: 2 } }, T0 + 6000);
  assert.deepStrictEqual([notes(T0 + 6100).map((r) => r.value.n), notes(T0 + 6100, 'I2')], [[1], []]);
  assert.deepStrictEqual([notes(T0 + 11_000), notes(T0 + 11_000, 'I2').map((r) => r.value.n)], [[], [2]]);
});

test('S2: FAIL_IF_EXISTS and transaction conditions read the current state while the query has not caught up', () => {
  const { h, keys } = store();
  h('/api/v1/set', { key: 'job:claim', value: 1 }, T0);
  assert.deepStrictEqual(keys(T0 + 200), []);
  assert.strictEqual(h('/api/v1/set', { key: 'job:claim', value: 2, options: { keyPolicy: 'FAIL_IF_EXISTS' } }, T0 + 200).body.code, 'KEY_CONFLICT');
  h('/api/v1/entity/set', { entityName: 'note', key: 'n', value: { issueId: 'I1', at: 1, n: 1 } }, T0);
  const bump = (from, t) => h('/api/v1/transaction', { set: [{ key: 'n', value: { issueId: 'I1', at: 1, n: from + 1 }, entityName: 'note',
    conditions: { and: [{ property: 'n', condition: 'EQUAL_TO', values: [from] }] } }] }, t);
  assert.strictEqual(bump(1, T0 + 200).status, 204);
  assert.strictEqual(bump(1, T0 + 400).body.code, 'CONDITIONAL_CHECK_FAILED');
});

test('S2: a write the harness makes (no request time) is settled at once; a write dated ahead of a query is not seen', () => {
  const { h, keys } = store();
  h('/api/v1/set', { key: 'job:preload', value: 'v1' });
  assert.deepStrictEqual(keys(T0), [{ key: 'job:preload', value: 'v1' }]);
  // Two concurrent invocations on their own clocks: the partner's write at +60 s is in the future of a query at +2 s.
  h('/api/v1/set', { key: 'job:later', value: 'x' }, T0 + 60_000);
  assert.deepStrictEqual(keys(T0 + 2000).map((r) => r.key), ['job:preload']);
  assert.deepStrictEqual(keys(T0 + 65_000).map((r) => r.key), ['job:later', 'job:preload']);
});

test('S2: dump/load carries the writes in flight (forge-dev, process to process); the floor drops what nothing can see', () => {
  let floor = T0;
  const a = store({ floor: () => floor });
  a.h('/api/v1/set', { key: 'job:1', value: 'v1' }, T0);
  const b = store({ floor: () => floor });
  b.kvs.load(JSON.parse(JSON.stringify(a.kvs.dump())));
  assert.deepStrictEqual(b.keys(T0 + 1000), []);
  assert.deepStrictEqual(b.keys(T0 + 5000).map((r) => r.key), ['job:1']);
  assert.strictEqual(b.kvs.dump().index.kv.length, 1);
  floor = T0 + 5000;
  b.keys(T0 + 5000);
  assert.deepStrictEqual(b.kvs.dump().index, { kv: [], ents: [] });
  // A dump made without the index (an older kit's state file, the v1 preload) has nothing in flight.
  const c = store();
  c.kvs.load({ kv: [['job:2', { value: 'v', createdAt: T0, updatedAt: T0, expireAt: null }]], secrets: [], ents: [] });
  assert.deepStrictEqual(c.keys(T0).map((r) => r.key), ['job:2']);
});

// ── S1: the READS-FIRST schedule, pure ───────────────────────────────────────────────────────────────────────────

// Each member: join, then requests through the group's turns against one shared Map, then end.
async function runGroup(programs) {
  const g = createConcurrentGroup({ id: 'c1', reason: 'same-event', members: programs.map((_, i) => ({ eventId: `e${i}` })) });
  const store = new Map();
  const served = [];
  const run = async (program, member) => {
    g.join(member, `inv-${member}`);
    await g.ready();
    const op = async (tv, write, fn) => {
      const turn = await g.turn(member, { tv, write });
      served.push(`${member}:${write ? 'w' : 'r'}@${tv}${turn.held ? ' held' : ''}`);
      const out = fn(store);
      turn.done();
      return out;
    };
    try { return await program(op); } finally { g.end(member); }
  };
  const results = await Promise.all(programs.map(run));
  return { g, store, served, results };
}
const checkThenAct = async (op) => {
  if (await op(1000, false, (s) => s.get('k'))) return 'skipped';
  await op(1120, true, (s) => s.set('k', true));
  return 'posted';
};
const claim = async (op) => (await op(1000, true, (s) => (s.has('k') ? false : (s.set('k', true), true))) ? 'posted' : 'skipped');

test('S1 schedule: two check-then-act members both read before either write lands; a conditional claim wins once', async () => {
  const naive = await runGroup([checkThenAct, checkThenAct]);
  assert.deepStrictEqual(naive.results, ['posted', 'posted']);
  assert.deepStrictEqual(naive.served, ['0:r@1000', '1:r@1000', '0:w@1120 held', '1:w@1120']);
  assert.deepStrictEqual(naive.g.firstWrites.map((w) => [w.index, w.held]), [[0, true], [1, false]]);
  // Both claims arrive before either is served: nothing is reordered, so nothing is marked held.
  const claims = await runGroup([claim, claim]);
  assert.deepStrictEqual(claims.results, ['posted', 'skipped']);
  assert.deepStrictEqual(claims.served, ['0:w@1000', '1:w@1000']);
  const three = await runGroup([checkThenAct, checkThenAct, checkThenAct]);
  assert.deepStrictEqual(three.results, ['posted', 'posted', 'posted']);
  assert.deepStrictEqual(three.served.slice(0, 3), ['0:r@1000', '1:r@1000', '2:r@1000']);
});

test('S1 schedule: a member that ends without writing releases the held write; earlier virtual time is served first', async () => {
  const readOnly = async (op) => { await op(1000, false, (s) => s.get('k')); await op(5000, false, (s) => s.get('k')); return 'read'; };
  const r = await runGroup([checkThenAct, readOnly]);
  assert.deepStrictEqual(r.results, ['posted', 'read']);
  assert.deepStrictEqual(r.served, ['0:r@1000', '1:r@1000', '1:r@5000', '0:w@1120 held']);
  // Member 1's clock is behind member 0's: its requests go first although member 0 was delivered first.
  const late = async (op) => { await op(2000, false, () => null); return 'late'; };
  const early = async (op) => { await op(1500, false, () => null); return 'early'; };
  assert.deepStrictEqual((await runGroup([late, early])).served, ['1:r@1500', '0:r@2000']);
});

test('S1 schedule: a request still waiting when its member ends (a killed process) is answered dropped, never served', async () => {
  const g = createConcurrentGroup({ id: 'c9', reason: 'same-issue', members: [{ eventId: 'a' }, { eventId: 'b' }] });
  g.join(0, 'inv-a');
  g.join(1, 'inv-b');
  const held = g.turn(0, { tv: 1000, write: true });
  g.end(0);
  assert.deepStrictEqual(Object.keys(await held).sort(), ['done', 'dropped', 'held']);
  assert.strictEqual((await held).dropped, true);
  const t = await g.turn(1, { tv: 1000, write: true });
  assert.strictEqual(t.held, false);
  t.done();
});

// ── the real thing: the pinned wrapper, the sandbox fence, the forge2 site ──────────────────────────────────────

const SOURCE = `import api, { route } from '@forge/api';
import { kvs, WhereConditions } from '@forge/kvs';
import { Queue } from '@forge/events';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
// A comment honouring Retry-After (a second write to the issue within 2 virtual s meets jira-per-issue-on-write).
async function comment(issueId, text) {
  for (let i = 0; i < 5; i++) {
    const r = await api.asApp().requestJira(route\`/rest/api/3/issue/\${issueId}/comment\`, { method: 'POST', headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ body: { type: 'doc', version: 1, content: [{ type: 'paragraph', content: [{ type: 'text', text }] }] } }) });
    if (r.status === 429) { await sleep(Number(r.headers.get('retry-after') ?? 1) * 1000); continue; }
    if (r.status >= 300) throw new Error('comment ' + r.status + ' ' + (await r.text()));
    return;
  }
  throw new Error('comment kept meeting 429');
}
// The test sets {queue, concurrency?} under 'mode'; the trigger hands every update to that queue.
export const trig = async (event) => {
  const mode = await kvs.get('mode');
  await new Queue({ key: mode.queue }).push({ body: { issueId: String(event.issue.id), changelogId: String(event.changelog.id) }, ...(mode.concurrency ? { concurrency: mode.concurrency } : {}) });
  return { pushed: mode.queue };
};
export const naive = async (event) => {
  const { issueId, changelogId } = event.body;
  if (await kvs.get('posted:' + changelogId)) return { skipped: true };
  await comment(issueId, 'naive ' + changelogId);
  await kvs.set('posted:' + changelogId, true);
  return { posted: true };
};
export const claim = async (event) => {
  const { issueId, changelogId } = event.body;
  try { await kvs.set('claimed:' + changelogId, true, { keyPolicy: 'FAIL_IF_EXISTS' }); } catch (e) {
    return { skipped: true, code: e.code ?? null, status: e.responseDetails?.status ?? null };
  }
  await comment(issueId, 'claim ' + changelogId);
  return { posted: true };
};
export const sched = async () => {
  const t0 = Date.now();
  const at = () => Date.now() - t0;
  const keys = async () => (await kvs.query().where('key', WhereConditions.beginsWith('s2:')).getMany()).results.map((r) => r.key);
  const notes = async () => (await kvs.entity('note').query().index('by-issue', { partition: ['I1'] }).getMany()).results.map((r) => r.key);
  await kvs.set('s2:key', 'fresh');
  await kvs.entity('note').set('n1', { issueId: 'I1', at: 1 });
  const first = { keys: await keys(), notes: await notes(), get: await kvs.get('s2:key'), entityGet: (await kvs.entity('note').get('n1'))?.issueId ?? null };
  first.at = at();
  await sleep(5000);
  const later = { keys: await keys(), notes: await notes() };
  later.at = at();
  return { first, later };
};
`;

function writeApp(dir) {
  fs.mkdirSync(path.join(dir, 'src'), { recursive: true });
  fs.writeFileSync(path.join(dir, 'src', 'index.js'), SOURCE);
  fs.writeFileSync(path.join(dir, 'manifest.yml'), `modules:
  trigger:
    - key: cc-trigger
      function: cc-trig
      events:
        - avi:jira:updated:issue
  consumer:
    - key: cc-naive
      queue: cc-naive
      function: cc-naive
    - key: cc-claim
      queue: cc-claim
      function: cc-claim
  scheduledTrigger:
    - key: cc-hourly
      function: cc-sched
      interval: hour
  function:
    - key: cc-trig
      handler: index.trig
    - key: cc-naive
      handler: index.naive
    - key: cc-claim
      handler: index.claim
    - key: cc-sched
      handler: index.sched
permissions:
  scopes:
    - read:jira-work
    - write:jira-work
    - storage:app
app:
  id: ${APP_ID}
  runtime:
    name: nodejs22.x
  storage:
    entities:
      - name: note
        attributes:
          issueId:
            type: string
          at:
            type: float
        indexes:
          - name: by-issue
            partition:
              - issueId
            range:
              - at
`);
}

async function freshWorld(name) {
  ensureKit();
  const { createSite } = require(SITE);
  const { createEmulator } = require(path.join(KIT, 'lib', 'emulator.cjs'));
  const appDir = path.join(scratch(name), 'app');
  writeApp(appDir);
  const site = await createSite({ seed: SEED });
  const invocations = [];
  const emu = await createEmulator({ appDir, site, runtime: 'wrapper', onInvocation: (_rec, out) => invocations.push(out) });
  const built = await emu.build();
  assert.ok(built.functions.every((f) => f.loaded), JSON.stringify(built.functions));
  return { site, emu, invocations, close: async () => { await emu.close(); await site.stop(); } };
}
let shared = null;
const world = async () => (shared ??= await freshWorld('concurrency'));
test.after(async () => { if (shared) await shared.close(); });

// Live changes of the plan, by issue: the scenarios use disjoint issues so their comments never mix.
const plan = (site) => {
  const byIssue = new Map();
  for (const c of site.pack.live.filter((x) => x.delivery.slot !== null)) {
    if (!byIssue.has(c.issueId)) byIssue.set(c.issueId, []);
    byIssue.get(c.issueId).push(c.changelogId);
  }
  return [...byIssue];
};
const setMode = (emu, mode) => assert.strictEqual(emu.kvs.handle('/api/v1/set', { key: 'mode', value: mode }).status, 204);
const commentsOn = (site, issueId, prefix) => site.comments.filter((c) => c.issueId === issueId && JSON.stringify(c.body).includes(prefix));

// A trigger-level duplicate: the same product event delivered twice before the queues drain.
async function duplicatePair(w, issueId, changelogId, queue) {
  setMode(w.emu, { queue });
  await w.emu.deliverProductEvent(changelogId);
  await w.emu.deliverProductEvent(changelogId);
  return w.emu.drainQueues();
}

test('S1: a check-then-act consumer double-posts when a duplicate delivery runs beside it; the pair is in every log', { timeout: 180_000 }, async () => {
  const w = await world();
  const [issueId, [changelogId]] = plan(w.site)[0];
  const l0 = w.emu.log.length;
  const ds = await duplicatePair(w, issueId, changelogId, 'cc-naive');
  assert.deepStrictEqual(ds.map((d) => [d.queueName, d.outcome, d.result]), [['cc-naive', 'ok', { posted: true }], ['cc-naive', 'ok', { posted: true }]]);
  assert.strictEqual(commentsOn(w.site, issueId, `naive ${changelogId}`).length, 2);
  // The delivery records: one group, each naming the other.
  const [a, b] = ds;
  assert.deepStrictEqual([a.concurrent.group, a.concurrent.reason, a.concurrent.schedule, a.concurrent.index, b.concurrent.index], [b.concurrent.group, 'same-event', 'reads-first', 0, 1]);
  assert.deepStrictEqual(a.concurrent.with, [{ index: 1, eventId: b.eventId, invocationId: b.invocationId }]);
  assert.deepStrictEqual(b.concurrent.with, [{ index: 0, eventId: a.eventId, invocationId: a.invocationId }]);
  assert.strictEqual(a.t, b.t, 'started together');
  // The invocation results (onInvocation), the invocation records and emu.concurrency say the same.
  for (const d of ds) {
    assert.deepStrictEqual(w.invocations.find((o) => o.invocationId === d.invocationId).concurrent, d.concurrent);
    assert.deepStrictEqual(w.emu.invocations.get(d.invocationId).concurrent, d.concurrent);
  }
  const g = w.emu.concurrency.at(-1);
  assert.deepStrictEqual([g.group, g.reason, g.members.map((m) => [m.invocationId, m.eventId, m.outcome])],
    [a.concurrent.group, 'same-event', [[a.invocationId, a.eventId, 'ok'], [b.invocationId, b.eventId, 'ok']]]);
  // READS-FIRST: both gets before either set; the first member's set waited for the second's.
  const kvsCalls = w.emu.log.slice(l0).filter((e) => e.service === 'kvs' && /^posted:/.test(e.body?.key ?? ''));
  assert.deepStrictEqual(kvsCalls.map((e) => [e.path, e.invocationId === a.invocationId ? 0 : 1, Boolean(e.heldReadsFirst), e.concurrentGroup]),
    [['/api/v1/get', 0, false, g.group], ['/api/v1/get', 1, false, g.group], ['/api/v1/set', 0, true, g.group], ['/api/v1/set', 1, false, g.group]]);
  assert.deepStrictEqual(g.firstWrites.map((x) => [x.index, x.held]), [[0, true], [1, false]]);
});

test('S1: a FAIL_IF_EXISTS claim posts exactly once under the same duplicate pair (409 KEY_CONFLICT for the second)', { timeout: 180_000 }, async () => {
  const w = await world();
  const [issueId, [changelogId]] = plan(w.site)[1];
  const ds = await duplicatePair(w, issueId, changelogId, 'cc-claim');
  assert.deepStrictEqual(ds.map((d) => [d.outcome, d.concurrent?.index]), [['ok', 0], ['ok', 1]]);
  assert.deepStrictEqual(ds.map((d) => d.result), [{ posted: true }, { skipped: true, code: 'KEY_CONFLICT', status: 409 }]);
  assert.strictEqual(commentsOn(w.site, issueId, `claim ${changelogId}`).length, 1);
});

test('S1: two changes of one issue pair as same-issue; deliveries of different issues stay one at a time; a concurrency key of 1 exempts no pair', { timeout: 180_000 }, async () => {
  const w = await world();
  const issues = plan(w.site);
  const [, two] = issues.slice(2).find(([, ids]) => ids.length >= 2);
  setMode(w.emu, { queue: 'cc-claim' });
  for (const id of two.slice(0, 2)) await w.emu.deliverProductEvent(id);
  const same = await w.emu.drainQueues();
  assert.deepStrictEqual(same.map((d) => [d.concurrent?.reason, d.concurrent?.index, d.result.posted]), [['same-issue', 0, true], ['same-issue', 1, true]]);
  // Different issues: no group, the second starts when the first has ended.
  const [x, y] = issues.slice(10, 12).map(([, ids]) => ids[0]);
  await w.emu.deliverProductEvent(x);
  await w.emu.deliverProductEvent(y);
  const apart = await w.emu.drainQueues();
  assert.deepStrictEqual(apart.map((d) => d.concurrent ?? null), [null, null]);
  assert.ok(Date.parse(apart[1].t) >= Date.parse(apart[0].t1), JSON.stringify(apart.map((d) => [d.t, d.t1])));
  // A duplicate pair whose pushes share a concurrency key of limit 1 still starts together (SPEC §2.8: the key is not
  // applied, contract §8), so the check-then-act consumer posts twice; only a conditional claim makes it exactly-once.
  const [dupIssue, [dupChange]] = issues[12];
  setMode(w.emu, { queue: 'cc-naive', concurrency: { key: 'cc', limit: 1 } });
  await w.emu.deliverProductEvent(dupChange);
  await w.emu.deliverProductEvent(dupChange);
  const keyed = await w.emu.drainQueues();
  assert.deepStrictEqual(keyed.map((d) => [d.concurrent?.reason, d.result]), [['same-event', { posted: true }], ['same-event', { posted: true }]]);
  assert.strictEqual(commentsOn(w.site, dupIssue, `naive ${dupChange}`).length, 2);
});

test('the site plan batches a duplicate with its original and a permuted same-issue pair; deliverNext carries batchEnd', () => {
  const { facts } = require(path.join(SITE_DIR, 'fixtures.cjs'));
  const { deliveryPlan, deliveryBatches } = require(path.join(SITE_DIR, 'state.cjs'));
  const pack = facts(SEED, { scoring: true });
  const plan = deliveryPlan(pack);
  const liveById = new Map(pack.live.map((c) => [c.changelogId, c]));
  const end = deliveryBatches(plan, liveById);
  assert.strictEqual(end.length, plan.length);
  const starts = end.map((e, i) => (e !== null && (i === 0 || end[i - 1] !== e) ? i : null)).filter((i) => i !== null);
  for (const s of starts) for (let k = s; k <= end[s]; k++) assert.strictEqual(end[k], end[s], `slot ${k} belongs to the batch ending at ${end[s]}`);
  // Every duplicate sits in the batch of its original.
  const original = new Map();
  plan.forEach((d, i) => { if (!d.duplicate) original.set(d.changelogId, i); });
  for (const [i, d] of plan.entries()) {
    if (!d.duplicate) continue;
    const o = original.get(d.changelogId);
    assert.ok(end[o] !== null && end[o] === end[i], `duplicate at ${i} batched with its original at ${o}`);
  }
  // Every permuted pair (consecutive non-duplicate deliveries out of creation order, as score_forge2's t_out_of_order
  // reads them) is one batch, and names one issue.
  const firsts = plan.map((d, i) => [d, i]).filter(([d]) => !d.duplicate);
  let permuted = 0;
  for (const [[a, i], [b, j]] of firsts.slice(1).map((x, k) => [firsts[k], x])) {
    const ca = liveById.get(a.changelogId);
    const cb = liveById.get(b.changelogId);
    if (Date.parse(ca.created) <= Date.parse(cb.created)) continue;
    permuted += 1;
    assert.strictEqual(ca.issueId, cb.issueId);
    assert.ok(end[i] !== null && end[i] === end[j], `permuted pair ${i},${j} batched`);
  }
  assert.ok(permuted >= 4 && starts.length >= permuted, `scoring plan: ${permuted} permuted pairs, ${starts.length} batches`);
  // Outside a batch nothing is held.
  assert.ok(end.filter((e) => e === null).length > plan.length / 2);
  // The site's delivery carries it: the first batch's opening delivery names the batch's last slot.
  const { createState } = require(path.join(SITE_DIR, 'state.cjs'));
  const state = createState(pack);
  let d = null;
  for (let k = 0; k <= starts[0]; k++) d = state.nextDelivery();
  assert.deepStrictEqual([d.slot, d.batchEnd], [starts[0], end[starts[0]]]);
});

test('S1: the interleaving is the same on every run (two fresh worlds, one seed)', { timeout: 240_000 }, async () => {
  const runOnce = async (name) => {
    const w = await freshWorld(name);
    try {
      const [issueId, [changelogId]] = plan(w.site)[0];
      const l0 = w.emu.log.length;
      const ds = await duplicatePair(w, issueId, changelogId, 'cc-naive');
      const member = new Map(ds.map((d) => [d.invocationId, d.concurrent.index]));
      return w.emu.log.slice(l0).filter((e) => member.has(e.invocationId))
        .map((e) => [member.get(e.invocationId), e.service, e.method, String(e.path).split('?')[0], e.t_virtual, e.status, Boolean(e.heldReadsFirst)]);
    } finally { await w.close(); }
  };
  const first = await runOnce('determinism-a');
  assert.ok(first.length >= 6, JSON.stringify(first));
  assert.deepStrictEqual(await runOnce('determinism-b'), first);
});

test('S2: inside an invocation, a query right after a set misses it and a get sees it; 5 virtual s later the query sees it', { timeout: 120_000 }, async () => {
  const w = await world();
  const r = await w.emu.invoke('cc-sched', { moduleKey: 'cc-hourly', event: {} });
  assert.ok(r.ok, JSON.stringify(r.error));
  // set 200 + entity set 200, then query (sent at 400) / entity query (520) / get / entity get, 120 each; after the
  // 5 s wait the query is sent at 5,880 and the entity query at 6,000: both writes are 5 s old by then.
  assert.deepStrictEqual(r.result, {
    first: { keys: [], notes: [], get: 'fresh', entityGet: 'I1', at: 880 },
    later: { keys: ['s2:key'], notes: ['n1'], at: 6120 },
  });
});
