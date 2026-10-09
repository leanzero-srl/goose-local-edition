'use strict';
// Virtual time for invocations (SPEC §2.2). Two halves share this file:
//
//  - THE COST TABLE, read by the platform proxy (proxy.cjs) and by the invocation process alike: every proxied request
//    advances its invocation's virtual clock by GET 120 ms, a search page 300 ms, a changelog bulkfetch 600 ms, a write
//    200 ms. KVS calls are classed the same way (reads 120 ms, writes and transactions 200 ms); queue pushes, Forge LLM
//    and Realtime calls are POSTs, so writes.
//
//  - THE AGENT, preloaded into every invocation process (`node --require __forge_clock__.cjs`, runtime.cjs) when
//    FORGE_VCLOCK_START is set. In that process:
//      * `Date` reads the invocation's virtual clock, which starts at the site's clock when the invocation starts;
//      * setTimeout/setInterval (global, `timers`, `timers/promises`) are virtual: a wait costs its face value in virtual
//        time and no wall time. Time jumps to the next timer only when nothing earlier is due — no request in flight
//        would complete (at its send time + its class cost) before it — and after one quiet turn of the event loop (no
//        timer set or cleared, no request sent or answered, no setImmediate), so a yield or a short un-proxied await
//        (a small crypto.subtle digest) is not overtaken. Unref'd timers, and the runtime wrapper's own log interval
//        (the setInterval it starts in the same turn it installs __forge_fetch__), never move time on their own; they
//        fire when time passes them for another reason. Known gap: a LONG un-proxied await (pbkdf2, zlib) can still be
//        overtaken by a pending timer of the app's;
//      * every platform request (global.__forge_fetch__, which @forge/api, kvs, events, llm and realtime all use) carries
//        its virtual send time in `forge-vclock`; the proxy answers with the virtual completion time, which becomes the
//        clock; the body is read whole before that, so nothing scheduled later can overtake it;
//      * the limit: once the clock would pass the deadline (start + the module's timeout) the process reports it on fd 4
//        and SIGKILLs itself — no result, as the platform kills an invocation that exceeds its limit. A request whose
//        completion passes the deadline was applied; its answer is never seen;
//      * once the runner starts writing the handler's result (fd 3), the clock stops: work left behind (an interval, an
//        unawaited promise) can neither move time nor kill an invocation that already returned;
//      * on exit it reports the final clock on fd 4: `{"vnow": <ms>}` (or `{"vnow": <deadline>, "killed": "limit"}`).
//    CPU time between requests is free (lenient, never unfair): a busy-wait on Date.now() never ends in virtual time and
//    meets the emulator's real-time guard instead.

const COST_MS = { read: 120, search: 300, bulkfetch: 600, write: 200 };
const KVS_READS = new Set(['/api/v1/get', '/api/v1/query', '/api/v1/secret/get', '/api/v1/entity/get', '/api/v1/entity/query', '/api/v1/batch/get']);
const VCLOCK_HEADER = 'forge-vclock';
const VCLOCK_FD = 4;

function costOf({ kvs = false, method = 'GET', path = '' } = {}) {
  const m = String(method || 'GET').toUpperCase();
  const p = String(path ?? '').split('?')[0];
  if (kvs) return KVS_READS.has(p) ? COST_MS.read : COST_MS.write;
  if (p === '/rest/api/3/search/jql' && (m === 'GET' || m === 'POST')) return COST_MS.search;
  if (p === '/rest/api/3/changelog/bulkfetch' && m === 'POST') return COST_MS.bulkfetch;
  return m === 'GET' || m === 'HEAD' ? COST_MS.read : COST_MS.write;
}

function install() {
  const fs = require('fs');
  const timers = require('timers');
  const timersPromises = require('timers/promises');
  const start = Number(process.env.FORGE_VCLOCK_START);
  const deadline = Number(process.env.FORGE_VCLOCK_DEADLINE);
  if (!Number.isFinite(start) || !Number.isFinite(deadline)) throw new Error('forge clock: FORGE_VCLOCK_START / FORGE_VCLOCK_DEADLINE are not numbers');
  const realSetImmediate = global.setImmediate;
  let vnow = start;
  let frozen = false;
  const report = (obj) => { try { fs.writeSync(VCLOCK_FD, `${JSON.stringify(obj)}\n`); } catch { /* fd 4 is the emulator's; absent outside it */ } };
  const kill = () => {
    vnow = deadline;
    report({ vnow, killed: 'limit' });
    process.kill(process.pid, 'SIGKILL');
  };

  const RealDate = Date;
  const now = () => vnow;
  global.Date = new Proxy(RealDate, {
    construct: (target, args, newTarget) => Reflect.construct(target, args.length ? args : [vnow], newTarget),
    apply: () => new RealDate(vnow).toString(),
    get: (target, prop, receiver) => (prop === 'now' ? now : Reflect.get(target, prop, receiver)),
  });

  // Timers: one queue ordered by (due, seq).
  const queue = [];
  const byId = new Map();
  let seq = 0;
  const inflight = new Map();
  let pumpArmed = false;
  let activity = 0; // bumped by every timer set/cleared, request sent/answered and setImmediate
  let quietAt = -1; // the activity count the previous pump turn saw
  let platformTurn = false; // the synchronous turn in which the wrapper installs __forge_fetch__
  const insert = (e) => {
    let i = queue.length;
    while (i > 0 && (queue[i - 1].due > e.due || (queue[i - 1].due === e.due && queue[i - 1].seq > e.seq))) i--;
    queue.splice(i, 0, e);
  };
  const remove = (e) => { const i = queue.indexOf(e); if (i >= 0) queue.splice(i, 1); };
  const schedulePump = () => { if (!pumpArmed && !frozen) { pumpArmed = true; realSetImmediate(pump); } };
  function pump() {
    pumpArmed = false;
    if (frozen || !queue.length) return;
    const next = queue[0];
    if (next.due > vnow) {
      if (!queue.some((e) => e.ref && !e.background)) return; // nothing the invocation itself waits on
      let horizon = Infinity;
      for (const done of inflight.values()) horizon = Math.min(horizon, done);
      if (next.due > horizon) return; // a response completes first; its arrival re-arms the pump
      if (quietAt !== activity) { quietAt = activity; schedulePump(); return; } // one quiet turn before time moves
      if (next.due > deadline) return kill();
      vnow = next.due;
    }
    queue.shift();
    if (next.repeat) { next.due = vnow + next.ms; next.seq = ++seq; insert(next); } else { next.active = false; byId.delete(next.id); }
    try { next.fn(...next.args); } catch (e) { process.nextTick(() => { throw e; }); }
    schedulePump();
  }
  class VirtualTimeout {
    constructor(e) { this._e = e; }
    ref() { this._e.ref = true; schedulePump(); return this; }
    unref() { this._e.ref = false; return this; }
    hasRef() { return this._e.ref; }
    refresh() {
      const e = this._e;
      if (e.active) remove(e); else { e.active = true; byId.set(e.id, e); }
      e.due = vnow + e.ms; e.seq = ++seq; activity++; insert(e); schedulePump();
      return this;
    }
    close() { clear(this); return this; }
    [Symbol.toPrimitive]() { return this._e.id; }
  }
  const add = (fn, ms, args, repeat) => {
    if (typeof fn !== 'function') throw new TypeError(`The "callback" argument must be of type function. Received ${typeof fn}`);
    let d = Number(ms);
    if (!(d >= 1 && d <= 2147483647)) d = 1; // Node's rule: outside 1..TIMEOUT_MAX the delay is 1 ms
    const e = { id: ++seq, seq, due: vnow + d, ms: d, fn, args, repeat, ref: true, active: true, background: repeat && platformTurn };
    byId.set(e.id, e);
    activity++;
    insert(e);
    schedulePump();
    return new VirtualTimeout(e);
  };
  function clear(h) {
    const e = h instanceof VirtualTimeout ? h._e : byId.get(typeof h === 'object' && h !== null ? Number(h) : h);
    if (!e || !e.active) return;
    e.active = false;
    activity++;
    byId.delete(e.id);
    remove(e);
  }
  const realPromiseImmediate = timersPromises.setImmediate;
  const vSetImmediate = (fn, ...args) => { activity++; return realSetImmediate(fn, ...args); };
  global.setImmediate = vSetImmediate;
  timers.setImmediate = vSetImmediate;
  timersPromises.setImmediate = (...args) => { activity++; return realPromiseImmediate(...args); };
  const vSetTimeout = (fn, ms, ...args) => add(fn, ms, args, false);
  const vSetInterval = (fn, ms, ...args) => add(fn, ms, args, true);
  const abortError = (signal) => Object.assign(new Error('The operation was aborted'), { name: 'AbortError', code: 'ABORT_ERR', cause: signal?.reason });
  const vSleep = (ms, value, options = {}) => new Promise((resolve, reject) => {
    const signal = options?.signal;
    if (signal?.aborted) return reject(abortError(signal));
    const onAbort = () => { clear(h); reject(abortError(signal)); };
    const h = add(() => { signal?.removeEventListener('abort', onAbort); resolve(value); }, ms, [], false);
    if (options?.ref === false) h.unref();
    signal?.addEventListener('abort', onAbort, { once: true });
  });
  for (const target of [global, timers]) {
    target.setTimeout = vSetTimeout;
    target.setInterval = vSetInterval;
    target.clearTimeout = clear;
    target.clearInterval = clear;
  }
  timersPromises.setTimeout = vSleep;
  if (timersPromises.scheduler) timersPromises.scheduler.wait = (ms, options) => vSleep(ms, undefined, options);

  // Platform requests: send time out, completion time back; the body is read whole before the clock moves past it.
  const isRequest = (x) => x !== null && typeof x === 'object' && typeof x.url === 'string' && typeof x.headers?.get === 'function';
  async function settle(res) {
    if (!res.body || [204, 205, 304].includes(res.status)) return res;
    const chunks = [];
    const reader = res.body.getReader();
    for (;;) { const { done, value } = await reader.read(); if (done) break; chunks.push(value); }
    let i = 0;
    // One chunk per read, as it arrived: a reader that depends on chunking (a streamed answer) sees the same reads.
    const body = new ReadableStream({ pull(c) { if (i < chunks.length) c.enqueue(chunks[i++]); else c.close(); } });
    return new Response(body, { status: res.status, statusText: res.statusText, headers: res.headers });
  }
  let platformFetch;
  let reqSeq = 0;
  async function virtualFetch(meta, path, init) {
    const method = init?.method ?? (isRequest(path) ? path.method : 'GET');
    const target = isRequest(path) ? path.url : String(path ?? '');
    const id = ++reqSeq;
    const sentAt = vnow;
    inflight.set(id, sentAt + costOf({ kvs: meta?.type === 'kvs', method, path: target }));
    activity++;
    const headers = new Headers(init?.headers ?? (isRequest(path) ? path.headers : undefined));
    headers.set(VCLOCK_HEADER, String(sentAt));
    try {
      const res = await platformFetch(meta, path, { ...(init ?? {}), headers });
      const out = await settle(res);
      const answered = Number(res.headers.get(VCLOCK_HEADER));
      const done = Number.isFinite(answered) && answered > 0 ? answered : inflight.get(id);
      if (!frozen) {
        if (done > deadline) kill();
        if (done > vnow) vnow = done;
      }
      return out;
    } finally {
      inflight.delete(id);
      activity++;
      schedulePump();
    }
  }
  Object.defineProperty(global, '__forge_fetch__', {
    configurable: true,
    enumerable: true,
    get: () => (platformFetch ? virtualFetch : undefined),
    set: (fn) => {
      platformFetch = fn;
      platformTurn = true;
      queueMicrotask(() => { platformTurn = false; });
    },
  });

  // The runner writes the handler's result to fd 3: from then on the invocation has returned and its clock is final.
  const realCreateWriteStream = fs.createWriteStream;
  fs.createWriteStream = function createWriteStream(p, options) {
    const stream = realCreateWriteStream.apply(this, arguments);
    if (options && options.fd === 3) {
      const end = stream.end;
      stream.end = function endResult(...a) { frozen = true; return end.apply(this, a); };
    }
    return stream;
  };
  process.on('exit', () => report({ vnow }));
}

if (process.env.FORGE_VCLOCK_START !== undefined) install();

module.exports = { costOf, COST_MS, KVS_READS, VCLOCK_HEADER, VCLOCK_FD };
