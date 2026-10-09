'use strict';
// The realm a UI Kit bundle runs in, its bridge, its virtual clock and the drive calls. Runs inside the FENCED child
// process (runner.cjs); index.cjs is the side the scorer and forge-dev call. Fidelity choices (research/uikit.md):
//  - The bundle runs in its own vm realm with `self === globalThis` (the reconciler reads self.__bridge, @forge/bridge
//    reads globalThis.__bridge at module load: verification #5/#6) and no DOM (UI Kit has none). The realm is an
//    environment, not a fence: the fence is the child process's sandbox (index.cjs).
//  - Time is VIRTUAL: setTimeout/setInterval and Date run on a clock that moves only when the host is told (`advance`),
//    so every reading is deterministic and nothing waits on a wall clock. A bridge call takes zero virtual time. Date
//    must move with the timers: @forge/bridge's own limiter (500 invokes per 25 s, utils/index.js) reads Date.now(),
//    and a poller advanced over virtual minutes would otherwise trip it in real milliseconds. Date starts at
//    `startTime`. performance.now() is the virtual clock plus 1 ms per commit so far: React's scheduler yields after
//    5 ms of work (scheduler 0.23.2 frameYieldMs), so on a frozen clock a commit loop would spin inside one task
//    forever; with commits costing time it yields between them as in a browser frame and is counted in turns.
//  - Handlers are delivered on a later macrotask, never inside a batch, with arguments serialised the way the bridge
//    does (functions cannot cross); the handler of the LATEST commit receives them. Inputs get the documented
//    SerialisableEvent (target.type; a text input's value as the string the DOM holds); a submit button calls its
//    Form's onSubmit with no argument (FormProps.onSubmit: () => ...). A disabled or loading button is not clickable
//    (@atlaskit/button 25.4.7 button-base: isInteractive = !isDisabled && !isLoading/overlay).
//  - Any bridge op this host does not model rejects in the app and is recorded in `harnessMissing`, never answered
//    with a quiet default.
const util = require('util');
const vm = require('vm');
const D = require('./doc.cjs');

class UikitHostError extends Error {
  constructor(code, message) { super(message); this.name = 'UikitHostError'; this.code = code; }
}

// Bridge ops with no observable effect in a text host (fire-and-forget in @forge/bridge 7.1.0).
const INERT = new Set(['emitReadyEvent', 'changeWindowTitle', 'emitFrontendCustomMetric']);
// measured: the fixture admin page (test/fixture-admin) settles in 2 turns at boot and 1 after set + toggle + save;
// a commit loop (an effect that always sets state, a 0 ms interval) never settles. A turn is one timer run or one
// bridge answer awaited at ONE virtual time, so this bounds work by count, never by time.
const MAX_TURNS = 1000;
// @forge/bridge's requestJira builds `new Request('', init)` only to normalise headers and body (fetch/fetch.js);
// in the product iframe '' resolves against the page's URL, which this host does not have, so it names a reserved one.
const PAGE_URL = 'https://uikit-host.invalid/';
class PageRequest extends Request {
  constructor(input, init) { super(typeof input === 'string' ? new URL(input, PAGE_URL) : input, init); }
}

const macrotask = () => new Promise((r) => setImmediate(r));
const plain = (v) => (v === undefined ? undefined : JSON.parse(JSON.stringify(v)));

// call(kind, args) -> the parent's answer: 'invoke' {functionKey, payload} -> the resolver result; 'fetchProduct'
// {product, restPath, fetchRequestInit} -> {body, headers, status, statusText, isAttachment}. A rejection carries
// {message, errorType}.
function createHost({ code, filename, context, moduleKey, startTime, call }) {
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
    __uikitNow: () => startTime + clock.now, // taken by the realm's Date below, then deleted
  };
  const ctx = vm.createContext(sandbox, { name: `uikit:${moduleKey}` });
  vm.runInContext('globalThis.self = globalThis; globalThis.window = globalThis;', ctx);
  // The realm's own Date, on the virtual clock: new Date() / Date.now() read it, Date() as a function too.
  vm.runInContext(`(() => {
    const read = globalThis.__uikitNow;
    delete globalThis.__uikitNow;
    class VirtualDate extends Date {
      constructor(...a) { if (a.length) super(...a); else super(read()); }
      static now() { return read(); }
    }
    globalThis.Date = new Proxy(VirtualDate, { apply: () => new VirtualDate().toString() });
  })();`, ctx);
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
          rec.result = plain(await call('invoke', { functionKey: rec.functionKey, payload: rec.payload }));
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
        let r;
        try {
          r = await call('fetchProduct', { product: payload?.product, restPath: payload?.restPath, fetchRequestInit: plain(payload?.fetchRequestInit ?? {}) });
        } catch (e) {
          if (e?.errorType === 'NOT_MODELLED') throw notModelled(e.message);
          throw appReject(String(e?.message ?? e));
        }
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

  try {
    vm.runInContext(code, ctx, { filename });
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
  // Move virtual time forward by `ms`, running each timer at its own time and settling after each. A loop is runs
  // at ONE virtual time (waitIdle counts those); a poller advanced over an hour is many times, not a loop.
  async function advance(ms) {
    const target = clock.now + Math.max(0, Number(ms) || 0);
    for (;;) {
      const t = nextTimer(target);
      if (!t) break;
      clock.now = Math.max(clock.now, t.at);
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
    const handled = await deliver(n, 'onClick', event(n, 'click', { tagName: n.type === 'Link' ? 'A' : 'BUTTON', type: n.props.type ?? 'button' }));
    // a link's navigation leaves the page: the product does it, this host only records that it would
    if (!handled && typeof n.props.href === 'string') harnessMissing.push({ what: `navigation to ${n.props.href}`, at: clock.now, moduleKey });
    const form = n.props.type === 'submit' ? [...m.ancestors].reverse().find((a) => a.type === 'Form') : null;
    const submitted = form ? await deliver(form, 'onSubmit') : false;
    return { clicked: true, submitted };
  }

  return {
    docs, log, invokes, flags, errors, console: consoleLines, harnessMissing, typed,
    now: () => clock.now,
    recordError,
    setValue,
    click,
    flush: () => flush(),
    waitIdle,
    advance,
    close: () => { closed = true; timers.clear(); },
  };
}

module.exports = { createHost, UikitHostError };
