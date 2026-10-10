// The golden's v2 proof on its own bed (virtual clock): the live migration (cut off and resumed), the
// first scheduled run after the upgrade, scope-status values, the world changes, the admin resolvers,
// the CI web trigger, the Forge LLM rules and the background dose — each against an independent oracle.
// Usage: node dev/test-v2.cjs [outDir]
'use strict';

const path = require('path');
const os = require('os');
const crypto = require('crypto');
const { createSite, oracle, fmtCreep, EST_OPS, EST_WEB } = require('./site.cjs');
const { createPlatform } = require('./runtime.cjs');
const { installClock } = require('./clock.cjs');

let failures = 0;
const ok = (cond, msg) => {
  if (!cond) failures += 1;
  console.log(`${cond ? 'PASS' : 'FAIL'}  ${msg}`);
};
const eq = (a, b, msg) => ok(JSON.stringify(a) === JSON.stringify(b), `${msg}${JSON.stringify(a) === JSON.stringify(b) ? '' : `\n      got      ${JSON.stringify(a)}\n      expected ${JSON.stringify(b)}`}`);

const DAY = 86_400_000;
const HOUR = 3_600_000;
const fmtPoints = (n) => String(Math.round(n * 1e6) / 1e6);
const BACKGROUND = new Set(['consumer', 'scheduledTrigger', 'trigger', 'webtrigger']);

// RATE-MODEL.json, independently of the app: a 429 is not charged.
function points(r) {
  if (r.status === 429) return 0;
  const p = r.path.split('?')[0];
  if (p === '/rest/api/3/search/jql') return 1 + Math.ceil((r.returned ?? 0) / 50);
  if (p === '/rest/api/3/changelog/bulkfetch') return 2;
  if (p === '/rest/api/3/app/field/value') return 1 + Math.ceil((r.body?.updates ?? []).reduce((n, u) => n + u.issueIds.length, 0) / 50);
  return r.method === 'GET' ? 1 : 2;
}
function backgroundPerHour(site) {
  const out = new Map();
  for (const r of site.requests) if (BACKGROUND.has(r.kind)) out.set(Math.floor(r.at / HOUR), (out.get(Math.floor(r.at / HOUR)) ?? 0) + points(r));
  return out;
}

// What v1 left behind: its rows for the first 2 days of each active sprint, and its config.
function preloadV1(site, platform) {
  const truth = oracle(site);
  const rows = new Map();
  for (const [sid, t] of Object.entries(truth)) {
    for (const c of t.changes) {
      if (c.at > t.sprint.start + 2 * DAY) continue;
      rows.set(`${c.changeId}:${sid}`, { sprintId: sid, changeId: c.changeId, at: c.at, created: new Date(c.at).toISOString(), issueId: c.issueId, issueKey: c.issueKey, kind: c.kind, authorId: '', authorName: c.by, source: 'reconcile' });
    }
  }
  platform.kvs.entities.set('scope-change', rows);
  const sprints = {};
  for (const s of site.sprints.filter((x) => x.state === 'active')) sprints[String(s.id)] = { id: String(s.id), name: s.name, startDate: new Date(s.start).toISOString(), startMs: s.start, boardId: String(s.board), estimateFieldId: site.boards.find((b) => b.id === s.board).estimateField };
  platform.kvs.plain.set('config', { sprintFieldId: site.SPRINT_FIELD, sprints });
  return rows;
}

const ledger = (platform) => platform.kvs.entities.get('scope-ledger') ?? new Map();
const activeIds = (site) => new Set(site.sprints.filter((s) => s.state === 'active').map((s) => String(s.id)));

function ledgerMatchesOracle(site, platform, label) {
  const truth = oracle(site);
  const want = new Set(Object.entries(truth).flatMap(([sid, t]) => t.changes.map((c) => `${c.changeId}:${sid}`)));
  const active = activeIds(site);
  const have = [...ledger(platform).values()].filter((r) => active.has(r.sprintId)).map((r) => `${r.changeId}:${r.sprintId}`);
  const missing = [...want].filter((k) => !have.includes(k));
  const extra = have.filter((k) => !want.has(k));
  ok(!missing.length && !extra.length && have.length === new Set(have).size, `${label}: ledger == oracle (${have.length} rows${missing.length ? `, missing ${missing.slice(0, 5)}` : ''}${extra.length ? `, extra ${extra.slice(0, 5)}` : ''})`);
}

function statusesMatchOracle(site, label) {
  const truth = oracle(site);
  const wrong = site.issues.filter((i) => !i.deleted && (i.fields[site.SCOPE_FIELD] ?? '') !== truth.status.get(i.id));
  ok(!wrong.length, `${label}: every issue's scope-status == oracle${wrong.length ? ` (${wrong.length} wrong, e.g. ${wrong.slice(0, 3).map((i) => `${i.key}=${JSON.stringify(i.fields[site.SCOPE_FIELD])} want ${JSON.stringify(truth.status.get(i.id))}`).join('; ')})` : ''}`);
}

async function widgetMatchesOracle(site, platform, aaid, label) {
  const truth = oracle(site);
  for (const board of [1, 2, 4]) {
    const w = await platform.resolver('dashboards:widget', 'scope-widget', 'widget', { boardId: String(board) }, { aaid, extension: { config: { boardId: String(board) } } });
    const want = site.sprints.filter((s) => s.board === board && s.state === 'active').sort((a, b) => a.start - b.start);
    eq(w.sprints.map((s) => s.id), want.map((s) => String(s.id)), `${label}: board ${board} shows its active sprints only`);
    for (const s of w.sprints) {
      const t = truth[s.id];
      eq(s.text, { committed: fmtPoints(t.committed), added: fmtPoints(t.added), removed: fmtPoints(t.removed), creep: fmtCreep(t.creep) }, `${label}: sprint ${s.id} totals`);
    }
  }
}

function sign(secret, ts, body) {
  return `sha256=${crypto.createHmac('sha256', secret).update(`${ts}.${body}`).digest('hex')}`;
}

async function main() {
  const outDir = process.argv[2] ?? path.join(os.tmpdir(), 'golden-forge2-v2');
  const clock = installClock(Date.parse('2026-10-02T12:00:00.000Z'));
  const site = createSite({ seed: 11, clock });
  const platform = createPlatform({ site, clock });
  await platform.build(path.join(outDir, 'bundle'));
  const { alice, bob } = site.users;
  const run = (fn, ev, moduleKey) => platform.invoke(fn, ev, { moduleKey });
  const asAdminPage = (key, payload, aaid) => platform.resolver('jira:adminPage', 'scope-admin', key, payload, { aaid });
  const sprintExt = (id) => ({ type: 'jira:sprintAction', sprint: { id: Number(id), state: 'active' }, board: { id: 1, type: 'scrum' } });
  const ledgerAs = (aaid, sid) => platform.resolver('jira:sprintAction', 'scope-sprint-ledger', 'sprintLedger', {}, { aaid, extension: sprintExt(sid) });

  // ===== R1: the live migration, cut off once and resumed ===========================================
  const v1 = preloadV1(site, platform);
  const v1Before = JSON.stringify([...v1]);
  ok(v1.size > 20, `v1 left ${v1.size} rows for the first 2 days of each active sprint`);
  let ledgerWrites = 0;
  platform.kvs.faults.push({ match: (op, body) => op === '/api/v1/entity/set' && body.entityName === 'scope-ledger' && (ledgerWrites += 1) > 7, times: 1 });
  await run('on-app-upgraded', { eventType: 'avi:forge:upgraded:app' }, 'scope-app-upgraded');
  const d1 = await platform.drain();
  ok(d1[0]?.error && d1.length >= 2, `the first migration step was cut off after 7 copies (${d1[0]?.error}) and redelivered`);
  const l1 = ledger(platform);
  ok([...v1.keys()].every((k) => l1.has(k)), `every v1 row is in scope-ledger (${[...v1.keys()].filter((k) => l1.has(k)).length}/${v1.size})`);
  ok(
    [...v1].every(([k, r]) => {
      const x = l1.get(k);
      return x.changeId === r.changeId && x.at === r.at && x.sprintId === r.sprintId && x.kind === r.kind && x.source === r.source && x.authorName === r.authorName && x.deleted === false;
    }),
    'each keeps its original changeId, time, kind, author and source',
  );
  ok(l1.size === v1.size, `exactly once: ${l1.size} scope-ledger rows for ${v1.size} v1 rows`);
  ok(JSON.stringify([...platform.kvs.entities.get('scope-change')]) === v1Before, "v1's entity is left intact");
  const adm0 = await asAdminPage('getAdmin', {}, alice.accountId);
  eq(adm0.migration?.text, `Migrated ${v1.size} of ${v1.size} v1 rows — complete`, 'the admin panel shows the migration complete');

  // ===== the first scheduled run after the upgrade: backfill the rest, memberships, scope statuses =====
  await run('reconcile', {}, 'scope-reconcile');
  await platform.drain();
  ledgerMatchesOracle(site, platform, 'after the first scheduled run');
  statusesMatchOracle(site, 'after the first scheduled run');
  await widgetMatchesOracle(site, platform, alice.accountId, 'after the first scheduled run');
  const fieldPosts = site.requests.filter((r) => r.path.startsWith('/rest/api/3/app/field/value'));
  ok(fieldPosts.length > 0 && fieldPosts.every((r) => r.as === 'app' && r.body.updates.reduce((n, u) => n + u.issueIds.length, 0) <= 200), `statuses are written in bulk as the app (${fieldPosts.length} requests, ≤ 200 updates each)`);
  const v2Rows = [...ledger(platform).values()].filter((r) => !v1.has(`${r.changeId}:${r.sprintId}`));
  ok(v2Rows.length > 0 && v2Rows.every((r) => r.estimateField && typeof r.estimate === 'number' && r.boardId), `rows v2 recorded carry estimate, board and estimation field (${v2Rows.length})`);

  // ===== R7 freshness + the event path =============================================================
  const ops = site.issues.filter((i) => i.project === 'OPS' && !i.restricted);
  const free = ops.filter((i) => !i.sprints.some((s) => site.sprintById.get(s).state === 'active'));
  const in11 = ops.filter((i) => i.sprints.includes(11));
  const deliver = async (ev) => {
    await run('on-issue-updated', ev, 'scope-issue-updated');
    return platform.drain();
  };
  const x = free[0];
  const hourOfChange = Math.floor(clock.now() / HOUR);
  await deliver(site.update(x.key, { sprints: [...x.sprints, 11] }));
  const xWrite = site.fieldWrites.filter((w) => w.issueId === x.id).at(-1);
  eq(x.fields[site.SCOPE_FIELD], `added +${fmtPoints(x.fields[x.estField] ?? 0)}`, `an issue added to an active sprint shows "added +<points>" (${x.key})`);
  ok(xWrite && Math.floor(xWrite.at / HOUR) === hourOfChange, 'its scope status is fresh within the same virtual hour');
  const y = in11[0];
  await deliver(site.update(y.key, { sprints: y.sprints.filter((s) => s !== 11) }));
  eq(y.fields[site.SCOPE_FIELD], 'removed', `an issue removed from an active sprint shows "removed" (${y.key})`);

  // ===== R4: the world changes =====================================================================
  // (a) a board's estimation field changes: later rows use the new field, earlier rows keep theirs
  const rows31Before = [...ledger(platform).values()].filter((r) => r.sprintId === '31').map((r) => [r.changeId, r.estimate, r.estimateField]);
  const z = free[1];
  z.fields[EST_WEB] = 13;
  site.setBoardField(4, EST_WEB);
  await deliver(site.update(z.key, { sprints: [...z.sprints, 31] }));
  const zRow = [...ledger(platform).values()].find((r) => r.sprintId === '31' && r.issueId === z.id);
  eq(zRow && [zRow.kind, zRow.estimate, zRow.estimateField], ['added', 13, EST_WEB], 'after the switch a change uses the new estimation field');
  eq([...ledger(platform).values()].filter((r) => r.sprintId === '31' && r.issueId !== z.id).map((r) => [r.changeId, r.estimate, r.estimateField]), rows31Before, 'earlier rows keep their estimate');
  // (b) an issue moves to another board's sprint: removed from the old, added to the new with its estimate
  const w = in11[1];
  w.fields[EST_WEB] = 8;
  const wEv = site.update(w.key, { sprints: [...w.sprints.filter((s) => s !== 11), 21] });
  await deliver(wEv);
  const wRows = [...ledger(platform).values()].filter((r) => r.changeId === wEv.changelog.id).map((r) => [r.sprintId, r.kind, r.estimate, r.estimateField]).sort();
  eq(wRows, [['11', 'removed', w.fields[w.estField] ?? 0, site.boards[0].estimateField], ['21', 'added', 8, EST_WEB]], 'a move to another board is removed from the old sprint and added to the new with the new board\'s estimate');
  // (c) a sprint closes: its ledger is final and it leaves the widget
  const in12 = ops.filter((i) => i.sprints.includes(12));
  // An issue leaves sprint 12, the sprint closes, and the issue is re-estimated before any event reveals the close:
  // the closed sprint's numbers stay as they stood at the close (contract §12).
  const left12 = in12[1];
  await deliver(site.update(left12.key, { sprints: left12.sprints.filter((s) => s !== 12) }));
  const member12 = () => {
    const m = platform.kvs.entities.get('sprint-issue')?.get(`12:${left12.id}`);
    return m && JSON.stringify([m.inSprint, m.estimate, m.estimates]);
  };
  // Two changes made before the close whose deliveries are handed over only after it (the rate limits held them back):
  // the closed sprint still takes them, and its numbers are as they stood at the close.
  const joins12 = free[2];
  const reestimated12 = in12[2];
  const late12 = [
    site.update(joins12.key, { sprints: [...joins12.sprints, 12] }),
    site.update(reestimated12.key, { estimate: (reestimated12.fields[reestimated12.estField] ?? 0) + 2 }),
  ];
  const totalsText = (t) => ({ committed: fmtPoints(t.committed), added: fmtPoints(t.added), removed: fmtPoints(t.removed), creep: fmtCreep(t.creep) });
  const delivered12 = oracle(site)['12'];
  // Two more whose events never arrive (lost events): only the scheduled run after the close can record them.
  const lostJoin12 = free[4];
  const lostJoinId = site.update(lostJoin12.key, { sprints: [...lostJoin12.sprints, 12] }).changelog.id;
  const lostEstimate12 = in12[3];
  site.update(lostEstimate12.key, { estimate: (lostEstimate12.fields[lostEstimate12.estField] ?? 0) + 1 });
  const atClose12 = oracle(site)['12'];
  site.closeSprint(12);
  const frozen12 = member12();
  for (const ev of late12) await deliver(ev);
  eq((await ledgerAs(alice.accountId, '12')).text, totalsText(delivered12),
    `changes made before the close and delivered after it count in the closed sprint's numbers (${joins12.key} joined, ${reestimated12.key} re-estimated)`);
  await deliver(site.update(left12.key, { estimate: (left12.fields[left12.estField] ?? 0) + 1 }));
  ok(frozen12 && member12() === frozen12, `a re-estimate after the close leaves the closed sprint's numbers as at the close (${left12.key})`);
  const rows12 = [...ledger(platform).values()].filter((r) => r.sprintId === '12').length;
  await deliver(site.update(in12[0].key, { sprints: in12[0].sprints.filter((s) => s !== 12) }));
  ok([...ledger(platform).values()].filter((r) => r.sprintId === '12').length === rows12, 'a change after a sprint closed adds nothing to its ledger');
  const truthClosed = oracle(site);
  const stale12 = site.issues.filter((i) => !i.deleted && i.sprints.includes(12) && (i.fields[site.SCOPE_FIELD] ?? '') !== truthClosed.status.get(i.id));
  const writes12 = site.fieldWrites.filter((fw) => in12.some((i) => i.id === fw.issueId)).at(-1);
  ok(!stale12.length && writes12 && Math.floor(writes12.at / HOUR) === Math.floor(site.sprintById.get(12).complete / HOUR), `the closed sprint's issues' statuses are fresh within the hour of the close (${stale12.length} stale)`);
  // (d) an issue is deleted: its rows stay as history with deleted: true
  const v = in11[2];
  const vRows = [...ledger(platform).values()].filter((r) => r.issueId === v.id).length;
  await run('on-issue-deleted', site.deleteIssue(v.key), 'scope-issue-deleted');
  await platform.drain();
  const vAfter = [...ledger(platform).values()].filter((r) => r.issueId === v.id);
  ok(vAfter.length === vRows && vAfter.every((r) => r.deleted === true), `a deleted issue's ${vRows} row(s) stay, marked deleted`);
  // (e) a person loses browse permission: their next request shows none of that issue's rows
  const u = in11.find((i) => i !== v && [...ledger(platform).values()].some((r) => r.issueId === i.id && r.sprintId === '11'));
  const before = await ledgerAs(bob.accountId, '11');
  const browse = bob.browse;
  bob.browse = (i) => browse(i) && i.id !== u.id;
  const after = await ledgerAs(bob.accountId, '11');
  ok(before.changes.some((c) => c.issueId === u.id) && !after.changes.some((c) => c.issueId === u.id) && after.hiddenCount > before.hiddenCount, `after losing browse on ${u.key} its rows are gone from the very next request`);

  // the next scheduled run converges everything on the oracle
  clock.advance(HOUR);
  await run('reconcile', {}, 'scope-reconcile');
  await platform.drain();
  ledgerMatchesOracle(site, platform, 'after the world changes');
  statusesMatchOracle(site, 'after the world changes');
  await widgetMatchesOracle(site, platform, alice.accountId, 'after the world changes');
  ok([...ledger(platform).values()].some((r) => r.changeId === lostJoinId && r.sprintId === '12' && r.source === 'reconcile'), `the scheduled run after the close records a change made before it whose event never arrived (${lostJoin12.key})`);
  eq((await ledgerAs(alice.accountId, '12')).text, totalsText(atClose12),
    `the closed sprint's numbers are as at the close: the lost join and re-estimate (${lostEstimate12.key}) count, the changes after the close do not`);

  // ===== R5: the admin resolvers ===================================================================
  for (const key of ['getAdmin', 'saveSettings', 'rotateSecret']) {
    const w0 = platform.kvs.writes().length;
    const s0 = platform.kvs.secrets.size;
    const res = await asAdminPage(key, { settings: { backgroundShare: 90 }, accountId: alice.accountId, isAdmin: true, role: 'admin' }, bob.accountId);
    ok(res.forbidden === true && platform.kvs.writes().length === w0 && platform.kvs.secrets.size === s0, `a non-admin calling ${key} (payload claiming admin) is refused with no side effect`);
  }
  const saved = await asAdminPage('saveSettings', { settings: { backgroundShare: '50', aiEnabled: true, dailyTokenBudget: '200000', commentGroup: 'jira-developers' } }, alice.accountId);
  ok(saved.ok && saved.settings.backgroundShare === 50 && saved.settings.commentGroup === 'jira-developers', 'an admin saves settings');
  ok(saved.audit.length === 2 && saved.audit.every((e) => e.who === 'Alice Admin' && e.accountId === alice.accountId) && saved.audit.some((e) => e.what === 'Background share (%): 70 → 50'), `the change is in Recent admin changes (${saved.audit.map((e) => e.what).join(' | ')})`);
  const bad = await asAdminPage('saveSettings', { settings: { backgroundShare: 5 } }, alice.accountId);
  ok(bad.ok === false && /10 to 90/.test(bad.error), 'an out-of-range background share is refused');
  const rotated = await asAdminPage('rotateSecret', {}, alice.accountId);
  const secret = rotated.newSecret;
  ok(/^[0-9a-f]{64}$/.test(secret) && rotated.secret.masked === `••••${secret.slice(-4)}`, 'Rotate CI secret shows the new secret once');
  const later = await asAdminPage('getAdmin', {}, alice.accountId);
  ok(!JSON.stringify(later).includes(secret) && later.secret.masked === `••••${secret.slice(-4)}`, 'afterwards only ••••<last4> is returned');
  ok(platform.kvs.secrets.get('ci-secret') === secret && ![...platform.kvs.plain.values()].some((v2) => JSON.stringify(v2).includes(secret)), 'the secret is stored with setSecret only');

  // the comment group restricts the summary comment's visibility
  const v11 = await ledgerAs(alice.accountId, '11');
  const posted = await platform.resolver('jira:sprintAction', 'scope-sprint-ledger', 'postSummary', { changeId: v11.changes[0].changeId }, { aaid: alice.accountId, extension: sprintExt('11') });
  eq(posted.ok && site.comments.at(-1).visibility, { type: 'group', value: 'jira-developers' }, 'the summary comment is restricted to the comment group');

  // ===== R6: the CI web trigger ====================================================================
  const target = [...ledger(platform).values()].find((r) => r.sprintId === '11' && !r.deleted);
  const targetKey = site.issueById.get(target.issueId).key;
  const ci = (headers, body) => run('on-ci-deploy', { method: 'POST', path: '/', headers, body, queryParameters: {} }, 'ci-deploy');
  const event = (id, env = 'staging') => JSON.stringify({ eventId: id, sentAt: Math.floor(clock.now() / 1000), environment: env, issueKeys: [targetKey] });
  const now = () => String(Math.floor(clock.now() / 1000));
  const hdr = (ts, sig) => ({ 'x-lz-timestamp': [ts], 'x-lz-signature': [sig] });
  const noEffect = async (label, headers, body, status) => {
    const w0 = platform.kvs.writes().length;
    const res = await ci(headers, body);
    ok(res.statusCode === status && platform.kvs.writes().length === w0, `${label} -> ${status}, zero side effects (${res.outputKey})`);
  };
  const b1 = event('evt-1');
  await noEffect('missing signature', { 'x-lz-timestamp': [now()] }, b1, 401);
  await noEffect('bad signature', hdr(now(), sign('wrong', now(), b1)), b1, 401);
  await noEffect('tampered body', hdr(now(), sign(secret, now(), b1)), b1.replace('staging', 'production'), 401);
  const stale = String(Math.floor(clock.now() / 1000) - 301);
  await noEffect('stale timestamp', hdr(stale, sign(secret, stale, b1)), b1, 401);
  const good = await ci(hdr(now(), sign(secret, now(), b1)), b1);
  ok(good.statusCode === 202 && good.outputKey === 'accepted', 'a valid signed event -> 202');
  const tRows = [...ledger(platform).values()].filter((r) => r.issueId === target.issueId);
  ok(tRows.length > 0 && tRows.every((r) => r.deployedEnvs.split(',').includes('staging')), `the issue's ${tRows.length} ledger row(s) show Deployed to staging`);
  const shown = (await ledgerAs(alice.accountId, '11')).changes.find((c) => c.changeId === target.changeId);
  eq(shown?.deployedTo, ['staging'], 'the sprint ledger shows the deployment');
  await noEffect('replayed eventId', hdr(now(), sign(secret, now(), b1)), b1, 200);
  const b2 = event('evt-2', 'production');
  const ts2 = now();
  const res2 = await ci({ 'X-Lz-Timestamp': [ts2], 'X-LZ-SIGNATURE': [sign(secret, ts2, b2)] }, b2);
  ok(res2.statusCode === 202 && [...ledger(platform).values()].filter((r) => r.issueId === target.issueId).every((r) => r.deployedEnvs === 'production,staging'), 'case-varied header names are accepted');
  // contract §14: "rows written later for that issue carry it too"
  const deployed = site.issueById.get(target.issueId);
  const laterEv = site.update(deployed.key, { sprints: deployed.sprints.includes(11) ? deployed.sprints.filter((s) => s !== 11) : [...deployed.sprints, 11] });
  await deliver(laterEv);
  eq([...ledger(platform).values()].find((r) => r.changeId === laterEv.changelog.id && r.sprintId === '11')?.deployedEnvs, 'production,staging', `a row written after the deployments carries them too (${deployed.key})`);

  // ===== R8: Forge LLM ==============================================================================
  const explain = (user) => platform.resolver('jira:sprintAction', 'scope-sprint-ledger', 'explain', {}, { aaid: user.accountId, extension: sprintExt('11') });
  const reply = (calls, extra = {}) => () => ({ choices: [{ finish_reason: 'tool_use', message: { role: 'assistant', content: [], tool_calls: calls } }], usage: { input_tokens: 900, output_tokens: 100 }, ...extra });
  const report = (args) => ({ id: 't', type: 'function', index: 0, function: { name: 'report_scope', arguments: args } });
  clock.advance(11 * 60000);
  const calls0 = platform.llm.calls.length;
  platform.llm.script.push(reply([report({ summary: 'Work was pulled in.', changeIds: [] }), { id: 'x', type: 'function', index: 1, function: { name: 'delete_issue', arguments: { key: targetKey } } }]));
  const e1 = await explain(alice);
  ok(e1.ok && platform.llm.calls.length === calls0 + 1, 'an explanation is one LLM call; a tool call the app does not offer is ignored');
  const e2 = await explain(alice);
  ok(e2.ok && e2.cached && platform.llm.calls.length === calls0 + 1, 'an identical request within 10 minutes is served from the cache');
  const e3 = await explain(bob);
  ok(platform.llm.calls.length === calls0 + 2 && e3.ok !== undefined, "a viewer with different visibility is not served another viewer's answer");
  clock.advance(11 * 60000);
  platform.llm.script.push(() => ({ choices: [{ message: { role: 'assistant', content: [], tool_calls: [report({ summary: 'x', changeIds: [] })] } }] }));
  const e4 = await explain(alice);
  ok(e4.ok === false && /finish reason/.test(e4.error), 'an answer with no finish_reason is a failure, never shown');
  clock.advance(11 * 60000);
  platform.llm.script.push(() => ({ status: 429, json: { code: 'RATE_LIMITED', message: 'Too many requests' } }));
  const e5 = await explain(alice);
  const c5 = platform.llm.calls.length;
  const e6 = await explain(alice);
  ok(e5.ok === false && e5.retryAfter === 20 && e6.ok === false && platform.llm.calls.length === c5, 'a 429 backs off: no new LLM call inside the 20 s backoff');
  clock.advance(21_000);
  platform.llm.script.push(reply([report({ summary: 'Back again.', changeIds: [] })]));
  ok((await explain(alice)).ok, 'after the backoff the call is made again');
  await asAdminPage('saveSettings', { settings: { aiEnabled: false } }, alice.accountId);
  clock.advance(11 * 60000);
  const c7 = platform.llm.calls.length;
  const e7 = await explain(alice);
  ok(e7.ok === false && platform.llm.calls.length === c7, 'the kill switch is enforced server-side');
  await asAdminPage('saveSettings', { settings: { aiEnabled: true, dailyTokenBudget: 1 } }, alice.accountId);
  const e8 = await explain(alice);
  ok(e8.ok === false && /budget/.test(e8.error) && platform.llm.calls.length === c7, 'the daily token budget is enforced server-side');
  await asAdminPage('saveSettings', { settings: { dailyTokenBudget: 200000, backgroundShare: 70 } }, alice.accountId);

  // ===== R2: the background dose ====================================================================
  for (const [hour, pts] of backgroundPerHour(site)) ok(pts <= 1680, `hour ${hour}: background used ${pts} points <= 70% of 2,400`);
  // another invocation already spent 230 of a 10% share (240): the pass must stop and finish next hour
  await asAdminPage('saveSettings', { settings: { backgroundShare: 10 } }, alice.accountId);
  clock.set((Math.floor(clock.now() / HOUR) + 1) * HOUR + 60000);
  const hour = Math.floor(clock.now() / HOUR);
  platform.kvs.plain.set(`dose:${hour}:another-invocation`, 230);
  for (const i of site.issues.filter((k) => !k.deleted).slice(0, 40)) i.updated = clock.now() - 60000; // a big incremental window
  const r0 = site.requests.length;
  await run('reconcile', {}, 'scope-reconcile');
  const spent = site.requests.slice(r0).filter((r) => Math.floor(r.at / HOUR) === hour).reduce((n, r) => n + points(r), 0);
  ok(spent <= 10, `with 230 of 240 points already spent this hour, the scheduled pass spent ${spent}`);
  ok(platform.queue.some((e) => e.body.type === 'reconcile'), 'and handed the rest to the queue');
  await platform.drain();
  ok(site.requests.slice(r0).some((r) => Math.floor(r.at / HOUR) === hour + 1 && r.path.startsWith('/rest/api/3/search/jql')), 'the pass finished in the next hour');
  // a quota 429 pauses all background work until the reset; a person-facing request still works
  clock.set((Math.floor(clock.now() / HOUR) + 1) * HOUR + 120000);
  const toReset = Math.ceil(((Math.floor(clock.now() / HOUR) + 1) * HOUR - clock.now()) / 1000);
  site.rateLimits.push({ match: (m, p, b, as) => as === 'app' && p === '/rest/api/3/field', times: 1, retryAfter: toReset, reason: 'jira-quota-tenant-based' });
  await asAdminPage('saveSettings', { settings: { backgroundShare: 70 } }, alice.accountId);
  const q0 = site.requests.length;
  await run('reconcile', {}, 'scope-reconcile');
  const pausedHour = Math.floor(clock.now() / HOUR);
  const wv = await platform.resolver('dashboards:widget', 'scope-widget', 'widget', { boardId: '1' }, { aaid: alice.accountId, extension: { config: { boardId: '1' } } });
  ok(Array.isArray(wv.sprints), 'a person-facing request in the paused hour is answered');
  await run('on-issue-updated', site.update(free[3].key, { sprints: [...free[3].sprints, 11] }), 'scope-issue-updated');
  await platform.drain();
  const bgPaused = site.requests.slice(q0).filter((r) => BACKGROUND.has(r.kind) && Math.floor(r.at / HOUR) === pausedHour && r.status !== 429);
  ok(bgPaused.length === 0, `after a quota 429 no background request until the hour resets (${bgPaused.length} made)`);
  ok(site.requests.slice(q0).some((r) => BACKGROUND.has(r.kind) && Math.floor(r.at / HOUR) === pausedHour + 1), 'background work resumes after the reset');
  // a burst 429 on one endpoint: the retry waits the Retry-After
  site.rateLimits.push({ match: (m, p, b, as) => as === 'app' && /^\/rest\/agile\/1\.0\/board\/\d+\/configuration$/.test(p), times: 1, retryAfter: 3, reason: 'jira-burst-based' });
  const b0 = site.requests.length;
  await run('reconcile', {}, 'scope-reconcile');
  const conf = site.requests.slice(b0).filter((r) => /\/configuration$/.test(r.path.split('?')[0]));
  const ib = conf.findIndex((r) => r.status === 429);
  ok(ib >= 0 && conf[ib + 1] && conf[ib + 1].at - conf[ib].at >= 3000, `after a burst 429 the endpoint is retried ${conf[ib + 1] ? (conf[ib + 1].at - conf[ib].at) / 1000 : '?'} s later (Retry-After 3)`);
  await platform.drain();
  ledgerMatchesOracle(site, platform, 'at the end');
  statusesMatchOracle(site, 'at the end');
  for (const [h, pts] of backgroundPerHour(site)) if (h >= hour) ok(pts <= (h === hour ? 240 - 230 : 1680), `hour ${h}: background ${pts} points within its share`);

  // ===== a field no active board uses any more =====================================================
  // Board 4 left EST_OPS in (a) and sprint 12 closed in (c); once board 1 switches too, no active sprint's board estimates
  // with EST_OPS, yet sprint 31's changes recorded before (a) show their issue's current EST_OPS value (contract §1): an
  // EST_OPS update still reaches the queue and moves them.
  site.setBoardField(1, EST_WEB);
  clock.advance(HOUR);
  await run('reconcile', {}, 'scope-reconcile');
  await platform.drain();
  const opsRow = [...ledger(platform).values()].find((r) => r.sprintId === '31' && r.estimateField === EST_OPS && !r.deleted);
  const opsIssue = site.issueById.get(opsRow.issueId);
  const opsValue = (opsIssue.fields[EST_OPS] ?? 0) + 3;
  await deliver(site.update(opsIssue.key, { estimate: opsValue }));
  const opsShown = (await ledgerAs(alice.accountId, '31')).changes.find((c) => c.changeId === opsRow.changeId);
  ok(opsShown?.points === opsValue, `a change recorded under a field no active board uses any more shows that field's current value (${opsIssue.key}: ${opsShown?.points}, want ${opsValue})`);
  statusesMatchOracle(site, 'after a field left every active board');
  await widgetMatchesOracle(site, platform, alice.accountId, 'after a field left every active board');

  console.log(`\n${failures ? `${failures} FAILED` : 'ALL PASSED'}  (Jira requests ${site.requests.length}, KVS ops ${platform.kvs.ops.length})`);
  process.exit(failures ? 1 : 0);
}

main().catch((e) => {
  console.error('CRASH', e);
  process.exit(2);
});
