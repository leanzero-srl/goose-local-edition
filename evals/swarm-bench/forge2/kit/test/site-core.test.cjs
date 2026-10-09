'use strict';
// Forge 2.0 site core (SPEC §2.1, §2.3-§2.5): the rate model over every /rest route, the world mutation API, the live
// state the REST handlers read, ADMINISTER, the app's field and the virtual clock.
//   node --test forge2/kit/test/site-core.test.cjs
const test = require('node:test');
const assert = require('node:assert');
const path = require('node:path');
const SITE = path.join(__dirname, '..', '..', 'site');
const { createSite } = require(path.join(SITE, 'site.cjs'));
const { createRate, costOf, kindOf, writtenRefs, MODEL } = require(path.join(SITE, 'rate.cjs'));

const SCOPES = ['read:jira-work', 'write:jira-work', 'read:jira-user', 'read:board-scope:jira-software', 'read:sprint:jira-software',
  'read:issue-details:jira', 'read:jql:jira', 'read:project:jira', 'read:board-scope.admin:jira-software', 'read:issue:jira-software'];
const HOUR = 3_600_000;

async function withSite(fn, opts = {}) {
  const site = await createSite({ seed: '0123456789abcdef', port: 0, ...opts });
  // as: 'app' | <accountId>; labels: the emulator's x-forge-module-type / x-forge-source; vt: the request's virtual instant
  const call = async (method, p, { body, as = 'app', moduleType, source, vt } = {}) => {
    const headers = { 'content-type': 'application/json', 'x-forge-as': as === 'app' ? 'app' : 'user', 'x-forge-scopes': JSON.stringify(SCOPES) };
    if (as !== 'app') headers['x-forge-account'] = as;
    if (moduleType) headers['x-forge-module-type'] = moduleType;
    if (source) headers['x-forge-source'] = source;
    if (vt !== undefined) headers['x-forge-vtime'] = String(vt);
    const res = await fetch(site.url + p, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
    const text = await res.text();
    let json = null;
    try { json = JSON.parse(text); } catch { json = text; }
    return { status: res.status, body: json, headers: res.headers };
  };
  try { await fn(site, call); } finally { await site.stop(); }
}

test('the SPEC §2.1 cost table, kinds and written issues', () => {
  assert.strictEqual(costOf('GET /rest/api/3/issue/{issueIdOrKey}', undefined), 1);
  assert.strictEqual(costOf('GET /rest/agile/1.0/board/{boardId}/sprint', undefined), 1);
  for (const [n, pts] of [[0, 1], [1, 2], [50, 2], [51, 3], [100, 3], [5000, 101]]) assert.strictEqual(costOf('POST /rest/api/3/search/jql', {}, n), pts, `search ${n}`);
  assert.strictEqual(costOf('GET /rest/api/3/search/jql', undefined, 100), 3);
  assert.strictEqual(costOf('POST /rest/api/3/changelog/bulkfetch', { issueIdsOrKeys: Array(1000).fill('1') }), 2);
  const updates = (n) => ({ updates: [{ customField: 'customfield_1', issueIds: Array.from({ length: n }, (_, i) => i + 1), value: 'committed' }] });
  assert.strictEqual(costOf('POST /rest/api/3/app/field/value', updates(1)), 2);
  assert.strictEqual(costOf('POST /rest/api/3/app/field/value', updates(200)), 5);
  assert.strictEqual(costOf('PUT /rest/api/3/app/field/{fieldIdOrKey}/value', updates(51)), 3);
  assert.strictEqual(costOf('POST /rest/api/3/issue/{issueIdOrKey}/comment', {}), 2);
  assert.strictEqual(costOf('POST /rest/api/3/issue/bulkfetch', {}), 2);
  assert.strictEqual(kindOf({ source: 'resolver', moduleType: 'jira:adminPage' }), 'person');
  assert.strictEqual(kindOf({ source: 'frontend', moduleType: 'dashboards:widget' }), 'person');
  assert.strictEqual(kindOf({ source: 'function', moduleType: 'action' }), 'person');
  for (const m of ['trigger', 'consumer', 'scheduledTrigger', 'webtrigger']) assert.strictEqual(kindOf({ source: 'function', moduleType: m }), 'background', m);
  assert.strictEqual(kindOf({ source: 'function', moduleType: null }), 'unlabelled');
  assert.deepStrictEqual(writtenRefs('POST /rest/api/3/issue/{issueIdOrKey}/comment', { issueIdOrKey: 'A-1' }, {}), ['A-1']);
  assert.deepStrictEqual(writtenRefs('POST /rest/api/3/issue/{issueIdOrKey}/changelog/list', { issueIdOrKey: 'A-1' }, {}), []);
  assert.deepStrictEqual(writtenRefs('GET /rest/api/3/issue/{issueIdOrKey}', { issueIdOrKey: 'A-1' }, {}), []);
  assert.deepStrictEqual(writtenRefs('POST /rest/api/3/app/field/value', {}, updates(2)), ['1', '2']);
});

test('the wall, the burst bucket and the per-issue window, in virtual time', () => {
  const rate = createRate();
  const t0 = Date.UTC(2026, 9, 20, 10, 59, 0);
  // Burst: 30 points at one instant, the 31st point refused until 1 token refills (Retry-After rounds up to 1 s).
  for (let i = 0; i < 30; i++) {
    assert.strictEqual(rate.check({ t: t0, endpoint: 'GET x', kind: 'background', cost: 1 }), null);
    rate.charge({ t: t0, endpoint: 'GET x', kind: 'background', cost: 1 });
  }
  assert.deepStrictEqual(rate.check({ t: t0, endpoint: 'GET x', kind: 'background', cost: 1 }), { reason: 'jira-burst-based', retryAfter: 1 });
  assert.strictEqual(rate.check({ t: t0, endpoint: 'GET y', kind: 'background', cost: 1 }), null, 'another endpoint has its own bucket');
  assert.strictEqual(rate.check({ t: t0 + 200, endpoint: 'GET x', kind: 'background', cost: 1 }), null, '200 ms refill 1 point');
  // Per issue: one write per issue per 2 s.
  rate.charge({ t: t0, endpoint: 'POST c', kind: 'person', cost: 2, issues: ['10'] });
  assert.deepStrictEqual(rate.check({ t: t0 + 1500, endpoint: 'POST c', kind: 'person', cost: 2, issues: ['10'] }), { reason: 'jira-per-issue-on-write', retryAfter: 1 });
  assert.strictEqual(rate.check({ t: t0 + 1500, endpoint: 'POST c', kind: 'person', cost: 2, issues: ['11'] }), null);
  assert.strictEqual(rate.check({ t: t0 + 2000, endpoint: 'POST c', kind: 'person', cost: 2, issues: ['10'] }), null);
  // Headers: the limit always; remaining + near-limit only below 20 % of the hour left.
  assert.deepStrictEqual(rate.headers(t0), { 'X-RateLimit-Limit': '2400' });
  rate.draw({ t: t0, points: 1900 });
  assert.deepStrictEqual(rate.headers(t0), { 'X-RateLimit-Limit': '2400', 'X-RateLimit-Remaining': '468', 'X-RateLimit-NearLimit': 'true' });
  // The wall: spent >= 2,400 refuses everything, person-facing included, until the top of the hour.
  rate.draw({ t: t0, points: 468 });
  assert.deepStrictEqual(rate.check({ t: t0 + 30_000, endpoint: 'GET z', kind: 'person', cost: 1 }), { reason: 'jira-quota-tenant-based', retryAfter: 30 });
  assert.strictEqual(rate.check({ t: Date.UTC(2026, 9, 20, 11, 0, 0), endpoint: 'GET z', kind: 'person', cost: 1 }), null, 'reset at the top of the hour');
  const [h] = rate.summary();
  assert.strictEqual(h.hour, '2026-10-20T10:00:00.000Z');
  assert.deepStrictEqual([h.background, h.person, h.external, h.total], [30, 2, 2368, 2400]);
  assert.deepStrictEqual(h.refused, { background: { 'jira-burst-based': 1 }, person: { 'jira-per-issue-on-write': 1, 'jira-quota-tenant-based': 1 } });
  assert.throws(() => rate.draw({ t: t0, points: 0 }), /positive/);
});

test('the site: every response carries the limit; refusals carry Jira\'s 429 shape and are logged with kind and reason', () => withSite(async (site, call) => {
  const vt = site.state.now();
  const iss = site.pack.issues.find((i) => !i.hiddenFrom.length);
  const ok = await call('GET', `/rest/api/3/issue/${iss.id}?fields=summary`, { moduleType: 'consumer', vt });
  assert.strictEqual(ok.status, 200);
  assert.strictEqual(ok.headers.get('x-ratelimit-limit'), '2400');
  assert.strictEqual(ok.headers.get('x-ratelimit-nearlimit'), null);
  // 29 more GETs on the same endpoint at the same instant empty the bucket; the 31st is refused.
  for (let i = 0; i < 29; i++) assert.strictEqual((await call('GET', `/rest/api/3/issue/${iss.id}?fields=summary`, { moduleType: 'consumer', vt })).status, 200);
  const refused = await call('GET', `/rest/api/3/issue/${iss.id}?fields=summary`, { moduleType: 'consumer', vt });
  assert.strictEqual(refused.status, 429);
  assert.deepStrictEqual(refused.body, { errorMessages: ['Rate limit exceeded'] });
  assert.strictEqual(refused.headers.get('ratelimit-reason'), 'jira-burst-based');
  assert.strictEqual(refused.headers.get('retry-after'), '1');
  assert.strictEqual(refused.headers.get('x-ratelimit-limit'), '2400');
  // The same endpoint 1 s later: refilled.
  assert.strictEqual((await call('GET', `/rest/api/3/issue/${iss.id}?fields=summary`, { moduleType: 'consumer', vt: vt + 1000 })).status, 200);
  const last = site.log.slice(-2);
  assert.deepStrictEqual(last.map((e) => [e.kind, e.points, e.status, e.rateLimited ?? null]), [['background', 0, 429, 'jira-burst-based'], ['background', 1, 200, null]]);
  assert.strictEqual(site.log.at(-1).t, new Date(vt + 1000).toISOString(), 'the log carries the request\'s virtual instant');
  // A search page is charged 1 + 1 per started 50 issues returned.
  const project = site.pack.projects[0].key;
  const s = await call('POST', '/rest/api/3/search/jql', { body: { jql: `project = ${project}`, fields: ['summary'], maxResults: 120 }, source: 'resolver', moduleType: 'jira:adminPage', vt: vt + 2000 });
  assert.strictEqual(s.status, 200);
  assert.strictEqual(s.body.issues.length, 100, 'fielded pages cap at 100');
  assert.deepStrictEqual([site.log.at(-1).kind, site.log.at(-1).points], ['person', 3]);
  // Per-issue writes: a second comment on the same issue within 2 s is refused; another issue is not.
  const adf = { type: 'doc', version: 1, content: [{ type: 'paragraph', content: [{ type: 'text', text: 'x' }] }] };
  assert.strictEqual((await call('POST', `/rest/api/3/issue/${iss.key}/comment`, { body: { body: adf }, moduleType: 'consumer', vt: vt + 3000 })).status, 201);
  const again = await call('POST', `/rest/api/3/issue/${iss.id}/comment`, { body: { body: adf }, moduleType: 'consumer', vt: vt + 4000 });
  assert.deepStrictEqual([again.status, again.headers.get('ratelimit-reason'), again.headers.get('retry-after')], [429, 'jira-per-issue-on-write', '1']);
  assert.strictEqual(site.comments.length, 1);
  const hours = site.control.rate().hours;
  const total = (k) => hours.reduce((n, h) => n + h[k], 0);
  // 31 GETs served (1 point each) and one comment (2); the refused GET and comment cost nothing.
  assert.deepStrictEqual([total('background'), total('person')], [31 + 2, 3]);
  assert.strictEqual(hours.reduce((n, h) => n + (h.refused.background?.['jira-burst-based'] ?? 0), 0), 1);
}));

test('the wall: a draw on the hour refuses person-facing requests too, until the top of the virtual hour', () => withSite(async (site, call) => {
  const t = site.state.now();
  const next = (Math.floor(t / HOUR) + 1) * HOUR;
  site.control.draw({ points: 2400 });
  const r = await call('GET', '/rest/api/3/myself', { as: site.pack.viewer, source: 'resolver', moduleType: 'dashboards:widget' });
  assert.strictEqual(r.status, 429);
  assert.strictEqual(r.headers.get('ratelimit-reason'), 'jira-quota-tenant-based');
  assert.strictEqual(Number(r.headers.get('retry-after')), Math.ceil((next - t) / 1000));
  assert.strictEqual(r.headers.get('x-ratelimit-remaining'), '0');
  site.state.advanceTo(next);
  assert.strictEqual((await call('GET', '/rest/api/3/myself', { as: site.pack.viewer, source: 'resolver', moduleType: 'dashboards:widget' })).status, 200);
  assert.strictEqual(site.log[0].kind, 'person');
  const [h] = site.control.rate().hours;
  assert.deepStrictEqual(h.refused, { person: { 'jira-quota-tenant-based': 1 } });
}));

test('the clock is virtual: it moves only when advanced', async () => {
  await withSite(async (site) => {
    const a = site.state.now();
    await new Promise((ok) => setTimeout(ok, 30));
    assert.strictEqual(site.state.now(), a);
    assert.strictEqual(site.state.advance(1500), a + 1500);
  });
});

test('world: an issue moves to another board\'s sprint with that board\'s estimate; search and agile lists follow', () => withSite(async (site, call) => {
  const { pack, state } = site;
  const fid = pack.sprintFieldId;
  const [b0, b1] = pack.boards.filter((b) => b.type === 'scrum' && b.projectKey === pack.boards[0].projectKey);
  const from = pack.sprints.find((s) => s.state === 'active' && s.originBoardId === b1.id);
  const to = pack.sprints.find((s) => s.state === 'active' && s.originBoardId === b0.id);
  const mover = pack.issues.find((i) => (i.fields[fid] ?? []).some((s) => s.id === from.id) && state.liveRemainingFor(i.id) === 0);
  const before = await call('POST', '/rest/api/3/search/jql', { body: { jql: `sprint = ${to.id}`, fields: ['id'], maxResults: 5000 } });
  assert.strictEqual(before.status, 200, JSON.stringify(before.body));
  assert.ok(!before.body.issues.some((i) => i.id === mover.id));
  const at = state.now() + 60_000;
  const w = site.control.moveissue({ issue: mover.key, sprintId: to.id, at, authorId: pack.users[0].accountId, estimate: 8 });
  assert.strictEqual(w.type, 'moveIssue');
  assert.deepStrictEqual([w.fromSprintId, w.toSprintId], [from.id, to.id]);
  assert.strictEqual(state.now(), at, 'the site clock moved to the change');
  const sprintItem = w.change.items[0];
  assert.deepStrictEqual(sprintItem.to.split(', ').map(Number).includes(to.id) && !sprintItem.to.split(', ').map(Number).includes(from.id), true);
  assert.deepStrictEqual(w.change.items[1], { field: pack.fields.find((f) => f.id === b0.estimationFieldId).name, fieldtype: 'custom', fieldId: b0.estimationFieldId,
    from: mover.fields[b0.estimationFieldId] === null ? '' : String(mover.fields[b0.estimationFieldId]), fromString: mover.fields[b0.estimationFieldId] === null ? '' : String(mover.fields[b0.estimationFieldId]), to: '8', toString: '8' });
  assert.ok(Number(w.change.changelogId) > Math.max(...[...pack.history, ...pack.live].map((h) => Number(h.changelogId))), 'world ids sit above every pack id');
  const after = await call('POST', '/rest/api/3/search/jql', { body: { jql: `sprint = ${to.id}`, fields: ['id'], maxResults: 5000 } });
  assert.ok(after.body.issues.some((i) => i.id === mover.id), 'the cached hits were dropped by the change');
  const issue = await call('GET', `/rest/api/3/issue/${mover.id}?fields=${fid},${b0.estimationFieldId}`);
  assert.strictEqual(issue.body.fields[b0.estimationFieldId], 8);
  assert.ok(issue.body.fields[fid].some((s) => s.id === to.id && s.state === 'active'));
  const log = await call('GET', `/rest/api/3/issue/${mover.id}/changelog?maxResults=100`);
  assert.strictEqual(log.body.values.at(-1).id, w.change.changelogId);
  const agile = await call('GET', `/rest/agile/1.0/sprint/${from.id}/issue?maxResults=1000&fields=summary`);
  assert.ok(!agile.body.issues.some((i) => i.id === mover.id));
  assert.throws(() => state.moveIssue(mover.id, to.id, { authorId: pack.users[0].accountId }), /already in sprint/);
  assert.throws(() => state.moveIssue(mover.id, from.id, {}), /needs an author/);
}));

test('world: deleted issues vanish everywhere; an issue with live changes left cannot be deleted', () => withSite(async (site, call) => {
  const { pack, state } = site;
  const busy = pack.issues.find((i) => state.liveRemainingFor(i.id) > 0);
  assert.throws(() => state.deleteIssue(busy.id), /live change/);
  const gone = pack.issues.find((i) => state.liveRemainingFor(i.id) === 0 && (i.fields[pack.sprintFieldId] ?? []).some((s) => s.state === 'active'));
  const w = site.control.deleteissue({ issue: gone.key });
  assert.strictEqual(w.issue.id, gone.id);
  assert.strictEqual((await call('GET', `/rest/api/3/issue/${gone.key}`)).status, 404);
  const sid = gone.fields[pack.sprintFieldId].find((s) => s.state === 'active').id;
  const s = await call('POST', '/rest/api/3/search/jql', { body: { jql: `sprint = ${sid}`, fields: ['id'], maxResults: 5000 } });
  assert.ok(!s.body.issues.some((i) => i.id === gone.id));
  const bulk = await call('POST', '/rest/api/3/changelog/bulkfetch', { body: { issueIdsOrKeys: [gone.id] } });
  assert.deepStrictEqual(bulk.body.issueChangeLogs, []);
  assert.strictEqual(state.st.deleted.get(gone.id).issue.key, gone.key);
}));

test('world: a sprint closes (carry-over written as Jira writes it), leaves openSprints(), and its board can switch estimation field', () => withSite(async (site, call) => {
  const { pack, state } = site;
  const fid = pack.sprintFieldId;
  const board = pack.boards.find((b) => b.type === 'scrum' && pack.sprints.some((s) => s.originBoardId === b.id && s.state === 'future'));
  const sprint = pack.sprints.find((s) => s.state === 'active' && s.originBoardId === board.id);
  const future = pack.sprints.find((s) => s.state === 'future' && s.originBoardId === board.id);
  const members = pack.issues.filter((i) => (i.fields[fid] ?? []).some((s) => s.id === sprint.id));
  const open = members.filter((i) => i.fields.status.statusCategory.key !== 'done');
  assert.throws(() => state.closeSprint(sprint.id, { carryTo: future.id }), /needs an author/);
  const w = site.control.closesprint({ sprintId: sprint.id, carryTo: future.id, authorId: pack.users[1].accountId });
  assert.strictEqual(w.carried.length, open.length);
  for (const c of w.carried) assert.strictEqual(c.items[0].to, `${c.items[0].from}, ${future.id}`);
  const got = await call('GET', `/rest/agile/1.0/sprint/${sprint.id}`);
  assert.deepStrictEqual([got.body.state, got.body.completeDate], ['closed', new Date(state.now()).toISOString()]);
  const active = await call('GET', `/rest/agile/1.0/board/${board.id}/sprint?state=active`);
  assert.ok(!active.body.values.some((s) => s.id === sprint.id));
  const inOpen = await call('POST', '/rest/api/3/search/jql', { body: { jql: `sprint in openSprints() AND sprint = ${sprint.id}`, fields: ['id'], maxResults: 5000 } });
  assert.deepStrictEqual(inOpen.body.issues, []);
  const one = await call('GET', `/rest/api/3/issue/${members[0].id}?fields=${fid}`);
  assert.strictEqual(one.body.fields[fid].find((s) => s.id === sprint.id).state, 'closed');
  assert.throws(() => state.closeSprint(sprint.id), /closed, not active/);
  // The board's estimation field switches; its configuration says so.
  const other = pack.boards.find((b) => b.type === 'scrum' && b.estimationFieldId !== board.estimationFieldId).estimationFieldId;
  site.control.estimationfield({ boardId: board.id, fieldId: other });
  const conf = await call('GET', `/rest/agile/1.0/board/${board.id}/configuration`);
  assert.strictEqual(conf.body.estimation.field.fieldId, other);
  assert.throws(() => state.setBoardEstimationField(board.id, 'summary'), /not a number custom field/);
  assert.deepStrictEqual(site.control.world({}).entries.map((e) => e.type), ['closeSprint', 'setBoardEstimationField']);
}));

test('world: a revoked person stops seeing the project at once; others and the app do not; ADMINISTER is the admin\'s', () => withSite(async (site, call) => {
  const { pack } = site;
  const project = pack.projects[0].key;
  const iss = pack.issues.find((i) => i.projectKey === project && !i.hiddenFrom.length);
  const viewer = pack.viewer;
  const asViewer = (method, p, extra = {}) => call(method, p, { as: viewer, source: 'resolver', moduleType: 'jira:sprintAction', ...extra });
  assert.strictEqual((await asViewer('GET', `/rest/api/3/issue/${iss.id}`)).status, 200);
  const warm = await asViewer('POST', '/rest/api/3/search/jql', { body: { jql: `project = ${project}`, fields: ['id'], maxResults: 5000 } });
  assert.ok(warm.body.issues.length > 0);
  site.control.revokebrowse({ accountId: viewer, projectKey: project });
  assert.strictEqual((await asViewer('GET', `/rest/api/3/issue/${iss.id}`)).status, 404);
  const cold = await asViewer('POST', '/rest/api/3/search/jql', { body: { jql: `project = ${project}`, fields: ['id'], maxResults: 5000 } });
  assert.deepStrictEqual(cold.body.issues, [], 'no stale cached hits');
  assert.strictEqual((await asViewer('GET', `/rest/api/3/project/${project}`)).status, 404);
  const mine = await asViewer('GET', `/rest/api/3/mypermissions?permissions=BROWSE_PROJECTS&projectKey=${project}`);
  assert.strictEqual(mine.body.permissions.BROWSE_PROJECTS.havePermission, false);
  const peer = pack.users.find((u) => u.accountId !== viewer && !iss.hiddenFrom.includes(u.accountId)).accountId;
  assert.strictEqual((await call('GET', `/rest/api/3/issue/${iss.id}`, { as: peer, source: 'resolver' })).status, 200);
  assert.strictEqual((await call('GET', `/rest/api/3/issue/${iss.id}`)).status, 200, 'the app still reads it');
  // ADMINISTER: the site's administrator only, asked as the person.
  const admin = pack.admins[0];
  const adminPerm = await call('GET', '/rest/api/3/mypermissions?permissions=ADMINISTER', { as: admin, source: 'resolver', moduleType: 'jira:adminPage' });
  assert.deepStrictEqual([adminPerm.body.permissions.ADMINISTER.havePermission, adminPerm.body.permissions.ADMINISTER.type], [true, 'GLOBAL']);
  const notAdmin = await asViewer('GET', '/rest/api/3/mypermissions?permissions=ADMINISTER');
  assert.strictEqual(notAdmin.body.permissions.ADMINISTER.havePermission, false);
  const appPerm = await call('GET', '/rest/api/3/mypermissions?permissions=ADMINISTER');
  assert.strictEqual(appPerm.body.permissions.ADMINISTER.havePermission, false, 'the app is not a Jira administrator');
  const bulk = await call('POST', '/rest/api/3/permissions/check', { body: { accountId: admin, globalPermissions: ['ADMINISTER'] } });
  assert.deepStrictEqual(bulk.body.globalPermissions, ['ADMINISTER']);
}));

test('the app\'s scope-status field joins the field list at install and its values show on issues', () => withSite(async (site, call) => {
  const { pack, state } = site;
  const id = pack.scopeStatusFieldId;
  assert.ok(!(await call('GET', '/rest/api/3/field')).body.some((f) => f.id === id), 'absent before install');
  site.control.addfield({ id, key: id, name: 'Scope status', custom: true, orderable: false, navigable: true, searchable: true,
    clauseNames: [`cf[${id.slice('customfield_'.length)}]`, 'Scope status'], schema: { type: 'string', custom: 'forge-app-field', customId: Number(id.slice('customfield_'.length)) } });
  assert.ok((await call('GET', '/rest/api/3/field')).body.some((f) => f.id === id));
  const iss = pack.issues[0];
  state.setFieldValue(iss.key, id, 'added +5');
  const got = await call('GET', `/rest/api/3/issue/${iss.id}?fields=${id}`);
  assert.strictEqual(got.body.fields[id], 'added +5');
  assert.throws(() => state.addField({ id, name: 'again' }), /exists/);
}));

test('a page walk slices one cached hit list; a live change applied between pages is visible on the next walk', () => withSite(async (site, call) => {
  const sprint = site.pack.sprints.find((s) => s.state === 'active');
  const walk = async () => {
    const ids = [];
    let token;
    do {
      const r = await call('POST', '/rest/api/3/search/jql', { body: { jql: `sprint = ${sprint.id} ORDER BY key`, fields: ['id'], maxResults: 7, ...(token ? { nextPageToken: token } : {}) },
        vt: site.state.now() + ids.length * 1000 });
      ids.push(...r.body.issues.map((i) => i.id));
      token = r.body.nextPageToken;
    } while (token);
    return ids;
  };
  const first = await walk();
  assert.strictEqual(new Set(first).size, first.length);
  const add = site.pack.live.find((c) => !c.delivery.liveUi && c.items[0].field === 'Sprint' && c.items[0].to.split(', ').includes(String(sprint.id)));
  site.applyChange(add.changelogId);
  const second = await walk();
  assert.ok(second.includes(add.issueId));
}));

test('the rate model the contract publishes', () => {
  assert.deepStrictEqual([MODEL.quotaPerHour, MODEL.backgroundShare, MODEL.burst.capacity, MODEL.burst.refillPerSecond, MODEL.perIssueWriteMs],
    [2400, 0.7, 30, 5, 2000]);
});
