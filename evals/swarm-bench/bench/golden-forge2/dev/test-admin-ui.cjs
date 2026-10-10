// The UI Kit admin page, rendered with the real @forge/react reconciler in this process: every ForgeDoc it
// sends over callBridge('reconcile') is captured, invokes are routed to the golden's admin resolver on the
// bed, and controls are found by their visible labels and driven through their own props.
// Usage: FORGE_REACT_MODULES=<node_modules holding @forge/react 12.3.0> node dev/test-admin-ui.cjs [outDir]
'use strict';

const path = require('path');
const os = require('os');
const esbuild = require('esbuild');
const { createSite } = require('./site.cjs');
const { createPlatform, APP } = require('./runtime.cjs');
const { installClock } = require('./clock.cjs');

let failures = 0;
const ok = (cond, msg) => {
  if (!cond) failures += 1;
  console.log(`${cond ? 'PASS' : 'FAIL'}  ${msg}`);
};

const textOf = (node) => (node.type === 'String' ? node.props.text : (node.children ?? []).map(textOf).join(''));
const walk = (node, out = []) => {
  out.push(node);
  for (const c of node.children ?? []) walk(c, out);
  return out;
};

function host(doc) {
  const nodes = walk(doc);
  const byLabel = (label) => {
    const l = nodes.find((n) => n.type === 'Label' && textOf(n) === label);
    return l ? nodes.find((n) => n.props?.id === l.props.labelFor && n.type !== 'Label') : undefined;
  };
  const button = (label) => nodes.find((n) => n.type === 'Button' && textOf(n).trim() === label);
  return { nodes, byLabel, button, text: textOf(doc) };
}

async function main() {
  const outDir = process.argv[2] ?? path.join(os.tmpdir(), 'golden-forge2-admin-ui');
  const modules = process.env.FORGE_REACT_MODULES;
  if (!modules) throw new Error('FORGE_REACT_MODULES must name a node_modules holding @forge/react 12.3.0');
  const clock = installClock(Date.parse('2026-10-02T12:00:00.000Z'));
  const site = createSite({ seed: 5, clock });
  const platform = createPlatform({ site, clock });
  await platform.build(path.join(outDir, 'bundle'));
  const built = await esbuild.build({
    entryPoints: [path.join(APP, 'src', 'frontend', 'admin.jsx')],
    bundle: true,
    write: false,
    format: 'iife',
    platform: 'browser',
    jsx: 'automatic',
    define: { 'process.env.NODE_ENV': '"production"' },
    nodePaths: [modules],
    // one React for the app and the reconciler
    alias: { react: path.join(modules, 'react') },
    logLevel: 'error',
  });
  const code = built.outputFiles[0].text;
  console.log(`      admin bundle ${code.length} bytes`);

  async function render(aaid) {
    const events = [];
    const docs = [];
    globalThis.self = globalThis;
    globalThis.window = globalThis;
    globalThis.__bridge = {
      callBridge: async (cmd, data) => {
        events.push(cmd === 'invoke' ? `invoke:${data.functionKey}` : cmd);
        if (cmd === 'reconcile') return void docs.push(data.forgeDoc);
        if (cmd === 'invoke') return platform.resolver('jira:adminPage', 'scope-admin', data.functionKey, data.payload, { aaid });
        if (cmd === 'getContext') return { accountId: aaid, moduleKey: 'scope-admin', extension: { type: 'jira:adminPage' } };
        return undefined;
      },
    };
    new Function(code)();
    const idle = async () => {
      for (let i = 0; i < 50; i += 1) await new Promise((r) => setImmediate(r));
    };
    await idle();
    return { events, docs, idle, current: () => host(docs.at(-1)) };
  }

  // a non-admin sees the refusal and no controls
  const asBob = await render(site.users.bob.accountId);
  ok(/Only Jira administrators/.test(asBob.current().text) && !asBob.current().button('Save settings'), 'a non-admin gets the refusal, no controls');

  const page = await render(site.users.alice.accountId);
  const firstRender = page.events.indexOf('reconcile');
  const invokesBefore = page.events.slice(0, firstRender).filter((e) => e.startsWith('invoke:')).length;
  ok(firstRender >= 0 && invokesBefore <= 1, `${invokesBefore} invoke(s) before the first render`);
  ok(page.events.filter((e) => e.startsWith('invoke:')).length === 1, 'one invoke loads the whole page');
  let h = page.current();
  for (const label of ['Background share (%)', 'AI explanations enabled', 'Daily AI token budget', 'Comment group', 'Migration']) ok(Boolean(h.byLabel(label)), `the control labelled "${label}" is found by its label`);
  for (const label of ['Save settings', 'Rotate CI secret']) ok(Boolean(h.button(label)), `the button "${label}" is found by its text`);
  // @forge/react sends a DynamicTable as Cell/Row children (head first, then rows).
  const table = (hh) => hh.nodes.find((n) => n.type === 'DynamicTable');
  const headCells = (hh) => walk(table(hh).children[0]).filter((n) => n.type === 'Cell').map(textOf);
  ok(/Recent admin changes/.test(h.text) && headCells(h).join() === 'When,Who,What', `Recent admin changes is a When/Who/What table (${headCells(h).join()})`);
  ok(h.byLabel('Background share (%)').props.value === '70' && h.byLabel('Daily AI token budget').props.value === '200000' && h.byLabel('AI explanations enabled').props.isChecked === true, 'the defaults are shown (70, on, 200000)');
  ok(/Migration has not started yet|Migrated \d+ of \d+ v1 rows/.test(h.text), `the migration status is shown ("${h.byLabel('Migration').props.value}")`);

  h.byLabel('Background share (%)').props.onChange({ target: { value: '40' } });
  await page.idle();
  page.current().byLabel('AI explanations enabled').props.onChange({ target: { checked: false } });
  await page.idle();
  page.current().button('Save settings').props.onClick();
  await page.idle();
  h = page.current();
  const stored = platform.kvs.plain.get('settings');
  ok(stored?.backgroundShare === 40 && stored?.aiEnabled === false, 'Save settings stores the edited values');
  ok(/Settings saved/.test(h.text) && textOf(table(h)).includes('Background share (%): 70 → 40') && textOf(table(h)).includes('Alice Admin'), 'the change appears in Recent admin changes, with who made it');

  h.button('Rotate CI secret').props.onClick();
  await page.idle();
  h = page.current();
  const secret = platform.kvs.secrets.get('ci-secret');
  ok(secret && h.text.includes(`CI secret: ${secret}`), 'Rotate CI secret shows the new secret once');
  const again = await render(site.users.alice.accountId);
  ok(!again.current().text.includes(secret) && again.current().text.includes(`CI secret: ••••${secret.slice(-4)}`), 'a fresh load shows only CI secret: ••••<last4>');

  console.log(`\n${failures ? `${failures} FAILED` : 'ALL PASSED'}`);
  process.exit(failures ? 1 : 0);
}

main().catch((e) => {
  console.error('CRASH', e);
  process.exit(2);
});
