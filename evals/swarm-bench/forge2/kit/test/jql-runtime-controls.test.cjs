'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { execFileSync } = require('node:child_process');
const { ensureKit, scratch, REPO } = require('./helpers.cjs');
const { createSite } = require('../../site/site.cjs');
const { createEmulator } = require('../lib/emulator.cjs');
const GOLDEN = path.join(REPO, 'bench', 'golden-forge2');
const CONTROLS = path.join(REPO, 'bench', 'forge2_controls.py');

async function rig(variant) {
  ensureKit();
  const scratchDir = scratch('jql-runtime-control');
  const dir = path.join(scratchDir, 'app');
  if (variant === 'golden') fs.cpSync(GOLDEN, dir, { recursive: true, filter: (file) => !file.split(path.sep).includes('node_modules') });
  else execFileSync('python3', ['-B', CONTROLS, '--materialise-runtime', variant, '--tree', dir]);
  fs.writeFileSync(path.join(dir, 'src', 'control.js'), `import { refreshScopePrecomputations } from './jql';
export const refresh = async (event) => refreshScopePrecomputations(event.sprintIds, { maxWaitSeconds: 0 });
`);
  const manifest = path.join(dir, 'manifest.yml');
  const text = fs.readFileSync(manifest, 'utf8');
  assert.equal(text.split('\nresources:\n').length, 2);
  fs.writeFileSync(manifest, text.replace('\nresources:\n', '\n    - key: control-refresh\n      handler: control.refresh\nresources:\n'));
  const site = await createSite({ seed: '0123456789abcdef' });
  const outputs = [];
  const emu = await createEmulator({ appDir: dir, site, onInvocation: (_, out) => outputs.push(out) });
  try { assert.ok((await emu.build()).functions.every((f) => f.loaded), JSON.stringify(emu.harnessMissing)); }
  catch (error) { await emu.close(); await site.stop(); fs.rmSync(scratchDir, { recursive: true, force: true }); throw error; }
  const viewer = site.pack.viewer;
  const hidden = site.pack.issues.find((issue) => issue.hiddenFrom.includes(viewer));
  const [visible, next] = site.pack.issues.filter((issue) => !issue.hiddenFrom.includes(viewer)).slice(0, 2);
  const peer = site.pack.users.find((user) => user.accountId !== viewer && !hidden.hiddenFrom.includes(user.accountId)).accountId;
  const sprint = site.pack.sprints.find((sp) => sp.state === 'active' && Date.parse(sp.startDate) < site.state.now());
  assert.ok(sprint && hidden && visible && next, 'same-object issue and sprint controls exist');
  assert.equal(emu.kvs.handle('/api/v1/set', { key: 'migration', value: { complete: true } }).status, 204);
  const addRow = (issue, number) => {
    const value = { sprintId: String(sprint.id), at: site.state.now() - 30000 + number, changeId: String(number), kind: 'added',
      issueId: String(issue.id), issueKey: issue.key, estimate: 1, boardId: String(sprint.originBoardId), estimateField: 'customfield_10016',
      deleted: false, deployedEnvs: '', authorId: viewer, authorName: 'Control author', source: 'event' };
    assert.equal(emu.kvs.handle('/api/v1/entity/set', { entityName: 'scope-ledger', key: `${number}:${sprint.id}`, value }).status, 204);
  };
  addRow(hidden, 1); addRow(visible, 2);
  const search = async (accountId, operator = 'in') => {
    const response = await fetch(site.url + '/rest/api/3/search/jql', { method: 'POST', headers: {
      'content-type': 'application/json', 'x-forge-as': 'user', 'x-forge-account': accountId,
      'x-forge-scopes': JSON.stringify(emu.manifest.permissions.scopes), 'x-forge-source': 'frontend',
    }, body: JSON.stringify({ jql: `issue ${operator} scopeChanged("${sprint.id}")`, fields: ['summary'], maxResults: 100 }) });
    return { status: response.status, body: await response.json(), expansion: site.log.filter((entry) => entry.jqlExpansion).at(-1)?.jqlExpansion };
  };
  const refresh = () => emu.invoke('control-refresh', { event: { sprintIds: [String(sprint.id)] } });
  return { site, emu, outputs, viewer, peer, hidden, visible, next, sprint, addRow, search, refresh,
    close: async () => { await emu.close(); await site.stop(); fs.rmSync(scratchDir, { recursive: true, force: true }); } };
}
const ids = (result) => result.body.issues.map((issue) => issue.id).sort();

for (const variant of ['golden', 'm_jql_stale_precomputation']) test(`${variant}: native precomputation changes only when the actual app refreshes it`, { timeout: 120000 }, async () => {
  const w = await rig(variant);
  try {
    const first = await w.search(w.peer);
    assert.equal(first.status, 200, JSON.stringify(first.body));
    assert.deepEqual(ids(first), [w.hidden.id, w.visible.id].sort());
    assert.equal(first.expansion.calls[0].invoked, true);
    w.addRow(w.next, 3);
    const cached = await w.search(w.peer);
    assert.deepEqual(ids(cached), ids(first));
    assert.equal(cached.expansion.calls[0].invoked, false);
    const out = await w.refresh();
    assert.equal(out.ok, true, JSON.stringify(out.error));
    const after = await w.search(w.peer);
    assert.equal(after.status, 200, JSON.stringify(after.body));
    assert.equal(after.expansion.calls[0].precomputationId, first.expansion.calls[0].precomputationId);
    assert.equal(after.expansion.calls[0].invoked, false, 'refresh updates the existing native mapping without reinvoking the function');
    if (variant === 'golden') {
      assert.deepEqual(ids(after), [w.hidden.id, w.visible.id, w.next.id].sort());
      assert.ok(w.site.log.some((entry) => entry.op === 'POST /rest/api/3/jql/function/computation' && entry.status === 204));
    } else {
      assert.deepEqual(ids(after), ids(first), 'the mutant really leaves the shared native mapping stale');
      assert.equal(w.site.log.filter((entry) => entry.op === 'POST /rest/api/3/jql/function/computation').length, 0);
    }
    assert.deepEqual(w.emu.harnessMissing, []);
  } finally { await w.close(); }
});

test('m_jql_missing_module: native HTTP search cannot invoke an uninstalled function', { timeout: 120000 }, async () => {
  const w = await rig('m_jql_missing_module');
  try {
    const result = await w.search(w.viewer);
    assert.equal(result.status, 400);
    assert.match(result.body.errorMessages[0], /(?:unknown|unable to find installed).*scopeChanged/i);
    assert.equal(w.outputs.filter((out) => out.moduleType === 'jira:jqlFunction').length, 0);
    assert.deepEqual(w.emu.harnessMissing, [], 'a candidate missing its module is a real defect, not a harness failure');
  } finally { await w.close(); }
});

for (const variant of ['golden', 'm_jql_user_baked_cache']) test(`${variant}: native shared cache after the actual person-facing Rovo action`, { timeout: 120000 }, async () => {
  const w = await rig(variant);
  try {
    const action = await w.emu.invokeAction('get-sprint-scope', { sprintId: String(w.sprint.id) }, { asUser: w.viewer });
    assert.equal(action.ok, true, JSON.stringify(action.error));
    assert.deepEqual(action.result.changes.map((change) => change.issueKey), [w.visible.key], 'the real person view is filtered to the viewer');
    const viewerResult = await w.search(w.viewer);
    assert.equal(viewerResult.status, 200, JSON.stringify(viewerResult.body));
    assert.deepEqual(ids(viewerResult), [w.visible.id]);
    assert.equal(viewerResult.expansion.calls[0].invoked, true);
    const peerResult = await w.search(w.peer);
    assert.equal(peerResult.status, 200, JSON.stringify(peerResult.body));
    assert.equal(peerResult.expansion.calls[0].precomputationId, viewerResult.expansion.calls[0].precomputationId);
    assert.equal(peerResult.expansion.calls[0].invoked, false);
    assert.deepEqual(ids(peerResult), variant === 'golden' ? [w.visible.id, w.hidden.id].sort() : [w.visible.id]);
    assert.deepEqual(ids(await w.search(w.viewer)), [w.visible.id], 'Jira still enforces viewer browse permission; this defect loses membership without inventing a leak');
    assert.deepEqual(w.emu.harnessMissing, []);
  } finally { await w.close(); }
});
