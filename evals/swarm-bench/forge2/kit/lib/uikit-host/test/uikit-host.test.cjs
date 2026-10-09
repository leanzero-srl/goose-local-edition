'use strict';
// node --test evals/swarm-bench/forge2/kit/lib/uikit-host/test/uikit-host.test.cjs
// Needs the forge2 kit's modules with @forge/react 12.3.0 in them: FORGE_KIT (a materialised kit) or FORGE_KIT_MODULES
// (its module cache); the emulator test also needs the pinned runtime wrapper beside them.
const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { render, renderInEmulator, UikitHostError } = require('../index.cjs');

if (!process.env.FORGE_KIT && !process.env.FORGE_KIT_MODULES) {
  throw new Error('REFUSED: set FORGE_KIT (a materialised forge2 kit) or FORGE_KIT_MODULES (its module cache, with @forge/react 12.3.0 in app-modules)');
}

const FIXTURE = path.join(__dirname, 'fixture-admin');
const SITE = path.resolve(__dirname, '..', '..', '..', '..', 'site', 'site.cjs');
const CONTEXT = { accountId: 'admin-1', cloudId: 'cloud-1', moduleKey: 'fixture-admin', extension: { type: 'jira:adminPage' } };
const RETENTION = 'Retention (days)';
const DIGEST = 'Email digest enabled';
const SAVE = 'Save preferences';

function backend({ getPreferences, savePreferences } = {}) {
  const state = { preferences: { retentionDays: 30, digestEnabled: true },
    changes: [{ when: 1, who: 'alice', what: 'installed' }, { when: 2, who: 'bob', what: 'retention 60' }, { when: 10, who: 'carol', what: 'digest off' }] };
  const calls = [];
  const invoke = async (call) => {
    calls.push(call);
    if (call.functionKey === 'getPreferences') return getPreferences ? getPreferences(state) : { ...state.preferences, changes: state.changes };
    if (call.functionKey === 'savePreferences') {
      const save = () => { state.preferences = { retentionDays: Number(call.payload.retentionDays), digestEnabled: call.payload.digestEnabled === true }; return state.preferences; };
      return savePreferences ? savePreferences(save) : save();
    }
    throw new Error(`no resolver '${call.functionKey}'`);
  };
  return { invoke, calls, state };
}
const renderFixture = (be, extra = {}) => render({ appDir: FIXTURE, moduleKey: 'fixture-admin', context: CONTEXT, invoke: be.invoke, ...extra });

function tempApp(source, { native = true } = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'uikit-host-'));
  fs.mkdirSync(path.join(dir, 'src', 'frontend'), { recursive: true });
  fs.writeFileSync(path.join(dir, 'src', 'frontend', 'index.jsx'), source);
  fs.writeFileSync(path.join(dir, 'manifest.yml'), `modules:
  jira:adminPage:
    - key: tmp-admin
      title: Tmp
${native ? '      render: native\n' : ''}      resource: ui
resources:
  - key: ui
    path: src/frontend/index.jsx
app:
  id: ari:cloud:ecosystem::app/00000000-0000-0000-0000-000000000000
`);
  return dir;
}
const renderTemp = (source, invoke = async () => ({}), extra = {}) => render({ appDir: tempApp(source), moduleKey: 'tmp-admin', context: { ...CONTEXT, moduleKey: 'tmp-admin' }, invoke, ...extra });

test('boot: the first commit paints before the one invoke; the screen reads as text', async () => {
  const be = backend();
  const host = await renderFixture(be);
  try {
    assert.strictEqual(host.text(host.docs[0]), 'Loading preferences…', 'the first ForgeDoc is the loading state');
    const idle = await host.waitIdle();
    assert.deepStrictEqual(host.invokes.map((i) => [i.functionKey, i.reconcilesBefore, i.state]), [['getPreferences', 1, 'ok']]);
    assert.strictEqual(be.calls[0].context.accountId, 'admin-1', 'the resolver gets the frontend context');
    assert.ok(idle.turns <= 4, `boot settles in a few turns, took ${idle.turns}`);
    assert.strictEqual(host.text(), [
      'Preferences', RETENTION, '[30]', `[x] ${DIGEST}`, `[${SAVE}]`,
      'Change history', 'When | Who | What', '10 | carol | digest off', '2 | bob | retention 60', '1 | alice | installed', '[Enable theming]', '[Check identity]',
    ].join('\n'));
    const days = host.findByLabel(RETENTION);
    assert.deepStrictEqual([days.type, days.via, days.value, days.props.type], ['Textfield', 'Label labelFor', '30', 'number']);
    assert.deepStrictEqual([host.findByLabel(DIGEST).type, host.findByLabel(DIGEST).checked], ['Toggle', true]);
    assert.strictEqual(host.findByLabel(SAVE).type, 'LoadingButton');
    assert.strictEqual(host.tree().type, 'Root');
    assert.ok(host.tree().children instanceof Array && host.tree().props.constructor === Object, 'a snapshot is plain host data, not app-realm objects');
    assert.deepStrictEqual([host.errors, host.harnessMissing], [[], []]);
  } finally { await host.close(); }
});

test('drive by label: type, toggle, submit -> exactly one invoke with the DOM\'s string value; timers are virtual', async () => {
  const be = backend();
  const host = await renderFixture(be);
  try {
    await host.waitIdle();
    assert.deepStrictEqual(await host.setValue(RETENTION, 60), { changed: true });
    assert.deepStrictEqual(await host.click(DIGEST), { clicked: true });
    assert.deepStrictEqual(await host.setValue(DIGEST, false), { changed: false, reason: 'unchanged' });
    assert.deepStrictEqual(await host.click(SAVE), { clicked: true, submitted: true });
    const idle = await host.waitIdle();
    const saves = host.invokes.filter((i) => i.functionKey === 'savePreferences');
    assert.strictEqual(saves.length, 1);
    assert.deepStrictEqual(saves[0].payload, { retentionDays: '60', digestEnabled: false }, 'a number input hands over the string the DOM holds');
    assert.match(host.text(), new RegExp(`\\[60\\]\\n\\[ \\] ${DIGEST}\\n\\[${SAVE}\\]\\nSaved: retention 60, digest off\\n`));
    assert.strictEqual(idle.timersPending, 1, 'the 3 s flash timer waits for virtual time');
    await host.advance(2999);
    assert.match(host.text(), /Saved: retention 60/);
    await host.advance(1);
    assert.doesNotMatch(host.text(), /Saved:/);
    assert.strictEqual(host.now(), 3000);
    assert.deepStrictEqual(host.errors, []);
  } finally { await host.close(); }
});

test('validation: an out-of-range value shows the error and nothing is submitted', async () => {
  const host = await renderFixture(backend());
  try {
    await host.waitIdle();
    await host.setValue(RETENTION, 0);
    await host.waitIdle();
    assert.match(host.text(), /\[0\] \(invalid\)\nEnter a number of days from 1 to 365/, 'useForm validates on blur');
    assert.deepStrictEqual(await host.click(SAVE), { clicked: true, submitted: true });
    await host.waitIdle();
    assert.deepStrictEqual(host.invokes.map((i) => i.functionKey), ['getPreferences']);
  } finally { await host.close(); }
});

test('a loading button is not clickable: a second Save while the first is in flight does nothing', async () => {
  let release;
  const gate = new Promise((r) => { release = r; });
  const host = await renderFixture(backend({ savePreferences: async (save) => { await gate; return save(); } }));
  try {
    await host.waitIdle();
    await host.click(SAVE);
    await host.flush();
    assert.strictEqual(host.findByLabel(SAVE).props.isLoading, true);
    assert.match(host.text(), new RegExp(`\\[${SAVE}\\] \\(loading\\)`));
    assert.deepStrictEqual(await host.click(SAVE), { clicked: false, reason: 'loading' });
    release();
    await host.waitIdle();
    const saves = host.invokes.filter((i) => i.functionKey === 'savePreferences');
    assert.strictEqual(saves.length, 1);
    assert.deepStrictEqual(saves[0].payload, { retentionDays: 30, digestEnabled: true },
      'untouched fields submit the app\'s own defaultValues (a number here); only typed ones carry the DOM\'s string');
  } finally { await host.close(); }
});

test('labels: a table by its caption (rows sorted as ADS sorts them), a missing label lists the screen, codes for misuse', async () => {
  const host = await renderFixture(backend());
  try {
    await host.waitIdle();
    const table = host.findByLabel('Change history');
    assert.strictEqual(table.type, 'DynamicTable');
    assert.strictEqual(host.text(table), 'Change history\nWhen | Who | What\n10 | carol | digest off\n2 | bob | retention 60\n1 | alice | installed');
    assert.deepStrictEqual(host.table('Change history'), { head: ['When', 'Who', 'What'], rows: [['10', 'carol', 'digest off'], ['2', 'bob', 'retention 60'], ['1', 'alice', 'installed']] });
    assert.throws(() => host.table('Preferences'), (e) => e.code === 'NOT_A_TABLE');
    assert.strictEqual(host.findByLabel('Preferences').type, 'FormSection');
    assert.strictEqual(host.findByLabel('Nope'), null);
    await assert.rejects(host.click('Nope'), (e) => e instanceof UikitHostError && e.code === 'NO_CONTROL' && /"Retention \(days\)"/.test(e.message));
    await assert.rejects(host.click(RETENTION), (e) => e.code === 'NOT_CLICKABLE');
    await assert.rejects(host.setValue(SAVE, 'x'), (e) => e.code === 'NOT_AN_INPUT');
    await assert.rejects(host.setValue(DIGEST, 'yes'), (e) => e.code === 'BAD_VALUE');
  } finally { await host.close(); }
});

test('a bridge op this host does not model rejects in the app and is recorded, never answered', async () => {
  const host = await renderFixture(backend());
  try {
    await host.waitIdle();
    await host.click('Enable theming');
    await host.waitIdle();
    assert.deepStrictEqual(host.harnessMissing.map((m) => m.what), ["bridge op 'enableTheming'"]);
    assert.deepStrictEqual(host.errors.map((e) => [e.kind, e.message]), [['handler', "uikit host: bridge op 'enableTheming' is not modelled"]]);
  } finally { await host.close(); }
});

test('context, flags, requestJira and links: what an admin page uses besides invoke', async () => {
  const fetched = [];
  const host = await renderTemp(`import React, { useState } from 'react';
import ForgeReconciler, { Button, Link, Stack, Text, useProductContext } from '@forge/react';
import { requestJira, showFlag } from '@forge/bridge';
const App = () => {
  const ctx = useProductContext();
  const [me, setMe] = useState('');
  const check = async () => {
    const r = await requestJira('/rest/api/3/mypermissions?permissions=ADMINISTER', { headers: { Accept: 'application/json' } });
    const body = await r.json();
    setMe(r.status + ' ' + body.permissions.ADMINISTER.havePermission);
    showFlag({ id: 'checked', title: 'Checked', type: 'success' });
  };
  return <Stack><Text>{ctx ? 'viewer ' + ctx.accountId + ' on ' + ctx.extension.type : 'no context yet'}</Text><Button onClick={check}>Check</Button><Text>{me}</Text><Link href="https://example.com/docs">Docs</Link></Stack>;
};
ForgeReconciler.render(<App />);
`, async () => ({}), {
    fetchProduct: async (req) => { fetched.push(req); return { body: JSON.stringify({ permissions: { ADMINISTER: { havePermission: false } } }), headers: { 'content-type': 'application/json' }, status: 200, statusText: 'OK', isAttachment: false }; },
  });
  try {
    assert.strictEqual(host.text(host.docs[0]), 'no context yet\n[Check]\nDocs', 'useProductContext is undefined on the first render');
    await host.waitIdle();
    assert.match(host.text(), /^viewer admin-1 on jira:adminPage\n\[Check\]\nDocs$/);
    await host.click('Check');
    await host.waitIdle();
    assert.match(host.text(), /\n200 false\nDocs$/);
    assert.deepStrictEqual(fetched.map((f) => [f.product, f.restPath, f.fetchRequestInit.method ?? 'GET', f.context.accountId]), [['jira', '/rest/api/3/mypermissions?permissions=ADMINISTER', 'GET', 'admin-1']]);
    assert.deepStrictEqual(host.flags.map((f) => [f.id, f.title, f.type, f.closed]), [['checked', 'Checked', 'success', false]]);
    assert.deepStrictEqual([host.errors, host.harnessMissing], [[], []]);
    assert.deepStrictEqual(await host.click('Docs'), { clicked: true, submitted: false });
    assert.deepStrictEqual(host.harnessMissing.map((m) => m.what), ['navigation to https://example.com/docs'], 'a link leaving the page is recorded, not followed');
  } finally { await host.close(); }
});

test('labels: a required Label still names its input; two elements with one label are AMBIGUOUS, never a guess', async () => {
  const host = await renderTemp(`import React from 'react';
import ForgeReconciler, { Button, Label, RequiredAsterisk, Stack, Textfield } from '@forge/react';
const App = () => <Stack><Label labelFor="name">Display name<RequiredAsterisk /></Label><Textfield id="name" name="name" defaultValue="Ada" /><Button>Go</Button><Button>Go</Button></Stack>;
ForgeReconciler.render(<App />);
`);
  try {
    assert.strictEqual(host.text(), 'Display name *\n[Ada]\n[Go]\n[Go]');
    assert.deepStrictEqual([host.findByLabel('Display name').type, host.findByLabel('Display name').value], ['Textfield', 'Ada']);
    assert.throws(() => host.findByLabel('Go'), (e) => e.code === 'AMBIGUOUS');
    await assert.rejects(host.click('Go'), (e) => e.code === 'AMBIGUOUS' && /names 2 elements/.test(e.message));
  } finally { await host.close(); }
});

test('a failing resolver reaches the app as a rejected invoke', async () => {
  const host = await renderFixture(backend({ getPreferences: () => { throw new Error('boom'); } }));
  try {
    await host.waitIdle();
    assert.strictEqual(host.text(), 'Could not load preferences: There was an error invoking the function - boom');
    assert.deepStrictEqual(host.invokes.map((i) => [i.functionKey, i.state, i.error.message]), [['getPreferences', 'error', 'boom']]);
  } finally { await host.close(); }
});

test('classic JSX: a .jsx file without `import React` fails at runtime, as Forge\'s Babel build does', async () => {
  const host = await renderTemp(`import ForgeReconciler, { Text } from '@forge/react';\nForgeReconciler.render(<Text>hi</Text>);\n`);
  try {
    assert.deepStrictEqual(host.errors.map((e) => [e.kind, e.message]), [['eval', 'React is not defined']]);
    assert.strictEqual(host.tree(), null);
  } finally { await host.close(); }
});

test('an unhandled rejection in the app is its console error, never the end of the page', async () => {
  const host = await renderTemp(`import React, { useEffect } from 'react';
import ForgeReconciler, { Text } from '@forge/react';
import { invoke } from '@forge/bridge';
const App = () => { useEffect(() => { invoke('missing'); }, []); return <Text>ok</Text>; };
ForgeReconciler.render(<App />);
`, async () => { throw new Error('no such resolver'); });
  try {
    await host.waitIdle();
    assert.deepStrictEqual(host.errors.map((e) => [e.kind, e.message]), [['unhandledRejection', 'There was an error invoking the function - no such resolver']]);
    assert.strictEqual(host.text(), 'ok');
  } finally { await host.close(); }
});

test('the fence: app code that escapes the realm is in a separate process that cannot read files or exec', async () => {
  const secret = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'uikit-secret-')), 'secret.json');
  fs.writeFileSync(secret, '{"answer": 42}');
  // the vm realm is not a security boundary: a host function's constructor is the host's Function
  const escape = `import React from 'react';
import ForgeReconciler, { Text } from '@forge/react';
const p = setTimeout.constructor('return process')();
const tryIt = (f) => { try { f(); return 'allowed'; } catch (e) { return e.code ?? e.message; } };
const read = tryIt(() => p.getBuiltinModule('fs').readFileSync(${JSON.stringify(secret)}, 'utf8'));
const exec = tryIt(() => p.getBuiltinModule('child_process').execFileSync('/bin/echo', ['x']));
const App = () => <Text>{'read ' + read + ', exec ' + exec + ', same process as the scorer ' + (p.pid === ${process.pid})}</Text>;
ForgeReconciler.render(<App />);
`;
  const host = await renderTemp(escape);
  try {
    assert.strictEqual(host.fence, 'sandbox');
    assert.strictEqual(host.text(), 'read EPERM, exec EPERM, same process as the scorer false');
  } finally { await host.close(); }
  // forge-dev's fallback inside the entrant's workspace, where macOS refuses a nested sandbox
  const dev = await renderTemp(escape, async () => ({}), { fence: 'node-permission' });
  try {
    assert.strictEqual(dev.text(), 'read ERR_ACCESS_DENIED, exec ERR_ACCESS_DENIED, same process as the scorer false');
  } finally { await dev.close(); }
});

test('a commit loop is reported as NOT_IDLE after a counted number of turns, never by a clock', async () => {
  const host = await renderTemp(`import React, { useEffect, useState } from 'react';
import ForgeReconciler, { Text } from '@forge/react';
const App = () => { const [n, setN] = useState(0); useEffect(() => { setN(n + 1); }); return <Text>{n}</Text>; };
ForgeReconciler.render(<App />);
`);
  try {
    await assert.rejects(host.waitIdle(), (e) => e.code === 'NOT_IDLE' && /did not settle after 1001 turns/.test(e.message));
    assert.ok(host.docs.length > 1000, `${host.docs.length} commits`);
    assert.strictEqual(host.now(), 0, 'virtual timer time never moved');
  } finally { await host.close(); }
});

test('a poller is not a loop: advance runs each interval at its own virtual time; Date moves with the timers', async () => {
  let polls = 0;
  const host = await renderTemp(`import React, { useEffect, useState } from 'react';
import ForgeReconciler, { Text } from '@forge/react';
import { invoke } from '@forge/bridge';
const start = Date.now();
const App = () => {
  const [n, setN] = useState('none');
  useEffect(() => { const id = setInterval(() => invoke('progress').then((r) => setN(r.n)), 1000); return () => clearInterval(id); }, []);
  return <Text>Migrated {n} after {Math.round((Date.now() - start) / 1000)} s, {typeof Date()} {new Date(0).toISOString()} {new Date() instanceof Date ? 'date' : 'no'}</Text>;
};
ForgeReconciler.render(<App />);
`, async () => ({ n: ++polls }), { startTime: Date.parse('2026-10-09T21:00:00Z') });
  try {
    await host.waitIdle();
    assert.strictEqual(host.text(), 'Migrated none after 0 s, string 1970-01-01T00:00:00.000Z date');
    // 1500 polls: more than @forge/bridge's 500 invokes per 25 s, which it counts on Date.now()
    const idle = await host.advance(1_500_000);
    assert.strictEqual(polls, 1500);
    assert.strictEqual(host.text(), 'Migrated 1500 after 1500 s, string 1970-01-01T00:00:00.000Z date');
    assert.deepStrictEqual([idle.now, idle.timersPending, host.errors], [1_500_000, 1, []]);
  } finally { await host.close(); }
});

test('what forge deploy refuses or cannot find is an app error code', async () => {
  const invoke = async () => ({});
  await assert.rejects(render({ appDir: FIXTURE, moduleKey: 'nope', context: CONTEXT, invoke }), (e) => e.code === 'NO_MODULE');
  await assert.rejects(render({ appDir: os.tmpdir(), moduleKey: 'x', context: CONTEXT, invoke }), (e) => e.code === 'BAD_MANIFEST');
  await assert.rejects(render({ appDir: tempApp('x', { native: false }), moduleKey: 'tmp-admin', context: CONTEXT, invoke }), (e) => e.code === 'NOT_NATIVE');
  await assert.rejects(renderTemp('import React from "react";\nconst = ;\n'), (e) => e.code === 'BUILD_FAILED');
  await assert.rejects(renderTemp('import x from "not-a-kit-package";\n'), (e) => e.code === 'BUILD_FAILED' && /not one of the installed packages/.test(e.message));
  await assert.rejects(render({ appDir: FIXTURE, moduleKey: 'fixture-admin', context: CONTEXT }), (e) => e.code === 'HARNESS');
  await assert.rejects(render({ appDir: FIXTURE, moduleKey: 'fixture-admin', context: CONTEXT, invoke, fence: 'none' }), (e) => e.code === 'HARNESS' && /REFUSED: unknown fence/.test(e.message));
});

test('through the emulator: resolvers run in the Forge runtime as the viewer; forge-dev uikit drives the same host', { timeout: 180_000 }, async () => {
  const { createSite } = require(SITE);
  const { createEmulator } = require('../../emulator.cjs');
  const { main } = require('../../../bin/uikit.cjs');
  const appDir = fs.mkdtempSync(path.join(os.tmpdir(), 'uikit-host-emu-'));
  fs.cpSync(FIXTURE, appDir, { recursive: true });
  const site = await createSite({ seed: '0123456789abcdef' });
  const emu = await createEmulator({ appDir, site, runtime: 'wrapper', fence: 'dev-auto' });
  try {
    const built = await emu.build();
    assert.ok(built.functions.every((f) => f.loaded), JSON.stringify(built.functions));
    const viewer = emu.siteInfo.viewer;
    const host = await renderInEmulator(emu, { moduleKey: 'fixture-admin', asUser: viewer });
    try {
      await host.waitIdle();
      assert.strictEqual(host.findByLabel(RETENTION).value, '30');
      await host.setValue(RETENTION, 55);
      await host.click(SAVE);
      await host.waitIdle();
      assert.deepStrictEqual(host.invokes.map((i) => [i.functionKey, i.state]), [['getPreferences', 'ok'], ['savePreferences', 'ok'], ['getPreferences', 'ok']], JSON.stringify(host.invokes));
      assert.match(host.text(), new RegExp(`Saved: retention 55, digest on[\\s\\S]*1 \\| ${viewer} \\| retention 55, digest on`));
      await host.click('Check identity');
      await host.waitIdle();
      assert.match(host.text(), /\nSigned in as [^\n]+ \(200\)$/, 'requestJira reaches the site as the viewer through the proxy');
      assert.deepStrictEqual([host.errors, host.harnessMissing.map((m) => m.what)], [[], []]);
    } finally { await host.close(); }
    const stored = emu.kvs.snapshot();
    assert.match(JSON.stringify(stored), /"retentionDays":55/, JSON.stringify(stored).slice(0, 400));

    const out = [];
    const code = await main({ emu, argv: ['fixture-admin', '--set', `${RETENTION}=40`, '--set', `${DIGEST}=false`, '--click', SAVE], print: (s) => out.push(s) });
    const printed = out.join('\n');
    assert.strictEqual(code, 0, printed);
    assert.match(printed, /> set "Retention \(days\)" = "40" -> \{"changed":true\}/);
    assert.match(printed, new RegExp(`== screen\\n[\\s\\S]*\\[40\\]\\n\\[ \\] ${DIGEST}[\\s\\S]*Saved: retention 40, digest off`));
    assert.match(printed, /savePreferences\(\{"retentionDays":"40","digestEnabled":false\}\) after \d+ commit\(s\) -> ok/);
    assert.match(printed, /== tree\nRoot\n {2}Stack/);
    const bad = [];
    assert.strictEqual(await main({ emu, argv: ['fixture-admin', '--click', 'Save'], print: (s) => bad.push(s) }), 1);
    assert.match(bad.join('\n'), /> click "Save" -> FAILED NO_CONTROL: no control labelled 'Save'; labels on screen: /);
  } finally {
    await emu.close();
    await site.stop();
  }
});
