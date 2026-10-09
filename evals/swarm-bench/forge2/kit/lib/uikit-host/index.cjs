'use strict';
// The UI Kit host (SPEC §3 P2). Runs a `render: native` module's frontend in Node with the REAL @forge/react
// reconciler, captures every ForgeDoc it sends (callBridge('reconcile', {forgeDoc})), routes `invoke` to the caller
// (the emulator), and drives the tree by its visible labels. The product's renderer (ForgeDoc -> Atlaskit) is
// Atlassian-internal and unpublished (research/uikit.md §2.6), so this host renders TEXT, never pixels.
//
//   const host = await render({ appDir, moduleKey, context, invoke, fetchProduct?, kitDir? });
//   await host.waitIdle();                         // every runnable timer, bridge call and commit has settled
//   host.text(); host.tree(); host.outline();       // the latest ForgeDoc as text / JSON / one node per line
//   host.findByLabel('Background share (%)');      // -> {type, key, props, children, value|checked, via} | null
//   await host.setValue('Background share (%)', 60); await host.click('Save settings'); await host.waitIdle();
//   host.invokes; host.log; host.docs; host.flags; host.errors; host.console; host.harnessMissing
//   await host.advance(ms); await host.flush(); host.close();
//   await renderInEmulator(emu, { moduleKey, asUser })   // the same, invoke + requestJira through the emulator
//
// Fidelity choices (each from research/uikit.md):
//  - The bundle runs in its own vm realm with `self === globalThis` (the reconciler reads self.__bridge,
//    @forge/bridge reads globalThis.__bridge at module load: verification #5/#6) and no DOM (UI Kit has none).
//  - .jsx compiles with the classic React.createElement runtime, as Forge's Babel does (F10): a .jsx file without
//    `import React` throws at runtime here as it does in Jira. .tsx follows the app's tsconfig, as ts-loader does.
//  - Bare imports resolve ONLY from the kit's modules (as the backend bundle does: runtime.cjs).
//  - Timers are VIRTUAL: setTimeout/setInterval run on a clock that moves only when the host is told (`advance`), so
//    every reading is deterministic and nothing waits on a wall clock. A bridge call takes zero virtual time. `Date`
//    stays real. performance.now() is that clock plus 1 ms per commit so far: React's scheduler yields after 5 ms of
//    work (scheduler 0.23.2 frameYieldMs), so on a frozen clock a commit loop would spin inside one task forever;
//    with commits costing time it yields between them as in a browser frame and is counted in turns (NOT_IDLE).
//  - Handlers are delivered on a later macrotask, never inside a batch, with arguments serialised the way the
//    bridge does (functions cannot cross); the handler of the LATEST commit receives them. Inputs get the documented
//    SerialisableEvent (target.type, value as a string for text inputs, as the DOM gives it); a submit button calls
//    its Form's onSubmit with no argument (FormProps.onSubmit: () => ...). A disabled or loading button is not
//    clickable (@atlaskit/button 25.4.7 button-base: isInteractive = !isDisabled && !isLoading/overlay).
//  - Any bridge op this host does not model rejects in the app and is recorded in `harnessMissing`, never answered
//    with a quiet default.
const fs = require('fs');
const path = require('path');
const http = require('http');
const util = require('util');
const vm = require('vm');
const { kitPaths } = require('../kitpaths.cjs');
const D = require('./doc.cjs');

class UikitHostError extends Error {
  constructor(code, message) { super(message); this.name = 'UikitHostError'; this.code = code; }
}
// Codes: NO_MODULE / NOT_NATIVE / NO_RESOURCE / BUILD_FAILED are the app's (what forge deploy would refuse or
// cannot find); NO_CONTROL / AMBIGUOUS / NOT_CLICKABLE / NOT_AN_INPUT / BAD_VALUE / NOT_IDLE are what a drive call
// found on the screen; HARNESS is this host's own failure (never app evidence).

// Bridge ops with no observable effect in a text host (fire-and-forget in @forge/bridge 7.1.0).
const INERT = new Set(['emitReadyEvent', 'changeWindowTitle', 'emitFrontendCustomMetric']);
// measured: the fixture admin page (test/fixture-admin) settles in 2 turns at boot and 1 after set + toggle + save;
// a commit loop (an effect that always sets state, a 0 ms interval) never settles. A turn is one timer run or one
// bridge answer awaited, so this bounds work by count, never by time.
const MAX_TURNS = 1000;

const macrotask = () => new Promise((r) => setImmediate(r));
const plain = (v) => (v === undefined ? undefined : JSON.parse(JSON.stringify(v)));
// @forge/bridge's requestJira builds `new Request('', init)` only to normalise headers and body (fetch/fetch.js);
// in the product iframe '' resolves against the page's URL, which this host does not have, so it names a reserved one.
const PAGE_URL = 'https://uikit-host.invalid/';
class PageRequest extends Request {
  constructor(input, init) { super(typeof input === 'string' ? new URL(input, PAGE_URL) : input, init); }
}

function locate(appDir, moduleKey, paths) {
  const YAML = paths.require('yaml');
  const manifest = YAML.parse(fs.readFileSync(path.join(appDir, 'manifest.yml'), 'utf8'));
  for (const [type, entries] of Object.entries(manifest?.modules ?? {})) {
    if (type === 'function' || !Array.isArray(entries)) continue;
    const module = entries.find((e) => e?.key === moduleKey);
    if (!module) continue;
    if (module.render !== 'native') throw new UikitHostError('NOT_NATIVE', `module '${moduleKey}' (${type}) is not UI Kit: it has no \`render: native\``);
    const resource = (manifest.resources ?? []).find((r) => r.key === module.resource);
    if (!resource) throw new UikitHostError('NO_RESOURCE', `module '${moduleKey}' names resource '${module.resource}', which is not declared under resources`);
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

// Unhandled rejections inside an app realm are the app's console errors, as in a browser; they never end the
// process that hosts it. Any other rejection keeps Node's default: with no other listener it is raised as an
// uncaught exception, with one it is that listener's.
const liveHosts = new Set();
function onUnhandledRejection(reason, promise) {
  for (const h of liveHosts) if (promise instanceof h.AppPromise) { h.recordError('unhandledRejection', reason); return; }
  if (process.listenerCount('unhandledRejection') === 1) process.nextTick(() => { throw reason; });
}
const hostOpened = (h) => { if (!liveHosts.size) process.on('unhandledRejection', onUnhandledRejection); liveHosts.add(h); };
const hostClosed = (h) => { liveHosts.delete(h); if (!liveHosts.size) process.off('unhandledRejection', onUnhandledRejection); };

async function render({ appDir, moduleKey, context, invoke, fetchProduct = null, kitDir } = {}) {
  if (typeof invoke !== 'function') throw new UikitHostError('HARNESS', 'render() needs invoke({moduleKey, functionKey, payload, context}) -> the resolver result');
  if (!context || typeof context !== 'object') throw new UikitHostError('HARNESS', 'render() needs the frontend context (what view.getContext() answers)');
  let paths;
  try { paths = kitPaths(kitDir); paths.resolve('@forge/react'); } catch (e) { throw new UikitHostError('HARNESS', `kit modules: ${e.message}`); }
  const { type, resource } = locate(appDir, moduleKey, paths);
  const { code, entry } = await build(appDir, moduleKey, resource, paths);

  const clock = { now: 0 };
  let workMs = 0; // 1 per commit, never reset: performance.now() stays monotonic
  const timers = new Map();
  let timerSeq = 0;
  let seq = 0;
  let closed = false;
  const pending = new Set();
  const docs = [];
  const log = [];
  const invokes = [];
  const flags = [];
  const errors = [];
  const consoleLines = [];
  const harnessMissing = [];
  const typed = new Map(); // what the person typed or toggled, by element key (an uncontrolled input's value)
  let latest = { doc: null, handlers: new Map() };

  const errorInfo = (e) => ({ message: String(e?.message ?? e), name: e?.name ?? null, stack: typeof e?.stack === 'string' ? e.stack : null });
  const recordError = (kind, e, extra = {}) => errors.push({ kind, ...errorInfo(e), at: clock.now, seq: ++seq, ...extra });
  const say = (level) => (...args) => consoleLines.push({ level, text: util.format(...args), at: clock.now });

  // Virtual timers: due when `at <= clock.now`; run in (at, id) order.
  const addTimer = (fn, ms, args, repeat) => {
    if (typeof fn !== 'function') throw new TypeError('uikit host: string timer callbacks are not supported');
    const id = ++timerSeq;
    const delay = Math.max(0, Number(ms) || 0);
    timers.set(id, { id, fn, args, at: clock.now + delay, every: repeat ? delay : null });
    return id;
  };
  const nextTimer = (limit) => {
    let t = null;
    for (const x of timers.values()) if (x.at <= limit && (!t || x.at < t.at || (x.at === t.at && x.id < t.id))) t = x;
    return t;
  };
  const runTimer = (t) => {
    if (t.every === null) timers.delete(t.id); else t.at += t.every;
    try { t.fn(...t.args); } catch (e) { recordError('timer', e); }
  };

  const sandbox = {
    console: { log: say('log'), info: say('info'), warn: say('warn'), error: say('error'), debug: say('debug') },
    setTimeout: (fn, ms, ...args) => addTimer(fn, ms, args, false),
    setInterval: (fn, ms, ...args) => addTimer(fn, ms, args, true),
    clearTimeout: (id) => { timers.delete(id); },
    clearInterval: (id) => { timers.delete(id); },
    queueMicrotask,
    performance: { now: () => clock.now + workMs },
    crypto: globalThis.crypto,
    TextEncoder, TextDecoder, URL, URLSearchParams, AbortController, AbortSignal, atob, btoa,
    // @forge/bridge's requestJira builds a Request and answers a Response (fetch/fetch.js)
    Request: PageRequest, Response, Headers, FormData, Blob,
    fetch: (url) => AppPromise.reject(notModelled(`fetch(${JSON.stringify(String(url))}) from the frontend`)),
  };
  const ctx = vm.createContext(sandbox, { name: `uikit:${moduleKey}` });
  vm.runInContext('globalThis.self = globalThis; globalThis.window = globalThis;', ctx);
  const AppPromise = vm.runInContext('Promise', ctx);
  const AppError = vm.runInContext('Error', ctx);
  const appJSON = vm.runInContext('JSON', ctx);
  const toApp = (v) => (v === undefined ? undefined : appJSON.parse(JSON.stringify(v)));
  const appReject = (message) => new AppError(message);

  const notModelled = (what) => {
    harnessMissing.push({ what, at: clock.now, moduleKey });
    return appReject(`uikit host: ${what} is not modelled`);
  };

  async function answer(op, payload, entry) {
    switch (op) {
      case 'getContext': return toApp(context);
      case 'invoke': {
        if (payload?.metadata !== undefined) throw notModelled('invoke metadata (rateLimitProperties)');
        const rec = { seq: entry.seq, functionKey: payload?.functionKey, payload: plain(payload?.payload), reconcilesBefore: entry.reconciles, at: clock.now, state: 'pending' };
        invokes.push(rec);
        try {
          rec.result = plain(await invoke({ moduleKey, functionKey: rec.functionKey, payload: plain(payload?.payload), context: plain(context) }));
          rec.state = 'ok';
          return toApp(rec.result);
        } catch (e) {
          rec.state = 'error';
          rec.error = { message: String(e?.message ?? e), errorType: e?.errorType ?? null };
          // the message a Custom UI invoke rejects with in this kit (bridge-host.cjs)
          throw appReject(`There was an error invoking the function - ${rec.error.message}`);
        }
      }
      case 'fetchProduct': {
        if (!fetchProduct) throw notModelled('requestJira/requestConfluence (no fetchProduct route was given to render())');
        const r = await fetchProduct({ moduleKey, moduleType: type, product: payload?.product, restPath: payload?.restPath, fetchRequestInit: plain(payload?.fetchRequestInit ?? {}), context: plain(context) });
        entry.status = r.status;
        return toApp(r);
      }
      case 'showFlag': flags.push({ ...plain(payload), shownAt: clock.now, closed: false }); return null;
      case 'closeFlag': for (const f of flags) if (f.id === payload?.id) f.closed = true; return null;
      case 'onError': recordError('onError', payload?.error); return null;
      default:
        if (INERT.has(op)) return null;
        throw notModelled(`bridge op '${op}'`);
    }
  }

  function callBridge(op, payload) {
    const entry = { seq: ++seq, op, at: clock.now, reconciles: docs.length };
    if (closed) return undefined;
    if (op === 'reconcile') {
      workMs += 1;
      const doc = D.snapshot(payload.forgeDoc);
      docs.push(doc);
      latest = { doc, handlers: D.handlersOf(payload.forgeDoc) };
      log.push({ ...entry, doc: docs.length - 1 });
      return undefined;
    }
    log.push(entry);
    const p = (async () => answer(op, payload, entry))();
    const settle = p.then(() => { entry.ok = true; }, (e) => { entry.ok = false; entry.error = String(e?.message ?? e); });
    pending.add(settle);
    settle.then(() => pending.delete(settle));
    return AppPromise.resolve(p);
  }
  sandbox.__bridge = { callBridge };

  const host = { AppPromise, recordError };
  hostOpened(host);
  try {
    vm.runInContext(code, ctx, { filename: entry });
  } catch (e) {
    recordError('eval', e);
  }

  // ---- settling -------------------------------------------------------------------------------------------------
  const notIdle = (turns) => new UikitHostError('NOT_IDLE', `the app did not settle after ${turns} turns (${docs.length} commits, ${timers.size} timers, ${pending.size} bridge calls pending): a commit or timer loop`);
  // Everything runnable NOW: microtasks and the timers due at the current virtual time. Bridge calls stay in flight.
  async function flush(turns = { n: 0 }) {
    for (;;) {
      await macrotask();
      const t = nextTimer(clock.now);
      if (!t) return turns.n;
      if (++turns.n > MAX_TURNS) throw notIdle(turns.n);
      runTimer(t);
    }
  }
  // flush + every bridge call answered, until nothing is runnable and nothing is in flight.
  async function waitIdle() {
    const turns = { n: 0 };
    for (;;) {
      await flush(turns);
      if (!pending.size) break;
      await Promise.race([...pending]);
      if (++turns.n > MAX_TURNS) throw notIdle(turns.n);
    }
    return { commits: docs.length, invokes: invokes.length, timersPending: timers.size, turns: turns.n, now: clock.now };
  }
  // Move virtual time forward by `ms`, running each timer at its own time and settling after each.
  async function advance(ms) {
    const target = clock.now + Math.max(0, Number(ms) || 0);
    const turns = { n: 0 };
    for (;;) {
      const t = nextTimer(target);
      if (!t) break;
      clock.now = Math.max(clock.now, t.at);
      if (++turns.n > MAX_TURNS) throw notIdle(turns.n);
      runTimer(t);
      await waitIdle();
    }
    clock.now = target;
    return waitIdle();
  }

  // ---- driving ---------------------------------------------------------------------------------------------------
  function resolve(label) {
    if (!latest.doc) throw new UikitHostError('NO_CONTROL', `no control labelled '${label}': the app has not rendered anything`);
    const r = D.findLabel(latest.doc, label);
    if (r.ambiguous) throw new UikitHostError('AMBIGUOUS', `'${label}' names ${r.ambiguous.length} elements: ${r.ambiguous.join(', ')}`);
    if (!r.match) throw new UikitHostError('NO_CONTROL', `no control labelled '${label}'; labels on screen: ${r.available.map((l) => JSON.stringify(l)).join(', ') || 'none'}`);
    return r.match;
  }
  const handlers = (node) => latest.handlers.get(node.key) ?? {};
  const event = (n, type, target) => ({ bubbles: true, cancelable: type === 'click', defaultPrevented: false, eventPhase: 2, isTrusted: true, timeStamp: clock.now, type,
    target: { name: n.props.name, id: n.props.id, ...target } });
  // Delivered on a later macrotask, outside any batch; returns once its microtask fallout has run.
  async function deliver(node, prop, arg) {
    await macrotask();
    const fn = handlers(node)[prop];
    if (fn) {
      try {
        const r = arg === undefined ? fn() : fn(toApp(arg));
        if (r && typeof r.then === 'function') r.then(undefined, (e) => recordError('handler', e, { element: node.type, handler: prop }));
      } catch (e) { recordError('handler', e, { element: node.type, handler: prop }); }
    }
    await macrotask();
    return Boolean(fn);
  }

  function findByLabel(label) {
    if (!latest.doc) return null;
    const r = D.findLabel(latest.doc, label);
    if (r.ambiguous) throw new UikitHostError('AMBIGUOUS', `'${label}' names ${r.ambiguous.length} elements: ${r.ambiguous.join(', ')}`);
    if (!r.match) return null;
    const n = r.match.node;
    const out = { ...n, via: r.match.via };
    if (D.CHECKS.has(n.type)) out.checked = D.isChecked(n, typed);
    else if (D.TEXT_INPUTS.has(n.type)) { const v = D.inputValue(n, typed); out.value = v === undefined || v === null ? '' : String(v); } // a DOM input's value is a string
    else if (D.INPUTS.has(n.type)) out.value = D.inputValue(n, typed);
    return out;
  }

  async function setValue(label, value) {
    const { node: n } = resolve(label);
    if (!D.INPUTS.has(n.type)) throw new UikitHostError('NOT_AN_INPUT', `'${label}' is a ${n.type}, not an input`);
    if (n.props.isDisabled) return { changed: false, reason: 'disabled' };
    if (n.props.isReadOnly) return { changed: false, reason: 'read-only' };
    if (D.TEXT_INPUTS.has(n.type)) {
      const t = { value: String(value), type: n.type === 'TextArea' ? 'textarea' : (n.props.type ?? 'text'), tagName: n.type === 'TextArea' ? 'TEXTAREA' : 'INPUT' };
      typed.set(n.key, t.value);
      await deliver(n, 'onChange', event(n, 'change', t));
      await deliver(n, 'onBlur', event(n, 'blur', t));
      return { changed: true };
    }
    if (D.CHECKS.has(n.type)) {
      if (typeof value !== 'boolean') throw new UikitHostError('BAD_VALUE', `'${label}' is a ${n.type}: set it to true or false`);
      if (D.isChecked(n, typed) === value) return { changed: false, reason: 'unchanged' };
      const t = { checked: value, type: 'checkbox', tagName: 'INPUT', ...(n.props.value !== undefined ? { value: n.props.value } : {}) };
      typed.set(n.key, value);
      await deliver(n, 'onChange', event(n, 'change', t));
      await deliver(n, 'onBlur', event(n, 'blur', t));
      return { changed: true };
    }
    if (n.type === 'Select' || n.type === 'RadioGroup') {
      const options = n.props.options ?? [];
      const pick = (v) => options.find((o) => o.value === v || o.label === v);
      const chosen = n.props.isMulti ? [].concat(value).map(pick) : [pick(value)];
      if (chosen.some((o) => !o)) throw new UikitHostError('BAD_VALUE', `'${label}' has no option ${JSON.stringify(value)}; options: ${options.map((o) => JSON.stringify(o.label ?? o.value)).join(', ')}`);
      if (n.type === 'RadioGroup') {
        typed.set(n.key, chosen[0].value);
        await deliver(n, 'onChange', event(n, 'change', { value: chosen[0].value, type: 'radio', tagName: 'INPUT' }));
        return { changed: true };
      }
      const v = n.props.isMulti ? chosen : chosen[0];
      typed.set(n.key, v);
      await deliver(n, 'onChange', v);
      await deliver(n, 'onBlur', event(n, 'blur', { tagName: 'INPUT' }));
      return { changed: true };
    }
    // DatePicker/TimePicker take the string, Range the number, UserPicker the user object (documented onChange types).
    typed.set(n.key, value);
    await deliver(n, 'onChange', value);
    return { changed: true };
  }

  async function click(label) {
    const m = resolve(label);
    const n = m.node;
    if (D.CHECKS.has(n.type)) {
      const r = await setValue(label, !D.isChecked(n, typed));
      return { clicked: r.changed, ...(r.reason ? { reason: r.reason } : {}) };
    }
    if (!D.BUTTONS.has(n.type)) throw new UikitHostError('NOT_CLICKABLE', `'${label}' is a ${n.type}, not a button`);
    if (n.props.isDisabled) return { clicked: false, reason: 'disabled' };
    if (n.props.isLoading) return { clicked: false, reason: 'loading' };
    await deliver(n, 'onClick', event(n, 'click', { tagName: n.type === 'Link' ? 'A' : 'BUTTON', type: n.props.type ?? 'button' }));
    const form = n.props.type === 'submit' ? [...m.ancestors].reverse().find((a) => a.type === 'Form') : null;
    const submitted = form ? await deliver(form, 'onSubmit') : false;
    return { clicked: true, submitted };
  }

  return {
    moduleKey,
    moduleType: type,
    get docs() { return docs; },
    log,
    invokes,
    flags,
    errors,
    console: consoleLines,
    harnessMissing,
    tree: () => latest.doc,
    text: (node = latest.doc) => D.textOf(node, typed),
    outline: (node = latest.doc) => D.outline(node),
    findByLabel,
    setValue,
    click,
    flush: () => flush(),
    waitIdle,
    advance,
    now: () => clock.now,
    close: () => { closed = true; timers.clear(); hostClosed(host); },
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

async function renderInEmulator(emu, { moduleKey, asUser, context, extension } = {}) {
  const via = viaEmulator(emu);
  return render({ appDir: emu.appDir, kitDir: emu.paths.kitDir, moduleKey, context: context ?? emulatorContext(emu, moduleKey, asUser, extension), invoke: via.invoke, fetchProduct: via.fetchProduct });
}

module.exports = { render, renderInEmulator, viaEmulator, emulatorContext, UikitHostError, textOf: D.textOf, outline: D.outline };
