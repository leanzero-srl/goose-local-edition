'use strict';
// Board and sprint visibility (site/rest/agile.cjs) through a live site: a person who loses browse permission on a
// project no longer sees its boards in the board list, nor the boards, their sprints, issues or configuration by id;
// a sprint stays visible to them only while one of its issues is (jsw.json GET /sprint/{sprintId}). The app and other
// people keep seeing everything.
// Run: node --test evals/swarm-bench/forge2/site/rest/agile.test.cjs
const test = require('node:test');
const assert = require('node:assert');
const { createSite } = require('../site.cjs');

const SCOPES = ['read:jira-work', 'read:board-scope:jira-software', 'read:sprint:jira-software', 'read:project:jira',
  'read:board-scope.admin:jira-software', 'read:issue-details:jira', 'read:jql:jira'];
const NO_BOARD = 'Board does not exist or you do not have permission to see it.';

async function withSite(fn) {
  const site = await createSite({ seed: '0123456789abcdef', port: 0 });
  let vt = site.state.now();
  // as: 'app' | <accountId>; each call 10 s after the previous one, so the burst bucket never refuses.
  const call = async (p, as = 'app') => {
    vt = Math.max(vt + 10_000, site.state.now());
    const headers = { 'x-forge-as': as === 'app' ? 'app' : 'user', 'x-forge-scopes': JSON.stringify(SCOPES), 'x-forge-vtime': String(vt),
      ...(as === 'app' ? {} : { 'x-forge-account': as, 'x-forge-source': 'resolver', 'x-forge-module-type': 'dashboards:widget' }) };
    const res = await fetch(site.url + p, { headers });
    return { status: res.status, body: await res.json() };
  };
  try { await fn(site, call); } finally { await site.stop(); }
}

test('after a browse revoke the project\'s boards and sprints leave the person\'s board lists and reads', () => withSite(async (site, call) => {
  const { pack } = site;
  const viewer = pack.viewer;
  const project = pack.boards.find((b) => b.type === 'scrum').projectKey;
  const lost = pack.boards.filter((b) => b.projectKey === project);
  const kept = pack.boards.filter((b) => b.projectKey !== project);
  const scrum = lost.find((b) => b.type === 'scrum');
  const fid = pack.sprintFieldId;
  const sprint = pack.sprints.find((s) => s.originBoardId === scrum.id && s.state === 'active');
  const ids = (r) => r.body.values.map((b) => b.id);
  assert.deepStrictEqual(ids(await call('/rest/agile/1.0/board', viewer)), pack.boards.map((b) => b.id).sort((a, b) => a - b));
  assert.strictEqual((await call(`/rest/agile/1.0/sprint/${sprint.id}`, viewer)).status, 200);

  site.control.revokebrowse({ accountId: viewer, projectKey: project });
  const list = await call('/rest/agile/1.0/board', viewer);
  assert.deepStrictEqual(ids(list), kept.map((b) => b.id).sort((a, b) => a - b), 'the board list leaves the project out');
  assert.strictEqual(list.body.total, kept.length);
  assert.deepStrictEqual(ids(await call(`/rest/agile/1.0/board?projectKeyOrId=${project}`, viewer)), []);
  for (const p of [`board/${scrum.id}`, `board/${scrum.id}/sprint`, `board/${scrum.id}/configuration`, `board/${scrum.id}/issue`,
    `board/${scrum.id}/backlog`, `board/${scrum.id}/sprint/${sprint.id}/issue`]) {
    const r = await call(`/rest/agile/1.0/${p}`, viewer);
    assert.deepStrictEqual([r.status, r.body.errorMessages], [404, [NO_BOARD]], p);
  }
  // The sprint's issues are all in the lost project: the sprint is hidden too.
  assert.ok(!site.state.allIssues().some((i) => (i.fields[fid] ?? []).some((s) => s.id === sprint.id) && site.state.canBrowse(viewer, i)));
  for (const p of [`sprint/${sprint.id}`, `sprint/${sprint.id}/issue`]) assert.strictEqual((await call(`/rest/agile/1.0/${p}`, viewer)).status, 404, p);

  // Everyone else, and the app, still see the project's boards and sprints.
  const peer = pack.users.find((u) => u.accountId !== viewer).accountId;
  assert.deepStrictEqual(ids(await call('/rest/agile/1.0/board', peer)), pack.boards.map((b) => b.id).sort((a, b) => a - b));
  assert.deepStrictEqual(ids(await call('/rest/agile/1.0/board')), pack.boards.map((b) => b.id).sort((a, b) => a - b));
  assert.strictEqual((await call(`/rest/agile/1.0/board/${scrum.id}/sprint`)).status, 200);
  assert.strictEqual((await call(`/rest/agile/1.0/sprint/${sprint.id}`)).status, 200);

  // An issue the person can still browse joins the sprint: the sprint is visible again (its board is not).
  const visible = pack.issues.find((i) => i.projectKey !== project && !i.hiddenFrom.length && site.state.liveRemainingFor(i.id) === 0
    && !(i.fields[fid] ?? []).some((s) => s.id === sprint.id));
  site.control.moveissue({ issue: visible.key, sprintId: sprint.id, authorId: pack.users[0].accountId });
  const back = await call(`/rest/agile/1.0/sprint/${sprint.id}`, viewer);
  assert.deepStrictEqual([back.status, back.body.id], [200, sprint.id]);
  const its = await call(`/rest/agile/1.0/sprint/${sprint.id}/issue?fields=summary`, viewer);
  assert.deepStrictEqual(its.body.issues.map((i) => i.key), [visible.key], 'only the issue the person can browse');
  assert.strictEqual((await call(`/rest/agile/1.0/board/${scrum.id}`, viewer)).status, 404);
}));
