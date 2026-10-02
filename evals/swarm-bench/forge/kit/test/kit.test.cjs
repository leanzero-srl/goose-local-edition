'use strict';
// The kit's own guarantees, each against the real thing it claims:
//   G7  one invocation = one deny-default sandbox: no reads outside the bundle, no exec, only the proxy port,
//       no internet — measured from inside Atlassian's runtime wrapper.
//   KVS documented limits and codes, and the 32-bit integer range named in the error (DESIGN §17.1 0b).
//   lint  bin/lint.cjs is Forge's staged linter: an unsupported runtime stops at its validator; a
//       `src/index.fn` handler is rejected; a clean app reaches 'complete'.
// Run: node --test forge/kit/test/kit.test.cjs
const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const net = require('net');
const path = require('path');
const { spawnSync } = require('child_process');
const { FORGE, ensureKit, scratch } = require('./helpers.cjs');

const APP_ID = 'ari:cloud:ecosystem::app/00000000-0000-0000-0000-000000000000';
const PROBE = `import api, { route } from '@forge/api';
import fs from 'fs';
import cp from 'child_process';
import net from 'net';
export const probe = async (event) => {
  const out = {};
  try { fs.readFileSync(event.secret, 'utf8'); out.secret = 'read'; } catch (e) { out.secret = e.code; }
  try { cp.execFileSync('/bin/echo', ['x']); out.exec = 'ran'; } catch (e) { out.exec = e.code ?? String(e.message).slice(0, 80); }
  const r = await api.asApp().requestJira(route\`/rest/api/3/myself\`);
  out.proxy = r.status;
  out.otherPort = await new Promise((res) => { const s = net.connect(event.port, '127.0.0.1');
    s.on('connect', () => { res('connected'); s.destroy(); }); s.on('error', (e) => res(e.code)); });
  // Global fetch in the Forge runtime is the platform's egress path (through the proxy, allowlisted by
  // permissions.external.fetch); a raw socket is what a fence must stop.
  try { const f = await fetch('https://example.com/'); out.egress = f.status; } catch (e) { out.egress = String(e.cause?.code ?? e.message); }
  out.internet = await new Promise((res) => { const s = net.connect(443, 'example.com');
    s.on('connect', () => { res('connected'); s.destroy(); }); s.on('error', (e) => res(e.code)); });
  return out;
};
`;

function writeApp(dir, { runtime = 'nodejs22.x', handler = 'index.probe', source = PROBE, extraModules = '' } = {}) {
  fs.mkdirSync(path.join(dir, 'src'), { recursive: true });
  fs.writeFileSync(path.join(dir, 'src', 'index.js'), source);
  fs.writeFileSync(path.join(dir, 'manifest.yml'), `modules:
  scheduledTrigger:
    - key: probe-hourly
      function: probe
      interval: hour
${extraModules}  function:
    - key: probe
      handler: ${handler}
permissions:
  scopes:
    - read:jira-user
    - storage:app
app:
  id: ${APP_ID}
  runtime:
    name: ${runtime}
`);
}

test('G7: an invocation reads only its bundle, cannot exec, reaches only the proxy, never the internet', { timeout: 120_000 }, async () => {
  ensureKit();
  const { createSite } = require(path.join(FORGE, 'site', 'site.cjs'));
  const { createEmulator } = require(path.join(FORGE, 'kit', 'lib', 'emulator.cjs'));
  const root = scratch('fence');
  const appDir = path.join(root, 'app');
  writeApp(appDir);
  const secret = path.join(root, 'secret.json');
  fs.writeFileSync(secret, '{"answer": 42}');
  const other = net.createServer((s) => s.end()).listen(0, '127.0.0.1');
  await new Promise((r) => other.on('listening', r));
  const site = await createSite({ seed: '0123456789abcdef' });
  const emu = await createEmulator({ appDir, site, runtime: 'wrapper' });
  try {
    assert.strictEqual(emu.fence, 'sandbox');
    const built = await emu.build();
    assert.ok(built.functions.every((f) => f.loaded), JSON.stringify(built.functions));
    const r = await emu.invoke('probe', { moduleKey: 'probe-hourly', event: { secret, port: other.address().port } });
    assert.ok(r.ok, JSON.stringify(r.error));
    assert.strictEqual(r.result.secret, 'EPERM');
    assert.notStrictEqual(r.result.exec, 'ran');
    assert.strictEqual(r.result.proxy, 200);
    assert.strictEqual(r.result.otherPort, 'EPERM');
    assert.strictEqual(r.result.egress, 403, 'platform fetch outside permissions.external.fetch is refused by the proxy');
    assert.notStrictEqual(r.result.internet, 'connected');
  } finally {
    other.close();
    await emu.close();
    await site.stop();
  }
});

test('KVS: documented limits and codes; an out-of-range integer names the 32-bit range', () => {
  const { createKvs } = require(path.join(FORGE, 'kit', 'lib', 'kvs.cjs'));
  const kvs = createKvs({ entities: [{ name: 'row', attributes: { n: { type: 'integer' }, s: { type: 'string' } } }] });
  const code = (op, body) => { const r = kvs.handle(op, body); return r.status < 300 ? 'ok' : `${r.status} ${r.body.code}: ${r.body.message}`; };
  assert.match(code('/api/v1/set', { key: '', value: 1 }), /^400 EMPTY_KEY/);
  assert.match(code('/api/v1/set', { key: 'a/b', value: 1 }), /^400 INVALID_KEY/);
  assert.match(code('/api/v1/set', { key: 'k'.repeat(501), value: 1 }), /^400 KEY_TOO_LONG/);
  assert.strictEqual(code('/api/v1/set', { key: 'k'.repeat(500), value: 1 }), 'ok');
  assert.match(code('/api/v1/set', { key: 'big', value: 'x'.repeat(240 * 1024) }), /^400 MAX_SIZE/);
  let deep = 1;
  for (let i = 0; i < 32; i++) deep = { d: deep };
  assert.match(code('/api/v1/set', { key: 'deep', value: deep }), /^400 MAX_DEPTH/);
  assert.match(code('/api/v1/query', { limit: 101 }), /^400 COMPLEX_QUERY_PAGE_LIMIT_NOT_IN_RANGE/);
  assert.match(code('/api/v1/entity/set', { entityName: 'nope', key: 'x', value: { n: 1 } }), /^400 INVALID_ENTITY_TYPE/);
  assert.strictEqual(code('/api/v1/entity/set', { entityName: 'row', key: 'x', value: { n: 2147483647 } }), 'ok');
  const over = code('/api/v1/entity/set', { entityName: 'row', key: 'y', value: { n: 2147483648 } });
  assert.match(over, /^400 INVALID_ENTITY_VALUE: Attribute 'n' must be a 32-bit signed integer \(-2,147,483,648 to 2,147,483,647\); got 2147483648\./);
  assert.match(code('/api/v1/entity/set', { entityName: 'row', key: 'z', value: { n: 1.5 } }), /32-bit signed integer/);
  const ops = Array.from({ length: 26 }, (_, i) => ({ set: { key: `t${i}`, value: i } }));
  assert.match(code('/api/v1/transaction', { set: ops.map((o) => o.set) }), /^400 TOO_MANY_OPERATIONS/);
  assert.strictEqual(code('/api/v1/transaction', { set: ops.slice(0, 25).map((o) => o.set) }), 'ok');
  assert.match(code('/api/v1/batch/set', ops.map((o) => o.set)), /^400 MAX_BATCH_SIZE/);
  assert.match(code('/api/v1/set', { key: 'k'.repeat(500), value: 2, options: { keyPolicy: 'FAIL_IF_EXISTS' } }), /^409 CONDITIONAL_CHECK_FAILED/);
  assert.match(code('/api/v1/get', { key: 'missing' }), /^404 KEY_NOT_FOUND/);
  assert.strictEqual(kvs.handle('/api/v1/not-an-op', {}).status, 501, 'an unmodelled KVS op is loud, never a silent 200');
});

test('lint: staged like forge lint — runtime stage, handler rule, clean app completes', { timeout: 180_000 }, () => {
  const kit = ensureKit();
  const lint = (dir) => {
    const r = spawnSync(process.execPath, [path.join(kit.kit_dir, 'bin', 'lint.cjs'), '--json', dir], { encoding: 'utf8' });
    assert.ok([0, 1].includes(r.status), r.stderr.slice(-1500));
    return { exit: r.status, ...JSON.parse(r.stdout.split('LINT_JSON ')[1]) };
  };
  const egress = path.join(scratch('lint-egress'), 'app');
  writeApp(egress);
  const re = lint(egress);
  assert.ok(re.problems.some((p) => p.reference === 'egress-permission-required' && p.file === 'src/index.js'), JSON.stringify(re.problems));
  const clean = path.join(scratch('lint-ok'), 'app');
  writeApp(clean, { source: 'export const probe = async () => ({ ok: true });\n' });
  const ok = lint(clean);
  assert.strictEqual(ok.exit, 0, JSON.stringify(ok.problems));
  assert.strictEqual(ok.stageReached, 'complete');

  const old = path.join(scratch('lint-runtime'), 'app');
  writeApp(old, { runtime: 'nodejs18.x', source: 'export const probe = async () => ({ ok: true });\n' });
  const r18 = lint(old);
  assert.strictEqual(r18.exit, 1);
  assert.strictEqual(r18.stageReached, 'SchemaValidator');
  assert.ok(r18.problems.some((p) => p.sev === 'error' && /'nodejs18.x' allowed values are/.test(p.message)), JSON.stringify(r18.problems));

  const dotted = path.join(scratch('lint-handler'), 'app');
  writeApp(dotted, { handler: 'src/index.probe', source: 'export const probe = async () => ({ ok: true });\n' });
  const rh = lint(dotted);
  assert.strictEqual(rh.exit, 1, JSON.stringify(rh.problems));
  assert.strictEqual(rh.stageReached, 'ModulesValidator');
  assert.ok(rh.problems.some((p) => p.sev === 'error' && /'src\/index.probe' cannot find associated file/.test(p.message)), JSON.stringify(rh.problems));
});
