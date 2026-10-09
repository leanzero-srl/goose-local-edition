'use strict';
// P3 web trigger (SPEC R6, §2.7): the ingress (lib/webtrigger.cjs), the scorer's CI sequence (site/ci.cjs) and
// `forge-dev ci send` (bin/ci.cjs).
//   - the signature equals an openssl-computed vector; the request/response shapes are Forge's documented ones;
//   - the sequence is seeded, rendered at send time, and DISCRIMINATES: a contract-correct handler meets every
//     expectation, and each classic defect fails exactly its own step(s);
//   - end to end through Atlassian's runtime wrapper: the CI sequence, the HTTP mount, and every `ci send` variation.
// Run: node --test evals/swarm-bench/forge2/kit/test/webtrigger.test.cjs
const test = require('node:test');
const assert = require('node:assert');
const crypto = require('crypto');
const fs = require('fs');
const http = require('http');
const path = require('path');
const { ensureKit, scratch } = require('./helpers.cjs');

const FORGE2 = path.resolve(__dirname, '..', '..');
const wt = require(path.join(FORGE2, 'kit', 'lib', 'webtrigger.cjs'));
const { createCiSequence, runCiSequence } = require(path.join(FORGE2, 'site', 'ci.cjs'));
const { ciCommand } = require(path.join(FORGE2, 'kit', 'bin', 'ci.cjs'));

const SECRET = 'lzci_0f1e2d3c4b5a69788796a5b4c3d2e1f0';
const SEED = '00c0ffee00c0ffee';
const KEYS = Array.from({ length: 14 }, (_, i) => `SL-${101 + i}`);
const STATIC_RESPONSE = { type: 'static', outputs: [{ key: 'accepted', statusCode: 202 }, { key: 'duplicate', statusCode: 200 }, { key: 'unauthorized', statusCode: 401, contentType: 'application/json', body: '{"error":"unauthorized"}' }] };

// Written from the contract text, independently of the module under test.
const hmac = (secret, ts, body) => `sha256=${crypto.createHmac('sha256', secret).update(`${ts}.${body}`).digest('hex')}`;
const lower = (headers) => Object.fromEntries(Object.entries(headers).map(([k, v]) => [k.toLowerCase(), v]));

test('the CI signature equals an openssl HMAC-SHA256 vector over "<timestamp>.<raw body>"', () => {
  // printf '%s' '1760000000.{"eventId":"e-1"}' | openssl dgst -sha256 -hmac 'ci-secret-1'
  assert.strictEqual(wt.ciSignature('ci-secret-1', 1760000000, '{"eventId":"e-1"}'), 'sha256=39b9f57a8afb4a918fd64806136fffc63135d4c293f575d5f2d62a9c4f62cabf');
  const body = wt.encodeCiEvent({ eventId: 'e', sentAt: 1, environment: 'staging', issueKeys: ['A-1'] });
  assert.notStrictEqual(body, JSON.stringify(JSON.parse(body)), 'the wire body is never the canonical re-serialisation');
});

test('request: raw body string, header arrays under the sender spelling, query arrays, method, path, userPath', () => {
  const r = wt.webtriggerRequest('ci', { method: 'post', path: '/x/webtrigger/ci/hooks/a?env=staging&env=prod&x=1',
    headers: [['X-LZ-Timestamp', '1'], ['x-lz-timestamp', '2'], ['Content-Type', 'application/json']], body: Buffer.from('{"a": 1}') });
  assert.deepStrictEqual(r, { method: 'POST', body: '{"a": 1}', path: '/x/webtrigger/ci/hooks/a', userPath: '/hooks/a',
    headers: { 'X-LZ-Timestamp': ['1', '2'], 'Content-Type': ['application/json'] }, queryParameters: { env: ['staging', 'prod'], x: ['1'] } });
  assert.deepStrictEqual(wt.webtriggerRequest('ci', { headers: { 'X-A': ['1', '2'], b: 'c' } }).headers, { 'X-A': ['1', '2'], b: ['c'] });
  assert.strictEqual(wt.webtriggerRequest('ci', {}).path, '/x/webtrigger/ci');
  assert.strictEqual(wt.webtriggerRequest('ci', {}).userPath, '');
});

test('response: dynamic shape checked, static outputKey mapped, anything else is the platform 500 with its cause', () => {
  const dyn = { key: 'd' };
  const st = { key: 's', response: STATIC_RESPONSE };
  assert.deepStrictEqual(wt.responseFor(dyn, { statusCode: 202, headers: { 'X-A': ['1'] }, body: 'ok' }), { statusCode: 202, statusText: undefined, headers: { 'X-A': ['1'] }, body: 'ok', error: null });
  assert.deepStrictEqual(wt.responseFor(st, { outputKey: 'unauthorized' }), { statusCode: 401, statusText: undefined, headers: { 'Content-Type': ['application/json'] }, body: '{"error":"unauthorized"}', error: null });
  assert.strictEqual(wt.responseFor(st, { outputKey: 'accepted' }).statusCode, 202);
  for (const [module, result, cause] of [
    [dyn, { body: 'x' }, /statusCode undefined/],
    [dyn, { statusCode: '202' }, /statusCode "202"/],
    [dyn, { statusCode: 202, body: { ok: true } }, /body must be a string/],
    [dyn, { statusCode: 202, headers: { 'X-A': '1' } }, /an array of strings/],
    [dyn, undefined, /returned undefined/],
    [st, { outputKey: 'nope' }, /naming one of response.outputs \(accepted, duplicate, unauthorized\)/],
    [st, { statusCode: 202 }, /must return \{outputKey\}/],
  ]) {
    const r = wt.responseFor(module, result);
    assert.strictEqual(r.statusCode, 500, JSON.stringify(result));
    assert.match(r.error, cause);
  }
});

function stubEmu(handler, { response = STATIC_RESPONSE, appSeconds = 0 } = {}) {
  let now = 1_760_000_000_000;
  const events = [];
  return {
    events,
    manifest: { modules: { webtrigger: [{ key: 'ci-deploy', function: 'ci', response }, { key: 'nofn' }] } },
    clock: { now: () => now },
    // The app spends `appSeconds` of virtual time before it reads its clock, as a slow handler would.
    invoke: async (fnKey, { moduleKey, event }) => {
      events.push({ fnKey, moduleKey, event });
      now += appSeconds * 1000;
      try { return { ok: true, result: await handler(event, Math.floor(now / 1000)), invocationId: `inv-${events.length}` }; } catch (e) {
        return { ok: false, error: { name: e.name, message: e.message }, invocationId: `inv-${events.length}` };
      }
    },
    drainQueues: async () => [],
  };
}

test('ingress: undeclared module 404, missing function and thrown or timed-out function are 500 with the cause', async () => {
  const emu = stubEmu(() => { throw new TypeError('boom'); });
  assert.deepStrictEqual(await wt.invokeWebtrigger(emu, 'other', {}), { statusCode: 404, statusText: undefined, headers: {}, body: '', error: "no webtrigger module 'other' in manifest.yml", request: null, invocation: null });
  assert.match((await wt.invokeWebtrigger(emu, 'nofn', {})).error, /names no function/);
  const thrown = await wt.invokeWebtrigger(emu, 'ci-deploy', { body: '{}' });
  assert.strictEqual(thrown.statusCode, 500);
  assert.strictEqual(thrown.error, 'the function failed: TypeError: boom');
  assert.strictEqual(emu.events[0].event.body, '{}');
  const slow = { manifest: emu.manifest, invoke: async () => ({ ok: false, timedOut: true, timeoutSec: 55, error: { name: 'TimeoutError', message: 'x' } }) };
  assert.strictEqual((await wt.invokeWebtrigger(slow, 'ci-deploy', {})).error, 'the function was stopped at its 55 s limit');
});

test('sequence: seeded, refuses missing inputs, every step built exactly as named', () => {
  assert.throws(() => createCiSequence({ seed: 'nope', secret: SECRET, issueKeys: KEYS }), /16 lowercase hex/);
  assert.throws(() => createCiSequence({ seed: SEED, secret: '', issueKeys: KEYS }), /secret/);
  assert.throws(() => createCiSequence({ seed: SEED, secret: SECRET, issueKeys: KEYS.slice(0, 7) }), /needs 8 distinct issue keys, got 7/);
  const a = createCiSequence({ seed: SEED, secret: SECRET, issueKeys: KEYS });
  const b = createCiSequence({ seed: SEED, secret: SECRET, issueKeys: [...KEYS].reverse() });
  const c = createCiSequence({ seed: '1111222233334444', secret: SECRET, issueKeys: KEYS });
  assert.deepStrictEqual(a.plan, b.plan, 'same seed, same plan, whatever the pool order');
  assert.notDeepStrictEqual(a.plan.map((s) => s.eventId), c.plan.map((s) => s.eventId));
  assert.ok(!JSON.stringify(a.plan).includes(SECRET), 'the plan never carries the secret');
  assert.deepStrictEqual(a.plan.map((s) => `${s.name}:${s.expect.status}`),
    ['valid:202', 'replay:200', 'bad-signature:401', 'unsigned:401', 'tampered:401', 'stale:401', 'stale-future:401', 'header-case:202']);
  const own = a.plan.filter((s) => s.name !== 'replay').flatMap((s) => s.issueKeys);
  assert.strictEqual(new Set(own).size, own.length, 'every step owns its issues');

  const now = 1_760_000_000;
  const sent = {};
  for (let s = a.next(now), t = now; s; t += 50, s = a.next(t)) sent[s.name] = { ...s, at: t, h: lower(Object.fromEntries(s.request.headers.map(([n, v]) => [n, [v]]))) };
  assert.strictEqual(a.next(now), null);
  const sig = (s) => s.h['x-lz-signature']?.[0];
  const ts = (s) => Number(s.h['x-lz-timestamp'][0]);
  const valid = sent.valid;
  assert.strictEqual(ts(valid), valid.at);
  assert.strictEqual(sig(valid), hmac(SECRET, valid.at, valid.request.body));
  assert.deepStrictEqual(JSON.parse(valid.request.body), { eventId: valid.eventId, sentAt: valid.at, environment: 'staging', issueKeys: a.plan[0].issueKeys });
  assert.deepStrictEqual(valid.request.headers.map(([n]) => n), ['Content-Type', 'X-LZ-Timestamp', 'X-LZ-Signature']);
  assert.deepStrictEqual(sent.replay.request, valid.request, 'the replay is the valid request byte for byte');
  assert.ok(sent.replay.at - valid.at < 300);
  assert.notStrictEqual(sig(sent['bad-signature']), hmac(SECRET, ts(sent['bad-signature']), sent['bad-signature'].request.body));
  assert.strictEqual(sig(sent.unsigned), undefined);
  const tampered = sent.tampered;
  assert.strictEqual(JSON.parse(tampered.request.body).environment, 'production');
  assert.strictEqual(sig(tampered), hmac(SECRET, ts(tampered), tampered.request.body.replace('"production"', '"staging"')), 'signed for staging, sent as production');
  for (const name of ['stale', 'stale-future']) {
    const s = sent[name];
    assert.strictEqual(sig(s), hmac(SECRET, ts(s), s.request.body), `${name} is validly signed: only its clock is wrong`);
    assert.strictEqual(JSON.parse(s.request.body).sentAt, ts(s));
  }
  assert.ok(ts(sent.stale) <= sent.stale.at - 300 - 110);
  assert.ok(ts(sent['stale-future']) >= sent['stale-future'].at + 300 + 110);
  const cased = sent['header-case'].request.headers.map(([n]) => n).slice(1);
  for (const n of cased) assert.ok(![n.toLowerCase(), n.toUpperCase()].includes(n) && !['X-LZ-Timestamp', 'X-LZ-Signature'].includes(n), n);
  assert.strictEqual(sig(sent['header-case']), hmac(SECRET, ts(sent['header-case']), sent['header-case'].request.body));
});

// A handler written from the contract; each `bug` is a classic defect the sequence must catch.
function contractApp(bug = null) {
  const seen = new Set();
  const effects = [];
  const handler = async (req, nowS) => {
    const get = (name) => {
      if (bug === 'canonical-names') return req.headers[{ ts: 'X-LZ-Timestamp', sig: 'X-LZ-Signature' }[name]]?.[0];
      const want = { ts: 'x-lz-timestamp', sig: 'x-lz-signature' }[name];
      const k = Object.keys(req.headers).find((h) => h.toLowerCase() === want);
      return k === undefined ? undefined : req.headers[k][0];
    };
    const ts = get('ts');
    const sig = get('sig');
    if (!ts || (!sig && bug !== 'signature-optional')) return { outputKey: 'unauthorized' };
    const age = nowS - Number(ts);
    if (bug === 'past-only-window' ? age > 300 : Math.abs(age) > 300) return { outputKey: 'unauthorized' };
    if (sig) {
      const want = Buffer.from(hmac(SECRET, ts, bug === 'reserialised-body' ? JSON.stringify(JSON.parse(req.body)) : req.body));
      const got = Buffer.from(sig);
      if (want.length !== got.length || !crypto.timingSafeEqual(want, got)) return { outputKey: 'unauthorized' };
    }
    const ev = JSON.parse(req.body);
    if (bug !== 'no-replay-claim') {
      if (seen.has(ev.eventId)) return { outputKey: 'duplicate' };
      seen.add(ev.eventId);
    }
    effects.push({ environment: ev.environment, issueKeys: ev.issueKeys });
    return { outputKey: 'accepted' };
  };
  return { handler, effects };
}

async function grade(bug, appSeconds) {
  const app = contractApp(bug);
  const run = await runCiSequence(stubEmu(app.handler, { appSeconds }), { moduleKey: 'ci-deploy', seed: SEED, secret: SECRET, issueKeys: KEYS });
  const failed = run.steps.filter((s) => s.statusCode !== s.expect.status).map((s) => s.name);
  const wanted = run.plan.filter((s) => s.expect.effect).map((s) => s.expect.effect);
  return { failed, effectsOk: JSON.stringify(app.effects) === JSON.stringify(wanted), run };
}

test('sequence discriminates: the contract app passes every step even at 54 s per call; each defect fails its own step', async () => {
  const good = await grade(null, 54);
  assert.deepStrictEqual(good.failed, []);
  assert.ok(good.effectsOk, 'exactly the valid and header-case effects, once each');
  assert.ok(good.run.steps.every((s) => s.error === null && s.invocation.ok));
  for (const [bug, steps] of [
    ['canonical-names', ['header-case']],
    ['signature-optional', ['unsigned']],
    ['past-only-window', ['stale-future']],
    ['no-replay-claim', ['replay']],
    ['reserialised-body', ['valid', 'replay', 'header-case']],
  ]) {
    const r = await grade(bug, 54);
    assert.deepStrictEqual(r.failed, steps, bug);
    assert.ok(!r.effectsOk, `${bug} leaves a wrong effect trail`);
  }
});

// ---------------------------------------------------------------------------------- through the real runtime
const APP_SOURCE = `import { kvs } from '@forge/kvs';
import crypto from 'crypto';
const header = (req, name) => { const k = Object.keys(req.headers).find((h) => h.toLowerCase() === name); return k === undefined ? undefined : req.headers[k][0]; };
export const ciDeploy = async (req) => {
  const ts = header(req, 'x-lz-timestamp');
  const sig = header(req, 'x-lz-signature');
  if (!ts || !sig || Math.abs(Math.floor(Date.now() / 1000) - Number(ts)) > 300) return { outputKey: 'unauthorized' };
  const secret = await kvs.getSecret('ci-secret');
  const want = Buffer.from('sha256=' + crypto.createHmac('sha256', String(secret)).update(ts + '.' + req.body).digest('hex'));
  const got = Buffer.from(sig);
  if (!secret || want.length !== got.length || !crypto.timingSafeEqual(want, got)) return { outputKey: 'unauthorized' };
  const ev = JSON.parse(req.body);
  try {
    await kvs.set('ci-event:' + ev.eventId, Number(ts), { keyPolicy: 'FAIL_IF_EXISTS' });
  } catch (e) {
    if (['KEY_CONFLICT', 'CONDITIONAL_CHECK_FAILED'].includes(e.code)) return { outputKey: 'duplicate' };
    throw e;
  }
  for (const key of ev.issueKeys) await kvs.set('deployed:' + key, ev.environment);
  return { outputKey: 'accepted' };
};
export const echo = async (req) => ({ statusCode: 207, statusText: 'Echoed', headers: { 'X-Echo': ['a', 'b'] }, body: JSON.stringify(req) });
`;
const APP_MANIFEST = `modules:
  webtrigger:
    - key: ci-deploy
      function: ci-deploy
      response:
        type: static
        outputs:
          - key: accepted
            statusCode: 202
          - key: duplicate
            statusCode: 200
          - key: unauthorized
            statusCode: 401
    - key: echo
      function: echo
      response:
        type: dynamic
  function:
    - key: ci-deploy
      handler: index.ciDeploy
    - key: echo
      handler: index.echo
permissions:
  scopes:
    - storage:app
app:
  id: ari:cloud:ecosystem::app/00000000-0000-0000-0000-000000000000
  runtime:
    name: nodejs22.x
`;

test('end to end through the runtime wrapper: the CI sequence, the HTTP mount, every ci send variation', { timeout: 600_000 }, async () => {
  ensureKit();
  const { createSite } = require(path.join(FORGE2, 'site', 'site.cjs'));
  const { createEmulator } = require(path.join(FORGE2, 'kit', 'lib', 'emulator.cjs'));
  const appDir = path.join(scratch('webtrigger'), 'app');
  fs.mkdirSync(path.join(appDir, 'src'), { recursive: true });
  fs.writeFileSync(path.join(appDir, 'src', 'index.js'), APP_SOURCE);
  fs.writeFileSync(path.join(appDir, 'manifest.yml'), APP_MANIFEST);
  const site = await createSite({ seed: '0123456789abcdef' });
  const emu = await createEmulator({ appDir, kitDir: path.join(FORGE2, 'kit'), site, runtime: 'wrapper' });
  const server = http.createServer((req, res) => { if (!wt.webtriggerRoute(emu)(req, res)) { res.writeHead(404); res.end(); } });
  try {
    const built = await emu.build();
    assert.ok(built.functions.every((f) => f.loaded), JSON.stringify(built.functions));
    assert.strictEqual(emu.kvs.handle('/api/v1/secret/set', { key: 'ci-secret', value: SECRET }).status, 204);

    const issueKeys = site.pack.issues.map((i) => i.key);
    const run = await runCiSequence(emu, { moduleKey: 'ci-deploy', seed: SEED, secret: SECRET, issueKeys });
    assert.deepStrictEqual(run.steps.map((s) => `${s.name}:${s.statusCode}`), run.plan.map((s) => `${s.name}:${s.expect.status}`), JSON.stringify(run.steps.map((s) => s.error)));
    const deployed = Object.entries(emu.kvs.snapshot().kvs).filter(([k]) => k.startsWith('deployed:')).map(([k, v]) => `${k.slice(9)}=${v}`).sort();
    const wanted = run.plan.filter((s) => s.expect.effect).flatMap((s) => s.expect.effect.issueKeys.map((k) => `${k}=${s.expect.effect.environment}`)).sort();
    assert.deepStrictEqual(deployed, wanted, 'effects only where the plan expects them');
    assert.strictEqual(run.steps.find((s) => s.name === 'valid').invocation.moduleType, 'webtrigger');
    assert.strictEqual(run.steps.find((s) => s.name === 'valid').invocation.timeoutSec, 55);

    await new Promise((ok) => server.listen(0, '127.0.0.1', ok));
    const body = '{"z": 1,  "a": [ ]}';
    const echoed = await new Promise((ok, fail) => {
      const req = http.request({ host: '127.0.0.1', port: server.address().port, method: 'POST', path: '/x/webtrigger/echo/deep/path?q=1&q=2',
        headers: { 'X-Mixed-Case': 'v1', 'Content-Type': 'application/json' } }, (res) => {
        let raw = '';
        res.on('data', (c) => (raw += c));
        res.on('end', () => ok({ status: res.statusCode, statusText: res.statusMessage, echo: res.headers['x-echo'], raw }));
      });
      req.on('error', fail);
      req.end(body);
    });
    assert.strictEqual(echoed.status, 207);
    assert.strictEqual(echoed.statusText, 'Echoed');
    assert.strictEqual(echoed.echo, 'a, b');
    const seen = JSON.parse(echoed.raw);
    assert.strictEqual(seen.body, body, 'the raw body string, byte for byte');
    assert.deepStrictEqual(seen.headers['X-Mixed-Case'], ['v1'], 'header names as sent, values as arrays');
    assert.deepStrictEqual(seen.queryParameters, { q: ['1', '2'] });
    assert.deepStrictEqual([seen.method, seen.path, seen.userPath], ['POST', '/x/webtrigger/echo/deep/path', '/deep/path']);
    const missing = await fetch(`http://127.0.0.1:${server.address().port}/x/webtrigger/nope`, { method: 'POST', body: '{}' });
    assert.strictEqual(missing.status, 404);
    assert.match(missing.headers.get('x-forge-emulator-error'), /no webtrigger module 'nope'/);

    const out = [];
    const send = async (...flags) => {
      out.length = 0;
      const r = await ciCommand(['send', '--module', 'ci-deploy', '--env', 'production', '--issues', 'CI-1,CI-2', '--secret', SECRET, ...flags], { emu, print: (l) => out.push(l) });
      return { code: r.code, statuses: r.exchanges.map((x) => x.response.statusCode) };
    };
    assert.deepStrictEqual(await send(), { code: 0, statuses: [202] });
    assert.strictEqual(emu.kvs.snapshot().kvs['deployed:CI-2'], 'production');
    assert.ok(out.some((l) => l.startsWith('-> 202')), out.join('\n'));
    assert.deepStrictEqual(await send('--replay'), { code: 0, statuses: [202, 200] });
    for (const flag of ['--bad-signature', '--unsigned', '--tamper', '--stale']) assert.deepStrictEqual(await send(flag), { code: 0, statuses: [401] }, flag);
    assert.deepStrictEqual(await send('--skew', '600'), { code: 0, statuses: [401] });
    assert.deepStrictEqual(await send('--header-case'), { code: 0, statuses: [202] });
    assert.deepStrictEqual(await send('--event-id', 'retry-1'), { code: 0, statuses: [202] });
    assert.deepStrictEqual(await send('--event-id', 'retry-1'), { code: 0, statuses: [200] }, 'a CI retry re-signs the same eventId');
    await assert.rejects(ciCommand(['send', '--module', 'ci-deploy', '--env', 'production', '--issues', 'CI-1'], { emu, print: () => {} }), /no CI secret/);
    await assert.rejects(ciCommand(['send', '--env', 'staging', '--issues', 'CI-1', '--secret', 's'], { emu, print: () => {} }), /several webtrigger modules \(ci-deploy, echo\)/);
    await assert.rejects(ciCommand(['send', '--module', 'ci-deploy', '--env', 'dev', '--issues', 'CI-1', '--secret', 's'], { emu, print: () => {} }), /--env must be one of staging, production/);
    await assert.rejects(ciCommand(['send', '--stale', '--replay'], { emu, print: () => {} }), /one variation per send/);
  } finally {
    server.close();
    await emu.close();
    await site.stop();
  }
});
