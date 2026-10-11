'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { spawn } = require('node:child_process');
const { ensureKit, scratch } = require('./helpers.cjs');
const { createSite } = require('../../site/site.cjs');
const { createEmulator } = require('../lib/emulator.cjs');
const SOURCE = `import api, { route } from '@forge/api';
import { kvs } from '@forge/kvs';
export const scoped = async (request, context) => {
  await kvs.set('last-jql-request', { request, principal: context?.principal?.accountId ?? null });
  if (!/^\\d+$/.test(request.clause.arguments[0] ?? '')) return { error: 'Sprint must be numeric' };
  const ids = await kvs.get('selected-ids');
  const issue = await api.asApp().requestJira(route\`/rest/api/3/issue/\${ids[0]}?fields=summary\`);
  if (issue.status !== 200) return { error: 'Native lookup failed: ' + issue.status };
  return { jql: 'id ' + (request.clause.operator === 'not in' ? 'not in' : 'in') + ' (' + ids.join(',') + ')' };
};
export const nested = async () => {
  const r = await api.asApp().requestJira(route\`/rest/api/3/search/jql\`, { method: 'POST', headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ jql: 'issue in hostScoped("456")', fields: ['summary'], maxResults: 100 }) });
  return { status: r.status, body: await r.json() };
};`;
function writeApp(dir) {
  fs.mkdirSync(path.join(dir, 'src'), { recursive: true });
  fs.writeFileSync(path.join(dir, 'src', 'index.js'), SOURCE);
  fs.writeFileSync(path.join(dir, 'manifest.yml'), `modules:
  jira:jqlFunction:
    - key: host-scoped
      name: hostScoped
      arguments:
        - name: sprint
          required: true
      types: [issue]
      operators: [in, not in]
      function: host-scoped
  scheduledTrigger:
    - key: host-nested
      function: host-nested
      interval: hour
  function:
    - key: host-scoped
      handler: index.scoped
    - key: host-nested
      handler: index.nested
permissions:
  scopes: [read:jira-work, storage:app]
app:
  id: ari:cloud:ecosystem::app/00000000-0000-0000-0000-000000000000
  runtime:
    name: nodejs22.x
`);
}
async function rig(remote) {
  ensureKit();
  const dir = scratch('jql-host');
  writeApp(dir);
  const site = await createSite({ seed: '0123456789abcdef' });
  const outputs = [];
  let inHook = false, outerHooks = 0;
  const emu = await createEmulator({ appDir: dir, site: remote ? { url: site.url, adminUrl: site.adminUrl } : site,
    onInvocation: (_, out) => outputs.push(out), aroundInvocation: async (run) => {
      assert.equal(inHook, false, 'native JQL child cannot reacquire the parent dev-state lock');
      outerHooks += 1; inHook = true;
      try { return await run(); } finally { inHook = false; }
    } });
  try { assert.ok((await emu.build()).functions.every((f) => f.loaded)); }
  catch (e) { await emu.close(); await site.stop(); throw e; }
  const setIds = (ids) => assert.equal(emu.kvs.handle('/api/v1/set', { key: 'selected-ids', value: ids }).status, 204);
  const call = async (method, resource, body, as = 'app') => {
    const r = await fetch(site.url + resource, { method, headers: { 'content-type': 'application/json', 'x-forge-as': as === 'app' ? 'app' : 'user',
      'x-forge-account': as === 'app' ? site.pack.appAccountId : as, 'x-forge-scopes': '["read:jira-work"]', 'x-forge-source': 'frontend' },
      body: body === undefined ? undefined : JSON.stringify(body) });
    const text = await r.text(); return { status: r.status, body: text ? JSON.parse(text) : null };
  };
  const search = (query, method = 'GET', as = site.pack.viewer) => method === 'GET'
    ? call('GET', '/rest/api/3/search/jql?' + new URLSearchParams({ jql: query, fields: 'summary', maxResults: '100' }), undefined, as)
    : call('POST', '/rest/api/3/search/jql', { jql: query, fields: ['summary'], maxResults: 100 }, as);
  return { dir, site, emu, outputs, setIds, call, search, hooks: () => outerHooks,
    close: async () => { await emu.close(); await site.stop(); fs.rmSync(dir, { recursive: true, force: true }); } };
}
for (const remote of [false, true]) test(`native JQL ${remote ? 'remote dev callback' : 'in-process scoring'}: HTTP search/cache/refresh/visibility and actual Forge handler`, { timeout: 120_000 }, async () => {
  const w = await rig(remote);
  try {
    const viewer = w.site.pack.viewer;
    const hidden = w.site.pack.issues.find((i) => i.hiddenFrom.includes(viewer));
    const [visible, next] = w.site.pack.issues.filter((i) => !i.hiddenFrom.includes(viewer)).slice(0, 2);
    assert.ok(hidden && visible && next, 'same-object visibility controls');
    w.setIds([hidden.id, visible.id]);
    const query = 'issue in hostScoped("123") ORDER BY key DESC';
    const first = await w.search(query);
    assert.equal(first.status, 200, JSON.stringify(first.body));
    assert.deepEqual(first.body.issues.map((i) => i.id), [visible.id]);
    const entry = w.site.log.find((e) => e.jqlExpansion);
    assert.equal(entry.jqlExpansion.calls[0].invoked, true);
    const id = entry.jqlExpansion.calls[0].precomputationId;
    const native = w.outputs.filter((r) => r.moduleType === 'jira:jqlFunction');
    assert.equal(native.length, 1);
    const nativeRead = w.site.log.find((e) => e.caller?.moduleType === 'jira:jqlFunction' || e.moduleType === 'jira:jqlFunction');
    assert.ok(nativeRead, 'native function reaches Jira under its own invocation identity');
    assert.deepEqual([nativeRead.kind, nativeRead.points], ['person', 2]);
    assert.equal(native[0].asUser, null, 'all Jira users share a caller-independent expansion');
    const last = w.emu.kvs.handle('/api/v1/get', { key: 'last-jql-request' }).body.value;
    assert.equal(last.principal, null);
    assert.equal(last.request.precomputationId, id);
    assert.deepEqual(last.request.clause.arguments, ['123']);
    assert.deepEqual((await w.search(query, 'POST', 'app')).body.issues.map((i) => i.id).sort(), [hidden.id, visible.id].sort());
    assert.equal(w.site.log.at(-1).jqlExpansion.calls[0].invoked, false);
    assert.equal(w.outputs.filter((r) => r.moduleType === 'jira:jqlFunction').length, 1);
    w.setIds([next.id]);
    assert.deepEqual((await w.search(query)).body.issues.map((i) => i.id), [visible.id], 'cached expansion stays stale until app refreshes native mapping');
    const mapping = await w.call('GET', '/rest/api/3/jql/function/computation');
    assert.equal(mapping.status, 200);
    assert.equal(mapping.body.values[0].functionKey, 'ari:cloud:ecosystem::extension/00000000-0000-0000-0000-000000000000/emulator-env/static/host-scoped');
    assert.equal((await w.call('POST', '/rest/api/3/jql/function/computation', { values: [{ id, value: `id in (${next.id})` }] })).status, 204);
    assert.deepEqual((await w.search(query)).body.issues.map((i) => i.id), [next.id]);
    const bad = await w.search('issue in hostScoped("abc")');
    assert.equal(bad.status, 400);
    assert.match(bad.body.errorMessages[0], /numeric/);
    assert.equal((await w.search('issue in nonexistentHostFunction("1")')).status, 400);
    const before = w.hooks();
    const nested = await w.emu.invoke('host-nested', { moduleKey: 'host-nested', event: {} });
    assert.equal(nested.ok, true, JSON.stringify(nested.error));
    assert.equal(nested.result.status, 200, JSON.stringify(nested.result));
    assert.deepEqual(nested.result.body.issues.map((i) => i.id), [next.id]);
    assert.equal(w.hooks(), before + 1, 'nested native expansion shares the parent invocation persistence hook');
    assert.deepEqual(w.emu.harnessMissing, []);
    if (remote) {
      const origin = new URL(w.emu.webtriggerUrl('unused')).origin;
      for (const nonce of ['', 'bad', 'é'.repeat(64)]) {
        const refused = await fetch(origin + '/x/jql-evaluate/host-scoped', { method: 'POST', headers: { 'content-type': 'application/json', 'x-forge-jql-nonce': nonce }, body: '{}' });
        assert.equal(refused.status, 403, 'private callback never admits a missing/invalid nonce');
      }
      const invalid = await fetch(w.site.adminUrl + '/jqlregister', { method: 'POST', headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ moduleKey: 'host-scoped', name: 'hostScoped', functionKey: mapping.body.values[0].functionKey, callbackUrl: 'https://example.com/x/jql-evaluate/host-scoped', nonce: 'a'.repeat(64) }) });
      assert.equal(invalid.status, 400, 'site cannot register an external callback');
    }
  } finally { await w.close(); }
});

test('forge-dev jql runs native HTTP search in a child command, preserves shared cache and saves full result', { timeout: 120_000 }, async () => {
  const w = await rig(true);
  try {
    const viewer = w.site.pack.viewer;
    const hidden = w.site.pack.issues.find((i) => i.hiddenFrom.includes(viewer));
    const visible = w.site.pack.issues.find((i) => !i.hiddenFrom.includes(viewer));
    w.setIds([hidden.id, visible.id]);
    const stateDir = path.join(w.dir, '.forge-dev');
    fs.mkdirSync(stateDir, { recursive: true });
    fs.writeFileSync(path.join(stateDir, 'state.json'), JSON.stringify({ kvs: w.emu.kvs.dump() }));
    const kit = ensureKit();
    const cli = async (args) => await new Promise((resolve, reject) => {
      const child = spawn(process.execPath, [path.join(__dirname, '..', 'bin', 'forge-dev.cjs'), 'jql', ...args], {
        cwd: w.dir, env: { ...process.env, FORGE_KIT: kit.kit_dir,
          FORGE_SITE_URL: w.site.url.replace('://', `://admin:${new URL(w.site.adminUrl).pathname.split('/')[2]}@`) },
      });
      let stdout = '', stderr = '';
      child.stdout.on('data', (chunk) => { stdout += chunk; });
      child.stderr.on('data', (chunk) => { stderr += chunk; });
      child.on('error', reject); child.on('close', (code) => resolve({ code, stdout, stderr }));
    });
    const first = await cli(['issue in hostScoped("123")']);
    assert.equal(first.code, 0, first.stdout + first.stderr);
    assert.match(first.stdout, /native JQL/);
    let result = JSON.parse(fs.readFileSync(path.join(stateDir, 'last-result.json'), 'utf8'));
    assert.deepEqual(result.issues.map((i) => i.id), [visible.id]);
    assert.equal(result.issues[0].key, visible.key);
    assert.equal(result.issues[0].fields.summary, visible.summary);
    const persisted = JSON.parse(fs.readFileSync(path.join(stateDir, 'state.json'), 'utf8'));
    const lastRequest = new Map(persisted.kvs.kv).get('last-jql-request').value;
    assert.deepEqual(lastRequest.request.clause.arguments, ['123'], 'actual native handler request persisted through the dev-state lock');
    assert.equal(lastRequest.principal, null);
    const shared = await cli(['issue in hostScoped("123")', '--as', 'app']);
    assert.equal(shared.code, 0, shared.stdout + shared.stderr);
    result = JSON.parse(fs.readFileSync(path.join(stateDir, 'last-result.json'), 'utf8'));
    assert.deepEqual(result.issues.map((i) => i.id).sort(), [hidden.id, visible.id].sort());
    assert.equal(w.site.log.filter((e) => e.jqlExpansion?.calls[0]?.invoked).length, 1, 'second CLI identity reuses the shared native precomputation');
    const bad = await cli(['issue in unknownCliFunction("123")']);
    assert.equal(bad.code, 1, bad.stdout + bad.stderr);
    result = JSON.parse(fs.readFileSync(path.join(stateDir, 'last-result.json'), 'utf8'));
    assert.match(result.errorMessages[0], /unknown/i);
    const restored = await w.search('issue in hostScoped("789")');
    assert.equal(restored.status, 200, JSON.stringify(restored.body));
    assert.deepEqual(restored.body.issues.map((issue) => issue.id), [visible.id]);
    assert.deepEqual(w.outputs.filter((out) => out.moduleType === 'jira:jqlFunction').at(-1).result, { jql: `id in (${hidden.id},${visible.id})` },
      'after each child command exits, a fresh query returns to the still-running original host');
    assert.deepEqual(w.site.harnessMissing, []);
  } finally { await w.close(); }
});


test('remote JQL host registration closes by ownership and restores earlier hosts across out-of-order process exits', { timeout: 120000 }, async () => {
  const w = await rig(true);
  let second, third;
  try {
    const [one, two, three] = w.site.pack.issues.filter((issue) => !issue.hiddenFrom.includes(w.site.pack.viewer)).slice(0, 3);
    w.setIds([one.id]);
    second = await createEmulator({ appDir: w.dir, site: { url: w.site.url, adminUrl: w.site.adminUrl } });
    assert.equal(second.kvs.handle('/api/v1/set', { key: 'selected-ids', value: [two.id] }).status, 204);
    third = await createEmulator({ appDir: w.dir, site: { url: w.site.url, adminUrl: w.site.adminUrl } });
    assert.equal(third.kvs.handle('/api/v1/set', { key: 'selected-ids', value: [three.id] }).status, 204);
    const current = await w.search('issue in hostScoped("101")');
    assert.equal(current.status, 200, JSON.stringify(current.body));
    assert.deepEqual(current.body.issues.map((issue) => issue.id), [three.id]);
    await second.close(); second = null;
    assert.deepEqual((await w.search('issue in hostScoped("102")')).body.issues.map((issue) => issue.id), [three.id],
      'closing a superseded host must not remove the active callback');
    await third.close(); third = null;
    const restored = await w.search('issue in hostScoped("103")');
    assert.equal(restored.status, 200, JSON.stringify(restored.body));
    assert.deepEqual(restored.body.issues.map((issue) => issue.id), [one.id]);
    assert.deepEqual(w.site.harnessMissing, []);
  } finally { if (second) await second.close(); if (third) await third.close(); await w.close(); }
});
