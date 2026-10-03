'use strict';
// I1 — the pack generator (DESIGN.md §5.1/§5.2/§13.2): deterministic per seed (byte-identical reruns in
// separate processes), seeds differ in every id class, and every pack carries the shapes the checks need.
const test = require('node:test');
const assert = require('node:assert');
const crypto = require('crypto');
const path = require('path');
const { execFileSync } = require('child_process');
const { FORGE } = require('./helpers.cjs');

const FIXTURES = path.join(FORGE, 'site', 'fixtures.cjs');
const { facts } = require(FIXTURES);
const SEEDS = ['0123456789abcdef', 'deadbeefcafef00d', '5eed5eed5eed5eed'];

test('byte-identical reruns per seed, in separate processes', () => {
  for (const seed of SEEDS) {
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
  const packs = SEEDS.map(facts);
  const classes = {
    cloudId: (p) => [p.cloudId], siteUrl: (p) => [p.siteUrl], appAccountId: (p) => [p.appAccountId],
    userAccountIds: (p) => p.users.map((u) => u.accountId), viewer: (p) => [p.viewer], peer: (p) => [p.peer],
    projectIds: (p) => p.projects.map((x) => x.id), projectKeys: (p) => p.projects.map((x) => x.key),
    boardIds: (p) => p.boards.map((b) => b.id), sprintIds: (p) => p.sprints.map((s) => s.id), sprintNames: (p) => p.sprints.map((s) => s.name),
    issueIds: (p) => p.issues.map((i) => i.id), issueKeys: (p) => p.issues.map((i) => i.key),
    customFieldIds: (p) => p.fields.filter((f) => f.custom).map((f) => f.id), sprintFieldId: (p) => [p.sprintFieldId],
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

// The §5.2 shapes, over many seeds (the generator policy every graded behaviour relies on).
function shapeProblems(p, scoring = false) {
  const out = [];
  const actives = p.sprints.filter((s) => s.state === 'active');
  const state = (id) => p.sprints.find((s) => String(s.id) === String(id))?.state;
  const ids = (v) => (v ? v.split(',').map((x) => x.trim()).filter(Boolean) : []);
  const touches = (e) => e.items[0].field === 'Sprint' && actives.some((a) => Date.parse(e.created) > Date.parse(a.startDate)
    && (ids(e.items[0].from).includes(String(a.id)) || ids(e.items[0].to).includes(String(a.id))));
  if (p.issues.length < 229 || p.issues.length > 245) out.push(`issues ${p.issues.length}`);
  // Numeric id classes are disjoint (seed 5eed0123456789ab once had changelog ids equal to issue ids).
  const classes = { board: p.boards.map((b) => b.id), sprint: p.sprints.map((x) => x.id), issue: p.issues.map((i) => i.id),
    changelog: [...p.history, ...p.live].map((e) => e.changelogId), project: p.projects.map((x) => x.id), status: p.statuses.map((x) => x.id),
    issueType: p.issueTypes.map((x) => x.id), securityLevel: [p.securityLevel.id] };
  const owner = new Map();
  for (const [cls, ids] of Object.entries(classes)) for (const id of ids.map(String)) {
    if (owner.has(id) && owner.get(id) !== cls) out.push(`id ${id} is both a ${owner.get(id)} and a ${cls}`);
    owner.set(id, cls);
  }
  // The live-UI slot (DESIGN §5.2, §8.7 step 8): two held-back changes for the first scrum board's active sprint.
  const ui = p.live.filter((e) => e.delivery.liveUi);
  const uiBoard = p.boards.find((b) => b.type === 'scrum');
  const uiSprints = new Set(p.sprints.filter((x) => x.state === 'active' && x.originBoardId === uiBoard.id).map((x) => String(x.id)));
  const issueOf = (e) => p.issues.find((i) => i.id === e.issueId);
  if (ui.length !== 2) out.push(`live-UI changes ${ui.length}`);
  if (ui.some((e) => e.delivery.slot !== null || e.delivery.duplicates.length || e.delivery.dropped)) out.push('a live-UI change is scheduled, duplicated or dropped');
  const lastScripted = Math.max(...p.live.filter((e) => !e.delivery.liveUi).map((e) => Date.parse(e.created)));
  if (ui.some((e) => Date.parse(e.created) <= lastScripted)) out.push('a live-UI change is not created after the scripted live changes');
  if (ui.some((e) => issueOf(e).hiddenFrom.includes(p.viewer) || issueOf(e).projectKey !== uiBoard.projectKey)) out.push("a live-UI change is not on the widget board's visible issues");
  if (!ui.some((e) => e.items[0].field === 'Sprint' && ids(e.items[0].to).some((x) => uiSprints.has(x)))) out.push('no live-UI change adds to the widget board\'s active sprint');
  if (!ui.some((e) => e.items[0].fieldId === uiBoard.estimationFieldId)) out.push('no live-UI estimate change on the widget board');
  // DESIGN §17.1 19/20: sprint start dates are distinct, and no change lands exactly on a sprint start.
  const starts = p.sprints.filter((s) => s.startDate).map((s) => Date.parse(s.startDate));
  // The SCORING site ties the first board's two active sprints on purpose (contract §4 "ties by sprint id"); every
  // other start date stays distinct.
  const actStarts = actives.map((s) => Date.parse(s.startDate));
  const ties = actStarts.length - new Set(actStarts).size;
  if (new Set(starts).size !== starts.length - (scoring ? 1 : 0) || ties !== (scoring ? 1 : 0)) out.push(`tied sprint startDates (${ties} active ties)`);
  if (scoring) {
    // F4: the tied pair shares a board; one active sprint starts EMPTY (committed 0); one carries a long name (<= 30).
    const tied = actives.filter((a) => actives.some((b) => b !== a && b.startDate === a.startDate));
    if (tied.length !== 2 || tied[0].originBoardId !== tied[1].originBoardId) out.push('the tied sprints are not one board\'s');
    const emptyAtStart = actives.filter((a) => !p.issues.some((i) => {
      const before = [...p.history].filter((h) => h.issueId === i.id && h.items[0].fieldId === p.sprintFieldId
        && Date.parse(h.created) <= Date.parse(a.startDate)).at(-1);
      return before && ids(before.items[0].to).includes(String(a.id));
    }));
    if (emptyAtStart.length !== 1) out.push(`active sprints empty at their start: ${emptyAtStart.length}`);
    if (p.sprints.some((s) => s.name.length > 30) || !actives.some((s) => s.name.length >= 26)) out.push('no long sprint name within 30 characters');
    if (p.faults.length !== 5) out.push(`faults ${p.faults.length}`);
  }
  const onStart = [...p.history, ...p.live].filter((e) => starts.includes(Date.parse(e.created)));
  if (onStart.length) out.push(`changes on a sprint start: ${onStart.map((e) => e.changelogId).join(',')}`);
  const post = p.history.filter(touches);
  if (post.length < 50 || post.length > 70) out.push(`post-start sprint changes ${post.length}`);
  const multi = p.issues.filter((i) => (i.fields[p.sprintFieldId] ?? []).length > 1 && (i.fields[p.sprintFieldId] ?? []).some((s) => s.state === 'active'));
  if (multi.length < 8) out.push(`carry-over multi-id issues ${multi.length}`);
  const removed = new Set(post.filter((e) => ids(e.items[0].from).some((id) => state(id) === 'active') && !ids(e.items[0].to).some((id) => state(id) !== 'closed')).map((e) => e.issueId));
  if (removed.size < 4) out.push(`removed to backlog ${removed.size}`);
  const scripted = p.live.filter((c) => !c.delivery.liveUi);
  if (scripted.length < 36 || scripted.length > 44) out.push(`scripted live ${scripted.length}`);
  const dups = p.live.filter((c) => c.delivery.duplicates.length).length;
  const drops = p.live.filter((c) => c.delivery.dropped).length;
  if (dups !== (scoring ? 8 : 4) || drops !== (scoring ? 5 : 3)) out.push(`dups ${dups} drops ${drops}`);
  const estimateDrops = p.live.filter((c) => c.delivery.dropped && c.items[0].field !== 'Sprint').length;
  if (estimateDrops !== (scoring ? 1 : 0)) out.push(`dropped estimate changes ${estimateDrops}`);
  const delivered = scripted.filter((c) => !c.delivery.dropped).sort((a, b) => a.delivery.slot - b.delivery.slot);
  let swapped = 0;
  for (let i = 0; i + 1 < delivered.length; i++) {
    const [x, y] = [delivered[i], delivered[i + 1]];
    if (Date.parse(x.created) > Date.parse(y.created) && x.issueId === y.issueId) swapped++;
  }
  if (swapped !== (scoring ? 4 : 2)) out.push(`same-issue permuted pairs ${swapped}`);
  const slots = p.live.flatMap((c) => [c.delivery.slot, ...c.delivery.duplicates]).filter((s) => s !== null).sort((a, b) => a - b);
  if (slots.some((s, i) => s !== i)) out.push('delivery slots are not 0..n-1');
  const hidden = p.issues.filter((i) => i.hiddenFrom.includes(p.viewer));
  if (hidden.length < 4 || hidden.length > 6) out.push(`hidden from viewer ${hidden.length}`);
  const hiddenChanged = hidden.filter((i) => post.some((e) => e.issueId === i.id));
  if (hiddenChanged.length < 2) out.push(`hidden with active-sprint changes ${hiddenChanged.length}`);
  if (!p.issues.some((i) => i.hiddenFrom.includes(p.peer) && !i.hiddenFrom.includes(p.viewer))) out.push('no issue hidden from the peer only');
  const forbidden = p.issues.filter((i) => i.commentForbiddenFor.includes(p.viewer));
  if (forbidden.length !== 1 || forbidden[0].hiddenFrom.includes(p.viewer)) out.push('comment-forbidden issue');
  const scrum = p.boards.filter((b) => b.type === 'scrum');
  if (scrum.length !== 2 || scrum[0].estimationFieldId === scrum[1].estimationFieldId || !p.boards.some((b) => b.type === 'kanban')) out.push('boards');
  const [A, B] = p.projects.map((x) => x.key);
  const count = (key, st) => p.sprints.filter((s) => s.state === st && scrum.find((b) => b.id === s.originBoardId)?.projectKey === key).length;
  if (count(A, 'active') !== 2 || count(A, 'future') !== 1 || count(A, 'closed') !== 2) out.push('first project sprints');
  if (count(B, 'active') !== 1 || count(B, 'future') !== 1 || count(B, 'closed') !== 1) out.push('second project sprints');
  const target = p.live.find((c) => c.changelogId === p.faults[0].match.changelogId);
  if (!target || target.delivery.dropped || target.delivery.duplicates.length || target.items[0].field !== 'Sprint') out.push('consumer fault target');
  // Install-time Sprint values agree with each issue's last history entry.
  for (const i of p.issues) {
    const last = p.history.filter((h) => h.issueId === i.id && h.items[0].fieldId === p.sprintFieldId).at(-1);
    const now = (i.fields[p.sprintFieldId] ?? []).map((s) => String(s.id)).join(', ');
    if (last && last.items[0].to !== now) { out.push(`issue ${i.key} sprint value disagrees with its history`); break; }
  }
  // Every issue's Sprint changelog is a chain in creation order: each entry's `from` is the previous `to`.
  const chain = new Map();
  for (const h of [...p.history, ...p.live].sort((x, y) => Date.parse(x.created) - Date.parse(y.created) || Number(x.changelogId) - Number(y.changelogId))) {
    if (h.items[0].fieldId !== p.sprintFieldId) continue;
    const prev = chain.get(h.issueId) ?? '';
    if (h.items[0].from !== prev) { out.push(`issue ${h.issueId} changelog ${h.changelogId} from '${h.items[0].from}' but previous to '${prev}'`); break; }
    chain.set(h.issueId, h.items[0].to);
  }
  // Both scrum boards' estimation fields move during the live script.
  for (const b of scrum) if (!scripted.some((c) => c.items[0].fieldId === b.estimationFieldId)) out.push(`no scripted live estimate change on ${b.estimationFieldId}`);
  // Changelog ids increase with creation time.
  const all = [...p.history, ...p.live];
  const byTime = all.slice().sort((x, y) => Date.parse(x.created) - Date.parse(y.created));
  if (new Set(all.map((h) => h.changelogId)).size !== all.length) out.push('duplicate changelog ids');
  if (p.live.some((c, k) => k && Date.parse(c.created) <= Date.parse(p.live[k - 1].created))) out.push('live not in creation order');
  if (Date.parse(byTime.at(-1).created) <= Date.parse(p.now) && p.live.length) out.push('live changes before now');
  return out;
}

test('pack shapes hold on 300 seeds (DESIGN §5.2)', () => {
  const bad = [];
  for (let i = 0; i < 300; i++) {
    const seed = crypto.createHash('sha256').update(`forge-fixture-${i}`).digest('hex').slice(0, 16);
    const problems = shapeProblems(facts(seed));
    if (problems.length) bad.push(`${seed}: ${problems.join('; ')}`);
  }
  assert.deepStrictEqual(bad, []);
});

// The SCORING pack (DESIGN §5.1, §17.6; 2026-10-03 stringency F4/F7): the same shapes, scaled faults, the stated tie,
// committed-0 and long-name cases — and the dev pack of the same seed stays the dev pack.
test('scoring pack shapes hold on 300 seeds', () => {
  const bad = [];
  for (let i = 0; i < 300; i++) {
    const seed = crypto.createHash('sha256').update(`forge-fixture-${i}`).digest('hex').slice(0, 16);
    const problems = shapeProblems(facts(seed, { scoring: true }), true);
    if (problems.length) bad.push(`${seed}: ${problems.join('; ')}`);
  }
  assert.deepStrictEqual(bad, []);
});

// WP2's oracle accepts every pack (no PackDefect: disjoint ids, ISO instants) and the live-UI pair moves the numbers
// of an active sprint the first scrum board's widget shows (the u_widget_live precondition), incl. seed 5eed0123456789ab.
test("WP2's oracle: packs are valid and the live-UI changes move the widget board's numbers", { timeout: 300_000 }, () => {
  const fs = require('fs');
  const os = require('os');
  const { execFileSync } = require('child_process');
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'forge-packs-'));
  const seeds = ['5eed0123456789ab', ...Array.from({ length: 60 }, (_, i) => crypto.createHash('sha256').update(`forge-fixture-${i}`).digest('hex').slice(0, 16))];
  for (const seed of seeds) fs.writeFileSync(path.join(dir, `${seed}.json`), JSON.stringify(facts(seed)));
  const out = execFileSync('python3', ['-c', `
import sys, json, glob
sys.path.insert(0, sys.argv[2])
import forge_oracle as fo
bad = []
for f in sorted(glob.glob(sys.argv[1] + '/*.json')):
    p = json.load(open(f))
    try:
        before, after = fo.Oracle(p), fo.Oracle(p, include_live_ui=True)
    except fo.PackDefect as e:
        bad.append(f"{p['seed']}: {e}"); continue
    board = next(b for b in p['boards'] if b['type'] == 'scrum')
    sids = [str(s['id']) for s in p['sprints'] if s['state'] == 'active' and s['originBoardId'] == board['id']]
    if all(before.numbers(s) == after.numbers(s) for s in sids):
        bad.append(f"{p['seed']}: the live-UI changes leave board {board['id']} unchanged")
print(json.dumps(bad))`, dir, path.join(__dirname, '..', '..', '..', 'bench')], { encoding: 'utf8' });
  fs.rmSync(dir, { recursive: true, force: true });
  assert.deepStrictEqual(JSON.parse(out.trim().split('\n').pop()), []);
});
