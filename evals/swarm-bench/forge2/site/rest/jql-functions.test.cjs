'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const { facts } = require('../fixtures.cjs');
const { createState } = require('../state.cjs');
const { compile } = require('../jql.cjs');
const { jqlModel } = require('./platform.cjs');
const { createOpenApi } = require('../openapi.cjs');
const jql = require('./jql-functions.cjs');
const PATH = '/rest/api/3/jql/function/computation';
const KEY = 'ari:cloud:ecosystem::extension/00000000-0000-0000-0000-000000000000/env/static/scope-changed';
function rig() {
  const pack = facts('0123456789abcdef');
  const state = createState(pack);
  const clause = { field: 'key', type: ['issue'], operator: 'in', functionName: 'scopeChanged', arguments: [String(pack.sprints[0].id)] };
  const call = (method, body, query = '', as = 'app') => jql.handlers[`${method} ${PATH}`]({ state,
    caller: { as, accountId: as === 'app' ? pack.appAccountId : pack.viewer, invocationId: 'inv-1' },
    req: { query: new URLSearchParams(query), body } });
  const begin = (c = clause) => jql.begin(state, c, { functionKey: KEY });
  const evaluate = (fragment, user) => {
    const query = compile(fragment, jqlModel({ state, t: state.now(), caller: { accountId: user } }));
    return state.allIssues().filter((i) => state.canBrowse(user, i) && query.matches(i)).map((i) => i.id).sort();
  };
  return { pack, state, clause, call, begin, evaluate };
}

test('documented routes and optional current OAuth alternatives exist in the pinned OpenAPI', () => {
  const api = createOpenApi();
  for (const [method, path] of [['GET', PATH], ['POST', PATH], ['POST', `${PATH}/search`]]) {
    const m = api.match(method, path);
    assert.ok(m);
    assert.ok(api.scopeCheck(m.op, []).ok, 'app-data scope is recommended but currently optional');
  }
  assert.equal(api.match('PUT', PATH), null);
});

test('first evaluation stores native JQL; all users share the expansion and final search applies visibility', () => {
  const { pack, state, begin, evaluate } = rig();
  const hidden = pack.issues.find((i) => i.hiddenFrom.includes(pack.viewer));
  const visible = pack.issues.find((i) => !i.hiddenFrom.includes(pack.viewer));
  assert.ok(hidden && visible, 'same-site positive and negative controls');
  const first = begin();
  assert.equal(first.cached, false);
  assert.equal(first.request.precomputationId, first.id);
  assert.equal(first.request.clause.field, 'key');
  jql.finish(state, first.id, { jql: `id in (${visible.id},${hidden.id})` });
  const second = begin();
  assert.equal(second.cached, true);
  assert.equal(second.id, first.id);
  assert.deepEqual(evaluate(second.result.jql, pack.viewer), [visible.id]);
  assert.deepEqual(evaluate(second.result.jql, pack.appAccountId), [visible.id, hidden.id].sort());
  assert.equal(jql.log(state).filter((r) => r.type === 'evaluate').length, 1);
});

test('cached results stay stale until actual POST; GET pages, filters, sorting and by-ID search expose own mappings', () => {
  const { pack, state, begin, call } = rig();
  const ids = pack.issues.slice(0, 2).map((i) => i.id);
  const first = begin();
  jql.finish(state, first.id, { jql: `id in (${ids[0]})` });
  assert.equal(begin().result.jql, `id in (${ids[0]})`, 'host never invokes an app to refresh a cached clause');
  const other = begin({ ...first.request.clause, operator: 'not in' });
  jql.finish(state, other.id, { jql: `id not in (${ids[0]})` });
  const page = call('GET', undefined, 'startAt=0&maxResults=1&orderBy=-updated');
  assert.equal(page.body.values.length, 1);
  assert.equal(page.body.total, 2);
  assert.equal(page.body.isLast, false);
  assert.equal(call('GET', undefined, 'functionKey=foreign').body.total, 0);
  assert.equal(call('GET', undefined, `functionKey=${encodeURIComponent(KEY)}`).body.total, 2);
  assert.equal(call('POST', { values: [{ id: first.id, value: `id in (${ids.join(',')})` }] }).status, 204);
  assert.equal(begin().result.jql, `id in (${ids.join(',')})`);
  const result = jql.handlers[`POST ${PATH}/search`]({ state, caller: { as: 'app' },
    req: { query: new URLSearchParams(), body: { precomputationIDs: [first.id, 'missing'] } } });
  assert.deepEqual(result.body.notFoundPrecomputationIDs, ['missing']);
  assert.equal(result.body.precomputations[0].value, `id in (${ids.join(',')})`);
  assert.equal(call('GET', undefined, '', 'user').status, 403);
});

test('update validation is atomic, rejects malformed native JQL, and skip-not-found returns documented shape', () => {
  const { pack, state, begin, call } = rig();
  const first = begin();
  const old = `id in (${pack.issues[0].id})`;
  jql.finish(state, first.id, { jql: old });
  assert.equal(call('POST', { values: [{ id: first.id, value: `id in (${pack.issues[1].id})` }, { id: 'missing', value: old }] }).status, 404);
  assert.equal(jql.cached(state, first.id).value, old);
  assert.equal(call('POST', { values: [{ id: first.id, value: 'id in (' }] }).status, 400);
  assert.equal(call('POST', { values: [{ id: first.id, jql: old }] }).status, 400, 'updates use value, not invocation-result jql');
  assert.equal(call('POST', { values: [{ id: first.id, value: old, error: 'x' }] }).status, 400);
  assert.equal(call('POST', { values: [{ id: first.id, value: `${old} order by key` }] }).status, 400);
  assert.equal(call('POST', { values: [{ id: first.id, value: `id in (${Array.from({ length: 1001 }, (_, i) => i + 1).join(',')})` }] }).status, 400);
  assert.deepEqual(call('POST', { values: [{ id: first.id, value: old }, { id: 'missing', value: old }] }, 'skipNotFoundPrecomputations=true'),
    { status: 200, body: { notFoundPrecomputationIDs: ['missing'] } });
  assert.equal(call('POST', { values: [{ id: first.id, error: 'sprint no longer supported' }] }).status, 204);
  assert.equal(begin().result.error, 'sprint no longer supported');
});

test('errors normally re-evaluate, explicit stored errors and unused seven-day expiration follow documented cache lifecycle', () => {
  const { state, begin } = rig();
  const first = begin();
  jql.finish(state, first.id, { error: 'Invalid sprint' });
  const second = begin();
  assert.equal(second.cached, false);
  jql.finish(state, second.id, { error: 'Invalid sprint', storeErrorAsPrecomputation: true });
  assert.equal(begin().cached, true);
  state.advance(7 * 24 * 60 * 60 * 1000);
  assert.equal(begin().cached, false);
  state.reset();
  assert.equal(jql.snapshot(state).length, 0);
});

test('AST expansion preserves full query composition and never re-invokes a shared cached clause', async () => {
  const { pack, state, evaluate } = rig();
  const iss = pack.issues.find((i) => !i.hiddenFrom.includes(pack.viewer));
  let invocations = 0;
  let args;
  const registered = { scopeChanged: { functionKey: KEY, invoke: async (request) => {
    invocations += 1;
    args = request;
    return { jql: `id in (${iss.id})` };
  } } };
  const source = `project = "${iss.projectKey}" AND (issue in scopeChanged("${pack.sprints[0].id}") OR id = "${iss.id}") ORDER BY key DESC`;
  const first = await jql.expand(state, source, registered);
  assert.equal(first.error, undefined);
  assert.deepEqual(evaluate(first.jql, pack.viewer), [iss.id]);
  assert.equal(args.clause.field, 'key');
  assert.equal(first.calls[0].invoked, true);
  const second = await jql.expand(state, source, registered);
  assert.equal(invocations, 1);
  assert.equal(second.calls[0].precomputationId, first.calls[0].precomputationId);
  assert.equal(second.calls[0].invoked, false);
  const builtIn = await jql.expand(state, 'sprint in openSprints()', registered);
  assert.equal(builtIn.calls.length, 0);
  assert.equal(evaluate(builtIn.jql, pack.viewer).length, evaluate('sprint in openSprints()', pack.viewer).length);
  const invalid = await jql.expand(state, `issue in scopeChanged("not numeric")`, { scopeChanged: { functionKey: KEY, invoke: async () => ({ error: 'Invalid sprint' }) } });
  assert.equal(invalid.error, 'Invalid sprint');
});
