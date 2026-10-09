'use strict';
// The UI Kit host (SPEC §3 P2). Runs a `render: native` module's frontend with the REAL @forge/react reconciler,
// captures every ForgeDoc it sends (callBridge('reconcile', {forgeDoc})), routes `invoke` to the caller (the emulator),
// and drives the tree by its visible labels. The product's renderer (ForgeDoc -> Atlaskit) is Atlassian-internal and
// unpublished (research/uikit.md §2.6), so this host renders TEXT, never pixels.
//
//   const host = await render({ appDir, moduleKey, context, invoke, fetchProduct?, fence?, startTime?, kitDir? });
//   await host.waitIdle();                         // every runnable timer, bridge call and commit has settled
//   host.text(); host.tree(); host.outline();       // the latest ForgeDoc as text / JSON / one node per line
//   host.findByLabel('Background share (%)');      // -> {type, key, props, children, value|checked, via} | null
//   host.table('Recent admin changes');            // -> {head: [..], rows: [[..]]} in display order
//   await host.setValue('Background share (%)', 60); await host.click('Save settings'); await host.waitIdle();
//   host.invokes; host.log; host.docs; host.flags; host.errors; host.console; host.harnessMissing
//   await host.advance(ms); await host.flush(); await host.close();
//   await renderInEmulator(emu, { moduleKey, asUser })   // the same, invoke + requestJira through the emulator
//
//   invoke({moduleKey, functionKey, payload, context}) -> the resolver's result (throw = the app's invoke rejects)
//   fetchProduct({moduleKey, moduleType, product, restPath, fetchRequestInit, context}) -> {body, headers, status, statusText, isAttachment}
//
// The app's frontend code runs in a CHILD process under the kit's fence, never in the caller's process: scoring never
// runs app code unfenced (README: the default fence REFUSES when sandbox-exec cannot apply). fence 'sandbox' (the
// default) is the deny-default sandbox-exec profile with no network; 'dev-auto' falls back to Node's permission model
// where a nested sandbox is refused (the entrant's workspace), as forge-dev's emulator does.
// Reads (text, tree, findByLabel, table, the arrays) are exact as of the last command that resolved (render, waitIdle,
// flush, advance, setValue, click): the child sends its state with every answer.
// Bare imports resolve ONLY from the kit's modules (as the backend bundle does, runtime.cjs), and .jsx compiles with the
// classic React.createElement runtime as Forge's Babel does (research F10): a .jsx without `import React` fails at
// runtime here as it does in Jira; .tsx follows the app's tsconfig, as ts-loader does.
const fs = require('fs');
const os = require('os');
const path = require('path');
const http = require('http');
const readline = require('readline');
const { spawn } = require('child_process');
const { kitPaths } = require('../kitpaths.cjs');
const { sandboxAvailable } = require('../runtime.cjs');
const D = require('./doc.cjs');
const { UikitHostError } = require('./host.cjs');
// Codes: BAD_MANIFEST / NO_MODULE / NOT_NATIVE / NO_RESOURCE / BUILD_FAILED are the app's (what forge deploy would
// refuse or cannot find); NO_CONTROL / AMBIGUOUS / NOT_CLICKABLE / NOT_AN_INPUT / NOT_A_TABLE / BAD_VALUE / NOT_IDLE
// are what a drive call found on the screen; HARNESS is this host's own failure (never app evidence).

const CHILD_FILES = ['runner.cjs', 'host.cjs', 'doc.cjs'];

function locate(appDir, moduleKey, paths) {
  const YAML = paths.require('yaml');
  let manifest;
  try { manifest = YAML.parse(fs.readFileSync(path.join(appDir, 'manifest.yml'), 'utf8')); } catch (e) { throw new UikitHostError('BAD_MANIFEST', `manifest.yml: ${e.message}`); }
  for (const [type, entries] of Object.entries(manifest?.modules ?? {})) {
    if (type === 'function' || !Array.isArray(entries)) continue;
    const module = entries.find((e) => e?.key === moduleKey);
    if (!module) continue;
    if (module.render !== 'native') throw new UikitHostError('NOT_NATIVE', `module '${moduleKey}' (${type}) is not UI Kit: it has no \`render: native\``);
    const resource = (Array.isArray(manifest.resources) ? manifest.resources : []).find((r) => r?.key === module.resource);
    if (!resource || typeof resource.path !== 'string') throw new UikitHostError('NO_RESOURCE', `module '${moduleKey}' names resource '${module.resource}', which is not declared (with a path) under resources`);
    return { manifest, type, module, resource };
  }
  throw new UikitHostError('NO_MODULE', `no module with key '${moduleKey}' in ${path.join(appDir, 'manifest.yml')}`);
}

// Bare imports from app code resolve only from the kit's pristine modules (the backend bundle's rule, runtime.cjs).
function kitModulesPlugin(paths) {
  const kitRoot = fs.realpathSync(paths.appModules);
  return {
    name: 'uikit-kit-modules',
    setup(build) {
      build.onResolve({ filter: /^[^./]/ }, async (args) => {
        if (args.pluginData?.kit) return undefined;
        if (args.importer && fs.realpathSync(args.importer).startsWith(kitRoot)) return undefined;
        const r = await build.resolve(args.path, { kind: args.kind, resolveDir: path.dirname(paths.appModules), pluginData: { kit: true } });
        if (r.errors.length) return { errors: [{ text: `Could not resolve "${args.path}": it is not one of the installed packages` }] };
        return r;
      });
    },
  };
}

async function build(appDir, moduleKey, resource, paths) {
  if (resource.bundler) throw new UikitHostError('HARNESS', `resource '${resource.key}' uses \`bundler: ${resource.bundler}\` (manual packaging), which this host does not model`);
  const entry = path.resolve(appDir, resource.path);
  // @forge/manifest resources-validator.js:192-199 and errors.js:63/240 (research/uikit.md §1.5).
  if (fs.existsSync(entry) && fs.statSync(entry).isDirectory()) throw new UikitHostError('BUILD_FAILED', `Client Side UI Kit resource (${resource.path}) cannot be a directory`);
  if (/\.html?$/i.test(entry)) throw new UikitHostError('BUILD_FAILED', `UI Kit resource entry '${resource.path}' referenced by ${moduleKey} module must not point to an .html file`);
  const esbuild = paths.require('esbuild');
  try {
    const r = await esbuild.build({
      absWorkingDir: appDir, entryPoints: [entry], bundle: true, write: false, format: 'iife', platform: 'browser',
      define: { 'process.env.NODE_ENV': '"production"' }, logLevel: 'silent', plugins: [kitModulesPlugin(paths)],
    });
    return { code: r.outputFiles[0].text, entry };
  } catch (e) {
    const msg = (e.errors ?? []).map((x) => `${x.location ? `${x.location.file}:${x.location.line}: ` : ''}${x.text}`).join('\n') || String(e.message);
    throw new UikitHostError('BUILD_FAILED', msg);
  }
}

// The deny-default profile of runtime.cjs sandboxProfile() minus its one network grant: the child reads its own
// directory and the Node runtime and talks to the parent only over the pipes it was started with.
function childProfile(dir, node) {
  const q = (p) => JSON.stringify(fs.realpathSync(p));
  const nodeRoot = path.dirname(path.dirname(fs.realpathSync(node)));
  return [
    '(version 1)', '(deny default)',
    `(allow process-exec (literal ${q(node)}))`,
    `(allow file-read* (subpath "/usr/lib") (subpath "/System") (subpath ${q(nodeRoot)}) (subpath ${q(dir)}) (literal "/dev/urandom") (literal "/dev/null") (literal "/"))`,
    '(allow file-read-metadata)', '(allow sysctl-read)', '(allow mach-lookup)', '(allow ipc-posix-shm)',
    '(allow signal (target self))',
  ].join('\n');
}
function resolveFence(fence) {
  if (fence === 'sandbox' || fence === 'node-permission') {
    if (fence === 'sandbox' && !sandboxAvailable()) throw new UikitHostError('HARNESS', 'REFUSED: sandbox-exec cannot apply the fence on this host; scoring never runs app code unfenced');
    return fence;
  }
  if (fence === 'dev-auto') return sandboxAvailable() ? 'sandbox' : 'node-permission';
  throw new UikitHostError('HARNESS', `REFUSED: unknown fence ${fence}`);
}

// workDir: where the child's directory (bundle + host files, the only files it may read) is made; os.tmpdir() unless
// given (forge-dev keeps its files under the workspace's .forge-dev/, as its emulator does).
async function render({ appDir, moduleKey, context, invoke, fetchProduct = null, fence = 'sandbox', startTime = Date.now(), kitDir, workDir = os.tmpdir(), node = process.execPath } = {}) {
  if (typeof invoke !== 'function') throw new UikitHostError('HARNESS', 'render() needs invoke({moduleKey, functionKey, payload, context}) -> the resolver result');
  if (!context || typeof context !== 'object') throw new UikitHostError('HARNESS', 'render() needs the frontend context (what view.getContext() answers)');
  const mode = resolveFence(fence);
  let paths;
  try { paths = kitPaths(kitDir); paths.resolve('@forge/react'); } catch (e) { throw new UikitHostError('HARNESS', `kit modules: ${e.message}`); }
  const { type, resource } = locate(appDir, moduleKey, paths);
  const { code, entry } = await build(appDir, moduleKey, resource, paths);

  fs.mkdirSync(workDir, { recursive: true });
  const dir = fs.realpathSync(fs.mkdtempSync(path.join(workDir, 'uikit-host-')));
  fs.writeFileSync(path.join(dir, 'ui.js'), code);
  for (const f of CHILD_FILES) fs.copyFileSync(path.join(__dirname, f), path.join(dir, f));
  const runner = path.join(dir, 'runner.cjs');
  const [cmd, args] = mode === 'sandbox'
    ? ['/usr/bin/sandbox-exec', ['-p', childProfile(dir, node), node, runner]]
    : [node, ['--permission', `--allow-fs-read=${dir}`, runner]];
  const child = spawn(cmd, args, { cwd: dir, env: { PATH: '/usr/bin:/bin', TZ: 'UTC', LANG: 'C' }, stdio: ['pipe', 'ignore', 'pipe', 'pipe'] });
  let stderr = '';
  child.stderr.on('data', (d) => { stderr = (stderr + d).slice(-4000); });
  // a write after the child ended (EPIPE) is the same end the 'exit' handler reports
  child.stdin.on('error', (e) => { stderr += ` [stdin ${e.code ?? e.message}]`; });

  const mirror = { docs: [], log: [], invokes: [], flags: [], errors: [], console: [], harnessMissing: [], typed: new Map(), now: 0 };
  const apply = (s) => {
    mirror.docs.splice(s.docsFrom, s.docs.length, ...s.docs);
    Object.assign(mirror, { log: s.log, invokes: s.invokes, flags: s.flags, errors: s.errors, console: s.console, harnessMissing: s.harnessMissing, typed: new Map(s.typed), now: s.now });
  };
  const waiting = new Map();
  let nextId = 0;
  let ended = null;
  const send = (m) => child.stdin.write(`${JSON.stringify(m)}\n`);
  let ready;
  const started = new Promise((resolve, reject) => { ready = { resolve, reject }; });
  const answerCall = async (m) => {
    try {
      let value;
      if (m.kind === 'invoke') value = await invoke({ moduleKey, functionKey: m.args.functionKey, payload: m.args.payload, context: JSON.parse(JSON.stringify(context)) });
      else if (m.kind === 'fetchProduct') {
        if (!fetchProduct) throw Object.assign(new Error('requestJira/requestConfluence (no fetchProduct route was given to render())'), { errorType: 'NOT_MODELLED' });
        value = await fetchProduct({ moduleKey, moduleType: type, ...m.args, context: JSON.parse(JSON.stringify(context)) });
      } else throw new Error(`unknown call '${m.kind}'`);
      send({ t: 'reply', id: m.id, ok: true, value: value === undefined ? null : value });
    } catch (e) {
      send({ t: 'reply', id: m.id, ok: false, error: { message: String(e?.message ?? e), errorType: e?.errorType ?? null } });
    }
  };
  readline.createInterface({ input: child.stdio[3] }).on('line', (line) => {
    const m = JSON.parse(line);
    if (m.t === 'call') { answerCall(m); return; }
    apply(m.state);
    if (m.t === 'ready') { ready.resolve(); return; }
    const w = waiting.get(m.id);
    waiting.delete(m.id);
    if (m.ok) w.resolve(m.value); else w.reject(new UikitHostError(m.error.code, m.error.message));
  });
  child.on('exit', (codeNum, signal) => {
    ended = `the UI Kit host process ended (${signal ?? `exit ${codeNum}`})${stderr ? `: ${stderr.trim().split('\n').slice(-6).join(' | ')}` : ''}`;
    const err = new UikitHostError('HARNESS', ended);
    ready.reject(err);
    for (const w of waiting.values()) w.reject(err);
    waiting.clear();
    fs.rmSync(dir, { recursive: true, force: true });
  });
  const command = (name, ...a) => {
    if (ended) return Promise.reject(new UikitHostError('HARNESS', ended));
    const id = ++nextId;
    return new Promise((resolve, reject) => { waiting.set(id, { resolve, reject }); send({ t: 'cmd', id, name, args: a }); });
  };
  send({ t: 'start', filename: entry, context, moduleKey, startTime });
  await started;

  const latest = () => mirror.docs[mirror.docs.length - 1] ?? null;
  const match = (label) => {
    const doc = latest();
    if (!doc) return null;
    const r = D.findLabel(doc, label);
    if (r.ambiguous) throw new UikitHostError('AMBIGUOUS', `'${label}' names ${r.ambiguous.length} elements: ${r.ambiguous.join(', ')}`);
    return r.match;
  };
  return {
    moduleKey,
    moduleType: type,
    fence: mode,
    get docs() { return mirror.docs; },
    get log() { return mirror.log; },
    get invokes() { return mirror.invokes; },
    get flags() { return mirror.flags; },
    get errors() { return mirror.errors; },
    get console() { return mirror.console; },
    get harnessMissing() { return mirror.harnessMissing; },
    tree: latest,
    text: (n = latest()) => D.textOf(n, mirror.typed),
    outline: (n = latest()) => D.outline(n),
    findByLabel(label) {
      const m = match(label);
      if (!m) return null;
      const n = m.node;
      const out = { ...n, via: m.via };
      if (D.CHECKS.has(n.type)) out.checked = D.isChecked(n, mirror.typed);
      else if (D.TEXT_INPUTS.has(n.type)) { const v = D.inputValue(n, mirror.typed); out.value = v === undefined || v === null ? '' : String(v); } // a DOM input's value is a string
      else if (D.INPUTS.has(n.type)) out.value = D.inputValue(n, mirror.typed);
      return out;
    },
    // the DynamicTable a label names -> { head: [cell text], rows: [[cell text]] } in display order
    table(label) {
      const m = match(label);
      if (!m) throw new UikitHostError('NO_CONTROL', `no table labelled '${label}'`);
      if (m.node.type !== 'DynamicTable') throw new UikitHostError('NOT_A_TABLE', `'${label}' is a ${m.node.type}, not a DynamicTable`);
      return D.tableOf(m.node, mirror.typed);
    },
    setValue: (label, value) => command('setValue', label, value),
    click: (label) => command('click', label),
    waitIdle: () => command('waitIdle'),
    flush: () => command('flush'),
    advance: (ms) => command('advance', ms),
    now: () => mirror.now,
    // ends the child (its pid only, gate 4) and resolves when it has gone
    close: () => new Promise((resolve) => {
      if (ended) { resolve(); return; }
      child.once('exit', () => resolve());
      child.stdin.end();
    }),
  };
}

// invoke and requestJira through the emulator: the resolver runs in the Forge runtime as the viewer (context.accountId),
// requestJira goes to the site as that user, exactly as a Custom UI surface's bridge does (bridge-host.cjs answer()).
function viaEmulator(emu) {
  return {
    invoke: async ({ moduleKey, functionKey, payload, context }) => {
      const r = await emu.invokeResolver(moduleKey, functionKey, payload, context, context?.accountId);
      if (!r.ok) {
        const e = new Error(r.error?.message ?? 'invoke failed');
        e.errorType = r.error?.errorType ?? null;
        throw e;
      }
      // a resolver that returns nothing reaches the frontend as {} (as bridge-host.cjs answers a Custom UI invoke)
      return r.result === undefined || r.result === null ? {} : r.result;
    },
    fetchProduct: async ({ moduleKey, moduleType, product, restPath, fetchRequestInit, context }) => {
      const headers = Object.fromEntries((fetchRequestInit.headers ?? []).map(([k, v]) => [String(k).toLowerCase(), v]));
      const r = await emu.proxy.productFetch({ inv: { id: `uikit:${moduleKey}`, moduleType, moduleKey, source: 'frontend', aaid: context?.accountId },
        provider: 'user', product, method: (fetchRequestInit.method ?? 'GET').toUpperCase(), path: restPath, headers, body: fetchRequestInit.body ?? undefined });
      return { body: r.body, headers: r.headers, status: r.status, statusText: http.STATUS_CODES[r.status] ?? '', isAttachment: false };
    },
  };
}

// The context a surface of this module gets from the emulator (the same shape bridge-host.cjs gives Custom UI).
function emulatorContext(emu, moduleKey, accountId, extension = {}) {
  const found = emu.moduleByKey(moduleKey);
  if (!found) throw new UikitHostError('NO_MODULE', `no module with key '${moduleKey}' in the manifest`);
  return require('../bridge-host.cjs').contextFor(emu, { type: found.type, moduleKey, asUser: accountId, theme: 'light', extension });
}

// The fence follows the emulator's (scoring: 'sandbox'; forge-dev in the entrant's workspace: its dev fallback).
async function renderInEmulator(emu, { moduleKey, asUser, context, extension, workDir } = {}) {
  const via = viaEmulator(emu);
  return render({ appDir: emu.appDir, kitDir: emu.paths.kitDir, moduleKey, context: context ?? emulatorContext(emu, moduleKey, asUser, extension),
    invoke: via.invoke, fetchProduct: via.fetchProduct, fence: emu.fence, startTime: emu.clock.now(), ...(workDir ? { workDir } : {}) });
}

module.exports = { render, renderInEmulator, viaEmulator, emulatorContext, UikitHostError, textOf: D.textOf, outline: D.outline };
