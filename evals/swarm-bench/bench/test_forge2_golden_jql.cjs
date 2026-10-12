'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { createKvs } = require('../forge2/kit/lib/kvs.cjs');
const { consumerBatch } = require('../forge2/kit/lib/emulator.cjs');
const { facts } = require('../forge2/site/fixtures.cjs');
const { createState } = require('../forge2/site/state.cjs');
const native = require('../forge2/site/rest/jql-functions.cjs');
const { costOf } = require('../forge2/site/rate.cjs');
const { compile } = require('../forge2/site/jql.cjs');
const { jqlModel } = require('../forge2/site/rest/platform.cjs');
const COMPUTATION = '/rest/api/3/jql/function/computation';
const FUNCTION_KEY = 'ari:cloud:ecosystem::extension/00000000-0000-0000-0000-000000000000/env/static/scope-changed';

function fixture() {
  const pack = facts('cf19718db1ebca41');
  const state = createState(pack);
  const storage = createKvs({ now: state.now, floor: state.now, entities: [{ name: 'scope-ledger',
    attributes: { sprintId: { type: 'string' }, issueId: { type: 'string' }, deleted: { type: 'boolean' } },
    indexes: [{ name: 'by-sprint', partition: ['sprintId'], range: ['issueId'] }] }] });
  const storageCalls = [];
  const wire = async (op, body) => {
    storageCalls.push({ op, body });
    const response = storage.handle(op, body, { t: state.now() });
    state.advance(op.endsWith('/get') || op.endsWith('/query') ? 120 : 200);
    if (response.status === 404 && op.endsWith('/get')) return undefined;
    if (response.status >= 400) throw Object.assign(new Error(response.body.message), { code: response.body.code });
    return response.body;
  };
  const kvs = { get: async (key) => (await wire('/api/v1/get', { key }))?.value,
    set: (key, value, options) => wire('/api/v1/set', { key, value, options }),
    delete: (key) => wire('/api/v1/delete', { key }),
    query: () => {
      const body = {};
      const query = { where(property, condition) { body.where = [{ property, ...condition }]; return this; },
        limit(n) { body.limit = n; return this; }, cursor(c) { body.after = c; return this; },
        async getMany() { const result = await wire('/api/v1/query', body); return { results: result.data, nextCursor: result.cursor }; } };
      return query;
    } };
  const queue = [];
  class Queue {
    constructor({ key }) { this.key = key; }
    async push(item) { queue.push({ ...item, seq: queue.length, queueName: this.key, readyAt: state.now() + item.delayInSeconds * 1000 }); state.advance(200); }
  }
  const jiraCalls = [];
  let injectedError = null;
  let injectedResult;
  const jiraJson = async (as, url, init) => {
    if (injectedError) { const error = injectedError; injectedError = null; throw error; }
    const parsed = new URL(String(url), 'https://jira.invalid');
    const body = init?.body ? JSON.parse(init.body) : undefined;
    const method = init?.method ?? 'GET';
    if (injectedResult !== undefined) { const result = injectedResult; injectedResult = undefined; return result; }
    const response = native.handlers[`${method} ${parsed.pathname}`]({ state, caller: { as, invocationId: 'reference-jql-refresh' },
      req: { query: parsed.searchParams, body } });
    jiraCalls.push({ method, path: parsed.pathname, body, response, points: costOf(`${method} ${parsed.pathname}`, body, response.body) });
    state.advance(200);
    if (response.status >= 400) throw new Error(JSON.stringify(response.body));
    return response.body;
  };
  const changesOfSprint = async (sprintId) => {
    const result = await wire('/api/v1/entity/query', { entityName: 'scope-ledger', indexName: 'by-sprint', partition: [String(sprintId)], limit: 100 });
    return result.data.map((r) => r.value);
  };
  const getSprint = async (sprintId) => {
    const s = state.st.sprints.get(Number(sprintId));
    return s ? { ...s, started: s.state !== 'future' && Boolean(s.startDate) } : null;
  };
  const source = fs.readFileSync(path.join(__dirname, 'golden-forge2/src/jql.js'), 'utf8')
    .replace(/^import[^\n]+\n/gm, '').replace(/\bexport (?=(?:async |function |const ))/g, '');
  const context = { kvs, Queue, changesOfSprint, getSprint, jiraJson, route: String.raw,
    postJson: (body) => ({ method: 'POST', body: JSON.stringify(body) }), QUERY_LAG_MS: 5000,
    WhereConditions: { beginsWith: (value) => ({ condition: 'BEGINS_WITH', values: [value] }) },
    Map, Set, console, module: { exports: {} } };
  vm.runInNewContext(`${source}\nmodule.exports = { scopeChanged, refreshScopePrecomputations, queueScopeRefresh };`, context);
  const scope = context.module.exports;
  const add = async (sprintId, issueId, deleted = false) => wire('/api/v1/entity/set', {
    entityName: 'scope-ledger', key: `${sprintId}:${issueId}`, value: { sprintId: String(sprintId), issueId: String(issueId), deleted } });
  const begin = (sprintId, operator = 'in') => native.begin(state, { field: 'key', operator,
    functionName: 'scopeChanged', arguments: [String(sprintId)] }, { functionKey: FUNCTION_KEY });
  const warm = async (sprintId, operator = 'in') => {
    const pre = begin(sprintId, operator);
    const result = await scope.scopeChanged(pre.request);
    assert.equal(result.error, undefined, JSON.stringify(result));
    native.finish(state, pre.id, result);
    return pre.id;
  };
  const evaluate = (fragment, user) => {
    const predicate = compile(fragment, jqlModel({ state, t: state.now(), caller: { accountId: user } }));
    return state.allIssues().filter((issue) => state.canBrowse(user, issue) && predicate.matches(issue)).map((issue) => String(issue.id)).sort();
  };
  return { pack, state, storage, storageCalls, kvs, queue, jiraCalls, add, begin, warm, evaluate, ...scope,
    advance: state.advance, failNext(error) { injectedError = error; }, nextResult(result) { injectedResult = result; } };
}

function activeSprint(f, n = 0) { return String(f.pack.sprints.filter((s) => s.state === 'active')[n].id); }
function nativeValue(f, id) { return native.cached(f.state, id).value; }
function registryRows(f) { return Object.entries(f.storage.snapshot().kvs).filter(([key]) => key.startsWith('scope-jql:')); }

test('native IN and NOT IN update together without site-wide GET and remain shared across viewers', async () => {
  const f = fixture();
  const sprint = activeSprint(f);
  const visible = f.pack.issues.find((i) => !i.hiddenFrom.includes(f.pack.viewer));
  const hidden = f.pack.issues.find((i) => i.hiddenFrom.includes(f.pack.viewer));
  const initial = f.pack.issues.find((i) => i.id !== hidden.id && i.id !== visible.id);
  await f.add(sprint, initial.id); f.advance(5000);
  const ids = await Promise.all(['in', 'not in'].map((operator) => f.warm(sprint, operator)));
  await f.add(sprint, visible.id); await f.add(sprint, hidden.id); f.advance(5000);
  await f.refreshScopePrecomputations([sprint, sprint], {});
  assert.equal(f.jiraCalls.length, 1);
  assert.equal(f.jiraCalls[0].method, 'POST');
  assert.equal(f.jiraCalls[0].body.values.length, 2);
  assert.equal(f.jiraCalls[0].points, 1);
  const inValue = nativeValue(f, ids[0]);
  assert.ok(inValue.includes(hidden.id));
  assert.ok(f.evaluate(inValue, f.pack.appAccountId).includes(hidden.id));
  assert.ok(!f.evaluate(inValue, f.pack.viewer).includes(hidden.id));
  assert.ok(f.evaluate(inValue, f.pack.viewer).includes(visible.id));
  const outValue = nativeValue(f, ids[1]);
  assert.ok(!f.evaluate(outValue, f.pack.appAccountId).includes(hidden.id));
  assert.ok(!f.evaluate(outValue, f.pack.appAccountId).includes(visible.id));
  assert.equal(f.begin(sprint).cached, true);
});

test('a repeated refresh strongly reads its recent successful registry value despite index lag', async () => {
  const f = fixture(); const sprint = activeSprint(f);
  const id = await f.warm(sprint); f.advance(5000);
  await f.add(sprint, f.pack.issues[0].id); f.advance(5000);
  await f.refreshScopePrecomputations([sprint], {});
  const after = nativeValue(f, id);
  await f.refreshScopePrecomputations([sprint], {});
  assert.equal(f.jiraCalls.length, 1);
  assert.equal(nativeValue(f, id), after);
  assert.ok(f.storageCalls.filter((c) => c.op === '/api/v1/get').length >= 2);
});

test('initial eventual-ledger race schedules a refresh that fixes the cached native expansion', async () => {
  const f = fixture(); const sprint = activeSprint(f); const issue = f.pack.issues[0];
  await f.add(sprint, issue.id);
  const id = await f.warm(sprint);
  assert.equal(nativeValue(f, id), '(id = 1 AND id != 1)');
  assert.equal(f.queue.length, 1);
  assert.equal(f.queue[0].delayInSeconds, 5);
  f.advance(5000);
  await f.refreshScopePrecomputations(f.queue[0].body.sprintIds, {});
  assert.equal(nativeValue(f, id), `id in (${issue.id})`);
});

test('independent clauses register concurrently without dropping either ID', async () => {
  const f = fixture(); const sprint = activeSprint(f);
  const ids = await Promise.all(['in', 'not in'].map((operator) => f.warm(sprint, operator)));
  assert.equal(registryRows(f).length, 2);
  assert.deepEqual(registryRows(f).map(([, value]) => value.id).sort(), ids.sort());
  assert.ok(f.storageCalls.filter((c) => c.op === '/api/v1/set').every((c) => c.body.options.keyPolicy === 'FAIL_IF_EXISTS'));
});

test('duplicate native evaluations claim one mapping and refresh after a stale first read', async () => {
  const f = fixture(); const sprint = activeSprint(f); const pre = f.begin(sprint);
  const results = await Promise.all([f.scopeChanged(pre.request), f.scopeChanged(pre.request)]);
  assert.ok(results.every((r) => r.error === undefined));
  assert.equal(registryRows(f).length, 1);
  assert.equal(registryRows(f)[0][1].id, pre.id);
  native.finish(f.state, pre.id, results[0]);
  await f.add(sprint, f.pack.issues[0].id); f.advance(5000);
  await f.refreshScopePrecomputations([sprint], {});
  assert.equal(nativeValue(f, pre.id), `id in (${f.pack.issues[0].id})`);
});

test('only touched sprint mappings update and deletion removes IDs from both cached operators', async () => {
  const f = fixture(); const a = activeSprint(f); const b = activeSprint(f, 1); const issue = f.pack.issues[0];
  await f.add(a, issue.id); await f.add(b, issue.id); f.advance(5000);
  const inId = await f.warm(a); const outId = await f.warm(a, 'not in'); const other = await f.warm(b); f.advance(5000);
  await f.add(a, issue.id, true); f.advance(5000);
  await f.refreshScopePrecomputations([a], {});
  assert.equal(f.jiraCalls.length, 1);
  assert.equal(f.jiraCalls[0].body.values.length, 2);
  assert.equal(nativeValue(f, inId), '(id = 1 AND id != 1)');
  assert.equal(nativeValue(f, outId), '(id = 1 OR id != 1)');
  assert.equal(nativeValue(f, other), `id in (${issue.id})`);
});

test('quota refusal preserves old last-successful value and later retry updates the real cache', async () => {
  const f = fixture(); const sprint = activeSprint(f); const id = await f.warm(sprint); f.advance(5000);
  await f.add(sprint, f.pack.issues[0].id); f.advance(5000);
  f.failNext(Object.assign(new Error('quota'), { retryAfterSeconds: 900, reason: 'background-share' }));
  await assert.rejects(f.refreshScopePrecomputations([sprint], {}), /quota/);
  assert.equal(registryRows(f)[0][1].value, '(id = 1 AND id != 1)');
  assert.equal(nativeValue(f, id), '(id = 1 AND id != 1)');
  await f.refreshScopePrecomputations([sprint], {});
  assert.equal(nativeValue(f, id), `id in (${f.pack.issues[0].id})`);
});

test('native seven-day expiration prunes only IDs the update response says were not found', async () => {
  const f = fixture(); const sprint = activeSprint(f); const old = await f.warm(sprint); f.advance(7 * 24 * 60 * 60 * 1000);
  const fresh = await f.warm(sprint); assert.notEqual(old, fresh); f.advance(5000);
  await f.add(sprint, f.pack.issues[0].id); f.advance(5000);
  await f.refreshScopePrecomputations([sprint], {});
  assert.equal(registryRows(f).length, 1);
  assert.equal(registryRows(f)[0][1].id, fresh);
  assert.equal(nativeValue(f, fresh), `id in (${f.pack.issues[0].id})`);
  assert.deepEqual(f.jiraCalls[0].response.body.notFoundPrecomputationIDs, [old]);
});

test('registry pagination does not silently leave expired mappings on a later page', async () => {
  const f = fixture(); const sprint = activeSprint(f); const id = await f.warm(sprint);
  for (let i = 0; i < 101; i++) await f.kvs.set(`scope-jql:${sprint}:expired-${String(i).padStart(3, '0')}`,
    { id: `expired-${i}`, sprintId: sprint, operator: 'in', value: '(id = 1 AND id != 1)' });
  f.advance(5000); await f.add(sprint, f.pack.issues[0].id); f.advance(5000);
  await f.refreshScopePrecomputations([sprint], {});
  assert.equal(f.jiraCalls.length, 1);
  assert.equal(f.jiraCalls[0].body.values.length, 102);
  assert.equal(registryRows(f).length, 1);
  assert.equal(registryRows(f)[0][1].id, id);
  assert.ok(f.storageCalls.some((c) => c.op === '/api/v1/query' && c.body.after));
});

test('unconfirmed update cannot mark its registry as fresh', async () => {
  const f = fixture(); const sprint = activeSprint(f); await f.warm(sprint); f.advance(5000);
  await f.add(sprint, f.pack.issues[0].id); f.advance(5000); f.nextResult({});
  await assert.rejects(f.refreshScopePrecomputations([sprint], {}), /did not confirm/);
  assert.equal(registryRows(f)[0][1].value, '(id = 1 AND id != 1)');
});

test('refresh concurrency is installation-wide while independent issue consumers can still run', async () => {
  const f = fixture(); await f.queueScopeRefresh([activeSprint(f)]); await f.queueScopeRefresh([activeSprint(f, 1)]);
  f.advance(5000);
  const first = f.queue[0]; const second = { ...f.queue[1], queueName: 'different-queue' };
  const independent = { seq: 2, queueName: 'scope-ledger', readyAt: f.state.now() };
  assert.deepEqual(consumerBatch([first, second, independent], { dueBy: f.state.now(), runnable: () => true }), [first, independent]);
  assert.deepEqual(consumerBatch([second], { dueBy: f.state.now(), runnable: () => true,
    active: new Map([['scope-jql-refresh', { count: 1, limit: 1 }]]) }), []);
  assert.equal(first.concurrency.limit, 1);
});

test('invalid arguments and missing precomputation ID refuse before registering a cache mapping', async () => {
  const f = fixture(); const sprint = activeSprint(f); const valid = f.begin(sprint).request;
  for (const request of [{ ...valid, clause: { ...valid.clause, arguments: ['not-numeric'] } },
                         { ...valid, precomputationId: undefined }]) {
    const result = await f.scopeChanged(request);
    assert.equal(typeof result.error, 'string');
  }
  assert.equal(registryRows(f).length, 0);
  assert.equal(f.queue.length, 0);
});
