'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { createSite } = require('../../site/site.cjs');
const { createEmulator } = require('../lib/emulator.cjs');
const { ensureKit, scratch } = require('./helpers.cjs');

async function rig() {
  const site = await createSite({ seed: '0123456789abcdef' });
  let t = site.state.now();
  const call = async (body, as = site.pack.viewer, scopes = ['read:jira-work']) => {
    t = Math.max(t, site.state.now()) + 1000;
    const response = await fetch(site.url + '/rest/api/3/jql/match', { method: 'POST', headers: {
      'content-type': 'application/json', 'x-forge-as': as === site.pack.appAccountId ? 'app' : 'user',
      'x-forge-account': as, 'x-forge-scopes': JSON.stringify(scopes), 'x-forge-vtime': String(t),
      'x-forge-source': 'resolver', 'x-forge-module-type': 'jira:adminPage',
    }, body: JSON.stringify(body) });
    return { status: response.status, body: await response.json(), entry: site.log.at(-1) };
  };
  const match = (ids, jqls, as) => call({ issueIds: ids.map(Number), jqls }, as);
  return { site, call, match, close: () => site.stop() };
}
const row = (ids, errors = []) => ({ matchedIssues: ids.map(Number), errors });

test('JQL match evaluates explicit IDs for each query in order, including issue aliases and unrestricted predicates', async () => {
  const w = await rig();
  try {
    const visible = w.site.pack.issues.filter((issue) => !issue.hiddenFrom.includes(w.site.pack.viewer));
    const [one, two] = visible;
    const input = [one.id, two.id, 999999999];
    const result = await w.match(input, [`issue in (${one.id},${two.id})`, `id = ${two.id}`, 'summary is not EMPTY', '']);
    assert.equal(result.status, 200, JSON.stringify(result.body));
    assert.deepEqual(result.body, { matches: [row([one.id, two.id]), row([two.id]), row([one.id, two.id]), row([one.id, two.id])] });
    assert.equal(result.entry.points, 2, 'read-like POST charges the documented benchmark response-envelope interpretation');
    assert.equal(result.entry.kind, 'person');
    assert.deepEqual(w.site.harnessMissing, []);
  } finally { await w.close(); }
});

test('JQL match separates per-query JQL errors from malformed request bodies and preserves official schema boundaries', async () => {
  const w = await rig();
  try {
    const issue = w.site.pack.issues.find((i) => !i.hiddenFrom.includes(w.site.pack.viewer));
    const mixed = await w.match([issue.id], [`issue = ${issue.id}`, 'broken = value', 'issue in (', `key = ${issue.key}`]);
    assert.equal(mixed.status, 200);
    assert.deepEqual(mixed.body.matches[0], row([issue.id]));
    assert.deepEqual(mixed.body.matches[3], row([issue.id]));
    for (const index of [1, 2]) {
      assert.deepEqual(mixed.body.matches[index].matchedIssues, []);
      assert.equal(mixed.body.matches[index].errors.length, 1);
      assert.ok(mixed.body.matches[index].errors[0].length);
    }
    const fifty = w.site.pack.issues.slice(0, 50).map((i) => Number(i.id));
    const boundary = await w.call({ issueIds: fifty, jqls: Array(10).fill('summary is not EMPTY') }, w.site.pack.appAccountId);
    assert.equal(boundary.status, 200);
    assert.equal(boundary.body.matches.length, 10);
    assert.ok(boundary.body.matches.every((entry) => entry.matchedIssues.length === 50 && entry.errors.length === 0));
    for (const body of [null, [], {}, { issueIds: [Number(issue.id)] }, { jqls: [''] },
      { issueIds: fifty.concat(999999999), jqls: [''] }, { issueIds: [], jqls: Array(11).fill('') },
      { issueIds: [issue.id], jqls: [''] }, { issueIds: [true], jqls: [''] }, { issueIds: [1.1], jqls: [''] },
      { issueIds: [1, 1], jqls: [''] }, { issueIds: [1], jqls: [null] }, { issueIds: [], jqls: [], invented: true }]) {
      assert.equal((await w.call(body)).status, 400, JSON.stringify(body));
    }
    assert.deepEqual((await w.call({ issueIds: [], jqls: [] })).body, { matches: [] }, 'pinned schema has no minimum array size');
    assert.deepEqual((await w.call({ issueIds: [], jqls: [''] })).body, { matches: [row([])] });
    assert.equal((await w.call({ issueIds: [Number(issue.id)], jqls: [''] }, w.site.pack.viewer, ['storage:app'])).status, 401);
    assert.equal((await w.call({ issueIds: [Number(issue.id)], jqls: [''] }, w.site.pack.viewer, ['read:issue-details:jira'])).status, 200);
    assert.deepEqual(w.site.harnessMissing, []);
  } finally { await w.close(); }
});

test('JQL match cannot expose an issue hidden by issue security; the same ID is positively visible asApp', async () => {
  const w = await rig();
  try {
    const viewer = w.site.pack.viewer;
    const hidden = w.site.pack.issues.find((i) => i.hiddenFrom.includes(viewer));
    assert.ok(hidden);
    const query = [`issue in (${hidden.id})`];
    assert.deepEqual((await w.match([hidden.id], query, w.site.pack.appAccountId)).body, { matches: [row([hidden.id])] });
    assert.deepEqual((await w.match([hidden.id], query, viewer)).body, { matches: [row([])] });
    assert.deepEqual((await w.match([hidden.id], query, w.site.pack.appAccountId)).body, { matches: [row([hidden.id])] });
    assert.deepEqual(w.site.harnessMissing, []);
  } finally { await w.close(); }
});

test('JQL match observes a same-object Browse revocation immediately and omits deleted IDs', async () => {
  const w = await rig();
  try {
    const viewer = w.site.pack.viewer;
    const visible = w.site.pack.issues.find((i) => !i.hiddenFrom.includes(viewer) && w.site.state.liveRemainingFor(i.id) === 0);
    assert.ok(visible, 'deletion control has no pending live script');
    const query = [`issue = ${visible.id}`];
    assert.deepEqual((await w.match([visible.id], query, viewer)).body, { matches: [row([visible.id])] });
    w.site.state.revokeBrowse(viewer, visible.projectKey);
    assert.deepEqual((await w.match([visible.id], query, viewer)).body, { matches: [row([])] });
    assert.deepEqual((await w.match([visible.id], query, w.site.pack.appAccountId)).body, { matches: [row([visible.id])] }, 'same issue still exists after the person loses Browse');
    w.site.state.deleteIssue(visible.id);
    assert.deepEqual((await w.match([visible.id], query, w.site.pack.appAccountId)).body, { matches: [row([])] }, 'same issue ceases to exist even for the app after deletion');
    const other = w.site.pack.issues.find((i) => i.projectKey !== visible.projectKey && !i.hiddenFrom.includes(viewer)
      && w.site.state.liveRemainingFor(i.id) === 0);
    assert.ok(other, 'same-person deletion control remains in a browsable project');
    const otherQuery = [`issue = ${other.id}`];
    assert.deepEqual((await w.match([other.id], otherQuery, viewer)).body, { matches: [row([other.id])] });
    w.site.state.deleteIssue(other.id);
    assert.deepEqual((await w.match([other.id], otherQuery, viewer)).body, { matches: [row([])] }, 'the still-authorized person no longer matches the deleted issue');
    assert.deepEqual((await w.match([other.id], otherQuery, w.site.pack.appAccountId)).body, { matches: [row([])] });
    assert.deepEqual(w.site.harnessMissing, []);
  } finally { await w.close(); }
});

test('valid Jira syntax outside the modeled subset remains an explicit harness gap during JQL match', async () => {
  const w = await rig();
  try {
    const result = await w.match([w.site.pack.issues[0].id], ['component is not EMPTY']);
    assert.equal(result.status, 501);
    assert.equal(result.body.code, 'EMULATOR_NOT_MODELLED');
    assert.ok(w.site.harnessMissing.some((entry) => String(entry.what).includes("JQL field 'component'")));
  } finally { await w.close(); }
});

const SOURCE = `import api, { route } from '@forge/api';
import Resolver from '@forge/resolver';
import { kvs } from '@forge/kvs';
const resolver = new Resolver();
resolver.define('match', async ({ payload }) => {
  const response = await api.asUser().requestJira(route\`/rest/api/3/jql/match\`, {
    method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(payload.body),
  });
  return { status: response.status, body: await response.json() };
});
export const resolve = resolver.getDefinitions();
export const scoped = async () => ({ jql: 'issue in (' + (await kvs.get('selected-ids')).join(',') + ')' });`;
function writeApp(dir) {
  fs.mkdirSync(path.join(dir, 'src'), { recursive: true });
  fs.mkdirSync(path.join(dir, 'static'));
  fs.writeFileSync(path.join(dir, 'src', 'index.js'), SOURCE);
  fs.writeFileSync(path.join(dir, 'static', 'index.html'), '<!doctype html><title>JQL match SDK control</title>');
  fs.writeFileSync(path.join(dir, 'manifest.yml'), `modules:
  jira:adminPage:
    - key: match-admin
      title: JQL match SDK control
      resource: main
      render: native
      resolver:
        function: match-resolver
  jira:jqlFunction:
    - key: match-scoped
      name: matchScoped
      types: [issue]
      operators: [in]
      function: match-scoped
  function:
    - key: match-resolver
      handler: index.resolve
    - key: match-scoped
      handler: index.scoped
resources:
  - key: main
    path: static
permissions:
  scopes: [read:jira-work, storage:app]
app:
  id: ari:cloud:ecosystem::app/00000000-0000-0000-0000-000000000000
  runtime:
    name: nodejs22.x
`);
}
for (const remote of [false, true]) test(`actual Forge SDK asUser JQL match with ${remote ? 'remote dev' : 'in-process scoring'} native precomputation host`, { timeout: 120000 }, async () => {
  ensureKit();
  const w = await rig();
  const dir = scratch('jql-match');
  writeApp(dir);
  let emu;
  const outputs = [];
  try {
    emu = await createEmulator({ appDir: dir, site: remote ? { url: w.site.url, adminUrl: w.site.adminUrl } : w.site,
      onInvocation: (_, output) => outputs.push(output) });
    assert.ok((await emu.build()).functions.every((fn) => fn.loaded));
    const viewer = w.site.pack.viewer;
    const hidden = w.site.pack.issues.find((i) => i.hiddenFrom.includes(viewer));
    const visible = w.site.pack.issues.find((i) => !i.hiddenFrom.includes(viewer));
    assert.ok(hidden && visible);
    assert.equal(emu.kvs.handle('/api/v1/set', { key: 'selected-ids', value: [hidden.id, visible.id] }).status, 204);
    const body = { issueIds: [hidden.id, visible.id].map(Number), jqls: ['issue in matchScoped()'] };
    const invoke = (asUser) => emu.callResolver({ moduleKey: 'match-admin', functionKey: 'match',
      payload: { body, requestedAccount: w.site.pack.appAccountId }, asUser });
    const first = await invoke(viewer);
    assert.equal(first.ok, true, JSON.stringify(first.error));
    assert.equal(first.result.status, 200, JSON.stringify(first.result));
    assert.deepEqual(first.result.body, { matches: [row([visible.id])] });
    assert.equal(first.asUser, viewer);
    const matchedRequest = w.site.log.find((entry) => entry.op === 'POST /rest/api/3/jql/match');
    assert.equal(matchedRequest.accountId, viewer, 'a payload claim cannot promote the person to the app');
    assert.equal(matchedRequest.jqlExpansions[0].calls[0].invoked, true);
    const id = matchedRequest.jqlExpansions[0].calls[0].precomputationId;
    const sameObjectApp = await w.call(body, w.site.pack.appAccountId);
    assert.deepEqual(sameObjectApp.body, { matches: [row([hidden.id, visible.id])] });
    assert.equal(sameObjectApp.entry.jqlExpansions[0].calls[0].precomputationId, id);
    assert.equal(sameObjectApp.entry.jqlExpansions[0].calls[0].invoked, false, 'asApp reuses the caller-independent native mapping');
    assert.equal(outputs.filter((output) => output.moduleType === 'jira:jqlFunction').length, 1);
    w.site.state.revokeBrowse(viewer, visible.projectKey);
    const revoked = await invoke(viewer);
    assert.equal(revoked.ok, true, JSON.stringify(revoked.error));
    assert.deepEqual(revoked.result.body, { matches: [row([])] });
    assert.deepEqual((await w.call(body, w.site.pack.appAccountId)).body, { matches: [row([hidden.id, visible.id])] });
    assert.equal(outputs.filter((output) => output.moduleType === 'jira:jqlFunction').length, 1, 'cached JQL expansion never caches final user visibility');
    assert.deepEqual(emu.harnessMissing, []);
    assert.deepEqual(w.site.harnessMissing, []);
  } finally { if (emu) await emu.close(); await w.close(); fs.rmSync(dir, { recursive: true, force: true }); }
});
