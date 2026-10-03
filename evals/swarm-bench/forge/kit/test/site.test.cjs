'use strict';
// The private dev/scoring site against the measured Jira Cloud facts and STARTER.md's promises:
// ISO-8601 dates everywhere (bulk changelog included), /search/jql token paging, the measured 400/404/410
// texts, OAuth2 scope refusal, v2 = harness_missing, the STARTER JQL vocabulary, ADF-only comments.
// Run: node --test forge/kit/test/site.test.cjs
const test = require('node:test');
const assert = require('node:assert');
const path = require('node:path');
const { createSite } = require(path.join(__dirname, '..', '..', 'site', 'site.cjs'));

const SCOPES = ['read:jira-work', 'write:jira-work', 'read:jira-user', 'read:board-scope:jira-software',
  'read:sprint:jira-software', 'read:issue-details:jira', 'read:jql:jira', 'read:project:jira',
  'read:board-scope.admin:jira-software', 'read:issue:jira-software', 'read:epic:jira-software'];
const ISO = /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{3}[+-]\d{4}$/;

async function withSite(fn) {
  const site = await createSite({ seed: '0123456789abcdef', port: 0 });
  const call = async (method, p, body, scopes = SCOPES) => {
    const res = await fetch(site.url + p, { method, headers: { 'content-type': 'application/json', 'x-forge-as': 'app',
      'x-forge-scopes': JSON.stringify(scopes) }, body: body === undefined ? undefined : JSON.stringify(body) });
    const text = await res.text();
    let json = null;
    try { json = JSON.parse(text); } catch { json = text; }
    return { status: res.status, body: json, headers: res.headers };
  };
  try { await fn(site, call); } finally { await site.stop(); }
}

test('every Jira timestamp is an ISO-8601 string, bulk changelog included (STARTER, DESIGN §17.1 0a)', () => withSite(async (site, call) => {
  const ids = site.pack.issues.slice(0, 20).map((i) => i.id);
  const bulk = await call('POST', '/rest/api/3/changelog/bulkfetch', { issueIdsOrKeys: ids, maxResults: 1000 });
  assert.strictEqual(bulk.status, 200, JSON.stringify(bulk.body));
  const hs = bulk.body.issueChangeLogs.flatMap((g) => g.changeHistories);
  assert.ok(hs.length > 0);
  for (const h of hs) assert.match(h.created, ISO, `bulkfetch created ${h.created}`);
  const g = bulk.body.issueChangeLogs.find((x) => x.changeHistories.length);
  const list = await call('POST', `/rest/api/3/issue/${g.issueId}/changelog/list`, { changelogIds: [Number(g.changeHistories[0].id)] });
  assert.strictEqual(list.status, 200, JSON.stringify(list.body));
  assert.deepStrictEqual(list.body.histories.map((h) => h.id), [g.changeHistories[0].id]);
  assert.match(list.body.histories[0].created, ISO);
  const one = await call('GET', `/rest/api/3/issue/${ids[0]}/changelog`);
  assert.strictEqual(one.status, 200);
  for (const h of one.body.values) assert.match(h.created, ISO);
  const issue = await call('GET', `/rest/api/3/issue/${ids[0]}?fields=created,updated`);
  assert.match(issue.body.fields.created, ISO);
  assert.match(issue.body.fields.updated, ISO);
}));

test('search: /search is 410, /search/jql pages by nextPageToken, the measured 400s', () => withSite(async (site, call) => {
  assert.strictEqual((await call('GET', '/rest/api/3/search?jql=project%3DX')).status, 410);
  const project = site.pack.projects[0].key;
  const seen = new Set();
  let token;
  let pages = 0;
  for (;;) {
    const r = await call('POST', '/rest/api/3/search/jql', { jql: `project = ${project} ORDER BY key ASC`, fields: ['summary'], maxResults: 7, ...(token ? { nextPageToken: token } : {}) });
    assert.strictEqual(r.status, 200, JSON.stringify(r.body));
    r.body.issues.forEach((i) => seen.add(i.id));
    pages++;
    if (!r.body.nextPageToken) { assert.strictEqual(r.body.isLast, true); break; }
    token = r.body.nextPageToken;
  }
  assert.strictEqual(seen.size, site.pack.issues.filter((i) => i.projectKey === project && !i.hiddenFrom?.includes(site.pack.appAccountId)).length);
  assert.ok(pages > 1);
  const unbounded = await call('POST', '/rest/api/3/search/jql', { jql: 'ORDER BY key', fields: ['id'] });
  assert.strictEqual(unbounded.status, 400);
  assert.deepStrictEqual(unbounded.body.errorMessages, ['Unbounded JQL queries are not allowed here. Please add a search restriction to your query.']);
  const eight = await call('POST', '/rest/api/3/search/jql', { jql: `project = ${project} ORDER BY key, created, updated, status, issuetype, assignee, reporter, summary` });
  assert.strictEqual(eight.status, 400);
  assert.deepStrictEqual(eight.body.errorMessages, ['JQL can be ordered by at most 7 fields.']);
}));

test('the STARTER JQL vocabulary compiles and answers', () => withSite(async (site, call) => {
  const p = site.pack.projects[0].key;
  const cf = site.pack.sprintFieldId.replace('customfield_', '');
  const viewer = site.pack.viewer?.accountId ?? site.pack.users[0].accountId;
  const queries = [
    `project = ${p} AND sprint in openSprints()`, `sprint in closedSprints() AND project = ${p}`, `sprint in futureSprints()`,
    `key = ${site.pack.issues[0].key}`, `project in (${site.pack.projects.map((x) => x.key).join(', ')})`,
    `project = ${p} AND updated >= -14d`, `project = ${p} AND created < now()`, `project = ${p} AND updated > startOfDay()`,
    `project = ${p} AND status != Done`, `project = ${p} AND statusCategory = Done`, `project = ${p} AND issuetype in (Story, Bug)`,
    `project = ${p} AND labels is EMPTY`, `project = ${p} AND labels is not EMPTY`, `project = ${p} AND assignee = currentUser()`,
    `project = ${p} AND reporter is not EMPTY`, `project = ${p} AND cf[${cf}] is not EMPTY`, `project = ${p} AND summary ~ "a"`,
    `project = ${p} AND NOT (status = Done OR status = "In Progress")`, `project = ${p} AND sprint not in openSprints()`,
    `project = ${p} AND created <= now() ORDER BY created DESC, key ASC`,
  ];
  for (const jql of queries) {
    const r = await call('POST', '/rest/api/3/search/jql', { jql, fields: ['summary'], maxResults: 5 });
    assert.strictEqual(r.status, 200, `${jql}: ${r.status} ${JSON.stringify(r.body)}`);
  }
  assert.deepStrictEqual(site.harnessMissing ?? [], [], 'no STARTER query may fall into harness_missing');
  void viewer;
}));

test('refusals: legacy v2 is harness_missing, unknown paths 404, scope mismatch 401, ADF-only comments, agile errors', () => withSite(async (site, call) => {
  const iss = site.pack.issues[0];
  const v2 = await call('GET', `/rest/api/2/issue/${iss.id}`);
  assert.strictEqual(v2.status, 501);
  assert.ok(site.harnessMissing.some((m) => String(m.what ?? m).includes('/rest/api/2/issue')), JSON.stringify(site.harnessMissing));
  assert.strictEqual((await call('GET', '/rest/api/3/not-a-resource')).status, 404);
  const noScope = await call('GET', `/rest/api/3/issue/${iss.id}`, undefined, ['storage:app']);
  assert.strictEqual(noScope.status, 401);
  assert.strictEqual(noScope.body.message, 'Unauthorized; scope does not match');
  // No scripted comment-path 429 since DESIGN §17.2 E: a plain-text body is refused at once.
  const plain = await call('POST', `/rest/api/3/issue/${iss.id}/comment`, { body: 'plain text' });
  assert.strictEqual(plain.status, 400);
  assert.deepStrictEqual(plain.body, { errorMessages: [], errors: { comment: 'Comment body is not valid!' } });
  const kanban = site.pack.boards.find((b) => b.type === 'kanban');
  const k = await call('GET', `/rest/agile/1.0/board/${kanban.id}/sprint`);
  assert.strictEqual(k.status, 400);
  assert.deepStrictEqual(k.body.errorMessages, ['The board does not support sprints']);
  const nos = await call('GET', '/rest/agile/1.0/sprint/1');
  assert.strictEqual(nos.status, 404);
  const scrum = site.pack.boards.find((b) => b.type === 'scrum');
  const sprints = await call('GET', `/rest/agile/1.0/board/${scrum.id}/sprint?state=active,closed`);
  assert.strictEqual(sprints.status, 200);
  for (const s of sprints.body.values) { assert.match(s.startDate, /^\d{4}-\d\d-\d\dT/); }
  const missingIssue = await call('GET', '/rest/api/3/issue/NOPE-1');
  assert.strictEqual(missingIssue.status, 404);
  assert.deepStrictEqual(missingIssue.body.errorMessages, ['Issue does not exist or you do not have permission to see it.']);
}));

test('every OpenAPI receipt in site/limits.cjs is quoted verbatim from the shipped OpenAPI', () => {
  const { LIMITS } = require(path.join(__dirname, '..', '..', 'site', 'limits.cjs'));
  const fs = require('fs');
  const dir = path.join(__dirname, '..', 'openapi');
  const text = fs.readdirSync(dir).map((f) => fs.readFileSync(path.join(dir, f), 'utf8')).join('\n');
  const missing = Object.entries(LIMITS).filter(([, v]) => v.receipt?.openapi && !text.includes(v.receipt.quote)).map(([k]) => k);
  assert.deepStrictEqual(missing, []);
  assert.ok(Object.values(LIMITS).every((v) => v.receipt && (v.receipt.openapi || v.receipt.measured || v.receipt.doc)), 'every limit carries a receipt');
});

test('sprint functions follow Jira Cloud: openSprints() is active only; EMPTY is unknown under NOT', () => withSite(async (site, call) => {
  const fid = site.pack.sprintFieldId;
  const states = (i) => (i.fields[fid] ?? []).map((s) => s.state);
  const visible = site.pack.issues.filter((i) => !i.hiddenFrom?.includes(site.pack.appAccountId));
  const futureOnly = visible.filter((i) => states(i).length && states(i).every((s) => s === 'future'));
  const active = visible.filter((i) => states(i).includes('active'));
  const closedOnly = visible.filter((i) => states(i).length && states(i).every((s) => s === 'closed'));
  const empty = visible.filter((i) => !states(i).length);
  assert.ok(futureOnly.length && active.length && closedOnly.length && empty.length, 'the pack has every sprint shape');
  const pick = [...futureOnly.slice(0, 3), ...active.slice(0, 3), ...closedOnly.slice(0, 3), ...empty.slice(0, 3)];
  const keys = pick.map((i) => i.key).join(', ');
  const run = async (clause) => {
    const r = await call('POST', '/rest/api/3/search/jql', { jql: `key in (${keys}) AND ${clause}`, fields: ['id'], maxResults: 100 });
    assert.strictEqual(r.status, 200, JSON.stringify(r.body));
    return new Set(r.body.issues.map((i) => i.id));
  };
  const ids = (list) => new Set(list.filter((i) => pick.includes(i)).map((i) => i.id));
  const has = (i, s) => states(i).includes(s);
  assert.deepStrictEqual(await run('sprint in openSprints()'), ids(pick.filter((i) => has(i, 'active'))));
  assert.deepStrictEqual(await run('sprint in futureSprints()'), ids(pick.filter((i) => has(i, 'future'))));
  assert.deepStrictEqual(await run('sprint in closedSprints()'), ids(pick.filter((i) => has(i, 'closed'))));
  // Measured on Jira Cloud: future-only issues ARE in `not in openSprints()`; issues with no sprint are in neither.
  const notOpen = ids(pick.filter((i) => states(i).length && !has(i, 'active')));
  assert.deepStrictEqual(await run('sprint not in openSprints()'), notOpen);
  assert.deepStrictEqual(await run('NOT sprint in openSprints()'), notOpen);
  assert.deepStrictEqual(await run('(sprint not in openSprints() OR sprint is EMPTY)'), ids(pick.filter((i) => !has(i, 'active'))));
}));

// Walk the shipped OpenAPI for every operation the site models: each answers 200/201 with only the top-level
// keys its documented response carries (schema properties, else the documented example), and serves the
// pagination shape its OWN parameters declare — token-paged operations answer isLast (+ nextPageToken while
// more remain) and never startAt/total; offset-paged ones carry isLast exactly when their schema has it.
test('every modelled operation answers the shape the shipped OpenAPI documents for it', () => withSite(async (site, call) => {
  const fs = require('fs');
  const { handlers: P } = require(path.join(__dirname, '..', '..', 'site', 'rest', 'platform.cjs'));
  const { handlers: A } = require(path.join(__dirname, '..', '..', 'site', 'rest', 'agile.cjs'));
  const specs = { jira: JSON.parse(fs.readFileSync(path.join(__dirname, '..', 'openapi', 'jira.json'), 'utf8')),
    jsw: JSON.parse(fs.readFileSync(path.join(__dirname, '..', 'openapi', 'jsw.json'), 'utf8')) };
  const deref = (spec, s) => { while (s && s.$ref) s = s.$ref.split('/').slice(1).reduce((o, k) => o[k], spec); return s; };
  const pack = site.pack;
  const scrum = pack.boards.find((b) => b.type === 'scrum');
  const sprint = pack.sprints.find((s) => s.state === 'active' && s.originBoardId === scrum.id);
  const issue = pack.issues.find((i) => !i.hiddenFrom?.length && i.projectKey === scrum.projectKey);
  const fill = { boardId: scrum.id, sprintId: sprint.id, issueIdOrKey: issue.key, projectIdOrKey: issue.projectKey };
  const query = { 'GET /rest/api/3/user': `accountId=${pack.users[0].accountId}`, 'GET /rest/api/3/user/bulk': `accountId=${pack.users[0].accountId}`,
    'GET /rest/api/3/search/jql': `jql=${encodeURIComponent(`project = ${issue.projectKey}`)}&maxResults=2`,
    'GET /rest/agile/1.0/issue/{issueIdOrKey}/estimation': `boardId=${scrum.id}`, 'GET /rest/api/3/mypermissions': 'permissions=BROWSE_PROJECTS' };
  const body = { 'POST /rest/api/3/issue/{issueIdOrKey}/changelog/list': { changelogIds: [1] },
    'POST /rest/api/3/issue/bulkfetch': { issueIdsOrKeys: [issue.key], fields: ['summary'] },
    'POST /rest/api/3/changelog/bulkfetch': { issueIdsOrKeys: [issue.key], maxResults: 1 },
    'POST /rest/api/3/search/jql': { jql: `project = ${issue.projectKey}`, maxResults: 2, fields: ['summary'] },
    'POST /rest/api/3/search/approximate-count': { jql: `project = ${issue.projectKey}` },
    'POST /rest/api/3/issue/{issueIdOrKey}/comment': { body: { type: 'doc', version: 1, content: [{ type: 'paragraph', content: [{ type: 'text', text: 'x' }] }] } },
    'POST /rest/api/3/permissions/check': { accountId: pack.users[0].accountId, projectPermissions: [{ issues: [Number(issue.id)], permissions: ['BROWSE_PROJECTS'] }] } };
  // Keys Jira Cloud answers beyond the OpenAPI text, each measured on a Jira Cloud site 2026-10-02 (kept: an app
  // written to the docs ignores them; the paging shape is what the docs decide).
  const MEASURED_EXTRA = {
    'GET /rest/agile/1.0/sprint/{sprintId}': ['createdDate'],
    'POST /rest/api/3/issue/bulkfetch': ['expand'],
    // The OpenAPI's 200 example for this deprecated operation is a single issue; Jira Cloud answers the agile page.
    'GET /rest/agile/1.0/sprint/{sprintId}/issue': ['startAt', 'maxResults', 'total', 'issues'],
  };
  const problems = [];
  let checked = 0;
  for (const key of Object.keys({ ...P, ...A }).sort()) {
    const [method, template] = key.split(' ');
    if (template === '/rest/api/3/search') continue; // removed from Jira Cloud: 410 (tested above)
    const spec = template.includes('/agile/') || template.includes('/software/') ? specs.jsw : specs.jira;
    const op = spec.paths[template]?.[method.toLowerCase()];
    if (!op) { problems.push(`${key}: modelled but absent from the shipped OpenAPI`); continue; }
    const url = template.replace(/\{(\w+)\}/g, (_, n) => encodeURIComponent(fill[n])) + (query[key] ? `?${query[key]}` : (op.parameters ?? []).some((p) => p.name === 'maxResults') ? '?maxResults=1' : '');
    const r = await call(method, url, body[key]);
    if (r.status !== 200 && r.status !== 201) { problems.push(`${key}: ${r.status} ${JSON.stringify(r.body).slice(0, 160)}`); continue; }
    const resp = op.responses[String(r.status)]?.content?.['application/json'];
    const schema = deref(spec, resp?.schema);
    let documented = schema?.properties ? Object.keys(schema.properties) : null;
    if (!documented && resp?.example) { try { const ex = JSON.parse(resp.example); if (ex && !Array.isArray(ex) && typeof ex === 'object') documented = Object.keys(ex); } catch { /* not JSON */ } }
    if (documented && r.body && typeof r.body === 'object' && !Array.isArray(r.body)) {
      const extra = Object.keys(r.body).filter((k) => !documented.includes(k) && !(MEASURED_EXTRA[key] ?? []).includes(k));
      if (extra.length) problems.push(`${key}: undocumented keys ${extra.join(',')}`);
    }
    const params = [...(op.parameters ?? []).map((p) => p.name), ...Object.keys(deref(spec, op.requestBody?.content?.['application/json']?.schema)?.properties ?? {})];
    if (params.includes('nextPageToken')) {
      if (typeof r.body.isLast !== 'boolean' && documented?.includes('isLast')) problems.push(`${key}: token-paged but no isLast`);
      if (r.body.isLast === false && !r.body.nextPageToken) problems.push(`${key}: isLast false without nextPageToken`);
      for (const k of ['startAt', 'total']) if (k in r.body && !documented?.includes(k)) problems.push(`${key}: token-paged but answers ${k}`);
    } else if (params.includes('startAt') && documented?.includes('isLast') && typeof r.body.isLast !== 'boolean') {
      problems.push(`${key}: schema documents isLast, response has none`);
    }
    checked++;
  }
  assert.deepStrictEqual(problems, []);
  assert.ok(checked >= 30, `checked ${checked} operations`);
}));

test('software 1.0 issue lists page by token exactly as Jira Cloud does', () => withSite(async (site, call) => {
  const scrum = site.pack.boards.find((b) => b.type === 'scrum');
  const all = await call('GET', `/rest/software/1.0/board/${scrum.id}/issue?maxResults=5000&fields=summary`);
  assert.deepStrictEqual(Object.keys(all.body), ['expand', 'issues', 'isLast']);
  const seen = [];
  let token = null;
  for (;;) {
    const r = await call('GET', `/rest/software/1.0/board/${scrum.id}/issue?maxResults=7&startAt=99${token ? `&nextPageToken=${encodeURIComponent(token)}` : ''}`);
    assert.strictEqual(r.status, 200);
    assert.ok(!('startAt' in r.body) && !('total' in r.body));
    seen.push(...r.body.issues.map((i) => i.id));
    if (r.body.isLast) { assert.ok(!('nextPageToken' in r.body)); break; }
    token = r.body.nextPageToken;
  }
  assert.deepStrictEqual(seen, all.body.issues.map((i) => i.id), 'startAt is ignored; the token walks every issue once');
  const first = await call('GET', `/rest/software/1.0/board/${scrum.id}/issue`);
  assert.strictEqual(first.body.issues.length, Math.min(50, all.body.issues.length));
  const count = await call('GET', `/rest/software/1.0/board/${scrum.id}/issue/approximate-count`);
  assert.deepStrictEqual(count.body, { count: all.body.issues.length });
  const agile = await call('GET', `/rest/agile/1.0/board/${scrum.id}/issue?maxResults=3`);
  assert.deepStrictEqual(Object.keys(agile.body), ['expand', 'startAt', 'maxResults', 'total', 'issues']);
}));

test('the live-UI changes stay held back: never in the delivery plan or the flush, applied only when delivered', () => withSite(async (site) => {
  const ui = site.pack.live.filter((e) => e.delivery.liveUi).map((e) => e.changelogId);
  assert.strictEqual(ui.length, 2);
  const delivered = [];
  for (let d = site.control.next(); !d.done; d = site.control.next()) delivered.push(...d.applied);
  site.flushLive();
  assert.ok(ui.every((id) => !site.state.st.applied.has(id)), 'neither the script nor the flush applies them');
  assert.ok(site.pack.live.filter((e) => !e.delivery.liveUi).every((e) => site.state.st.applied.has(e.changelogId)));
  const r = site.control.event({ changelogId: ui[0] });
  assert.deepStrictEqual(r.applied, [ui[0]], 'delivering one applies exactly that change');
}));

test('the default viewer may not comment on one issue per seed: Jira\'s measured 400, discoverable in info', async () => {
  for (const seed of ['feedfacefeedface', '0123456789abcdef', '5eed0123456789ab']) {
    const site = await createSite({ seed, port: 0 });
    try {
      const forbidden = site.control.info().commentForbidden;
      assert.strictEqual(forbidden.length, 1);
      assert.deepStrictEqual(forbidden[0].accountIds, [site.pack.viewer]);
      const viewerName = site.pack.users.find((u) => u.accountId === site.pack.viewer).displayName;
      const adf = { type: 'doc', version: 1, content: [{ type: 'paragraph', content: [{ type: 'text', text: 'x' }] }] };
      const post = async (key, who, body) => {
        const r = await fetch(`${site.url}/rest/api/3/issue/${key}/comment`, { method: 'POST', headers: { 'content-type': 'application/json',
          'x-forge-as': 'user', 'x-forge-account': who, 'x-forge-scopes': JSON.stringify(SCOPES) }, body: JSON.stringify({ body }) });
        return { status: r.status, body: await r.json() };
      };
      const denied = await post(forbidden[0].issueKey, site.pack.viewer, adf);
      assert.deepStrictEqual(denied, { status: 400, body: { errorMessages: [`${viewerName}, you do not have the permission to comment on this issue.`], errors: {} } });
      const other = site.pack.users.find((u) => u.accountId !== site.pack.viewer && !site.pack.issues.find((i) => i.key === forbidden[0].issueKey).hiddenFrom.includes(u.accountId));
      assert.strictEqual((await post(forbidden[0].issueKey, other.accountId, adf)).status, 201, 'another user may comment there');
      assert.strictEqual(site.comments.length, 1);
    } finally {
      await site.stop();
    }
  }
});
