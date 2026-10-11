'use strict';
// Forge 2.0 site core (SPEC §2.1, §2.3-§2.5): the rate model over every /rest route, the world mutation API, the live
// state the REST handlers read, ADMINISTER, the app's field and the virtual clock.
//   node --test forge2/kit/test/site-core.test.cjs
const test = require('node:test');
const assert = require('node:assert');
const path = require('node:path');
const SITE = path.join(__dirname, '..', '..', 'site');
const { createSite } = require(path.join(SITE, 'site.cjs'));
const { createRate, costOf, returnedObjects, kindOf, writtenRefs, MODEL } = require(path.join(SITE, 'rate.cjs'));

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

test('documented object cost classes price actual response records and read-like POSTs', () => {
  const issue = 'GET /rest/api/3/issue/{issueIdOrKey}';
  assert.strictEqual(costOf(issue, undefined, { id: '1', fields: { summary: 'x', assignee: { accountId: 'a' } } }), 2);
  assert.strictEqual(costOf(issue, undefined, { errorMessages: ['Not found'] }), 1);
  for (const n of [0, 1, 50, 51, 100]) {
    const response = { issues: Array(n).fill({ id: '1' }), isLast: true, maxResults: 100 };
    for (const method of ['GET', 'POST']) assert.strictEqual(costOf(`${method} /rest/api/3/search/jql`, {}, response), 1 + n);
  }
  assert.strictEqual(costOf('POST /rest/api/3/changelog/bulkfetch', {}, { issueChangeLogs: [{ issueId: '1', changeHistories: [1, 2] }, { issueId: '2', changeHistories: [3] }] }), 3);
  assert.strictEqual(costOf('POST /rest/api/3/issue/bulkfetch', {}, { issues: [{ id: '1' }, { id: '2' }] }), 3);
  assert.strictEqual(costOf('GET /rest/api/3/group/member', {}, { values: Array(8).fill({ accountId: 'a' }) }), 17);
  assert.strictEqual(costOf('GET /rest/api/3/mypermissions', {}, { permissions: { ADMINISTER: {}, BROWSE_PROJECTS: {} } }), 5);
  assert.strictEqual(costOf('POST /rest/api/3/permissions/check', {}, { globalPermissions: ['ADMINISTER'], projectPermissions: [{ id: '1' }] }), 5);
  const updates = (n) => ({ updates: [{ customField: 'f', issueIds: Array.from({ length: n }, (_, i) => i + 1), value: 'committed' }] });
  assert.strictEqual(costOf('POST /rest/api/3/app/field/value', updates(200)), 1);
  assert.strictEqual(costOf('PUT /rest/api/3/app/field/{fieldIdOrKey}/value', updates(51)), 1);
  assert.strictEqual(costOf('POST /rest/api/3/issue/{issueIdOrKey}/comment', {}, { id: '123' }), 1);
  assert.strictEqual(returnedObjects({ values: [], total: 1000 }), 0, 'pagination metadata is not a returned domain record');
  assert.strictEqual(kindOf({ source: 'resolver', moduleType: 'jira:adminPage' }), 'person');
  assert.strictEqual(kindOf({ source: 'frontend', moduleType: 'dashboards:widget' }), 'person');
  assert.strictEqual(kindOf({ source: 'function', moduleType: 'action' }), 'person');
  for (const m of ['trigger', 'consumer', 'scheduledTrigger', 'webtrigger']) assert.strictEqual(kindOf({ source: 'function', moduleType: m }), 'background', m);
  assert.strictEqual(kindOf({ source: 'function', moduleType: null }), 'unlabelled');
  assert.deepStrictEqual(writtenRefs('POST /rest/api/3/issue/{issueIdOrKey}/comment', { issueIdOrKey: 'A-1' }, {}), ['A-1']);
  assert.deepStrictEqual(writtenRefs('POST /rest/api/3/issue/{issueIdOrKey}/changelog/list', { issueIdOrKey: 'A-1' }, {}), []);
  assert.deepStrictEqual(writtenRefs(issue, { issueIdOrKey: 'A-1' }, {}), []);
  assert.deepStrictEqual(writtenRefs('POST /rest/api/3/app/field/value', {}, updates(2)), ['1', '2']);
});

test('burst tokens count requests independently of points and endpoints', () => {
  const rate = createRate();
  const t = Date.UTC(2026, 9, 20, 10, 0, 0);
  const endpoint = 'POST /rest/api/3/search/jql';
  for (let i = 0; i < 100; i++) {
    assert.strictEqual(rate.check({ t, endpoint, kind: 'background' }), null);
    rate.charge({ t, endpoint, kind: 'background', cost: 10 });
  }
  assert.deepStrictEqual(rate.check({ t, endpoint, kind: 'background' }), { reason: 'jira-burst-based', retryAfter: 1 });
  assert.strictEqual(rate.check({ t, endpoint: 'GET /rest/api/3/project', kind: 'background' }), null);
  assert.strictEqual(rate.check({ t: t + 10, endpoint, kind: 'background' }), null, '100RPS refills one request in10ms');
  assert.strictEqual(rate.summary()[0].background, 1000, 'ten-point replies still consumed one burst token each');
});

test('both issue windows protect every issue of a bulk write and refused writes consume no window', () => {
  const rate = createRate();
  const endpoint = 'POST /rest/api/3/app/field/value';
  const t = Date.UTC(2026, 9, 20, 10, 0, 0);
  const req = (at, issues) => ({ t: at, endpoint, kind: 'background', cost: 1, issues });
  for (let burst = 0; burst < 5; burst++) {
    const at = t + burst * 2000;
    for (let n = 0; n < 20; n++) {
      assert.strictEqual(rate.check(req(at, ['10'])), null);
      rate.charge(req(at, ['10', '10']));
    }
    const blocked = rate.check(req(at, ['11', '10']));
    assert.deepStrictEqual(blocked, { reason: 'jira-per-issue-on-write', retryAfter: burst < 4 ? 2 : 22 });
    assert.strictEqual(rate.check(req(at, ['11'])), null, 'another issue remains writable');
  }
  assert.deepStrictEqual(rate.check(req(t + 10_000, ['10'])), { reason: 'jira-per-issue-on-write', retryAfter: 20 }, '100/30s persists beyond2s');
  assert.strictEqual(rate.check(req(t + 30_000, ['10'])), null, 'the oldest20 writes leave the long window');
  rate.charge(req(t + 30_000, ['10']));
  assert.strictEqual(rate.summary()[0].background, 101, 'only served operations are charged once, even with repeated issue IDs');
});

test('global and tenant quota reasons reset on UTC hours; point headers are separate from request headers', () => {
  const t = Date.UTC(2026, 9, 20, 10, 59, 0);
  const endpoint = 'GET /rest/api/3/issue/{issueIdOrKey}';
  for (const quotaScope of ['global', 'tenant']) {
    const rate = createRate({ ...MODEL, quotaScope });
    const policy = `${quotaScope}-app-quota`;
    assert.deepStrictEqual(rate.headers(t, endpoint), { 'RateLimit-Policy': `"${policy}";q=2400;w=3600`, 'RateLimit': `"${policy}";t=60`,
      'X-RateLimit-Limit': '150', 'X-RateLimit-Remaining': '150' });
    rate.draw({ t, points: 2000 });
    assert.strictEqual(rate.headers(t, endpoint).RateLimit, `"${policy}";r=400;t=60`);
    assert.strictEqual(rate.headers(t, endpoint)['X-RateLimit-NearLimit'], 'true');
    rate.draw({ t, points: 400 });
    const req = { t: t + 30_000, endpoint, kind: 'person' };
    assert.deepStrictEqual(rate.check(req), { reason: `jira-quota-${quotaScope}-based`, retryAfter: 30 });
    assert.strictEqual(rate.check({ ...req, charged: false }), null, 'frontend bridge traffic is outside app quota');
    assert.strictEqual(rate.check({ ...req, t: t + 60_000 }), null, 'quota resets at top ofUTC hour');
    assert.throws(() => rate.draw({ t, points: 0 }), /positive/);
  }
});

test('site responses expose request capacity, structured quota and accurate response costs', () => withSite(async (site, call) => {
  const vt = site.state.now();
  const iss = site.pack.issues.find((i) => !i.hiddenFrom.length);
  const p = `/rest/api/3/issue/${iss.id}?fields=summary`;
  const ok = await call('GET', p, { moduleType: 'consumer', vt });
  assert.strictEqual(ok.status, 200);
  assert.strictEqual(ok.headers.get('x-ratelimit-limit'), '150');
  assert.strictEqual(ok.headers.get('x-ratelimit-remaining'), '149');
  assert.strictEqual(ok.headers.get('ratelimit-policy'), '"global-app-quota";q=2400;w=3600');
  assert.strictEqual(ok.headers.get('x-ratelimit-reset'), null, 'reset timestamp is only returned with429');
  assert.strictEqual(site.log.at(-1).points, 2);
  for (let i = 0; i < 149; i++) assert.strictEqual((await call('GET', p, { moduleType: 'consumer', vt })).status, 200);
  const refused = await call('GET', p, { moduleType: 'consumer', vt });
  assert.deepStrictEqual([refused.status, refused.body, refused.headers.get('ratelimit-reason'), refused.headers.get('retry-after')],
    [429, { errorMessages: ['Rate limit exceeded'] }, 'jira-burst-based', '1']);
  assert.strictEqual(refused.headers.get('x-ratelimit-reset'), new Date(vt + 1000).toISOString());
  assert.strictEqual((await call('GET', p, { moduleType: 'consumer', vt: vt + 1000 })).status, 200);
  assert.deepStrictEqual(site.log.slice(-2).map((e) => [e.kind, e.points, e.status]), [['background', 0, 429], ['background', 2, 200]]);
  const s = await call('POST', '/rest/api/3/search/jql', { body: { jql: `project = ${site.pack.projects[0].key}`, fields: ['summary'], maxResults: 120 }, source: 'resolver', moduleType: 'jira:adminPage', vt: vt + 2000 });
  assert.strictEqual(s.status, 200);
  assert.strictEqual(s.body.issues.length, 100);
  assert.deepStrictEqual([site.log.at(-1).kind, site.log.at(-1).points], ['person', 101]);
}));

test('site rejects the21st issue write in2s without posting it, then admits afterRetryAfter', () => withSite(async (site, call) => {
  const vt = site.state.now();
  const iss = site.pack.issues.find((i) => !i.hiddenFrom.length);
  const body = { body: { type: 'doc', version: 1, content: [{ type: 'paragraph', content: [{ type: 'text', text: 'rate-test' }] }] } };
  for (let n = 0; n < 20; n++) assert.strictEqual((await call('POST', `/rest/api/3/issue/${iss.key}/comment`, { body, moduleType: 'consumer', vt })).status, 201);
  const blocked = await call('POST', `/rest/api/3/issue/${iss.id}/comment`, { body, moduleType: 'consumer', vt: vt + 1000 });
  assert.deepStrictEqual([blocked.status, blocked.headers.get('ratelimit-reason'), blocked.headers.get('retry-after')], [429, 'jira-per-issue-on-write', '1']);
  assert.strictEqual(site.comments.length, 20);
  assert.strictEqual((await call('POST', `/rest/api/3/issue/${iss.id}/comment`, { body, moduleType: 'consumer', vt: vt + 2000 })).status, 201);
  assert.strictEqual(site.comments.length, 21);
  assert.strictEqual(site.control.rate().hours[0].background, 21);
}));

test('quota exhaustion refuses person requests; bridge traffic remains exempt untilUTC reset', () => withSite(async (site, call) => {
  const t = site.state.now();
  const next = (Math.floor(t / HOUR) + 1) * HOUR;
  site.control.draw({ points: 2400 });
  const r = await call('GET', '/rest/api/3/myself', { as: site.pack.viewer, source: 'resolver', moduleType: 'dashboards:widget' });
  assert.strictEqual(r.status, 429);
  assert.strictEqual(r.headers.get('ratelimit-reason'), 'jira-quota-global-based');
  assert.strictEqual(Number(r.headers.get('retry-after')), Math.ceil((next - t) / 1000));
  assert.ok(r.headers.get('ratelimit').includes(';r=0;'));
  const bridge = await call('GET', '/rest/api/3/myself', { as: site.pack.viewer, source: 'frontend', moduleType: 'dashboards:widget' });
  assert.strictEqual(bridge.status, 200);
  assert.deepStrictEqual([site.log.at(-1).kind, site.log.at(-1).points], ['person', 0]);
  site.state.advanceTo(next);
  assert.strictEqual((await call('GET', '/rest/api/3/myself', { as: site.pack.viewer, source: 'resolver', moduleType: 'dashboards:widget' })).status, 200);
  assert.deepStrictEqual(site.control.rate().hours[0].refused, { person: { 'jira-quota-global-based': 1 } });
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
  assert.deepStrictEqual([w.events.length, w.events[0].change.id, w.events[0].issue.key, w.events[0].slot], [1, w.change.changelogId, mover.key, null]);
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
  assert.deepStrictEqual([w.issue.id, w.snapshot.id, w.snapshot.key], [gone.id, gone.id, gone.key]);
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
  assert.deepStrictEqual(w.events.map((e) => e.change.id), w.carried.map((c) => c.changelogId));
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
  // Groups (the admin panel's Comment group): everyone's group, and a team group the viewer is not in.
  const team = pack.groups[1];
  const viewerGroups = await call('GET', `/rest/api/3/user/groups?accountId=${viewer}`);
  assert.deepStrictEqual(viewerGroups.body.map((g) => g.name), ['jira-software-users']);
  const member = team.members[0];
  const me = await call('GET', '/rest/api/3/myself?expand=groups', { as: member, source: 'resolver' });
  assert.deepStrictEqual(me.body.groups.items.map((g) => g.name).sort(), ['jira-software-users', team.name].sort());
  assert.strictEqual(me.body.groups.size, 2);
  assert.strictEqual((await call('GET', '/rest/api/3/user/groups?accountId=nobody')).status, 404);
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

test('the disclosed normalized quota and documented request/write limits', () => {
  assert.deepStrictEqual([MODEL.quotaPerHour, MODEL.backgroundShare, MODEL.burst.capacitySeconds], [2400, 0.7, 1]);
  assert.deepStrictEqual(MODEL.burst.requestsPerSecond, { GET: 100, POST: 100, PUT: 50, PATCH: 50, DELETE: 50 });
  assert.deepStrictEqual(MODEL.perIssueWrites, [{ maxWrites: 20, windowMs: 2000 }, { maxWrites: 100, windowMs: 30_000 }]);
});
