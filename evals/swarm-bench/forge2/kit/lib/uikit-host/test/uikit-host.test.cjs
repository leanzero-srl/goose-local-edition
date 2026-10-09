'use strict';
// node --test evals/swarm-bench/forge2/kit/lib/uikit-host/test/
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

function backend({ getSettings, saveSettings } = {}) {
  const state = { settings: { backgroundShare: 70, aiEnabled: true },
    changes: [{ when: 1, who: 'alice', what: 'installed' }, { when: 2, who: 'bob', what: 'share 60' }, { when: 10, who: 'carol', what: 'AI off' }] };
  const calls = [];
  const invoke = async (call) => {
    calls.push(call);
    if (call.functionKey === 'getSettings') return getSettings ? getSettings(state) : { ...state.settings, changes: state.changes };
    if (call.functionKey === 'saveSettings') {
      const save = () => { state.settings = { backgroundShare: Number(call.payload.backgroundShare), aiEnabled: call.payload.aiEnabled === true }; return state.settings; };
      return saveSettings ? saveSettings(save) : save();
    }
    throw new Error(`no resolver '${call.functionKey}'`);
  };
  return { invoke, calls, state };
}

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
const renderTemp = (source, invoke = async () => ({})) => render({ appDir: tempApp(source), moduleKey: 'tmp-admin', context: { ...CONTEXT, moduleKey: 'tmp-admin' }, invoke });

test('boot: the first commit paints before the one invoke; the screen reads as text', async () => {
  const be = backend();
  const host = await render({ appDir: FIXTURE, moduleKey: 'fixture-admin', context: CONTEXT, invoke: be.invoke });
  try {
    assert.strictEqual(host.text(host.docs[0]), 'Loading settings…', 'the first ForgeDoc is the loading state');
    const idle = await host.waitIdle();
    assert.deepStrictEqual(host.invokes.map((i) => [i.functionKey, i.reconcilesBefore, i.state]), [['getSettings', 1, 'ok']]);
    assert.strictEqual(be.calls[0].context.accountId, 'admin-1', 'the resolver gets the frontend context');
    assert.strictEqual(idle.turns <= 4, true, `boot settles in a few turns, took ${idle.turns}`);
    assert.strictEqual(host.text(), [
      'Settings', 'Background share (%)', '[70]', '[x] AI explanations enabled', '[Save settings]',
      'Recent admin changes', 'When | Who | What', '10 | carol | AI off', '2 | bob | share 60', '1 | alice | installed', '[Enable theming]', '[Check identity]',
    ].join('\n'));
    const share = host.findByLabel('Background share (%)');
    assert.strictEqual(share.type, 'Textfield');
    assert.strictEqual(share.via, 'Label labelFor');
    assert.strictEqual(share.value, '70');
    assert.strictEqual(share.props.type, 'number');
    assert.strictEqual(host.findByLabel('AI explanations enabled').checked, true);
    assert.strictEqual(host.findByLabel('Save settings').type, 'LoadingButton');
    assert.strictEqual(host.tree().type, 'Root');
    assert.deepStrictEqual([host.errors, host.harnessMissing], [[], []]);
  } finally { host.close(); }
});

test('drive by label: type, toggle, submit -> exactly one invoke with the DOM\'s string value; timers are virtual', async () => {
  const be = backend();
  const host = await render({ appDir: FIXTURE, moduleKey: 'fixture-admin', context: CONTEXT, invoke: be.invoke });
  try {
    await host.waitIdle();
    assert.deepStrictEqual(await host.setValue('Background share (%)', 60), { changed: true });
    assert.deepStrictEqual(await host.click('AI explanations enabled'), { clicked: true });
    assert.deepStrictEqual(await host.setValue('AI explanations enabled', false), { changed: false, reason: 'unchanged' });
    assert.deepStrictEqual(await host.click('Save settings'), { clicked: true, submitted: true });
    const idle = await host.waitIdle();
    const saves = host.invokes.filter((i) => i.functionKey === 'saveSettings');
    assert.strictEqual(saves.length, 1);
    assert.deepStrictEqual(saves[0].payload, { backgroundShare: '60', aiEnabled: false }, 'a number input hands over the string the DOM holds');
    assert.match(host.text(), /\[60\]\n\[ \] AI explanations enabled\n\[Save settings\]\nSaved: share 60, AI off\n/);
    assert.strictEqual(idle.timersPending, 1, 'the 3 s flash timer waits for virtual time');
    await host.advance(2999);
    assert.match(host.text(), /Saved: share 60/);
    await host.advance(1);
    assert.doesNotMatch(host.text(), /Saved:/);
    assert.strictEqual(host.now(), 3000);
    assert.deepStrictEqual(host.errors, []);
  } finally { host.close(); }
});

test('validation: an out-of-range value shows the error and nothing is submitted', async () => {
  const be = backend();
  const host = await render({ appDir: FIXTURE, moduleKey: 'fixture-admin', context: CONTEXT, invoke: be.invoke });
  try {
    await host.waitIdle();
    await host.setValue('Background share (%)', 5);
    await host.waitIdle();
    assert.match(host.text(), /\[5\] \(invalid\)\nEnter a number from 10 to 90/, 'useForm validates on blur');
    assert.deepStrictEqual(await host.click('Save settings'), { clicked: true, submitted: true });
    await host.waitIdle();
    assert.deepStrictEqual(host.invokes.map((i) => i.functionKey), ['getSettings']);
  } finally { host.close(); }
});

test('a loading button is not clickable: a second Save while the first is in flight does nothing', async () => {
  let release;
  const gate = new Promise((r) => { release = r; });
  const be = backend({ saveSettings: async (save) => { await gate; return save(); } });
  const host = await render({ appDir: FIXTURE, moduleKey: 'fixture-admin', context: CONTEXT, invoke: be.invoke });
  try {
    await host.waitIdle();
    await host.click('Save settings');
    await host.flush();
    assert.strictEqual(host.findByLabel('Save settings').props.isLoading, true);
    assert.match(host.text(), /\[Save settings\] \(loading\)/);
    assert.deepStrictEqual(await host.click('Save settings'), { clicked: false, reason: 'loading' });
    release();
    await host.waitIdle();
    assert.strictEqual(host.invokes.filter((i) => i.functionKey === 'saveSettings').length, 1);
    assert.deepStrictEqual(host.invokes.find((i) => i.functionKey === 'saveSettings').payload, { backgroundShare: 70, aiEnabled: true },
      'untouched fields submit the app\'s own defaultValues (a number here); only typed ones carry the DOM\'s string');
  } finally { host.close(); }
});

test('labels: a table by its caption (rows sorted as ADS sorts them), a missing label lists the screen, codes for misuse', async () => {
  const be = backend();
  const host = await render({ appDir: FIXTURE, moduleKey: 'fixture-admin', context: CONTEXT, invoke: be.invoke });
  try {
    await host.waitIdle();
    const table = host.findByLabel('Recent admin changes');
    assert.strictEqual(table.type, 'DynamicTable');
    assert.strictEqual(host.text(table), 'Recent admin changes\nWhen | Who | What\n10 | carol | AI off\n2 | bob | share 60\n1 | alice | installed');
    assert.strictEqual(host.findByLabel('Settings').type, 'FormSection');
    assert.strictEqual(host.findByLabel('Nope'), null);
    await assert.rejects(host.click('Nope'), (e) => e instanceof UikitHostError && e.code === 'NO_CONTROL' && /"Background share \(%\)"/.test(e.message));
    await assert.rejects(host.click('Background share (%)'), (e) => e.code === 'NOT_CLICKABLE');
    await assert.rejects(host.setValue('Save settings', 'x'), (e) => e.code === 'NOT_AN_INPUT');
    await assert.rejects(host.setValue('AI explanations enabled', 'yes'), (e) => e.code === 'BAD_VALUE');
  } finally { host.close(); }
});

test('a bridge op this host does not model rejects in the app and is recorded, never answered', async () => {
  const be = backend();
  const host = await render({ appDir: FIXTURE, moduleKey: 'fixture-admin', context: CONTEXT, invoke: be.invoke });
  try {
    await host.waitIdle();
    await host.click('Enable theming');
    await host.waitIdle();
    assert.deepStrictEqual(host.harnessMissing.map((m) => m.what), ["bridge op 'enableTheming'"]);
    assert.deepStrictEqual(host.errors.map((e) => [e.kind, e.message]), [['handler', "uikit host: bridge op 'enableTheming' is not modelled"]]);
  } finally { host.close(); }
});

test('context, flags and requestJira: the bridge ops an admin page uses besides invoke', async () => {
  const fetched = [];
  const host = await render({
    appDir: tempApp(`import React, { useState } from 'react';
import ForgeReconciler, { Button, Stack, Text, useProductContext } from '@forge/react';
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
  return <Stack><Text>{ctx ? 'viewer ' + ctx.accountId + ' on ' + ctx.extension.type : 'no context yet'}</Text><Button onClick={check}>Check</Button><Text>{me}</Text></Stack>;
};
ForgeReconciler.render(<App />);
`),
    moduleKey: 'tmp-admin', context: { ...CONTEXT, moduleKey: 'tmp-admin', locale: 'en-US' }, invoke: async () => ({}),
    fetchProduct: async (req) => { fetched.push(req); return { body: JSON.stringify({ permissions: { ADMINISTER: { havePermission: false } } }), headers: { 'content-type': 'application/json' }, status: 200, statusText: 'OK', isAttachment: false }; },
  });
  try {
    assert.strictEqual(host.text(host.docs[0]), 'no context yet\n[Check]', 'useProductContext is undefined on the first render');
    await host.waitIdle();
    assert.match(host.text(), /^viewer admin-1 on jira:adminPage\n\[Check\]$/);
    await host.click('Check');
    await host.waitIdle();
    assert.match(host.text(), /\n200 false$/);
    assert.deepStrictEqual(fetched.map((f) => [f.product, f.restPath, f.fetchRequestInit.method ?? 'GET', f.context.accountId]), [['jira', '/rest/api/3/mypermissions?permissions=ADMINISTER', 'GET', 'admin-1']]);
    assert.deepStrictEqual(host.flags.map((f) => [f.id, f.title, f.type, f.closed]), [['checked', 'Checked', 'success', false]]);
    assert.deepStrictEqual([host.errors, host.harnessMissing], [[], []]);
  } finally { host.close(); }
});

test('a failing resolver reaches the app as a rejected invoke', async () => {
  const be = backend({ getSettings: () => { throw new Error('boom'); } });
  const host = await render({ appDir: FIXTURE, moduleKey: 'fixture-admin', context: CONTEXT, invoke: be.invoke });
  try {
    await host.waitIdle();
    assert.strictEqual(host.text(), 'Could not load settings: There was an error invoking the function - boom');
    assert.deepStrictEqual(host.invokes.map((i) => [i.functionKey, i.state, i.error.message]), [['getSettings', 'error', 'boom']]);
  } finally { host.close(); }
});

test('classic JSX: a .jsx file without `import React` fails at runtime, as Forge\'s Babel build does', async () => {
  const host = await renderTemp(`import ForgeReconciler, { Text } from '@forge/react';\nForgeReconciler.render(<Text>hi</Text>);\n`);
  try {
    assert.deepStrictEqual(host.errors.map((e) => [e.kind, e.message]), [['eval', 'React is not defined']]);
    assert.strictEqual(host.tree(), null);
  } finally { host.close(); }
});

test('an unhandled rejection in the app is its console error, never the end of the hosting process', async () => {
  // node:test listens for unhandled rejections itself (and fails the test); a probe process does not.
  const theirs = process.listeners('unhandledRejection');
  process.removeAllListeners('unhandledRejection');
  try {
    const host = await renderTemp(`import React, { useEffect } from 'react';
import ForgeReconciler, { Text } from '@forge/react';
import { invoke } from '@forge/bridge';
const App = () => { useEffect(() => { invoke('missing'); }, []); return <Text>ok</Text>; };
ForgeReconciler.render(<App />);
`, async () => { throw new Error('no such resolver'); });
    await host.waitIdle();
    await new Promise((r) => setImmediate(r));
    assert.deepStrictEqual(host.errors.map((e) => [e.kind, e.message]), [['unhandledRejection', 'There was an error invoking the function - no such resolver']]);
    assert.strictEqual(host.text(), 'ok');
    host.close();
    assert.strictEqual(process.listenerCount('unhandledRejection'), 0, 'closing the last host removes its listener');
  } finally {
    for (const l of theirs) process.on('unhandledRejection', l);
  }
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
  } finally { host.close(); }
});

test('what forge deploy refuses or cannot find is an app error code', async () => {
  const invoke = async () => ({});
  await assert.rejects(render({ appDir: FIXTURE, moduleKey: 'nope', context: CONTEXT, invoke }), (e) => e.code === 'NO_MODULE');
  await assert.rejects(render({ appDir: tempApp('x', { native: false }), moduleKey: 'tmp-admin', context: CONTEXT, invoke }), (e) => e.code === 'NOT_NATIVE');
  await assert.rejects(renderTemp('import React from "react";\nconst = ;\n'), (e) => e.code === 'BUILD_FAILED');
  await assert.rejects(renderTemp('import x from "not-a-kit-package";\n'), (e) => e.code === 'BUILD_FAILED' && /not one of the installed packages/.test(e.message));
  await assert.rejects(render({ appDir: FIXTURE, moduleKey: 'fixture-admin', context: CONTEXT }), (e) => e.code === 'HARNESS');
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
      assert.strictEqual(host.findByLabel('Background share (%)').value, '70');
      await host.setValue('Background share (%)', 55);
      await host.click('Save settings');
      await host.waitIdle();
      assert.deepStrictEqual(host.invokes.map((i) => [i.functionKey, i.state]), [['getSettings', 'ok'], ['saveSettings', 'ok'], ['getSettings', 'ok']], JSON.stringify(host.invokes));
      assert.match(host.text(), new RegExp(`Saved: share 55, AI on[\\s\\S]*1 \\| ${viewer} \\| share 55, AI on`));
      assert.strictEqual(host.tree().children.length, 1);
      await host.click('Check identity');
      await host.waitIdle();
      assert.match(host.text(), /\nSigned in as [^\n]+ \(200\)$/, 'requestJira reaches the site as the viewer through the proxy');
      assert.deepStrictEqual([host.errors, host.harnessMissing.map((m) => m.what)], [[], []]);
    } finally { host.close(); }
    const stored = emu.kvs.snapshot();
    assert.match(JSON.stringify(stored), /"backgroundShare":55/, JSON.stringify(stored).slice(0, 400));

    const out = [];
    const code = await main({ emu, argv: ['fixture-admin', '--set', 'Background share (%)=40', '--set', 'AI explanations enabled=false', '--click', 'Save settings'], print: (s) => out.push(s) });
    const printed = out.join('\n');
    assert.strictEqual(code, 0, printed);
    assert.match(printed, /> set "Background share \(%\)" = "40" -> \{"changed":true\}/);
    assert.match(printed, /== screen\n[\s\S]*\[40\]\n\[ \] AI explanations enabled[\s\S]*Saved: share 40, AI off/);
    assert.match(printed, /saveSettings\(\{"backgroundShare":"40","aiEnabled":false\}\) after \d+ commit\(s\) -> ok/);
    assert.match(printed, /== tree\nRoot\n {2}Stack/);
    const bad = [];
    assert.strictEqual(await main({ emu, argv: ['fixture-admin', '--click', 'Save'], print: (s) => bad.push(s) }), 1);
    assert.match(bad.join('\n'), /> click "Save" -> FAILED NO_CONTROL: no control labelled 'Save'; labels on screen: /);
  } finally {
    await emu.close();
    await site.stop();
  }
});
