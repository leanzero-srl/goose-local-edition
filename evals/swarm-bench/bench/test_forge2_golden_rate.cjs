'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { createKvs } = require('../forge2/kit/lib/kvs.cjs');
const { costOf } = require('../forge2/site/rate.cjs');
const { randomUUID } = require('node:crypto');

function sourceModule(name, bindings, exports) {
  const source = fs.readFileSync(path.join(__dirname, 'golden-forge2/src', name), 'utf8')
    .replace(/^import[\s\S]*?;\n/gm, '')
    .replace(/^export \{[^}]+\};\n/gm, '')
    .replace(/\bexport (?=(?:async |class |function |const ))/g, '');
  const context = { ...bindings, module: { exports: {} }, URLSearchParams, console, Map, Set, WeakMap };
  vm.runInNewContext(`${source}\nmodule.exports = {${exports.join(',')}};`, context, { filename: name });
  return context.module.exports;
}
class Filter {
  constructor() { this.items = []; }
  and(property, c) { this.items.push({ property, ...c }); return this; }
  filters() { return { and: this.items }; }
}
const FilterConditions = { equalTo: (x) => ({ condition: 'EQUAL_TO', values: [x] }) };

function fixture() {
  let now = Date.parse('2026-10-11T12:00:00Z');
  const storage = createKvs({ entities: [{ name: 'rate-dose', attributes: { spent: { type: 'float' }, pausedUntil: { type: 'float' } }, indexes: [{ name: 'by-spent', range: ['spent'] }] }], now: () => now, floor: () => now });
  const wire = async (op, body) => {
    const result = storage.handle(op, body, { t: now });
    if (result.status === 404 && op.endsWith('/get')) return undefined;
    if (result.status >= 400) throw Object.assign(new Error(result.body.message), { code: result.body.code });
    return result.body;
  };
  const kvs = {
    get: async (key) => (await wire('/api/v1/get', { key }))?.value,
    set: (key, value, options) => wire('/api/v1/set', { key, value, options }),
    delete: (key) => wire('/api/v1/delete', { key }),
    entity: (entityName) => ({ get: async (key) => (await wire('/api/v1/entity/get', { entityName, key }))?.value,
      set: (key, value, options) => wire('/api/v1/entity/set', { entityName, key, value, options }) }),
    transact: () => {
      const body = { set: [] };
      return { set(key, value, entity) { body.set.push({ key, value, entityName: entity.entityName, conditions: entity.conditions.filters() }); return this; },
        execute: () => wire('/api/v1/transaction', body) };
    },
  };
  const requests = [];
  const replies = [];
  const api = { asApp: () => ({ requestJira: async (url, init) => { requests.push({ as: 'app', url, init }); return replies.shift(); } }),
    asUser: () => ({ requestJira: async (url, init) => { requests.push({ as: 'user', url, init }); return replies.shift(); } }) };
  const FakeDate = class extends Date { static now() { return now; } };
  const timer = (resolve, ms) => { now += ms; resolve(); };
  const module = sourceModule('jira.js', { kvs, api, Filter, FilterConditions, Date: FakeDate, setTimeout: timer, route: String.raw },
    ['Background', 'RateLimited', 'pointsOf', 'actualPoints', 'postJson', 'hourOf', 'jiraJson']);
  const response = (status, body, headers = {}) => ({ status, ok: status >= 200 && status < 300, headers: new Headers(headers), json: async () => body, text: async () => JSON.stringify(body) });
  return { ...module, storage, kvs, requests, replies, response, bindings: { kvs, randomUUID, Date: FakeDate, setTimeout: timer, route: String.raw }, advance(ms) { now += ms; }, now: () => now };
}

test('concurrent invocations cannot overbook an hour hidden by the query index', async () => {
  const f = fixture();
  const tasks = Array.from({ length: 40 }, () => new f.Background({ sharePercent: 70, limitSeconds: 900 }).admit('POST', '/rest/api/3/search/jql', f.postJson({ maxResults: 100 })));
  const result = await Promise.allSettled(tasks);
  assert.equal(result.filter((r) => r.status === 'fulfilled').length, 16);
  assert.equal(result.filter((r) => r.status === 'rejected' && r.reason.reason === 'background-share').length, 24);
  const records = f.storage.snapshot().entities['rate-dose'];
  assert.equal(records[String(f.hourOf(f.now()))].spent, 1616);
  const delayedIndex = f.storage.handle('/api/v1/entity/query', { entityName: 'rate-dose', indexName: 'by-spent', partition: [] }, { t: f.now() });
  assert.equal(delayedIndex.status, 200);
  assert.deepEqual(delayedIndex.body.data, []);
  f.advance(5000);
  const settledIndex = f.storage.handle('/api/v1/entity/query', { entityName: 'rate-dose', indexName: 'by-spent', partition: [] }, { t: f.now() });
  assert.equal(settledIndex.body.data[0].value.spent, 1616);
});

test('an empty response refunds its reservation before the next independent invocation', async () => {
  const f = fixture();
  const a = new f.Background({ sharePercent: 10, limitSeconds: 900 });
  const b = new f.Background({ sharePercent: 10, limitSeconds: 900 });
  const init = f.postJson({ maxResults: 100 });
  const held = await a.admit('POST', '/rest/api/3/search/jql', init);
  await b.admit('POST', '/rest/api/3/search/jql', init);
  await assert.rejects(a.admit('POST', '/rest/api/3/search/jql', init), (e) => e.reason === 'background-share');
  await a.settle(held, 1);
  await a.admit('POST', '/rest/api/3/search/jql', init);
  assert.equal(f.storage.snapshot().entities['rate-dose'][String(f.hourOf(f.now()))].spent, 203);
});

test('every sampled response is charged by domain records, including read-like POST and identity', () => {
  const f = fixture();
  const cases = [
    ['POST', '/rest/api/3/search/jql', { maxResults: 100 }, { issues: [{}, {}, {}] }, 4],
    ['POST', '/rest/api/3/changelog/bulkfetch', { issueIdsOrKeys: ['1', '2'], maxResults: 100 }, { issueChangeLogs: [{ issueId: '1', changeHistories: [{}, {}, {}] }] }, 2],
    ['POST', '/rest/api/3/issue/bulkfetch', { issueIdsOrKeys: ['1', '2'] }, { issues: [{}, {}] }, 3],
    ['POST', '/rest/api/3/permissions/check', { globalPermissions: ['ADMINISTER'], projectPermissions: [{ permissions: ['BROWSE_PROJECTS'] }] }, { globalPermissions: ['ADMINISTER'], projectPermissions: [{}] }, 5],
    ['POST', '/rest/api/3/app/field/value', { updates: [{ issueIds: [1, 2] }] }, undefined, 1],
    ['GET', '/rest/api/3/field/search?maxResults=50', undefined, { values: [{}, {}] }, 3],
  ];
  for (const [method, url, request, body, expected] of cases) {
    const init = request ? f.postJson(request) : undefined;
    assert.equal(f.actualPoints(method, url, body), expected);
    const template = url.split('?')[0];
    assert.equal(costOf(`${method} ${template}`, request, body), expected);
    assert.ok(f.pointsOf(method, url, init) >= expected);
  }
  assert.throws(() => f.pointsOf('GET', '/rest/api/3/field'), /response bound/);
});

for (const reason of ['jira-quota-global-based', 'jira-quota-tenant-based']) {
  test(`${reason} pauses all background callers while person requests still run`, async () => {
    const f = fixture();
    f.replies.push(f.response(429, { errorMessages: ['Rate limit exceeded'] }, { 'retry-after': '60', 'ratelimit-reason': reason }));
    await assert.rejects(f.jiraJson('app', '/rest/agile/1.0/sprint/11', undefined, new f.Background({ sharePercent: 70, limitSeconds: 900 })), (e) => e.reason === reason);
    await assert.rejects(new f.Background({ sharePercent: 70, limitSeconds: 900 }).admit('GET', '/rest/agile/1.0/sprint/12'), (e) => e.reason === 'paused');
    assert.equal(f.requests.length, 1);
    assert.equal(f.storage.snapshot().entities['rate-dose'][String(f.hourOf(f.now()))].spent, 0);
    f.replies.push(f.response(200, { id: '11' }));
    assert.equal((await f.jiraJson('user', '/rest/agile/1.0/sprint/11', undefined, { maxWaitSeconds: 0 })).id, '11');
    assert.equal(f.requests.at(-1).as, 'user');
  });
}

test('a reservation settles to its originating hour across the boundary', async () => {
  const f = fixture();
  const work = new f.Background({ sharePercent: 70, limitSeconds: 900 });
  const held = await work.admit('POST', '/rest/api/3/search/jql', f.postJson({ maxResults: 100 }));
  f.advance(3_600_000);
  await work.settle(held, 2);
  assert.equal(f.storage.snapshot().entities['rate-dose'][String(held.hour)].spent, 2);
  assert.equal(f.storage.snapshot().entities['rate-dose'][String(held.hour + 1)], undefined);
});

test('a quota-cut reconciliation advances its persisted page instead of buying the prefix twice', async () => {
  const f = fixture();
  let stopped = false;
  const requests = [];
  const cfg = { jiraTimeZone: 'UTC', sprintFieldId: 'sprint', scopeFieldId: null, estimateFields: [], sprints: { '11': { id: '11', startMs: f.now() - 86_400_000, boardId: '1' } } };
  const jiraJson = async (_as, url, init) => {
    const body = JSON.parse(init.body);
    requests.push({ url, body });
    if (url.includes('/search/jql')) {
      if (body.nextPageToken === 'second' && !stopped) { stopped = true; throw new f.RateLimited(60, 'background-share'); }
      return body.nextPageToken ? { issues: [{ id: '2', key: 'TEST-2', fields: { sprint: [] } }], isLast: true } : { issues: [{ id: '1', key: 'TEST-1', fields: { sprint: [] } }], nextPageToken: 'second' };
    }
    if (url.includes('/changelog/bulkfetch')) return { issueChangeLogs: [] };
    throw new Error(`Unexpected request ${url}`);
  };
  const noopRows = async () => [];
  const sync = sourceModule('sync.js', { ...f.bindings, jiraJson, postJson: f.postJson, RateLimited: f.RateLimited,
    sprintReader: () => {}, changesOfSprint: noopRows, membersOfSprint: noopRows, membersOfIssue: noopRows, rowsOfIssue: noopRows,
    recordChange: async () => false, writeMember: async () => false, sameMember: () => true,
    changeKey: (a, b) => `${a}:${b}`, memberKey: (a, b) => `${a}:${b}`, markIssueDeleted: async () => [], scopeStatus: () => '', compareIds: () => 0,
    estimateOf: () => 0, writeStatuses: async () => 0, currentStatus: () => '' }, ['reconcileAll']);
  const work = { timeLeftMs: () => 900_000, hasTimeFor: () => true, limitMs: 900_000 };
  await assert.rejects(sync.reconcileAll(cfg, cfg, work), (e) => e.reason === 'background-share');
  assert.equal((await f.kvs.get('reconcile-progress')).page, 1);
  assert.equal(await f.kvs.get('reconcile-lease'), undefined);
  f.advance(60_000);
  const result = await sync.reconcileAll(cfg, cfg, work);
  assert.equal(result.issues, 2);
  assert.equal(requests.filter((r) => r.url.includes('/search/jql') && !r.body.nextPageToken).length, 1);
  assert.equal(requests.filter((r) => r.url.includes('/changelog/bulkfetch') && r.body.issueIdsOrKeys.includes('1')).length, 1);
  assert.equal(await f.kvs.get('reconcile-progress'), undefined);
});


test('incremental reconciliation uses a fixed minute watermark in the app caller time zone', () => {
  const f = fixture();
  const sync = sourceModule('sync.js', { ...f.bindings }, ['candidateJql']);
  const cfg = { jiraTimeZone: 'America/New_York', sprints: { '11': { id: '11', startMs: f.now() - 86_400_000 } } };
  const watermark = Date.parse('2026-10-11T11:31:59.000Z');
  assert.equal(sync.candidateJql(cfg, watermark), 'updated >= "2026-10-11 07:31" ORDER BY id ASC');
  f.advance(3_600_000);
  assert.equal(sync.candidateJql(cfg, watermark), 'updated >= "2026-10-11 07:31" ORDER BY id ASC');
  assert.match(sync.candidateJql(cfg, null), /^sprint in \(11\) OR updated >= "/);
});


test('admission that crosses the hour while persisting its reservation rebooks before Jira is sent', async () => {
  const f = fixture();
  f.advance(3_599_000);
  const work = new f.Background({ sharePercent: 70, limitSeconds: 900 });
  const original = work.mutateDose.bind(work);
  let crossed = false;
  work.mutateDose = async (hour, update) => {
    const result = await original(hour, update);
    if (!crossed) { crossed = true; f.advance(2000); }
    return result;
  };
  const reservation = await work.admit('POST', '/rest/api/3/search/jql', f.postJson({ maxResults: 100 }));
  assert.equal(reservation.hour, f.hourOf(f.now()));
  const doses = f.storage.snapshot().entities['rate-dose'];
  assert.equal(doses[String(reservation.hour - 1)].spent, 0);
  assert.equal(doses[String(reservation.hour)].spent, 101);
});


test('incremental JQL selects the changed record and excludes unchanged active-sprint issues', () => {
  const f = fixture();
  const sync = sourceModule('sync.js', { ...f.bindings }, ['candidateJql']);
  const { compile } = require('../forge2/site/jql.cjs');
  const cfg = { jiraTimeZone: 'UTC', sprints: { '11': { id: '11', startMs: f.now() - 86_400_000 } } };
  const model = { now: f.now, fields: [], sprints: [{ id: 11, state: 'active' }], projects: [], statuses: [], issueTypes: [], sprintFieldId: 'sprint' };
  const cut = f.now() - 3_600_000;
  const query = compile(sync.candidateJql(cfg, cut), model);
  const untouched = { id: '1', key: 'TEST-1', fields: { updated: new Date(cut - 60_000).toISOString(), sprint: [{ id: 11 }] } };
  const changed = { id: '2', key: 'TEST-2', fields: { updated: new Date(cut + 1000).toISOString(), sprint: [] } };
  assert.equal(query.matches(untouched), false);
  assert.equal(query.matches(changed), true);
  f.advance(3_600_000);
  assert.equal(query.matches(changed), true);
});
