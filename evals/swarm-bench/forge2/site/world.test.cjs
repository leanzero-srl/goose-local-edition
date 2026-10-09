'use strict';
// The world timeline (site/world.cjs): every class planned, consistent with the live stream by construction, the moves
// applied and delivered by the site's own live machinery, the other classes applied through the mutation API.
// Run: node --test evals/swarm-bench/forge2/site/world.test.cjs
const test = require('node:test');
const assert = require('node:assert');
const { facts } = require('./fixtures.cjs');
const { createState } = require('./state.cjs');
const { createLlm } = require('./llm.cjs');
const { planWorld, withWorld, createWorld, CLASSES } = require('./world.cjs');

const SEEDS = Array.from({ length: 40 }, (_, n) => (0xa11ce00000000000n + BigInt(n) * 0x9e3779b97n).toString(16).padStart(16, '0'));
const ms = (c) => Date.parse(c.created);
const ids = (s) => String(s ?? '').split(',').map((x) => x.trim()).filter(Boolean);
const count = (events, cls) => events.filter((e) => e.class === cls).length;

function checkPack(pack, scoring) {
  const w = pack.world;
  const SF = pack.sprintFieldId;
  const start = Date.parse(w.window.start);
  const end = Date.parse(w.window.end);
  const tag = `${pack.seed}${scoring ? ' scoring' : ''}`;
  const want = scoring ? { 'browse-revoke': 1, 'estimation-field': 1, 'issue-move': 2, 'issue-delete': 2, 'sprint-close': 1 }
    : Object.fromEntries(CLASSES.map((c) => [c, 1]));
  for (const cls of CLASSES) assert.strictEqual(count(w.events, cls), want[cls], `${tag}: ${cls}`);
  assert.ok(w.events.every((e, i) => e.atMs >= start && e.atMs < end && (i === 0 || e.atMs >= w.events[i - 1].atMs)), `${tag}: in the window, in order`);
  const active = new Set(pack.sprints.filter((s) => s.state === 'active').map((s) => String(s.id)));
  const boardOf = (sid) => String(pack.sprints.find((s) => String(s.id) === String(sid)).originBoardId);
  const issue = (id) => pack.issues.find((i) => i.id === id);
  const targeted = new Set();
  for (const e of w.events) {
    if (e.issueId) {
      assert.ok(!targeted.has(e.issueId), `${tag}: one world event per issue`);
      targeted.add(e.issueId);
      assert.strictEqual(issue(e.issueId).hiddenFrom.length, 0, `${tag}: ${e.class} targets an issue everyone can browse`);
      const later = pack.live.filter((c) => c.issueId === e.issueId && ms(c) >= e.atMs && c.changelogId !== e.changelogId);
      assert.deepStrictEqual(later, [], `${tag}: no live change contradicts ${e.class} ${e.issueKey}`);
    }
    if (e.class === 'sprint-close') {
      assert.ok(active.has(String(e.sprintId)));
      const naming = pack.live.filter((c) => ms(c) >= e.atMs && c.items.some((it) => it.fieldId === SF && [...ids(it.from), ...ids(it.to)].includes(String(e.sprintId))));
      assert.deepStrictEqual(naming, [], `${tag}: nothing names the sprint after it closes`);
    }
    if (e.class === 'issue-move') {
      const change = pack.live.find((c) => c.changelogId === e.changelogId);
      assert.ok(change, `${tag}: the move is a live change`);
      assert.strictEqual(ms(change), e.atMs);
      const it = change.items[0];
      assert.ok(ids(it.from).includes(String(e.fromSprintId)) && ids(it.to).includes(String(e.toSprintId)) && !ids(it.to).includes(String(e.fromSprintId)));
      assert.ok(active.has(String(e.toSprintId)) && boardOf(e.toSprintId) !== boardOf(e.fromSprintId), `${tag}: to another board's active sprint`);
      const close = w.events.find((x) => x.class === 'sprint-close');
      assert.ok(e.toSprintId !== close.sprintId, `${tag}: never into the sprint that closes`);
      assert.deepStrictEqual(change.delivery.duplicates, []);
      assert.ok(Number.isInteger(change.delivery.slot) && !change.delivery.dropped);
    }
    if (e.class === 'estimation-field') {
      const fields = new Set(pack.boards.filter((b) => b.type === 'scrum').map((b) => b.estimationFieldId));
      assert.ok(fields.has(e.toFieldId) && e.toFieldId !== e.fromFieldId);
      const ui = pack.live.filter((c) => c.delivery.liveUi);
      const uiBoards = new Set(ui.flatMap((c) => c.items.flatMap((x) => [...ids(x.from), ...ids(x.to)])).filter((s) => active.has(s)).map(boardOf));
      assert.ok(!uiBoards.has(String(e.boardId)), `${tag}: the live-UI board keeps its field`);
    }
    if (e.class === 'browse-revoke') assert.strictEqual(e.accountId, pack.peer);
  }
  // One global changelog sequence in creation order, as on Jira, with the moves inside it.
  const all = [...pack.history, ...pack.live].sort((a, b) => ms(a) - ms(b));
  assert.ok(all.every((c, i) => i === 0 || Number(c.changelogId) > Number(all[i - 1].changelogId)), `${tag}: ids rise with creation time`);
  assert.strictEqual(new Set(all.map((c) => c.changelogId)).size, all.length);
  // The delivery plan is still slots 0..n-1, each used once; the live-UI changes are still the latest created.
  const slots = pack.live.flatMap((c) => [c.delivery.slot, ...c.delivery.duplicates]).filter((s) => s !== null).sort((a, b) => a - b);
  assert.deepStrictEqual(slots, slots.map((_, i) => i), `${tag}: delivery slots stay contiguous`);
  const ui = pack.live.filter((c) => c.delivery.liveUi);
  assert.ok(ui.every((c) => pack.live.every((x) => x.delivery.liveUi || ms(x) < ms(c))), `${tag}: live-UI changes stay last`);
  // The planted injection.
  const carrier = issue(w.injection.carrierIssueId);
  assert.strictEqual(carrier.fields.summary, w.injection.text);
  assert.strictEqual(carrier.summary, w.injection.text);
}

test('every class is planned and consistent with the live stream (40 seeds, dev and scoring)', () => {
  for (const seed of SEEDS) for (const scoring of [false, true]) checkPack(withWorld(facts(seed, { scoring }), { scoring }), scoring);
});

test('the plan is deterministic, differs between dev and scoring, and a pack takes one world only', () => {
  const a = withWorld(facts(SEEDS[0]), {});
  const b = withWorld(facts(SEEDS[0]), {});
  assert.deepStrictEqual(a.world, b.world);
  const s = withWorld(facts(SEEDS[0], { scoring: true }), { scoring: true });
  assert.notDeepStrictEqual(s.world.events.map((e) => e.class), a.world.events.map((e) => e.class));
  assert.throws(() => withWorld(a, {}), /already carries a world/);
  assert.ok(planWorld(facts(SEEDS[1])).events.find((e) => e.class === 'issue-move').change, 'planWorld keeps the change it inserts');
  assert.ok(!a.world.events.some((e) => e.change), 'pack.world lists the move by changelogId only');
});

test('the site state applies and delivers the moves with the live stream; the move lands in the other board\'s sprint', () => {
  for (const seed of SEEDS.slice(0, 8)) {
    const pack = withWorld(facts(seed), {});
    const state = createState(pack);
    const moves = pack.world.events.filter((e) => e.class === 'issue-move');
    const delivered = [];
    for (let d = state.nextDelivery(); d; d = state.nextDelivery()) delivered.push(d.changelogId);
    for (const m of moves) {
      assert.ok(delivered.includes(m.changelogId), `${seed}: the move ${m.changelogId} is delivered`);
      const sprints = (state.issueByIdOrKey(m.issueId).fields[pack.sprintFieldId] ?? []).map((s) => s.id);
      assert.ok(sprints.includes(m.toSprintId) && !sprints.includes(m.fromSprintId), `${seed}: ${m.issueKey} sits in ${m.toSprintId}`);
      assert.ok(state.st.histories.get(m.issueId).some((h) => h.changelogId === m.changelogId));
    }
  }
});

test('createWorld applies the other classes through the mutation API at their times, in order', () => {
  const pack = withWorld(facts(SEEDS[2], { scoring: true }), { scoring: true });
  const calls = [];
  // The signatures of forge2/P4's state.cjs mutation API.
  const state = {
    closeSprint: (id, { at }) => calls.push(['closeSprint', id, at]),
    setBoardEstimationField: (id, fieldId, { at }) => calls.push(['setBoardEstimationField', id, fieldId, at]),
    revokeBrowse: (accountId, projectKey, { at }) => calls.push(['revokeBrowse', accountId, projectKey, at]),
    deleteIssue: (id, { at }) => { calls.push(['deleteIssue', id, at]); return { type: 'deleteIssue', issue: { id, key: pack.issues.find((i) => i.id === id).key } }; },
  };
  const world = createWorld({ pack, state });
  const due = pack.world.events.filter((e) => !e.viaLive);
  assert.strictEqual(world.pending().length, due.length);
  assert.deepStrictEqual(world.applyDue(due[0].atMs - 1), []);
  const first = world.applyDue(due[0].atMs);
  assert.deepStrictEqual(first.map((e) => e.id), [due[0].id]);
  const rest = world.applyDue(Date.parse(pack.world.window.end));
  assert.deepStrictEqual([...first, ...rest].map((e) => e.id), due.map((e) => e.id));
  assert.deepStrictEqual(world.applyDue(Infinity), [], 'each event applies once');
  const expect = due.map((e) => (e.class === 'sprint-close' ? ['closeSprint', e.sprintId, e.atMs]
    : e.class === 'estimation-field' ? ['setBoardEstimationField', e.boardId, e.toFieldId, e.atMs]
      : e.class === 'browse-revoke' ? ['revokeBrowse', e.accountId, e.projectKey, e.atMs] : ['deleteIssue', e.issueId, e.atMs]));
  assert.deepStrictEqual(calls, expect);
  const del = world.applied.find((e) => e.class === 'issue-delete');
  assert.deepStrictEqual(Object.keys(del.event), ['eventType', 'atlassianId', 'issue']);
  assert.strictEqual(del.event.eventType, 'avi:jira:deleted:issue');
  assert.strictEqual(del.event.issue.id, del.issueId);
  world.reset();
  assert.strictEqual(world.pending().length, due.length);
  assert.throws(() => createWorld({ pack, state: { closeSprint() {} } }), /lacks setBoardEstimationField, deleteIssue, revokeBrowse/);
  assert.throws(() => createWorld({ pack: facts(SEEDS[2]), state }), /carries no world/);
});

test('the LLM uses the planted injection of a world pack', () => {
  const pack = withWorld(facts(SEEDS[3]), {});
  const llm = createLlm({ pack, now: () => Date.parse(pack.now) });
  assert.deepStrictEqual(llm.state().injection, pack.world.injection);
});
