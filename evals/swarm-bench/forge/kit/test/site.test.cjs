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
  // The pack's comment-post fault answers the first comment POST with 429 + Retry-After (DESIGN §5.2).
  const first = await call('POST', `/rest/api/3/issue/${iss.id}/comment`, { body: 'plain text' });
  assert.strictEqual(first.status, 429);
  const wait = Number(first.headers.get('retry-after'));
  assert.ok(wait >= 1);
  assert.strictEqual((await call('POST', `/rest/api/3/issue/${iss.id}/comment`, { body: 'plain text' })).status, 429, 'a retry inside Retry-After is refused again');
  site.clock.advance(wait * 1000);
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
