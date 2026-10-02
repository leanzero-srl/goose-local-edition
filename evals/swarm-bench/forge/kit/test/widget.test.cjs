'use strict';
// The dashboards widget host (DESIGN §6.4, P1/P2) driven by real @forge/hooks 2.0.0 widgets:
// useWidgetConfig sees EDIT_CONFIG_CHANGED after updateConfig and CONFIG_CHANGED after the host's Save;
// useWidgetContext sees LAYOUT_CHANGED on a resize; a second widget instance keeps its own config (G3);
// Save with no handler stores the last updateConfig; an onProductSave returning null stores nothing.
// Run: node --test forge/kit/test/widget.test.cjs
const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');
const { FORGE, ensureKit, playwright, scratch } = require('./helpers.cjs');

const VIEW = `import React from 'react';
import { createRoot } from 'react-dom/client';
import { view } from '@forge/bridge';
import { useWidgetConfig, useWidgetContext } from '@forge/hooks';
view.theme.enable();
function App() {
  const { config } = useWidgetConfig();
  const ctx = useWidgetContext();
  return <div><pre id="cfg">{JSON.stringify(config ?? null)}</pre><span id="w">{ctx ? String(ctx.layout?.width) : ''}</span>
    <span id="placement">{ctx?.placement ?? ''}</span><span id="wid">{ctx?.widgetId ?? ''}</span></div>;
}
createRoot(document.getElementById('root')).render(<App />);
`;
const EDIT = (nullSave) => `import React from 'react';
import { createRoot } from 'react-dom/client';
import { widgetEdit } from '@forge/dashboards-bridge';
import { useWidgetConfig } from '@forge/hooks';
${nullSave ? 'widgetEdit.onProductSave(async () => null);' : ''}
function App() {
  const { config, updateConfig } = useWidgetConfig();
  return <div><pre id="cfg">{JSON.stringify(config ?? null)}</pre>
    <button id="set" onClick={() => updateConfig({ n: (config?.n ?? 0) + 1 })}>set</button></div>;
}
createRoot(document.getElementById('root')).render(<App />);
`;
const MANIFEST = `modules:
  dashboards:widget:
    - key: widget-a
      title: A
      description: Widget A
      thumbnail: https://developer.atlassian.com/platform/forge/images/icons/issue-panel-icon.svg
      resource: view
      edit:
        resource: edit-plain
    - key: widget-b
      title: B
      description: Widget B
      thumbnail: https://developer.atlassian.com/platform/forge/images/icons/issue-panel-icon.svg
      resource: view
      edit:
        resource: edit-null
resources:
  - key: view
    path: static/view/build
  - key: edit-plain
    path: static/edit-plain/build
  - key: edit-null
    path: static/edit-null/build
permissions:
  scopes:
    - storage:app
app:
  id: ari:cloud:ecosystem::app/00000000-0000-0000-0000-000000000000
  runtime:
    name: nodejs22.x
`;

test('dashboards host: hooks events, Save semantics, independent instances, live resize', { timeout: 240_000 }, async () => {
  ensureKit();
  const { createSite } = require(path.join(FORGE, 'site', 'site.cjs'));
  const { createEmulator } = require(path.join(FORGE, 'kit', 'lib', 'emulator.cjs'));
  const { kitPaths } = require(path.join(FORGE, 'kit', 'lib', 'kitpaths.cjs'));
  const appDir = path.join(scratch('widget'), 'app');
  fs.mkdirSync(path.join(appDir, 'src'), { recursive: true });
  fs.writeFileSync(path.join(appDir, 'src', 'index.js'), '');
  fs.writeFileSync(path.join(appDir, 'manifest.yml'), MANIFEST);
  const paths = kitPaths(path.join(FORGE, 'kit'));
  for (const [name, src] of [['view', VIEW], ['edit-plain', EDIT(false)], ['edit-null', EDIT(true)]]) {
    const dir = path.join(appDir, 'static', name);
    fs.mkdirSync(path.join(dir, 'build'), { recursive: true });
    fs.writeFileSync(path.join(dir, 'index.jsx'), src);
    await paths.require('esbuild').build({ entryPoints: [path.join(dir, 'index.jsx')], bundle: true, format: 'iife', platform: 'browser',
      outfile: path.join(dir, 'build', 'main.js'), jsx: 'automatic', define: { 'process.env.NODE_ENV': '"production"' }, nodePaths: [paths.appModules], logLevel: 'silent' });
    fs.writeFileSync(path.join(dir, 'build', 'index.html'), '<!doctype html><html><head><meta charset="utf-8"></head><body><div id="root"></div><script src="./main.js"></script></body></html>');
  }

  const site = await createSite({ seed: '0123456789abcdef' });
  const emu = await createEmulator({ appDir, site, runtime: 'wrapper' });
  const browser = await playwright().chromium.launch({ headless: true, executablePath: process.env.BENCH_BROWSER_EXECUTABLE || undefined });
  const errors = [];
  const open = async (opts) => {
    const page = await browser.newPage();
    page.on('pageerror', (e) => errors.push(String(e)));
    page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });
    await emu.openSurface(page, { theme: 'light', layout: { width: 480, height: 300 }, ...opts });
    return page;
  };
  const text = (page, sel, want) => page.waitForFunction(([s, w]) => document.querySelector(s)?.textContent === w, [sel, want], { timeout: 20_000 })
    .catch(async () => assert.fail(`${sel}: wanted ${want}, saw ${await page.locator(sel).textContent().catch(() => '(absent)')}`));
  try {
    const view1 = await open({ moduleKey: 'widget-a', entry: 'view', widgetId: 'w1' });
    await text(view1, '#cfg', 'null');
    await text(view1, '#w', '480');
    await text(view1, '#placement', 'DASHBOARD_VIEW');
    const view2 = await open({ moduleKey: 'widget-a', entry: 'view', widgetId: 'w2' });
    await text(view2, '#wid', 'w2');

    // Edit: each updateConfig is visible to useWidgetConfig through EDIT_CONFIG_CHANGED + getContext.
    const edit1 = await open({ moduleKey: 'widget-a', entry: 'edit', widgetId: 'w1' });
    await text(edit1, '#cfg', 'null');
    await edit1.click('#set');
    await text(edit1, '#cfg', '{"n":1}');
    await edit1.click('#set');
    await text(edit1, '#cfg', '{"n":2}');
    await text(view1, '#cfg', 'null'); // nothing reaches the dashboard before Save
    const saved = await emu.hostSave(edit1);
    assert.deepStrictEqual(saved.config, { n: 2 });
    assert.strictEqual(saved.via, 'updateConfig');
    await text(view1, '#cfg', '{"n":2}'); // CONFIG_CHANGED reached the live view of the same widget
    await text(view2, '#cfg', 'null');    // the second instance keeps its own (absent) config
    assert.deepStrictEqual(emu.widgetConfigs(), { w1: { n: 2 } });

    // Live resize: useWidgetContext follows LAYOUT_CHANGED.
    await emu.resizeSurface(view1, { width: 720, height: 300 });
    await text(view1, '#w', '720');

    // onProductSave returning null stores nothing.
    const edit3 = await open({ moduleKey: 'widget-b', entry: 'edit', widgetId: 'w3' });
    await edit3.click('#set');
    await text(edit3, '#cfg', '{"n":1}');
    const nulled = await emu.hostSave(edit3);
    assert.strictEqual(nulled.config, null);
    assert.deepStrictEqual(emu.widgetConfigs(), { w1: { n: 2 } });

    // A fresh view of w1 opens with the stored config.
    const view1b = await open({ moduleKey: 'widget-a', entry: 'view', widgetId: 'w1' });
    await text(view1b, '#cfg', '{"n":2}');

    assert.deepStrictEqual(errors, []);
    assert.deepStrictEqual(emu.cspReports(), []);
    assert.deepStrictEqual(emu.harnessMissing, []);
  } finally {
    await browser.close();
    await emu.close();
    await site.stop();
  }
});
