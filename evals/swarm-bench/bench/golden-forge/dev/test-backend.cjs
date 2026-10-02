// The golden's backend proof on WP3's own bed: backfill through pagination and 429s, the event path
// under duplicates / reordering / loss / retry requests, heal, a rerun that writes nothing, and the
// person-facing reads (resolvers, Rovo action) against an independently computed oracle.
// Usage: node dev/test-backend.cjs [outDir]
'use strict';

const path = require('path');
const os = require('os');
const { createSite, oracle, fmtCreep } = require('./site.cjs');
const { createPlatform } = require('./runtime.cjs');

let failures = 0;
const ok = (cond, msg) => {
  if (!cond) failures += 1;
  console.log(`${cond ? 'PASS' : 'FAIL'}  ${msg}`);
};
const eq = (a, b, msg) => ok(JSON.stringify(a) === JSON.stringify(b), `${msg}${JSON.stringify(a) === JSON.stringify(b) ? '' : `\n      got      ${JSON.stringify(a)}\n      expected ${JSON.stringify(b)}`}`);

const fmtPoints = (n) => String(Math.round(n * 1e6) / 1e6);

function ledgerRows(platform) {
  const m = platform.kvs.entities.get('scope-change') ?? new Map();
  return [...m.values()];
}

function expectedRowSet(truth) {
  const out = new Map();
  for (const [sid, t] of Object.entries(truth)) for (const c of t.changes) out.set(`${c.changeId}:${sid}`, { ...c, sprintId: sid });
  return out;
}

async function main() {
  const outDir = process.argv[2] ?? path.join(os.tmpdir(), 'golden-forge-backend');
  const site = createSite({ seed: 7 });
  const platform = createPlatform({ site });
  await platform.build(path.join(outDir, 'bundle'));
  const { alice, bob } = site.users;

  // ---------- 1. first scheduled run: backfill through board pagination, search pages and 429s ----------
  let searchCalls = 0;
  site.rateLimits.push({ match: (m, p) => p === '/rest/api/3/search/jql' && (searchCalls += 1) === 2, times: 1, retryAfter: 2 });
  site.rateLimits.push({ match: (m, p) => p === '/rest/agile/1.0/board', times: 1, retryAfter: 1 });
  const t0 = site.requests.length;
  await platform.invoke('reconcile', { context: { cloudId: 'golden', moduleKey: 'scope-reconcile' } }, { moduleKey: 'scope-reconcile' });
  const backfillReqs = site.requests.slice(t0);
  const truth0 = oracle(site);
  const rows0 = ledgerRows(platform);
  const expected0 = expectedRowSet(truth0);
  ok(rows0.length === expected0.size && rows0.every((r) => expected0.has(`${r.changeId}:${r.sprintId}`)), `backfill: ${rows0.length} rows == oracle ${expected0.size} (every change of every active sprint)`);
  ok(rows0.every((r) => r.source === 'reconcile'), 'backfill rows are source=reconcile');
  ok(!backfillReqs.some((r) => r.path.startsWith('/rest/api/3/search?') || r.path === '/rest/api/3/search'), 'no call to the removed /rest/api/3/search');
  ok(backfillReqs.every((r) => r.as === 'app'), 'background work is asApp only');
  const i429 = backfillReqs.findIndex((r) => r.status === 429 && r.path.startsWith('/rest/api/3/search/jql'));
  const retry = backfillReqs[i429 + 1];
  ok(i429 >= 0 && retry && retry.path.startsWith('/rest/api/3/search/jql') && retry.at - backfillReqs[i429].at >= 2000, `search 429 waited ${retry ? retry.at - backfillReqs[i429].at : '?'} ms >= Retry-After 2 s, then retried the same page`);
  ok(JSON.stringify(retry?.body) === JSON.stringify(backfillReqs[i429]?.body), 'the retried search carried the same nextPageToken');
  const removedOnly = [...expected0.values()].filter((c) => c.kind === 'removed' && !site.issueByKey.get(c.issueKey).sprints.some((s) => site.sprintById.get(s).state === 'active'));
  ok(removedOnly.length > 0 && removedOnly.every((c) => rows0.some((r) => r.changeId === c.changeId)), `backfill found ${removedOnly.length} removals of issues that have left every active sprint`);
  console.log(`      backfill used ${backfillReqs.length} Jira requests`);

  async function widgetTotals(boardId, aaid = alice.accountId) {
    return platform.resolver('dashboards:widget', 'scope-widget', 'widget', { boardId }, { aaid, extension: { config: { boardId } } });
  }
  async function checkTotals(label) {
    const truth = oracle(site);
    for (const board of [1, 2, 4]) {
      const w = await widgetTotals(String(board));
      const want = site.sprints.filter((s) => s.board === board && s.state === 'active').sort((a, b) => a.start - b.start);
      eq(w.sprints.map((s) => s.id), want.map((s) => String(s.id)), `${label}: board ${board} widget sprints ordered by startDate`);
      for (const s of w.sprints) {
        const t = truth[s.id];
        eq(s.text, { committed: fmtPoints(t.committed), added: fmtPoints(t.added), removed: fmtPoints(t.removed), creep: fmtCreep(t.creep) }, `${label}: sprint ${s.id} numbers`);
      }
    }
  }
  await checkTotals('after backfill');

  // ---------- 2. live events: duplicates, reordering, loss, estimate changes, irrelevant edits ----------
  const ops11 = site.issues.filter((i) => i.project === 'OPS' && !i.restricted);
  const inSprint = (s) => ops11.filter((i) => i.sprints.includes(s));
  const notIn = ops11.filter((i) => !i.sprints.some((s) => site.sprintById.get(s).state === 'active'));
  const a = notIn[0];
  const b = inSprint(11)[0];
  const c = inSprint(11)[1];
  const d = notIn[1];
  const e = inSprint(12)[0];
  const ev = [];
  ev.push(site.update(a.key, { sprints: [...a.sprints.filter((s) => s === 10), 11] })); // 0 add a -> 11
  ev.push(site.update(b.key, { sprints: b.sprints.filter((s) => s !== 11) })); //               1 remove b from 11
  ev.push(site.update(c.key, { sprints: [...c.sprints.filter((s) => s !== 11), 12] })); //   2 move c 11 -> 12 (two rows)
  ev.push(site.update(a.key, { estimate: 21 })); //                                            3 re-estimate a
  ev.push(site.update(d.key, { sprints: [11] })); //                                           4 add d -> 11 (event LOST)
  ev.push(site.update(e.key, { summary: 'renamed' })); //                                      5 irrelevant
  ev.push(site.update(b.key, { estimate: 3 })); //                                             6 re-estimate a removed issue
  const lost = new Set([4]);
  const order = [2, 0, 1, 0, 3, 5, 2, 6, 1]; // reordered, with duplicates; 4 never arrives
  // one 429 on the consumer's issue read: must become a retry request, never a sleep or a throw
  site.rateLimits.push({ match: (m, p, body, as) => m === 'GET' && p === `/rest/api/3/issue/${a.id}`, times: 1, retryAfter: 17 });

  const reqBefore = site.requests.length;
  const pushesBefore = platform.queuePushes.length;
  await platform.invoke('on-issue-updated', ev[5], { moduleKey: 'scope-issue-updated' });
  ok(site.requests.length === reqBefore && platform.queuePushes.length === pushesBefore, 'an update touching neither sprint nor estimate does no Jira or queue work');

  for (const n of order) await platform.invoke('on-issue-updated', ev[n], { moduleKey: 'scope-issue-updated' });
  const deliveries = await platform.drain();
  const retried = deliveries.filter((dl) => dl.result && dl.result._retry);
  ok(retried.length === 1 && retried[0].result.retryOptions.retryAfter >= 17, `consumer 429 -> InvocationError retryAfter ${retried[0]?.result?.retryOptions?.retryAfter} >= Retry-After 17`);
  ok(deliveries.every((dl) => !dl.error), 'no consumer delivery threw');

  const truth1 = oracle(site);
  const rows1 = ledgerRows(platform);
  const want1 = expectedRowSet(truth1);
  const lostIds = new Set([...lost].map((n) => ev[n].changelog.id));
  const missing = [...want1.keys()].filter((k) => !rows1.some((r) => `${r.changeId}:${r.sprintId}` === k));
  ok(missing.length > 0 && missing.every((k) => lostIds.has(k.split(':')[0])), `after events: only the lost event's change is missing (${missing.join(', ')})`);
  ok(rows1.length === want1.size - missing.length, `exactly one row per change under duplicates: ${rows1.length} rows`);
  const evRows = rows1.filter((r) => ev.some((x) => x.changelog.id === r.changeId));
  ok(evRows.length === 4 && evRows.every((r) => r.source === 'event'), `the delivered sprint changes are 4 rows with source=event (c's move is two)`);
  const moveRows = rows1.filter((r) => r.changeId === ev[2].changelog.id).map((r) => `${r.sprintId}:${r.kind}`).sort();
  eq(moveRows, ['11:removed', '12:added'], 'one changelog entry moving an issue between sprints is a removed in one and an added in the other');

  const evPubs = platform.realtime.published.filter((pb) => pb.moduleType === 'consumer');
  const evSprints = new Set(evPubs.flatMap((pb) => pb.payload.sprintIds));
  ok(evPubs.length > 0 && evPubs.every((pb) => pb.isGlobal), `consumer announced ledger changes with publishGlobal (${evPubs.length} publications)`);
  ok(['11', '12'].every((sid) => evSprints.has(sid)), `the announced sprint ids cover the sprints the events changed (${[...evSprints].join(',')})`);
  ok(platform.realtime.published.every((pb) => JSON.stringify(Object.keys(pb.payload)) === '["sprintIds"]' && pb.payload.sprintIds.every((x) => /^\d+$/.test(x))), 'every realtime payload carries sprint ids only');
  ok(platform.realtime.signed.length === 0, 'no realtime token is signed outside a resolver');

  // ---------- 3. heal: the next scheduled run records what the stream missed, then a rerun writes nothing ----------
  await platform.invoke('reconcile', {}, { moduleKey: 'scope-reconcile' });
  const rows2 = ledgerRows(platform);
  const healed = rows2.find((r) => r.changeId === ev[4].changelog.id);
  ok(rows2.length === want1.size && healed && healed.source === 'reconcile', `heal recorded the lost change as reconcile; ledger == oracle (${rows2.length})`);
  ok(rows2.filter((r) => r.source === 'event').length === 4, 'heal did not overwrite event rows');
  await checkTotals('after heal');

  const writesBefore = platform.kvs.writes().length;
  const pushes2 = platform.queuePushes.length;
  const pubs2 = platform.realtime.published.length;
  await platform.invoke('reconcile', {}, { moduleKey: 'scope-reconcile' });
  ok(platform.realtime.published.length === pubs2, 'a scheduled run with nothing new announces nothing');
  ok(platform.kvs.writes().length === writesBefore && platform.queuePushes.length === pushes2, `a scheduled run with nothing new writes nothing (${platform.kvs.writes().length - writesBefore} writes)`);

  // redelivering an old event again changes nothing
  const w3 = platform.kvs.writes().length;
  await platform.invoke('on-issue-updated', ev[0], { moduleKey: 'scope-issue-updated' });
  await platform.drain();
  ok(platform.kvs.writes().length === w3 && ledgerRows(platform).length === rows2.length, `a late duplicate after heal writes nothing ${JSON.stringify(platform.kvs.writes().slice(w3)).slice(0, 400)}`);

  // ---------- 4. what a person sees ----------
  const truth = oracle(site);
  const sprintExt = (id) => ({ type: 'jira:sprintAction', sprint: { id: Number(id), state: 'active' }, board: { id: 1, type: 'scrum' } });
  for (const [who, user] of [['alice', alice], ['bob', bob]]) {
    for (const sid of ['11', '21']) {
      const v = await platform.resolver('jira:sprintAction', 'scope-sprint-ledger', 'sprintLedger', {}, { aaid: user.accountId, extension: sprintExt(sid) });
      const t = truth[sid];
      const vis = t.changes.filter((c) => user.browse(site.issueById.get(c.issueId)));
      eq(v.changes.map((r) => r.changeId), vis.map((c) => c.changeId), `${who} sprint ${sid}: visible rows in table order (${vis.length})`);
      eq(v.hiddenCount, t.changes.length - vis.length, `${who} sprint ${sid}: hidden count ${t.changes.length - vis.length}`);
      eq(v.text, { committed: fmtPoints(t.committed), added: fmtPoints(t.added), removed: fmtPoints(t.removed), creep: fmtCreep(t.creep) }, `${who} sprint ${sid}: team totals are the same for everyone`);
      const act = await platform.invoke('rovo-get-sprint-scope', { sprintId: sid, context: { accountId: user.accountId } }, { aaid: user.accountId, moduleKey: 'get-sprint-scope' });
      eq(
        { committed: act.committed, added: act.added, removed: act.removed, creepPercent: act.creepPercent, hiddenChanges: act.hiddenChanges, ids: act.changes.map((x) => x.changeId) },
        { committed: t.committed, added: t.added, removed: t.removed, creepPercent: t.creep, hiddenChanges: t.changes.length - vis.length, ids: vis.map((x) => x.changeId) },
        `${who} sprint ${sid}: Rovo action`,
      );
      ok(act.changes.every((x) => /Z$/.test(x.at) && vis.find((y) => y.changeId === x.changeId).at === Date.parse(x.at)), `${who} sprint ${sid}: Rovo 'at' are UTC instants equal to the changelog created`);
    }
  }
  const bobRows = platform.kvs.entities.get('scope-change');
  ok([...bobRows.values()].some((r) => !bob.browse(site.issueById.get(r.issueId))), 'the bed holds changes bob cannot browse (the permission check is exercised)');
  eq(await platform.invoke('rovo-get-sprint-scope', { sprintId: '999' }, { aaid: alice.accountId }), { error: 'No sprint with id 999 exists on this site.' }, 'Rovo: unknown sprintId -> {error}');
  ok(typeof (await platform.invoke('rovo-get-sprint-scope', {}, { aaid: alice.accountId })).error === 'string', 'Rovo: missing sprintId -> {error}');
  const fut = await platform.resolver('jira:sprintAction', 'scope-sprint-ledger', 'sprintLedger', {}, { aaid: alice.accountId, extension: sprintExt('13') });
  ok(fut.notStarted === true, 'sprint action: a future sprint is not-started');

  // ---------- 5. the comment: ADF, as the viewer, exactly one through a 429 ----------
  const sprint11 = await platform.resolver('jira:sprintAction', 'scope-sprint-ledger', 'sprintLedger', {}, { aaid: bob.accountId, extension: sprintExt('11') });
  const target = sprint11.changes[0];
  site.rateLimits.push({ match: (m, p) => m === 'POST' && p.endsWith('/comment'), times: 1, retryAfter: 1 });
  const c0 = site.comments.length;
  const posted = await platform.resolver('jira:sprintAction', 'scope-sprint-ledger', 'postSummary', { changeId: target.changeId }, { aaid: bob.accountId, extension: sprintExt('11') });
  const newComments = site.comments.slice(c0);
  ok(posted.ok === true && newComments.length === 1, `postSummary through a 429: ${newComments.length} comment`);
  const text = JSON.stringify(newComments[0]?.body);
  ok(newComments[0]?.author === bob.accountId && newComments[0].issueKey === target.issueKey, 'the comment is authored by the viewer on the selected change\'s issue');
  ok(text.includes(target.issueKey) && text.includes(site.sprintById.get(11).name) && text.includes(fmtCreep(truth['11'].creep)), `the ADF names the issue key, sprint name and creep (${fmtCreep(truth['11'].creep)})`);
  site.rateLimits.push({ match: (m, p) => m === 'POST' && p.endsWith('/comment'), times: 1, retryAfter: 9 });
  const c1 = site.comments.length;
  const deferred = await platform.resolver('jira:sprintAction', 'scope-sprint-ledger', 'postSummary', { changeId: target.changeId }, { aaid: bob.accountId, extension: sprintExt('11') });
  ok(deferred.rateLimited === true && deferred.retryAfter === 9 && site.comments.length === c1, 'a Retry-After longer than the resolver can wait goes back to the page, nothing posted');

  // ---------- 6. Forge LLM explanation ----------
  const explainAs = (user) => platform.resolver('jira:sprintAction', 'scope-sprint-ledger', 'explain', {}, { aaid: user.accountId, extension: sprintExt('11') });
  const bobView = await platform.resolver('jira:sprintAction', 'scope-sprint-ledger', 'sprintLedger', {}, { aaid: bob.accountId, extension: sprintExt('11') });
  const hiddenIds = truth['11'].changes.filter((c) => !bob.browse(site.issueById.get(c.issueId))).map((c) => c.changeId);
  const toolReply = (args) => () => ({ choices: [{ finish_reason: 'tool_use', message: { role: 'assistant', content: [], tool_calls: [{ id: 't', type: 'function', index: 0, function: { name: 'report_scope', arguments: args } }] } }] });
  platform.llm.script.push(toolReply({ summary: 'Work kept being pulled in after the start.', changeIds: [bobView.changes[1].changeId, hiddenIds[0], 'nope', bobView.changes[0].changeId] }));
  let ex = await explainAs(bob);
  const call0 = platform.llm.calls.at(-1);
  ok(ex.ok && call0.model === 'claude-sonnet-4-7', `explain uses a model list() reports active (${call0.model})`);
  ok(JSON.stringify(call0.body.tool_choice) === JSON.stringify({ type: 'function', function: { name: 'report_scope' } }) && call0.body.tools[0].function.name === 'report_scope', 'explain forces the report_scope tool');
  const sent = JSON.stringify(call0.body);
  ok(hiddenIds.length > 0 && hiddenIds.every((id) => !sent.includes(`"${id}"`)) && truth['11'].changes.filter((c) => hiddenIds.includes(c.changeId)).every((c) => !sent.includes(c.issueKey)), 'the prompt holds nothing the viewer cannot see');
  eq(ex.changes.map((c) => c.changeId), [bobView.changes[1].changeId, bobView.changes[0].changeId], 'only returned ids that are visible changes of this sprint are kept');
  eq(ex.summary, 'Work kept being pulled in after the start.', 'a summary without digits is shown as is');
  platform.llm.script.push(toolReply({ summary: 'Scope grew 40% because 12 points came in.', changeIds: [] }));
  ex = await explainAs(alice);
  ok(ex.ok && !/40%|12 points/.test(ex.summary) && ex.summary.includes(fmtCreep(truth['11'].creep)), `a summary with digits is replaced by the ledger's own sentence ("${ex.summary}")`);
  platform.llm.script.push(() => ({ choices: [{ finish_reason: 'end_turn', message: { role: 'assistant', content: [{ type: 'text', text: 'I cannot help with that.' }] } }] }));
  ex = await explainAs(alice);
  ok(ex.ok === false && typeof ex.error === 'string', 'a refusal (no tool call) is an error answer, not a throw');
  platform.llm.script.push(toolReply({ summary: 42, changeIds: 'all' }));
  ex = await explainAs(alice);
  ok(ex.ok === false && typeof ex.error === 'string', 'malformed tool arguments are an error answer');
  platform.llm.script.push(() => ({ status: 500, json: { code: 'INTERNAL', message: 'model unavailable' } }));
  ex = await explainAs(alice);
  ok(ex.ok === false && /LLM/.test(ex.error), 'an LLM error is an error answer');

  // ---------- 7. scopes: every call the golden made is covered by the manifest ----------
  ok(!site.requests.some((r) => r.status === 401), 'no Jira call was refused for a missing scope');
  console.log(`\n${failures ? `${failures} FAILED` : 'ALL PASSED'}  (Jira requests total ${site.requests.length}, KVS ops ${platform.kvs.ops.length})`);
  process.exit(failures ? 1 : 0);
}

main().catch((e) => {
  console.error('CRASH', e);
  process.exit(2);
});
