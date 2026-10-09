'use strict';
// The app field-value endpoints (site/rest/fields.cjs) through the site's own state, renderer and OpenAPI matcher,
// and the value's exposure on GET /issue through the platform handler.
// Run: node --test evals/swarm-bench/forge2/site/rest/fields.test.cjs
const test = require('node:test');
const assert = require('node:assert');
const { facts } = require('../fixtures.cjs');
const { createState } = require('../state.cjs');
const { createRenderer } = require('./render.cjs');
const { createOpenApi } = require('../openapi.cjs');
const platform = require('./platform.cjs');
const fields = require('./fields.cjs');

const FIELD = 'customfield_29999';
const DEF = { id: FIELD, key: FIELD, name: 'Scope status', untranslatedName: 'Scope status', custom: true, orderable: true, navigable: true,
  searchable: true, clauseNames: ['cf[29999]', 'Scope status'], schema: { type: 'string', custom: 'forge:scope-status', customId: 29999 } };
function rig({ installed = true } = {}) {
  const pack = facts('0123456789abcdef');
  pack.scopeStatusFieldId = FIELD;
  const state = createState(pack);
  // P4's state API (forge2/P4 state.cjs): the live field list, addField when v2 is installed, setFieldValue. This
  // branch's state.cjs is still 1.0's, whose renderer reads pack.fields, so the shim's addField feeds both lists.
  const fieldList = pack.fields.slice();
  state.fields = () => fieldList;
  state.addField = (def) => { fieldList.push(def); pack.fields.push(def); };
  state.setFieldValue = (ref, fieldId, value) => { state.issueByIdOrKey(ref).fields[fieldId] = value; };
  if (installed) state.addField(DEF);
  const render = createRenderer(state);
  const call = (opKey, { as = 'app', body, params = {}, query = '' } = {}) => {
    const accountId = as === 'app' ? pack.appAccountId : pack.viewer;
    const caller = { as, accountId, scopes: [], invocationId: 'inv-1', moduleType: 'consumer' };
    const handler = { ...platform.handlers, ...fields.handlers }[opKey];
    return handler({ state, render, limits: pack.limits, paging: pack.paging, caller, params,
      req: { method: opKey.split(' ')[0], pathname: '', query: new URLSearchParams(query), body },
      canBrowse: (iss) => state.canBrowse(accountId, iss), canComment: (iss) => state.canComment(accountId, iss) });
  };
  const ids = pack.issues.filter((i) => !i.hiddenFrom.length).slice(0, 3).map((i) => i.id);
  return { pack, state, call, ids };
}
const POST = 'POST /rest/api/3/app/field/value';
const PUT = 'PUT /rest/api/3/app/field/{fieldIdOrKey}/value';
const GET = 'GET /rest/api/3/issue/{issueIdOrKey}';

test('Jira routes exactly the two documented operations; PUT on the bulk path is 405', () => {
  const api = createOpenApi();
  assert.strictEqual(api.match('POST', '/rest/api/3/app/field/value').op.template, '/rest/api/3/app/field/value');
  assert.strictEqual(api.match('PUT', '/rest/api/3/app/field/customfield_1/value').op.template, '/rest/api/3/app/field/{fieldIdOrKey}/value');
  assert.strictEqual(api.match('PUT', '/rest/api/3/app/field/value'), null);
  assert.ok(api.pathExists('/rest/api/3/app/field/value'), 'so the site answers 405, not 404');
  for (const op of [POST, PUT]) {
    const m = api.ops.find((o) => `${o.method} ${o.template}` === op);
    assert.ok(api.scopeCheck(m, []).ok, `${op} needs no scope (classic alternative is empty)`);
  }
});

test('bulk POST and per-field PUT write values that GET /issue shows; an empty string or null clears', () => {
  const { call, ids, state } = rig();
  const r = call(POST, { body: { updates: [{ customField: FIELD, issueIds: [Number(ids[0]), ids[1]], value: 'committed' }] }, query: 'generateChangelog=false' });
  assert.deepStrictEqual(r, { status: 204, body: undefined });
  assert.strictEqual(call(PUT, { params: { fieldIdOrKey: FIELD }, body: { updates: [{ issueIds: [ids[2]], value: 'added +5' }] } }).status, 204);
  const shown = (id) => call(GET, { as: 'user', params: { issueIdOrKey: id }, query: `fields=${FIELD}` }).body.fields[FIELD];
  assert.deepStrictEqual(ids.map(shown), ['committed', 'committed', 'added +5']);
  const all = call(GET, { as: 'user', params: { issueIdOrKey: ids[0] } }).body.fields;
  assert.strictEqual(all[FIELD], 'committed', 'the default *navigable read lists it too');
  call(POST, { body: { updates: [{ customField: FIELD, issueIds: [ids[0]], value: '' }, { customField: FIELD, issueIds: [ids[1]], value: null }] } });
  assert.deepStrictEqual(ids.map(shown), [null, null, 'added +5']);
  assert.deepStrictEqual(fields.fieldValues(state), { [ids[2]]: 'added +5' });
  const log = fields.fieldWrites(state);
  assert.strictEqual(log.length, 5);
  assert.deepStrictEqual([log[0].generateChangelog, log[0].invocationId, log[0].value], ['false', 'inv-1', 'committed']);
  assert.strictEqual(state.st.histories.get(ids[0]).length, state.pack.history.filter((h) => h.issueId === ids[0]).length, 'no changelog entry is written');
});

test('refusals: asUser, unknown and foreign fields, duplicates, type, size, missing or deleted issues, stray properties', () => {
  const { call, ids, pack, state } = rig();
  const upd = (u) => ({ body: { updates: [u] } });
  assert.strictEqual(call(POST, { as: 'user', ...upd({ customField: FIELD, issueIds: [ids[0]], value: 'x' }) }).status, 403);
  assert.strictEqual(call(POST, upd({ customField: 'customfield_1', issueIds: [ids[0]], value: 'x' })).status, 404);
  assert.strictEqual(call(POST, upd({ customField: pack.sprintFieldId, issueIds: [ids[0]], value: 'x' })).status, 403, 'not the app\'s field');
  assert.strictEqual(call(POST, { body: { updates: [{ customField: FIELD, issueIds: [ids[0]], value: 'a' }, { customField: FIELD, issueIds: [ids[0]], value: 'b' }] } }).status, 400);
  assert.strictEqual(call(POST, upd({ customField: FIELD, issueIds: [ids[0]], value: 5 })).status, 400);
  assert.strictEqual(call(POST, upd({ customField: FIELD, issueIds: [ids[0]] })).status, 400, 'value is required');
  assert.strictEqual(call(POST, upd({ customField: FIELD, issueIds: [ids[0]], value: 'x', extra: 1 })).status, 400);
  assert.strictEqual(call(POST, { body: { updates: [], more: 1 } }).status, 400);
  assert.strictEqual(call(PUT, { params: { fieldIdOrKey: FIELD }, ...upd({ customField: FIELD, issueIds: [ids[0]], value: 'x' }) }).status, 400, 'PUT entries carry no customField');
  const many = pack.issues.slice(0, 201).map((i) => i.id);
  const big = call(POST, upd({ customField: FIELD, issueIds: many, value: 'x' }));
  assert.strictEqual(big.status, 400);
  assert.match(big.body.errorMessages[0], /at most 200/);
  assert.strictEqual(call(POST, upd({ customField: FIELD, issueIds: many.slice(0, 200), value: 'x' })).status, 204, '200 is allowed');
  const gone = call(POST, upd({ customField: FIELD, issueIds: [ids[0], '999999999'], value: 'removed' }));
  assert.strictEqual(gone.status, 400);
  assert.match(gone.body.errorMessages[0], /999999999/);
  state.st.issues.delete(ids[1]); // what state.deleteIssue does (forge2/P4)
  assert.strictEqual(call(POST, upd({ customField: FIELD, issueIds: [ids[1]], value: 'removed' })).status, 400, 'a deleted issue is gone');
  assert.strictEqual(state.st.issues.get(ids[0]).fields[FIELD], 'x', 'a refused request applies nothing');
});

test('before v2 installs the field a write is a 404; a pack without the field id fails loudly', () => {
  const { call, ids } = rig({ installed: false });
  assert.strictEqual(call(POST, { body: { updates: [{ customField: FIELD, issueIds: [ids[0]], value: 'committed' }] } }).status, 404);
  const pack = facts('0123456789abcdef');
  const state = createState(pack);
  assert.throws(() => fields.fieldValues(state), /scopeStatusFieldId is missing/);
});
