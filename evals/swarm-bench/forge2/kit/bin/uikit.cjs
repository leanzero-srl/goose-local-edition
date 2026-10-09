'use strict';
// forge-dev uikit: render a UI Kit (`render: native`) module offline with the real @forge/react reconciler and print
// what a person sees, the ForgeDoc tree, every invoke and every error — the same host the scorer drives
// (lib/uikit-host). forge-dev.cjs owns the emulator and wires the subcommand:
//
//   if (cmd === 'uikit') { const code = await require(path.join(kitDir, 'bin', 'uikit.cjs')).main({ emu, argv: argv.slice(1) }); ... }
const path = require('path');

const USAGE = `  uikit <moduleKey> [--as <accountId>] [--set "<label>=<value>"]... [--click "<label>"]... [--advance <ms>] [--json]
        Render a UI Kit (render: native) module offline with the real @forge/react reconciler and print the screen as
        text, the ForgeDoc tree, every invoke (your resolvers run in the Forge runtime as the viewer, --as defaults to
        the dev viewer) and every error. --set and --click drive it by VISIBLE LABEL, in the order given: a Label's text
        (its labelFor names the input), a label/title/caption prop, or a button's text. --set "<label>=true|false" sets
        a Toggle or Checkbox. Timers are virtual: they fire only when --advance <ms> moves the clock. The scorer drives
        the same host the same way.`;

function parse(argv) {
  const out = { moduleKey: null, as: null, json: false, actions: [] };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const value = () => { if (i + 1 >= argv.length) throw new Error(`${a} needs a value`); return argv[++i]; };
    if (a === '--json') out.json = true;
    else if (a === '--as') out.as = value();
    else if (a === '--click') out.actions.push({ op: 'click', label: value() });
    else if (a === '--advance') {
      const ms = Number(value());
      if (!Number.isFinite(ms) || ms < 0) throw new Error(`--advance ${argv[i]}: milliseconds, a number >= 0`);
      out.actions.push({ op: 'advance', ms });
    } else if (a === '--set') {
      const s = value();
      const eq = s.indexOf('=');
      if (eq < 1) throw new Error(`--set "${s}": expected "<label>=<value>"`);
      out.actions.push({ op: 'set', label: s.slice(0, eq), value: s.slice(eq + 1) });
    } else if (a.startsWith('--')) throw new Error(`unknown option ${a}`);
    else if (!out.moduleKey) out.moduleKey = a;
    else throw new Error(`unexpected argument ${a}`);
  }
  if (!out.moduleKey) throw new Error('uikit <moduleKey> ...');
  return out;
}

const SHORT = 300; // ratio: one invoke per terminal line or two; --json carries every value whole
const short = (v) => { const s = v === undefined ? '' : JSON.stringify(v); return s.length > SHORT ? `${s.slice(0, SHORT)}… (${s.length} chars)` : s; };

async function act(host, a) {
  if (a.op === 'click') return host.click(a.label);
  if (a.op === 'advance') return host.advance(a.ms);
  const target = host.findByLabel(a.label);
  if (target && (target.type === 'Toggle' || target.type === 'Checkbox')) {
    if (a.value !== 'true' && a.value !== 'false') throw new Error(`--set "${a.label}=${a.value}": '${a.label}' is a ${target.type}, set it to true or false`);
    return host.setValue(a.label, a.value === 'true');
  }
  return host.setValue(a.label, a.value);
}

// -> exit code: 0 when the module rendered and nothing failed, 1 otherwise.
async function main({ emu, argv, print = console.log }) {
  const { renderInEmulator } = require(path.join(__dirname, '..', 'lib', 'uikit-host', 'index.cjs'));
  let opts;
  try { opts = parse(argv); } catch (e) { print(`${e.message}\n\n${USAGE}`); return 2; }
  const asUser = opts.as ?? emu.siteInfo.viewer;
  let host;
  try {
    host = await renderInEmulator(emu, { moduleKey: opts.moduleKey, asUser, workDir: path.join(emu.appDir, '.forge-dev', 'uikit') });
  } catch (e) {
    if (!e.code) throw e;
    print(`uikit ${opts.moduleKey}: ${e.code}: ${e.message}`);
    return 1;
  }
  const steps = [];
  let failed = null;
  try {
    await host.waitIdle();
    for (const a of opts.actions) {
      try {
        const result = await act(host, a);
        await host.waitIdle();
        steps.push({ ...a, result });
      } catch (e) {
        failed = { ...a, error: `${e.code ? `${e.code}: ` : ''}${e.message}` };
        steps.push(failed);
        break;
      }
    }
    const report = { moduleKey: opts.moduleKey, moduleType: host.moduleType, asUser, commits: host.docs.length, virtualMs: host.now(), steps,
      text: host.text(), tree: host.tree(), invokes: host.invokes, flags: host.flags, errors: host.errors, console: host.console, harnessMissing: host.harnessMissing };
    if (opts.json) print(JSON.stringify(report, null, 2));
    else {
      print(`uikit ${opts.moduleKey} (${host.moduleType}) as ${asUser}: ${report.commits} commit(s), ${host.invokes.length} invoke(s), virtual time ${report.virtualMs} ms`);
      for (const s of steps) {
        const what = s.op === 'set' ? `set ${JSON.stringify(s.label)} = ${JSON.stringify(s.value)}` : s.op === 'click' ? `click ${JSON.stringify(s.label)}` : `advance ${s.ms} ms`;
        print(`> ${what} -> ${s.error ? `FAILED ${s.error}` : short(s.result)}`);
      }
      print('== screen');
      print(report.text || '(nothing rendered)');
      print('== tree');
      print(host.outline() || '(no ForgeDoc)');
      print('== invokes');
      if (!host.invokes.length) print('(none)');
      for (const i of host.invokes) {
        const outcome = i.state === 'ok' ? `ok ${short(i.result)}` : i.state === 'error' ? `ERROR ${i.error.message}` : 'pending';
        print(`  ${i.functionKey}(${short(i.payload)}) after ${i.reconcilesBefore} commit(s) -> ${outcome}`);
      }
      if (host.flags.length) { print('== flags'); for (const f of host.flags) print(`  ${f.type ?? 'info'} ${short(f.title)} ${f.description ? short(f.description) : ''}${f.closed ? ' (closed)' : ''}`); }
      // the app's own frames only (the bundle's file:line); the host's frames say nothing about the app
      const appFrames = (e) => (e.stack ?? '').split('\n').filter((l) => /^\s+at /.test(l) && l.includes(emu.appDir)).slice(0, 3);
      if (host.errors.length) { print('== errors'); for (const e of host.errors) print([`  ${e.kind}: ${e.name && e.name !== 'Error' ? `${e.name}: ` : ''}${e.message}`, ...appFrames(e)].join('\n')); }
      const loud = host.console.filter((c) => c.level === 'error' || c.level === 'warn');
      if (loud.length) { print('== console'); for (const c of loud) print(`  ${c.level}: ${c.text}`); }
      if (host.harnessMissing.length) { print('== not modelled by this host (a harness gap, not your app\'s fault)'); for (const m of host.harnessMissing) print(`  ${m.what}`); }
    }
    return failed || host.errors.length || host.harnessMissing.length ? 1 : 0;
  } finally {
    await host.close();
  }
}

module.exports = { main, parse, USAGE };
