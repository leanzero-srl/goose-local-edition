'use strict';
// The Forge 2.0 pack generator (SPEC §2.3, forge2/site/fixtures.cjs): deterministic per seed (byte-identical reruns in
// separate processes), seeds differ in every id class, and every pack carries the 2.0 scale and the shapes the checks
// need. The pack's acceptance by the scoring oracle is forge2_oracle's own test (bench/).
//   node --test forge2/kit/test/fixtures.test.cjs
const test = require('node:test');
const assert = require('node:assert');
const crypto = require('crypto');
const path = require('path');
const { execFileSync } = require('child_process');

const FIXTURES = path.join(__dirname, '..', '..', 'site', 'fixtures.cjs');
const { facts, sprintMoves, V1_WINDOW } = require(FIXTURES);
const SEEDS = ['0123456789abcdef', 'deadbeefcafef00d', '5eed5eed5eed5eed'];
const seedOf = (i) => crypto.createHash('sha256').update(`forge2-fixture-${i}`).digest('hex').slice(0, 16);

test('byte-identical reruns per seed, in separate processes', () => {
  for (const seed of SEEDS.slice(0, 2)) {
    const a = execFileSync(process.execPath, [FIXTURES, '--seed', seed], { encoding: 'utf8', maxBuffer: 64 << 20 });
    const b = execFileSync(process.execPath, [FIXTURES, '--seed', seed], { encoding: 'utf8', maxBuffer: 64 << 20 });
    assert.strictEqual(crypto.createHash('sha256').update(a).digest('hex'), crypto.createHash('sha256').update(b).digest('hex'), seed);
    assert.strictEqual(a, JSON.stringify(facts(seed), null, 1) + '\n');
  }
});

test('refuses a malformed seed', () => {
  assert.throws(() => facts('XYZ'), /16 lowercase hex/);
  assert.throws(() => facts('0123456789ABCDEF'), /16 lowercase hex/);
});

test('three seeds differ in every id class', () => {
  const packs = SEEDS.map((s) => facts(s));
  const classes = {
    cloudId: (p) => [p.cloudId], siteUrl: (p) => [p.siteUrl], appAccountId: (p) => [p.appAccountId],
    userAccountIds: (p) => p.users.map((u) => u.accountId), viewer: (p) => [p.viewer], peer: (p) => [p.peer], admins: (p) => p.admins,
    projectIds: (p) => p.projects.map((x) => x.id), projectKeys: (p) => p.projects.map((x) => x.key),
    boardIds: (p) => p.boards.map((b) => b.id), sprintIds: (p) => p.sprints.map((s) => s.id), sprintNames: (p) => p.sprints.map((s) => s.name),
    issueIds: (p) => p.issues.map((i) => i.id), issueKeys: (p) => p.issues.map((i) => i.key),
    customFieldIds: (p) => p.fields.filter((f) => f.custom).map((f) => f.id), sprintFieldId: (p) => [p.sprintFieldId],
    scopeStatusFieldId: (p) => [p.scopeStatusFieldId],
    estimationFieldIds: (p) => p.boards.map((b) => b.estimationFieldId), statusIds: (p) => p.statuses.map((s) => s.id),
    issueTypeIds: (p) => p.issueTypes.map((t) => t.id), securityLevel: (p) => [p.securityLevel.id],
    changelogIds: (p) => [...p.history, ...p.live].map((h) => h.changelogId), faultIds: (p) => p.faults.map((f) => f.id),
    now: (p) => [p.now], sprintDates: (p) => p.sprints.map((s) => s.startDate),
  };
  for (const [name, get] of Object.entries(classes)) {
    for (let i = 0; i < packs.length; i++) {
      for (let j = i + 1; j < packs.length; j++) {
        assert.notDeepStrictEqual(get(packs[i]).map(String).sort(), get(packs[j]).map(String).sort(), `${name}: ${SEEDS[i]} vs ${SEEDS[j]}`);
      }
    }
  }
});

const HOUR = 3_600_000;
// The SPEC §2.3 scale and the shapes every graded behaviour relies on.
function shapeProblems(p, scoring = false) {
  const out = [];
  const t = (iso) => Date.parse(iso);
  const ids = (v) => (v ? v.split(',').map((x) => x.trim()).filter(Boolean) : []);
  const sprint = (id) => p.sprints.find((s) => String(s.id) === String(id));
  const actives = p.sprints.filter((s) => s.state === 'active');
  const issueById = new Map(p.issues.map((i) => [i.id, i]));
  const touches = (e) => e.items[0].field === 'Sprint' && actives.some((a) => t(e.created) > t(a.startDate)
    && (ids(e.items[0].from).includes(String(a.id)) || ids(e.items[0].to).includes(String(a.id))));

  // Scale.
  const scrum = p.boards.filter((b) => b.type === 'scrum');
  if (p.projects.length !== 3) out.push(`projects ${p.projects.length}`);
  if (scrum.length !== 4 || p.boards.filter((b) => b.type === 'kanban').length !== 1) out.push('boards');
  const perField = new Map();
  for (const b of scrum) perField.set(b.estimationFieldId, (perField.get(b.estimationFieldId) ?? 0) + 1);
  if (perField.size !== 2 || [...perField.values()].some((n) => n !== 2)) out.push('two scrum boards per estimation field');
  const twoBoard = scrum.filter((b) => b.projectKey === scrum[0].projectKey);
  if (twoBoard.length !== 2 || twoBoard[0].estimationFieldId === twoBoard[1].estimationFieldId) out.push("the first project's two boards share a field");
  const count = (st) => p.sprints.filter((s) => s.state === st).length;
  if (count('active') !== 6 || count('future') !== 2 || count('closed') !== 6) out.push(`sprints ${count('active')}/${count('future')}/${count('closed')}`);
  if (p.sprints.some((s) => !scrum.some((b) => b.id === s.originBoardId))) out.push('a sprint off the scrum boards');
  if (p.issues.length < 960 || p.issues.length > 1040) out.push(`issues ${p.issues.length}`);
  const inActive = p.issues.filter((i) => (i.fields[p.sprintFieldId] ?? []).some((s) => s.state === 'active')).length;
  if (inActive < 240 || inActive > 360) out.push(`issues in active sprints ${inActive}`);

  // Numeric id classes are disjoint, at bands that stay disjoint at any scale up to 50,000 issues.
  const classes = { board: p.boards.map((b) => b.id), sprint: p.sprints.map((x) => x.id), issue: p.issues.map((i) => i.id),
    changelog: [...p.history, ...p.live].map((e) => e.changelogId), project: p.projects.map((x) => x.id), status: p.statuses.map((x) => x.id),
    issueType: p.issueTypes.map((x) => x.id), securityLevel: [p.securityLevel.id] };
  const owner = new Map();
  for (const [cls, list] of Object.entries(classes)) for (const id of list.map(String)) {
    if (owner.has(id) && owner.get(id) !== cls) out.push(`id ${id} is both a ${owner.get(id)} and a ${cls}`);
    owner.set(id, cls);
  }
  if (p.issues.some((i) => Number(i.id) >= 240_000) || classes.changelog.some((c) => Number(c) < 1_000_000)) out.push('id bands');
  if (p.fields.some((f) => f.id === p.scopeStatusFieldId) || !/^customfield_\d+$/.test(p.scopeStatusFieldId)) out.push('scopeStatusFieldId');
  if (p.admins.length !== 1 || p.admins.includes(p.viewer) || p.admins.includes(p.peer)) out.push('admins');
  if (new Set(p.issues.map((i) => i.summary)).size !== p.issues.length) out.push('summaries repeat');

  // The live-UI slot (DESIGN §5.2, §8.7 step 8): two held-back changes for the first scrum board's active sprint.
  const ui = p.live.filter((e) => e.delivery.liveUi);
  const uiBoard = scrum[0];
  const uiSprints = new Set(actives.filter((x) => x.originBoardId === uiBoard.id).map((x) => String(x.id)));
  if (ui.length !== 2) out.push(`live-UI changes ${ui.length}`);
  if (ui.some((e) => e.delivery.slot !== null || e.delivery.duplicates.length || e.delivery.dropped)) out.push('a live-UI change is scheduled, duplicated or dropped');
  const lastScripted = Math.max(...p.live.filter((e) => !e.delivery.liveUi).map((e) => t(e.created)));
  if (ui.some((e) => t(e.created) <= lastScripted)) out.push('a live-UI change is not created after the scripted live changes');
  if (ui.some((e) => issueById.get(e.issueId).hiddenFrom.includes(p.viewer) || issueById.get(e.issueId).projectKey !== uiBoard.projectKey)) out.push("a live-UI change is not on the widget board's visible issues");
  if (!ui.some((e) => e.items[0].field === 'Sprint' && ids(e.items[0].to).some((x) => uiSprints.has(x)))) out.push("no live-UI change adds to the widget board's active sprint");
  if (!ui.some((e) => e.items[0].fieldId === uiBoard.estimationFieldId)) out.push('no live-UI estimate change on the widget board');

  // Start dates distinct; the SCORING site ties the first board's two active sprints; no change on a start instant.
  const starts = p.sprints.filter((s) => s.startDate).map((s) => t(s.startDate));
  const actStarts = actives.map((s) => t(s.startDate));
  const ties = actStarts.length - new Set(actStarts).size;
  if (new Set(starts).size !== starts.length - (scoring ? 1 : 0) || ties !== (scoring ? 1 : 0)) out.push(`tied sprint startDates (${ties} active ties)`);
  // The v1 window (first 2 days of each active sprint) closed before the upgrade.
  if (actives.some((a) => t(a.startDate) + V1_WINDOW >= t(p.now))) out.push('an active sprint started less than 2 days before now');
  if (scoring) {
    const tied = actives.filter((a) => actives.some((b) => b !== a && b.startDate === a.startDate));
    if (tied.length !== 2 || tied[0].originBoardId !== tied[1].originBoardId || tied[0].originBoardId !== uiBoard.id) out.push("the tied sprints are not the first board's");
    const emptyAtStart = actives.filter((a) => !p.issues.some((i) => {
      const before = p.history.filter((h) => h.issueId === i.id && h.items[0].fieldId === p.sprintFieldId && t(h.created) <= t(a.startDate)).at(-1);
      return before && ids(before.items[0].to).includes(String(a.id));
    }));
    if (emptyAtStart.length !== 1) out.push(`active sprints empty at their start: ${emptyAtStart.length}`);
    if (p.sprints.some((s) => s.name.length > 30) || !actives.some((s) => s.name.length >= 26)) out.push('no long sprint name within 30 characters');
    if (p.faults.length !== 5) out.push(`faults ${p.faults.length}`);
  }
  if (new Set(p.sprints.map((s) => s.name)).size !== p.sprints.length) out.push('sprint names repeat');
  const onStart = [...p.history, ...p.live].filter((e) => starts.includes(t(e.created)));
  if (onStart.length) out.push(`changes on a sprint start: ${onStart.map((e) => e.changelogId).join(',')}`);

  // The pre-history.
  const post = p.history.filter(touches);
  if (post.length < 150 || post.length > 215) out.push(`post-start sprint changes ${post.length}`);
  for (const a of actives) if (!post.some((e) => ids(e.items[0].from).includes(String(a.id)) || ids(e.items[0].to).includes(String(a.id)))) out.push(`sprint ${a.id} has no post-start change`);
  const multi = p.issues.filter((i) => (i.fields[p.sprintFieldId] ?? []).length > 1 && (i.fields[p.sprintFieldId] ?? []).some((s) => s.state === 'active'));
  if (multi.length < 8) out.push(`carry-over multi-id issues ${multi.length}`);
  const removed = new Set(post.filter((e) => ids(e.items[0].from).some((id) => sprint(id)?.state === 'active') && !ids(e.items[0].to).some((id) => sprint(id)?.state !== 'closed')).map((e) => e.issueId));
  if (removed.size < 4) out.push(`removed to backlog ${removed.size}`);
  // A move between the first project's two boards happens before the upgrade.
  const boardOf = (id) => sprint(id)?.originBoardId;
  if (!post.some((e) => { const f = ids(e.items[0].from).find((x) => sprint(x)?.state === 'active'); const to = ids(e.items[0].to).find((x) => sprint(x)?.state === 'active');
    return f && to && boardOf(f) !== boardOf(to); })) out.push('no move to another board\'s sprint in the history');

  // The live script: ~200 relevant among ~800 irrelevant updates over the 6 scored hours.
  const estimation = new Set(scrum.map((b) => b.estimationFieldId));
  const relevant = p.live.filter((e) => e.items[0].field === 'Sprint' || estimation.has(e.items[0].fieldId)).length;
  if (relevant < 180 || relevant > 235) out.push(`relevant live changes ${relevant}`);
  if (p.live.length - relevant < 760 || p.live.length - relevant > 840) out.push(`irrelevant live updates ${p.live.length - relevant}`);
  if (lastScripted - t(p.now) < 5 * HOUR || lastScripted - t(p.now) > 7 * HOUR) out.push(`live span ${((lastScripted - t(p.now)) / HOUR).toFixed(2)} h`);
  const scripted = p.live.filter((c) => !c.delivery.liveUi);
  const dups = p.live.filter((c) => c.delivery.duplicates.length).length;
  const drops = p.live.filter((c) => c.delivery.dropped).length;
  if (dups !== (scoring ? 8 : 4) || drops !== (scoring ? 5 : 3)) out.push(`dups ${dups} drops ${drops}`);
  const estimateDrops = p.live.filter((c) => c.delivery.dropped && c.items[0].field !== 'Sprint').length;
  if (estimateDrops !== (scoring ? 1 : 0)) out.push(`dropped estimate changes ${estimateDrops}`);
  const delivered = scripted.filter((c) => !c.delivery.dropped).sort((a, b) => a.delivery.slot - b.delivery.slot);
  let swapped = 0;
  for (let i = 0; i + 1 < delivered.length; i++) {
    const [x, y] = [delivered[i], delivered[i + 1]];
    if (t(x.created) > t(y.created) && x.issueId === y.issueId) swapped++;
  }
  if (swapped !== (scoring ? 4 : 2)) out.push(`same-issue permuted pairs ${swapped}`);
  const slots = p.live.flatMap((c) => [c.delivery.slot, ...c.delivery.duplicates]).filter((s) => s !== null).sort((a, b) => a - b);
  if (slots.some((s, i) => s !== i)) out.push('delivery slots are not 0..n-1');
  for (const b of scrum) if (!scripted.some((c) => c.items[0].fieldId === b.estimationFieldId && issueById.get(c.issueId).projectKey === b.projectKey)) out.push(`no scripted live estimate change on board ${b.id}`);
  const target = p.live.find((c) => c.changelogId === p.faults[0].match.changelogId);
  if (!target || target.delivery.dropped || target.delivery.duplicates.length || target.items[0].field !== 'Sprint') out.push('consumer fault target');

  // Visibility.
  const hidden = p.issues.filter((i) => i.hiddenFrom.includes(p.viewer));
  if (hidden.length < 8 || hidden.length > 12) out.push(`hidden from viewer ${hidden.length}`);
  if (hidden.filter((i) => post.some((e) => e.issueId === i.id)).length < 4) out.push('hidden with active-sprint changes');
  if (!p.issues.some((i) => i.hiddenFrom.includes(p.peer) && !i.hiddenFrom.includes(p.viewer))) out.push('no issue hidden from the peer only');
  const forbidden = p.issues.filter((i) => i.commentForbiddenFor.includes(p.viewer));
  if (forbidden.length !== 1 || forbidden[0].hiddenFrom.includes(p.viewer)) out.push('comment-forbidden issue');

  // Changelog consistency: install-time values agree with history; Sprint chains; ids grow with time.
  const lastSprint = new Map();
  for (const h of p.history) if (h.items[0].fieldId === p.sprintFieldId) lastSprint.set(h.issueId, h);
  for (const i of p.issues) {
    const last = lastSprint.get(i.id);
    if (last && last.items[0].to !== (i.fields[p.sprintFieldId] ?? []).map((s) => String(s.id)).join(', ')) { out.push(`issue ${i.key} sprint value disagrees with its history`); break; }
  }
  const all = [...p.history, ...p.live];
  const chain = new Map();
  for (const h of all.slice().sort((x, y) => t(x.created) - t(y.created) || Number(x.changelogId) - Number(y.changelogId))) {
    if (h.items[0].fieldId !== p.sprintFieldId) continue;
    if (h.items[0].from !== (chain.get(h.issueId) ?? '')) { out.push(`issue ${h.issueId} changelog ${h.changelogId} breaks its Sprint chain`); break; }
    chain.set(h.issueId, h.items[0].to);
  }
  const byTime = all.slice().sort((x, y) => t(x.created) - t(y.created));
  if (byTime.some((h, k) => k && Number(h.changelogId) <= Number(byTime[k - 1].changelogId))) out.push('changelog ids do not grow with creation time');
  if (new Set(all.map((h) => h.changelogId)).size !== all.length) out.push('duplicate changelog ids');
  if (p.live.some((c, k) => k && t(c.created) <= t(p.live[k - 1].created))) out.push('live not in creation order');
  if (p.live.some((c) => t(c.created) <= t(p.now)) || p.history.some((h) => t(h.created) >= t(p.now))) out.push('history/live split at now');

  // The v1 preload: exactly the rows v1 writes for the first 2 days of each active sprint, keyed changeId:sprintId.
  const rows = p.v1Preload.entities['scope-change'];
  const expected = new Map();
  for (const h of p.history) {
    if (h.items[0].field !== 'Sprint') continue;
    for (const { sprintId, kind } of sprintMoves(h.items[0])) {
      const s = sprint(sprintId);
      if (s.state === 'active' && t(h.created) > t(s.startDate) && t(h.created) <= t(s.startDate) + V1_WINDOW) expected.set(`${h.changelogId}:${sprintId}`, { h, kind });
    }
  }
  if (Object.keys(rows).length !== expected.size || expected.size < 40) out.push(`v1 rows ${Object.keys(rows).length} of ${expected.size}`);
  for (const [key, { h, kind }] of expected) {
    const row = rows[key];
    const user = p.users.find((u) => u.accountId === h.authorId);
    if (!row || row.changeId !== h.changelogId || row.sprintId !== key.split(':')[1] || row.at !== t(h.created) || row.created !== h.created
      || row.issueId !== h.issueId || row.issueKey !== issueById.get(h.issueId).key || row.kind !== kind || row.authorId !== h.authorId
      || row.authorName !== user.displayName || !['event', 'reconcile'].includes(row.source)) { out.push(`v1 row ${key}`); break; }
  }
  if (actives.filter((a) => [...expected.keys()].some((k) => k.endsWith(`:${a.id}`))).length < 5) out.push('v1 rows cover fewer than 5 active sprints');
  return out;
}

for (const scoring of [false, true]) {
  test(`${scoring ? 'scoring' : 'dev'} pack shapes hold on 100 seeds (SPEC §2.3)`, () => {
    const bad = [];
    for (let i = 0; i < 100; i++) {
      const problems = shapeProblems(facts(seedOf(i), { scoring }), scoring);
      if (problems.length) bad.push(`${seedOf(i)}: ${problems.join('; ')}`);
    }
    assert.deepStrictEqual(bad, []);
  });
}
