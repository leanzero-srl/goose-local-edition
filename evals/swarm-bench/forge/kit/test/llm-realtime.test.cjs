'use strict';
// Forge LLM and Realtime through Atlassian's pinned runtime wrapper (DESIGN §17.2 A/B):
//   LLM       list() = the public models page (8 models, all active), the scripted answers in order
//             (clean, digits + hidden + unknown ids, refusal, malformed, 500 ForgeLlmAPIError, then clean),
//             phase() restarts it, every prompt logged, the docs' validation rules, stream() deltas assembled by
//             index, no llm module -> refused.
//   Realtime  a widget's bridge subscriptions receive: publishGlobal from a consumer (async event); NOT publish()
//             from a consumer (rejected, logged); publish() from a frontend-invoked resolver; the page's own
//             publish; token claims scope a global channel.
// Run: node --test forge/kit/test/llm-realtime.test.cjs
const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');
const { FORGE, ensureKit, playwright, scratch } = require('./helpers.cjs');

const APP_ID = 'ari:cloud:ecosystem::app/00000000-0000-0000-0000-000000000000';
const load = () => ({ createSite: require(path.join(FORGE, 'site', 'site.cjs')).createSite, createEmulator: require(path.join(FORGE, 'kit', 'lib', 'emulator.cjs')).createEmulator });

const LLM_SRC = `import Resolver from '@forge/resolver';
import { chat, list, stream } from '@forge/llm';
const TOOL = { type: 'function', function: { name: 'report_scope', description: 'Report the scope change.',
  parameters: { type: 'object', properties: { summary: { type: 'string' }, changeIds: { type: 'array', items: { type: 'string' } } }, required: ['summary', 'changeIds'] } } };
const call = async (f) => { try { return await f(); } catch (e) { return { threw: e.name, status: e.status, code: e.code, message: e.message }; } };
const resolver = new Resolver();
resolver.define('explain', async ({ payload }) => {
  const models = await list();
  const model = models.models.find((m) => m.status === 'active').model;
  const out = { models, model, answers: [] };
  for (let i = 0; i < 6; i++) {
    out.answers.push(await call(() => chat({ model, messages: [{ role: 'system', content: 'Explain sprint scope creep.' }, { role: 'user', content: 'Visible changes: ' + payload.ids.join(', ') }],
      tools: [TOOL], tool_choice: { type: 'function', function: { name: 'report_scope' } } })));
  }
  out.invalid = await call(() => chat({ model, temperature: 0.2, top_p: 0.5, messages: [{ role: 'user', content: 'x' }] }));
  out.noSampling = await call(() => chat({ model: 'claude-opus-5', temperature: 0.2, messages: [{ role: 'user', content: 'x' }] }));
  const chunks = [];
  const s = await stream({ model, messages: [{ role: 'user', content: 'Visible changes: ' + payload.ids[0] }], tools: [TOOL], tool_choice: 'required' });
  for await (const c of s) chunks.push(c);
  s.close();
  out.stream = chunks;
  out.sampled = await call(() => chat({ model: 'claude-sonnet-4-6', temperature: 0.2, messages: [{ role: 'user', content: 'x' }] }));
  return out;
});
export const handler = resolver.getDefinitions();
`;
const llmManifest = (withLlm) => `modules:
  jira:sprintAction:
    - key: explainer
      title: Explain
      resource: main
      resolver:
        function: resolver
${withLlm ? '  llm:\n    - key: llm\n      model:\n        - claude\n' : ''}  function:
    - key: resolver
      handler: index.handler
resources:
  - key: main
    path: static/main
permissions:
  scopes:
    - read:jira-work
app:
  id: ${APP_ID}
  runtime:
    name: nodejs22.x
`;

test('Forge LLM: list, the scripted answers in order, logged prompts, validation, stream, refusal without the module', { timeout: 180_000 }, async () => {
  ensureKit();
  const { createSite, createEmulator } = load();
  const appDir = path.join(scratch('llm'), 'app');
  fs.mkdirSync(path.join(appDir, 'src'), { recursive: true });
  fs.mkdirSync(path.join(appDir, 'static', 'main'), { recursive: true });
  fs.writeFileSync(path.join(appDir, 'static', 'main', 'index.html'), '<!doctype html><html><body></body></html>');
  fs.writeFileSync(path.join(appDir, 'src', 'index.js'), LLM_SRC);
  fs.writeFileSync(path.join(appDir, 'manifest.yml'), llmManifest(true));
  const site = await createSite({ seed: '0123456789abcdef' });
  const emu = await createEmulator({ appDir, site, runtime: 'wrapper' });
  try {
    const viewer = site.pack.viewer;
    const visible = site.pack.history.filter((c) => !site.pack.issues.find((i) => i.id === c.issueId).hiddenFrom.includes(viewer)).slice(0, 3).map((c) => c.changelogId);
    const sprint = site.pack.sprints.find((s) => s.state === 'active');
    const ctx = { cloudId: site.pack.cloudId, moduleKey: 'explainer', extension: { type: 'jira:sprintAction', sprint: { id: String(sprint.id), state: 'active' } } };
    const r = await emu.invokeResolver('explainer', 'explain', { ids: visible }, ctx, viewer);
    assert.ok(r.ok, JSON.stringify(r.error));
    const { models, model, answers } = r.result;
    assert.deepStrictEqual(models.models, ['claude-haiku-4-5-20251001', 'claude-sonnet-4-5-20250929', 'claude-sonnet-4-6', 'claude-sonnet-5',
      'claude-opus-4-6', 'claude-opus-4-7', 'claude-opus-4-8', 'claude-opus-5'].map((m) => ({ model: m, status: 'active' })), 'exactly the public models page (Aug 3, 2026)');
    const [clean, digits, refusal, malformed, error, after] = answers;
    const args = (a) => a.choices[0].message.tool_calls[0].function.arguments;
    assert.strictEqual(clean.choices[0].finish_reason, 'tool_use');
    assert.deepStrictEqual(clean.choices[0].message.content.map((p) => p.type), ['text'], 'text parts beside the tool call, as the README example');
    assert.doesNotMatch(args(clean).summary, /\d/);
    assert.deepStrictEqual(args(clean).changeIds, visible, 'clean cites exactly the ids the prompt carried');
    assert.match(args(digits).summary, /\d/);
    const hidden = args(digits).changeIds[1];
    const hiddenIssue = site.pack.issues.find((i) => i.id === [...site.pack.history, ...site.pack.live].find((c) => c.changelogId === hidden).issueId);
    assert.ok(hiddenIssue.hiddenFrom.includes(viewer), 'the digits answer cites a change hidden from the asking user');
    assert.ok(![...site.pack.history, ...site.pack.live].some((c) => c.changelogId === args(digits).changeIds[2]), 'and one unknown id');
    assert.strictEqual(refusal.choices[0].finish_reason, 'refusal');
    assert.strictEqual(refusal.choices[0].message.tool_calls, undefined);
    assert.notStrictEqual(typeof args(malformed).summary, 'string');
    assert.ok(!Array.isArray(args(malformed).changeIds));
    assert.deepStrictEqual([error.threw, error.status, error.code], ['ForgeLlmAPIError', 500, 'INTERNAL_SERVER_ERROR']);
    assert.deepStrictEqual(args(after).changeIds, visible, 'then clean again');
    assert.deepStrictEqual([r.result.invalid.threw, r.result.invalid.status, r.result.invalid.code], ['ForgeLlmAPIError', 400, 'INVALID_REQUEST']);
    assert.match(r.result.invalid.message, /temperature and top_p cannot be specified together/);
    assert.match(r.result.noSampling.message, /claude-opus-5 does not support the temperature and top_p sampling parameters/);
    assert.ok(r.result.sampled.choices, 'a model the docs do not list accepts temperature');
    // The stream: text deltas, one tool-call delta per argument key (same id and index), then finish_reason.
    const chunks = r.result.stream;
    assert.ok(chunks.length >= 4, JSON.stringify(chunks));
    assert.deepStrictEqual(chunks.map((c) => c.choices[0].finish_reason ?? null).filter(Boolean), ['tool_use']);
    assert.strictEqual(chunks.at(-1).choices[0].finish_reason, 'tool_use');
    const assembled = {};
    for (const c of chunks) for (const tc of c.choices[0].message.tool_calls ?? []) {
      const slot = (assembled[tc.index] ??= { id: tc.id, name: tc.function.name, arguments: {} });
      assert.strictEqual(slot.id, tc.id);
      Object.assign(slot.arguments, tc.function.arguments);
    }
    assert.deepStrictEqual(Object.keys(assembled), ['0']);
    assert.strictEqual(assembled[0].name, 'report_scope');
    assert.deepStrictEqual(assembled[0].arguments.changeIds, [visible[0]]);
    assert.ok(chunks.filter((c) => c.choices[0].message.tool_calls).length >= 2, 'the tool call arrives in more than one delta');
    assert.strictEqual(chunks.map((c) => c.choices[0].message.content.map((p) => p.text).join('')).join(''), "I'll report the sprint's scope change with report_scope.");

    const log = await emu.llm.log();
    const chats = log.entries.filter((e) => e.op === 'chat');
    assert.strictEqual(chats.length, 9);
    assert.deepStrictEqual(chats.slice(0, 6).map((e) => e.step), ['clean', 'digits', 'refusal', 'malformed', 'error', 'clean']);
    assert.ok(chats.slice(0, 6).every((e) => e.asUser === viewer && e.moduleKey === 'explainer' && e.model === model && e.modelStatus === 'active'));
    assert.ok(log.entries.some((e) => e.op === 'stream' && e.chunks.length === chunks.length));
    assert.match(chats[0].request.messages[1].content, new RegExp(visible[0]), 'the full prompt is logged for the leak scan');
    assert.ok(emu.log.some((c) => c.service === 'llm' && c.op === 'list'));
    await emu.llm.phase('ui');
    assert.strictEqual((await emu.llm.log()).state.next, 'clean');
  } finally {
    await emu.close();
    await site.stop();
  }

  // The same app without an `llm` module: the call is refused loudly.
  fs.writeFileSync(path.join(appDir, 'manifest.yml'), llmManifest(false));
  const site2 = await createSite({ seed: '0123456789abcdef' });
  const emu2 = await createEmulator({ appDir, site: site2, runtime: 'wrapper' });
  try {
    const r = await emu2.invokeResolver('explainer', 'explain', { ids: [] }, { extension: {} }, site2.pack.viewer);
    assert.strictEqual(r.ok, false);
    assert.match(r.error.message, /LLM package is used but 'llm' module is not defined/);
    assert.ok(emu2.log.some((c) => c.service === 'llm' && c.status === 403 && c.refused));
  } finally {
    await emu2.close();
    await site2.stop();
  }
});

const RT_SRC = `import Resolver from '@forge/resolver';
import { publish, publishGlobal, signRealtimeToken } from '@forge/realtime';
const resolver = new Resolver();
resolver.define('token', async () => (await signRealtimeToken('scope', { board: 1 }, ['subscribe'])).token);
resolver.define('publishLocal', async () => publish('local', { from: 'resolver' }));
resolver.define('publishScoped', async () => publishGlobal('scope', { sprintId: 8 }, { token: (await signRealtimeToken('scope', { board: 1 }, ['publish'])).token }));
export const handler = resolver.getDefinitions();
// An async-event consumer: publishGlobal with no token (the documented path), and publish(), which the docs say
// async events cannot use.
export const consume = async () => ({ global: await publishGlobal('scope', { sprintId: 7 }), local: await publish('local', { sprintId: 7 }) });
`;
const RT_PAGE = `import { realtime, invoke } from '@forge/bridge';
const add = (id, p) => { const li = document.createElement('li'); li.textContent = typeof p === 'string' ? 'STRING:' + p : JSON.stringify(p); document.getElementById(id).appendChild(li); };
(async () => {
  await realtime.subscribeGlobal('scope', (p) => add('g', p));
  await realtime.subscribe('local', (p) => add('l', p));
  const token = await invoke('token');
  await realtime.subscribeGlobal('scope', (p) => add('t', p), { token });
  document.getElementById('ready').textContent = 'ready';
})();
document.getElementById('resolver').onclick = () => invoke('publishLocal');
document.getElementById('page').onclick = async () => { const r = await realtime.publish('local', { from: 'page' }); document.getElementById('result').textContent = JSON.stringify(Object.keys(r)); };
document.getElementById('plain').onclick = () => realtime.publish('local', 'Here is an event payload!');
document.getElementById('scoped').onclick = () => invoke('publishScoped');
`;

test('Realtime: consumer publishGlobal reaches the widget, consumer publish() does not, frontend publish does, tokens scope', { timeout: 240_000 }, async () => {
  ensureKit();
  const { createSite, createEmulator } = load();
  const { kitPaths } = require(path.join(FORGE, 'kit', 'lib', 'kitpaths.cjs'));
  const appDir = path.join(scratch('rt'), 'app');
  fs.mkdirSync(path.join(appDir, 'src'), { recursive: true });
  fs.writeFileSync(path.join(appDir, 'src', 'index.js'), RT_SRC);
  fs.writeFileSync(path.join(appDir, 'manifest.yml'), `modules:
  dashboards:widget:
    - key: live
      title: Live
      description: Live widget
      thumbnail: https://developer.atlassian.com/platform/forge/images/icons/issue-panel-icon.svg
      resource: view
      resolver:
        function: resolver
  consumer:
    - key: updates
      queue: updates
      function: consume
  function:
    - key: resolver
      handler: index.handler
    - key: consume
      handler: index.consume
resources:
  - key: view
    path: static/view
app:
  id: ${APP_ID}
  runtime:
    name: nodejs22.x
`);
  const paths = kitPaths(path.join(FORGE, 'kit'));
  const dir = path.join(appDir, 'static', 'view');
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(appDir, 'page.js'), RT_PAGE);
  await paths.require('esbuild').build({ entryPoints: [path.join(appDir, 'page.js')], bundle: true, format: 'iife', platform: 'browser', outfile: path.join(dir, 'main.js'), nodePaths: [paths.appModules], logLevel: 'silent' });
  fs.writeFileSync(path.join(dir, 'index.html'), '<!doctype html><html><body><span id="ready"></span><button id="resolver">r</button><button id="page">p</button><button id="plain">t</button><button id="scoped">s</button><span id="result"></span><ul id="g"></ul><ul id="l"></ul><ul id="t"></ul><script src="./main.js"></script></body></html>');

  const site = await createSite({ seed: '0123456789abcdef' });
  const emu = await createEmulator({ appDir, site, runtime: 'wrapper' });
  const browser = await playwright().chromium.launch({ headless: true, executablePath: process.env.BENCH_BROWSER_EXECUTABLE || undefined });
  const errors = [];
  try {
    const page = await browser.newPage();
    page.on('pageerror', (e) => errors.push(String(e)));
    await emu.openSurface(page, { moduleKey: 'live', entry: 'view', theme: 'light', layout: { width: 380, height: 300 }, widgetId: 'w1' });
    await page.locator('#ready').filter({ hasText: 'ready' }).waitFor({ timeout: 30_000 });
    const items = (id) => page.locator(`#${id} li`).allTextContents();
    const waitFor = (id, text) => page.waitForFunction(([i, t]) => [...document.querySelectorAll(`#${i} li`)].some((li) => li.textContent === t), [id, text], { timeout: 20_000 });

    const c = await emu.invoke('consume', { moduleKey: 'updates', event: { body: {}, queueName: 'updates', jobId: 'j1', eventId: 'j1#0' } });
    assert.ok(c.ok, JSON.stringify(c.error));
    assert.ok(c.result.global.eventId, 'publishGlobal from a consumer is delivered');
    assert.strictEqual(c.result.local.eventId, null);
    assert.strictEqual(c.result.local.errors[0].message, 'Error publishing event to channel', 'the documented publish error');
    await waitFor('g', '{"sprintId":7}');
    await page.click('#scoped');
    await waitFor('t', '{"sprintId":8}');
    assert.deepStrictEqual(await items('g'), ['{"sprintId":7}'], 'the token-scoped event skips the tokenless subscription');
    assert.deepStrictEqual(await items('t'), ['{"sprintId":8}']);
    assert.deepStrictEqual(await items('l'), [], "the consumer's publish() delivered nothing");

    await page.click('#resolver');
    await waitFor('l', '{"from":"resolver"}');
    await page.click('#page');
    await waitFor('l', '{"from":"page"}');
    await page.waitForFunction(() => document.getElementById('result').textContent === '["eventId","eventTimestamp","errors"]');
    // A string publish arrives as that string; an object publish as an object (site/realtime.cjs PAYLOAD).
    await page.click('#plain');
    await waitFor('l', 'STRING:Here is an event payload!');

    const log = await emu.realtime.log();
    const byOrigin = log.events.map((e) => [e.origin.source, e.origin.moduleType ?? null, e.isGlobal, e.rejected ?? null, e.delivered.length]);
    assert.deepStrictEqual(byOrigin, [
      ['function', 'consumer', true, null, 1],
      ['function', 'consumer', false, 'PUBLISH_WITHOUT_FRONTEND_CONTEXT', 0],
      ['function', 'dashboards:widget', true, null, 1],
      ['function', 'dashboards:widget', false, null, 1],
      ['frontend', null, false, null, 1],
      ['frontend', null, false, null, 1],
    ]);
    assert.strictEqual(log.events[1].rejected, 'PUBLISH_WITHOUT_FRONTEND_CONTEXT');
    // An empty channel name is refused, never silently dropped.
    const bad = await site.control.rtpublish({ channelName: '', payload: '{}', isGlobal: true });
    assert.match(bad.errors[0].message, /non-empty/);
    assert.strictEqual(emu.realtime.deliveries().length, 5);
    assert.ok(emu.bridgeLog.some((b) => b.op === 'realtimeEvent'));
    assert.deepStrictEqual(errors, []);
    assert.deepStrictEqual(emu.harnessMissing, []);
  } finally {
    await browser.close();
    await emu.close();
    await site.stop();
  }
});
