'use strict';
// Relative-date JQL counts from the REQUEST's virtual instant (x-forge-vtime, the invocation's clock), never from the
// site's clock, which other invocations, world events and the probe move ahead (SPEC §2.2). Through a live site:
// /search/jql and an agile issue list's jql; and compile's `timed`, which keys the cached hits on the instant.
// Run: node --test evals/swarm-bench/forge2/site/jql.test.cjs
const test = require('node:test');
const assert = require('node:assert');
const { createSite } = require('./site.cjs');
const { compile } = require('./jql.cjs');

const SCOPES = ['read:jira-work', 'read:board-scope:jira-software', 'read:issue-details:jira', 'read:jql:jira'];
const MINUTE = 60_000;

test('relative dates and now() count from the request\'s instant while the site\'s clock is an hour later', async () => {
  const site = await createSite({ seed: '0123456789abcdef', port: 0 });
  try {
    const { pack, state } = site;
    const call = async (method, p, vt, body) => {
      const res = await fetch(site.url + p, { method, body: body && JSON.stringify(body), headers: { 'content-type': 'application/json', 'x-forge-as': 'app',
        'x-forge-scopes': JSON.stringify(SCOPES), 'x-forge-vtime': String(vt), 'x-forge-module-type': 'consumer' } });
      return { status: res.status, body: await res.json() };
    };
    // The issue updated last, and a request one minute after that update, made while the site is an hour ahead.
    const last = state.allIssues().reduce((a, b) => (Date.parse(b.fields.updated) > Date.parse(a.fields.updated) ? b : a));
    const vt = Date.parse(last.fields.updated) + MINUTE;
    state.advanceTo(Math.max(state.now(), vt) + 60 * MINUTE);
    const search = async (jql, at) => {
      const r = await call('POST', '/rest/api/3/search/jql', at, { jql, fields: ['id'], maxResults: 5000 });
      assert.strictEqual(r.status, 200, JSON.stringify(r.body));
      return r.body.issues.map((i) => i.id);
    };
    assert.deepStrictEqual(await search('updated >= -2m', vt), [last.id], 'two minutes back from the request, not from the site');
    assert.deepStrictEqual(await search('updated >= -2m', vt + 10 * MINUTE), [], 'the same query ten minutes later: the cache keys on the instant');
    assert.ok((await search('updated <= now()', vt)).includes(last.id));
    assert.ok(!(await search('updated <= now()', vt - 2 * MINUTE)).includes(last.id), 'now() is the request\'s instant');
    const board = pack.boards.find((b) => b.projectKey === last.projectKey);
    const agile = await call('GET', `/rest/agile/1.0/board/${board.id}/issue?fields=summary&jql=${encodeURIComponent('updated >= -2m')}`, vt);
    assert.strictEqual(agile.status, 200, JSON.stringify(agile.body));
    assert.deepStrictEqual(agile.body.issues.map((i) => i.id), [last.id], 'agile jql counts from the request too');
  } finally {
    await site.stop();
  }
});

test('compile says whether a query read the clock', () => {
  const site = { fields: [], sprints: [{ id: 1, name: 'S1', state: 'active' }], statuses: [], issueTypes: [], projects: [], sprintFieldId: 'customfield_1' };
  let reads = 0;
  const model = { ...site, fields: [{ id: 'customfield_1', name: 'Sprint', custom: true, schema: { custom: 'com.pyxis.greenhopper.jira:gh-sprint' } }],
    now: () => { reads += 1; return Date.UTC(2026, 9, 20); } };
  assert.strictEqual(compile('sprint = 1', model).timed, false);
  assert.strictEqual(reads, 0);
  for (const jql of ['updated >= -1d', 'created <= now()', 'updated >= startOfDay(-1)', 'created >= "2026/10/01"']) assert.strictEqual(compile(jql, model).timed, true, jql);
});
