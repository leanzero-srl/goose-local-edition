'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const { facts, worldPack } = require('./fixtures.cjs');
const { createState } = require('./state.cjs');
const { createWorld } = require('./world.cjs');
const { createSite } = require('./site.cjs');
const SCOPES = ['read:jira-work', 'read:jira-user', 'read:board-scope:jira-software',
  'read:sprint:jira-software', 'read:issue-details:jira', 'read:jql:jira', 'read:project:jira',
  'read:board-scope.admin:jira-software', 'read:issue:jira-software'];
const iso = (at) => new Date(at).toISOString();

async function fixture(mode, fn) {
  const site = await createSite({ pack: facts('0123456789abcdef'), port: 0 });
  const base = site.state.now();
  const at = mode === 'default' ? undefined : mode === 'lagged-string' ? String(base + 60_000) : base + (mode === 'immediate' ? 0 : 60_000);
  if (mode.startsWith('lagged')) site.state.advance(5 * 60_000);
  const before = site.state.now();
  const expectedAt = at === undefined ? before : Number(at);
  const call = async (path, account) => {
    const headers = { 'x-forge-as': account ? 'user' : 'app', 'x-forge-scopes': JSON.stringify(SCOPES) };
    if (account) headers['x-forge-account'] = account;
    const r = await fetch(site.url + path, { headers });
    return { status: r.status, body: await r.json() };
  };
  try {
    await fn({ ...site, call, at, expectedAt });
    assert.equal(site.state.now(), Math.max(before, expectedAt), 'observation clock remains monotonic');
    assert.equal(site.state.st.world.at(-1).at, iso(expectedAt), 'mutation record keeps requested creation time');
  } finally { await site.stop(); }
}

for (const mode of ['future', 'lagged', 'lagged-string', 'immediate', 'default']) {
  test(`move retains its creation instant and real issue/changelog values (${mode})`, () => fixture(mode, async (f) => {
    const active = f.pack.sprints.filter((s) => s.state === 'active');
    const from = active[0];
    const to = active.find((s) => s.originBoardId !== from.originBoardId);
    const issue = f.pack.issues.find((i) => f.state.liveRemainingFor(i.id) === 0 &&
      (i.fields[f.pack.sprintFieldId] ?? []).some((s) => s.id === from.id));
    const path = `/rest/api/3/issue/${issue.id}?fields=${f.pack.sprintFieldId}`;
    const before = await f.call(path); assert.equal(before.status, 200);
    assert.ok(before.body.fields[f.pack.sprintFieldId].some((s) => s.id === from.id));
    const moved = f.control.moveissue({ issue: issue.id, sprintId: to.id, authorId: f.pack.users[0].accountId, at: f.at, estimate: 8 });
    assert.equal(moved.change.created, iso(f.expectedAt));
    assert.equal(moved.events[0].change.created, iso(f.expectedAt));
    const after = await f.call(path); assert.equal(after.status, 200);
    assert.ok(after.body.fields[f.pack.sprintFieldId].some((s) => s.id === to.id));
    assert.ok(!after.body.fields[f.pack.sprintFieldId].some((s) => s.id === from.id));
    const field = f.state.board(to.originBoardId).estimationFieldId;
    assert.equal((await f.call(`/rest/api/3/issue/${issue.id}?fields=${field}`)).body.fields[field], 8);
    const log = await f.call(`/rest/api/3/issue/${issue.id}/changelog?maxResults=100`);
    assert.equal(Date.parse(log.body.values.at(-1).created), f.expectedAt);
  }));

  test(`close date and all carried changes retain the creation instant (${mode})`, () => fixture(mode, async (f) => {
    const sprint = f.pack.sprints.find((s) => s.state === 'active' && f.pack.sprints.some((t) => t.state === 'future' && t.originBoardId === s.originBoardId));
    const future = f.pack.sprints.find((s) => s.state === 'future' && s.originBoardId === sprint.originBoardId);
    const before = await f.call(`/rest/agile/1.0/sprint/${sprint.id}`);
    assert.equal(before.status, 200); assert.equal(before.body.state, 'active');
    const result = f.control.closesprint({ sprintId: sprint.id, carryTo: future.id, authorId: f.pack.users[0].accountId, at: f.at });
    assert.ok(result.carried.length > 0);
    assert.ok(result.carried.every((c) => c.created === iso(f.expectedAt)));
    assert.ok(result.events.every((e) => e.change.created === iso(f.expectedAt)));
    const after = await f.call(`/rest/agile/1.0/sprint/${sprint.id}`);
    assert.equal(after.body.state, 'closed'); assert.equal(after.body.completeDate, iso(f.expectedAt));
    const member = result.carried[0];
    const issue = await f.call(`/rest/api/3/issue/${member.issueId}?fields=${f.pack.sprintFieldId}`);
    assert.equal(issue.body.fields[f.pack.sprintFieldId].find((s) => s.id === sprint.id).completeDate, iso(f.expectedAt));
    assert.ok(issue.body.fields[f.pack.sprintFieldId].some((s) => s.id === future.id));
  }));

  test(`estimation switch retains its creation instant and real configuration (${mode})`, () => fixture(mode, async (f) => {
    const board = f.pack.boards.find((b) => b.type === 'scrum');
    const field = f.pack.boards.find((b) => b.type === 'scrum' && b.estimationFieldId !== board.estimationFieldId).estimationFieldId;
    const path = `/rest/agile/1.0/board/${board.id}/configuration`;
    assert.equal((await f.call(path)).body.estimation.field.fieldId, board.estimationFieldId);
    const result = f.control.estimationfield({ boardId: board.id, fieldId: field, at: f.at });
    assert.equal(result.at, iso(f.expectedAt));
    assert.equal((await f.call(path)).body.estimation.field.fieldId, field);
  }));

  test(`deletion retains its creation instant and removes the actual issue (${mode})`, () => fixture(mode, async (f) => {
    const issue = f.pack.issues.find((i) => f.state.liveRemainingFor(i.id) === 0 && !i.hiddenFrom.length);
    const path = `/rest/api/3/issue/${issue.id}`;
    assert.equal((await f.call(path)).status, 200);
    const result = f.control.deleteissue({ issue: issue.id, at: f.at });
    assert.equal(result.at, iso(f.expectedAt)); assert.equal(result.snapshot.id, issue.id);
    assert.equal(f.state.st.deleted.get(issue.id).at, iso(f.expectedAt));
    assert.equal((await f.call(path)).status, 404);
  }));

  test(`Browse revocation retains its creation instant and real caller boundary (${mode})`, () => fixture(mode, async (f) => {
    const issue = f.pack.issues.find((i) => !i.hiddenFrom.length);
    const path = `/rest/api/3/issue/${issue.id}`;
    assert.equal((await f.call(path, f.pack.viewer)).status, 200);
    const result = f.control.revokebrowse({ accountId: f.pack.viewer, projectKey: issue.projectKey, at: f.at });
    assert.equal(result.at, iso(f.expectedAt));
    assert.equal((await f.call(path, f.pack.viewer)).status, 404);
    assert.equal((await f.call(path)).status, 200);
    const peer = f.pack.users.find((u) => u.accountId !== f.pack.viewer).accountId;
    assert.equal((await f.call(path, peer)).status, 200);
    const p = await f.call(`/rest/api/3/mypermissions?permissions=BROWSE_PROJECTS&projectKey=${issue.projectKey}`, f.pack.viewer);
    assert.equal(p.body.permissions.BROWSE_PROJECTS.havePermission, false);
  }));
}

test('late world processing keeps every planned event instant in actual seeded state', () => {
  for (const seed of ['3c04f61f28a4a511', '936027a50d545ae6', 'bac25cab1aed9e26', 'c299c14673c45358', '1ff91b71737b133e']) {
    const pack = worldPack(seed, { scoring: true });
    const state = createState(pack); const world = createWorld({ pack, state });
    state.setWorldHook((t) => world.applyDue(t));
    const observed = Date.parse(pack.world.window.end) + 60_000;
    state.advanceTo(observed);
    state.applyUntil(Date.parse(pack.world.window.end));
    assert.equal(state.now(), observed);
    assert.equal(world.pending().length, 0);
    for (const event of world.applied) {
      assert.equal(event.result.at, event.at);
      if (event.class === 'sprint-close') assert.equal(state.sprint(event.sprintId).completeDate, event.at);
      if (event.class === 'estimation-field') assert.equal(state.board(event.boardId).estimationFieldId, event.toFieldId);
      if (event.class === 'issue-delete') {
        assert.equal(state.st.deleted.get(event.issueId).at, event.at);
        assert.equal(state.issueByIdOrKey(event.issueId), undefined);
      }
      if (event.class === 'browse-revoke') {
        const issue = state.allIssues().find((i) => i.projectKey === event.projectKey && !i.hiddenFrom.length);
        assert.equal(state.canBrowse(event.accountId, issue), false);
        assert.equal(state.canBrowse(pack.appAccountId, issue), true);
      }
    }
  }
});

test('seed4 post-close reestimate is disclosed after the same close the oracle planned', async () => {
  const pack = worldPack('c299c14673c45358', { scoring: true });
  const site = await createSite({ pack, port: 0 });
  try {
    const close = pack.world.events.find((e) => e.class === 'sprint-close');
    const changed = pack.live.find((h) => h.issueId === '15044' && h.items.some((i) => i.fieldId === 'customfield_19818'));
    const observedAt = Date.parse('2026-10-10T12:17:35.545Z');
    site.state.advanceTo(observedAt);
    site.control.world({ until: Date.parse(changed.created) });
    const headers = { 'x-forge-as': 'app', 'x-forge-scopes': JSON.stringify(SCOPES) };
    const response = await fetch(`${site.url}/rest/api/3/issue/${changed.issueId}?fields=${pack.sprintFieldId},customfield_19818&expand=changelog`, { headers });
    assert.equal(response.status, 200);
    const issue = await response.json();
    const closed = issue.fields[pack.sprintFieldId].find((s) => s.id === close.sprintId);
    const history = issue.changelog.histories.find((h) => h.id === changed.changelogId);
    assert.equal(closed.completeDate, close.at);
    assert.ok(Date.parse(history.created) > Date.parse(closed.completeDate));
    assert.equal(issue.fields.customfield_19818, 5);
    assert.equal(history.items.find((i) => i.fieldId === 'customfield_19818').from, '2');
    assert.equal(site.state.now(), observedAt);
  } finally { await site.stop(); }
});
