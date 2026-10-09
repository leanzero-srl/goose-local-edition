'use strict';
// The Forge emulator (DESIGN.md §6, interface I2). One instance = one installed app on one site.
//
//   const emu = await createEmulator({ appDir, kitDir, site, runtime: 'wrapper' | 'shim' });
//   emu.manifest; emu.functions; emu.modules(type);
//   await emu.build();                       -> { functions: [{key, handler, bundled, loaded, error}], files }
//   await emu.invoke(fnKey, { moduleKey, event, asUser })   -> {ok, result, error, logs, ms, calls, ...}
//   await emu.deliverProductEvent(change)    -> the site applies it, every subscribed trigger runs (no drain)
//   await emu.drainQueues()                  -> [{eventId, attempt, result, retryAfter, ...}] (queue events and trigger retries)
//   await emu.runScheduled(moduleKey)        -> invoke + drain
//   await emu.invokeAction(actionKey, inputs, { asUser })
//   await emu.callResolver({ moduleKey, functionKey, payload, asUser, extension })   the platform's context for asUser
//   await emu.invokeWebtrigger(moduleKey, request) / emu.webtriggerUrl(moduleKey)    the public route /x/webtrigger/<key>
//
// TIME IS VIRTUAL (SPEC §2.2, clock.cjs). `emu.clock.now()` is the site's clock as last seen. An invocation starts at
// the site's current time, its process runs on its own virtual clock (proxied requests cost their class, waits their
// face value), it is killed when that clock passes the module's limit, and when it ends the site's clock moves to its
// end. Each result carries t0/t1 (virtual ISO), vms (virtual ms) and ms (real ms, never a grading input).
//   await emu.openSurface(page, { moduleKey, entry, theme, layout, asUser, extension })   (bridge-host.cjs)
//   await emu.hostSave(page)
//   emu.kvs.snapshot(); emu.log; emu.bridgeLog; emu.harnessMissing; emu.close()
//   await emu.llm.log(since) / emu.llm.phase(name)        Forge LLM: every prompt and answer; restart the script
//   await emu.realtime.log(since); emu.realtime.deliveries()   every publish (delivered/rejected); page deliveries
//
// `site` is the object createSite() returns (scoring, in-process) or {url, adminUrl} of a running site
// (the dev kit). Either way the emulator reaches the site only through its control surface and the proxy.
const fs = require('fs');
const os = require('os');
const path = require('path');
const crypto = require('crypto');
const { kitPaths } = require('./kitpaths.cjs');
const { createKvs } = require('./kvs.cjs');
const { createProxy } = require('./proxy.cjs');
const rt = require('./runtime.cjs');
const host = require('./bridge-host.cjs');

const DAY_MS = 86_400_000;
// developer.atlassian.com/platform/forge/limits-invocation (RESEARCH §2): standard functions 25 s; web
// trigger, action 55 s; async consumer and scheduledTrigger 55 s by default, up to 900 s via timeoutSeconds.
const TIMEOUTS = { standard: 25, action: 55, webtrigger: 55, longRunningDefault: 55, longRunningMax: 900 };
// Async events (runtime-reference/async-events-api): retried within a 24 h retention window with
// exponential backoff "up to approximately 15 minutes"; the harness's schedule is stated in contract §8.
const REDELIVERY_MINUTES = [1, 2, 4, 8];
const REDELIVERY_CAP_MINUTES = 15;
const RETENTION_MS = DAY_MS;
const MAX_RETRY_AFTER_S = 900;
// limits-async-events: "Cyclic invocation limit 1000 ... push requests across all handlers originating from a single
// initial function invocation" (forge2/research/BRIEF.md rob#18).
const CYCLIC_INVOCATION_LIMIT = 1000;
const LONG_RUNNING_PAYLOAD_BYTES = 100 * 1024;
// events-reference/product_events: "You can only retry an event for a maximum of four times"; platform errors (a timeout)
// are retried too (CHANGE-681). SPEC §2.2: product-event triggers, up to 4 retries.
const TRIGGER_MAX_RETRIES = 4;

function siteClient(site) {
  if (site.control) {
    const c = site.control;
    return { url: site.url, call: async (op, args = {}) => c[op](args), onRealtime: site.realtime ? (fn) => site.realtime.onDeliver(fn) : null };
  }
  const admin = site.adminUrl;
  return {
    url: site.url,
    call: async (op, args = {}) => {
      const r = await fetch(`${admin}/${op}`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(args) });
      const body = await r.json();
      if (!r.ok) throw new Error(`site control ${op} failed: ${body.error ?? r.status}`);
      return body;
    },
  };
}

// Errors carry both the wrapper's {errorType, errorMessage} and the JS {name, message}.
const errorOf = (e) => (e ? { name: e.name ?? e.errorType ?? 'Error', message: e.message ?? e.errorMessage ?? String(e), errorType: e.errorType ?? e.name ?? 'Error', errorMessage: e.errorMessage ?? e.message ?? String(e) } : null);

function functionUsers(manifest) {
  const users = {};
  for (const [type, entries] of Object.entries(manifest?.modules ?? {})) {
    if (type === 'function' || !Array.isArray(entries)) continue;
    for (const e of entries) {
      const fns = [e.function, e.resolver?.function, e.handler?.function].filter((x) => typeof x === 'string');
      for (const f of fns) (users[f] ??= []).push({ type, key: e.key, module: e, via: e.function === f ? 'function' : 'resolver' });
    }
  }
  return users;
}

async function createEmulator({ appDir, kitDir, site, runtime = 'wrapper', fence, workDir, devState = null, onInvocation = null, aroundInvocation = null } = {}) {
  if (!['wrapper', 'shim'].includes(runtime)) throw new Error(`REFUSED: runtime must be 'wrapper' or 'shim', got ${runtime}`);
  const paths = kitPaths(kitDir);
  const wrapper = runtime === 'wrapper' ? rt.verifyWrapper(paths) : null;
  if (runtime === 'shim') process.stderr.write('\n*** RUNTIME SHIM: not Atlassian\'s runtime wrapper. Results are UNPUBLISHABLE (runtime: "shim"). ***\n\n');
  let fenceMode = fence;
  if (fenceMode === undefined || fenceMode === 'sandbox') {
    if (!rt.sandboxAvailable()) throw new Error('REFUSED: sandbox-exec cannot apply the per-invocation fence on this host; scoring never runs app code unfenced');
    fenceMode = 'sandbox';
  } else if (fenceMode === 'dev-auto') {
    fenceMode = rt.sandboxAvailable() ? 'sandbox' : 'node-permission';
  }
  const YAML = paths.require('yaml');
  let manifest = null;
  let manifestError = null;
  try {
    manifest = YAML.parse(fs.readFileSync(path.join(appDir, 'manifest.yml'), 'utf8'));
    if (!manifest || typeof manifest !== 'object') throw new Error('manifest.yml is empty');
  } catch (e) { manifestError = String(e.message); }

  const client = siteClient(site);
  const info = await client.call('info');
  // The emulator's clock mirrors the site's (the world's) clock and never runs on wall time by itself.
  let vnow = Date.parse(info.now);
  // The site's and the emulator's clock both reach at least t (the site's may already be later: world events, probes).
  const advanceTo = async (t) => {
    let siteNow = (await client.call('clock')).now;
    if (t > siteNow) siteNow = (await client.call('advance', { ms: t - siteNow })).now;
    vnow = Math.max(vnow, siteNow);
    return vnow;
  };
  const syncClock = () => advanceTo(vnow);
  const clock = { now: () => vnow, observe: (t) => { if (t > vnow) vnow = t; }, advanceTo };
  const advance = async (ms) => { const r = await client.call('advance', { ms }); vnow = Math.max(vnow, r.now); return vnow; };
  await syncClock();

  const entities = manifest?.app?.storage?.entities ?? [];
  const kvs = createKvs({ entities: Array.isArray(entities) ? entities : [], now: clock.now });
  const invocations = new Map();
  const log = [];
  const harnessMissing = [];
  const bridgeLog = [];
  const queueState = { pending: [], jobs: new Map(), seq: 0 };
  const users = functionUsers(manifest);
  const functions = (manifest?.modules?.function ?? []).filter((f) => f && typeof f.key === 'string');
  const modules = (type) => (Array.isArray(manifest?.modules?.[type]) ? manifest.modules[type] : []);
  const moduleByKey = (key) => {
    for (const [type, entries] of Object.entries(manifest?.modules ?? {})) {
      if (type === 'function' || !Array.isArray(entries)) continue;
      const m = entries.find((e) => e?.key === key);
      if (m) return { type, module: m };
    }
    return null;
  };

  const consumerFor = (queueName) => modules('consumer').find((c) => c.queue === queueName);
  // A consumer names its function directly (`function:`) or through a resolver (`resolver: {function, method}`): both are
  // arms of the schema's oneOf and both are invoked (1.0 defect C invoked only the first).
  const consumerFunction = (consumer) => consumer?.function ?? consumer?.resolver?.function;
  const queue = {
    push(inv, body, at = clock.now()) {
      const items = Array.isArray(body?.payload) ? body.payload : [];
      if ((inv.depth ?? 0) >= CYCLIC_INVOCATION_LIMIT) return { status: 405, body: { message: 'Cyclic invocation limit reached' } };
      const consumer = consumerFor(body.queueName);
      const fnDef = consumer && functions.find((f) => f.key === consumerFunction(consumer));
      if (fnDef && Number(fnDef.timeoutSeconds) > TIMEOUTS.longRunningDefault && Buffer.byteLength(JSON.stringify(items)) > LONG_RUNNING_PAYLOAD_BYTES) {
        return { status: 413, body: { errorMessage: 'Payload size for long running functions exceeds 100 KB' } };
      }
      const job = queueState.jobs.get(body.jobId) ?? { success: 0, inProgress: 0, failed: 0 };
      queueState.jobs.set(body.jobId, job);
      items.forEach((item, i) => {
        const delay = Number(item.delayInSeconds ?? 0);
        queueState.pending.push({
          seq: queueState.seq++, eventId: `${body.jobId}#${i}`, jobId: body.jobId, queueName: body.queueName, body: item.body,
          delayInSeconds: item.delayInSeconds, concurrency: item.concurrency, enqueuedAt: at, readyAt: at + delay * 1000,
          attempt: 0, retryReason: null, retryData: null, lineage: { originChange: inv.originChange ?? null, scheduledRun: inv.scheduledRun ?? null, depth: (inv.depth ?? 0) + 1 },
        });
        job.inProgress += 1;
      });
      return { status: 201, body: {} };
    },
    stats(body) {
      const job = queueState.jobs.get(body?.jobId);
      return job ? { status: 200, body: { success: job.success, inProgress: job.inProgress, failed: job.failed } } : { status: 404, body: { message: 'Job does not exist' } };
    },
    cancel(body) {
      const job = queueState.jobs.get(body?.jobId);
      if (!job) return { status: 404, body: { message: 'Job does not exist' } };
      const before = queueState.pending.length;
      queueState.pending = queueState.pending.filter((e) => e.jobId !== body.jobId);
      job.inProgress -= before - queueState.pending.length;
      return { status: 204, body: undefined };
    },
  };

  // The public web-trigger route answers through the ingress P3 owns (lib/webtrigger.cjs: handle(emu, moduleKey, request)
  // -> {statusCode, headers, body}); without it the route is a loud 501, never a quiet success.
  const webtriggerIngress = async (moduleKey, request) => {
    let ingress;
    try { ingress = require('./webtrigger.cjs'); } catch (e) {
      if (e?.code !== 'MODULE_NOT_FOUND') throw e;
      harnessMissing.push({ what: `webtrigger ingress (lib/webtrigger.cjs) for ${moduleKey}`, at: new Date(clock.now()).toISOString(), invocationId: null, moduleType: 'webtrigger' });
      return { statusCode: 501, headers: { 'content-type': 'application/json' }, body: { code: 'EMULATOR_NOT_MODELLED', message: 'web-trigger ingress lib/webtrigger.cjs is not installed in this kit' } };
    }
    if (typeof ingress.handle !== 'function') throw new Error('lib/webtrigger.cjs exports no handle(emu, moduleKey, request)');
    return ingress.handle(emu, moduleKey, request);
  };
  const proxy = createProxy({ siteUrl: client.url, siteCall: client.call, manifest, kvs, queue, invocations, clock, log, harnessMissing, webtrigger: webtriggerIngress });
  const proxyAddr = await proxy.listen();
  const ownWorkDir = !workDir;
  const work = workDir ?? fs.mkdtempSync(path.join(os.tmpdir(), 'forge-emu-'));
  const bundleDir = path.join(work, 'bundle');
  let built = null;

  if (devState) {
    if (devState.kvs) kvs.load(devState.kvs);
    if (devState.queue) queueState.pending = devState.queue;
  }

  const appId = String(manifest?.app?.id ?? 'ari:cloud:ecosystem::app/unknown').split('/').pop();
  const contextAri = `ari:cloud:jira::site/${info.cloudId}`;
  let invSeq = 0;
  const appContext = (functionKey, moduleKey, invocationId) => ({
    appId, environmentId: 'emulator-env', environmentType: 'DEVELOPMENT', appVersion: '1.0.0', invocationId, installationId: 'emulator-install',
    moduleKey, functionKey, contextAri, installationSummary: { id: 'emulator-install', primaryInstallationContext: contextAri, secondaryInstallationContexts: [] },
  });
  const timeoutFor = (fnDef, moduleType) => {
    if (['consumer', 'scheduledTrigger'].includes(moduleType)) {
      const t = Number(fnDef?.timeoutSeconds);
      return Number.isFinite(t) && t > 0 ? Math.min(t, TIMEOUTS.longRunningMax) : TIMEOUTS.longRunningDefault;
    }
    if (moduleType === 'action') return TIMEOUTS.action;
    if (moduleType === 'webtrigger') return TIMEOUTS.webtrigger;
    return TIMEOUTS.standard;
  };

  async function build() {
    if (!manifest) { built = { functions: [], files: {}, error: manifestError }; return built; }
    fs.rmSync(bundleDir, { recursive: true, force: true });
    fs.mkdirSync(bundleDir, { recursive: true });
    const files = await rt.bundle({ appDir, outDir: bundleDir, manifest, paths });
    rt.prepareBundleDir(bundleDir, paths, runtime);
    const loadable = Object.entries(files).filter(([, f]) => f.bundled).map(([k]) => k);
    let probe = {};
    if (loadable.length) {
      const r = await rt.runInvocation({ bundleDir, lambdaEvent: null, timeoutSec: TIMEOUTS.standard, vStart: clock.now(), runtime: 'probe', fence: fenceMode,
        proxyPort: proxyAddr.port, extra: { files: loadable, appContext: appContext('probe', 'probe', 'probe') } });
      probe = r.result ?? {};
      if (!r.result) for (const k of loadable) probe[k] = { functions: [], error: r.crash?.message ?? 'load probe produced no result' };
    }
    const out = functions.map((f) => {
      const h = rt.parseHandler(f.handler);
      if (h.error) return { key: f.key, handler: f.handler, bundled: false, loaded: false, error: h.error };
      const file = files[h.module];
      if (!file?.bundled) return { key: f.key, handler: f.handler, bundled: false, loaded: false, error: file?.error ?? 'not bundled' };
      const p = probe[h.module];
      if (p?.error) return { key: f.key, handler: f.handler, bundled: true, loaded: false, error: p.error };
      const exported = p?.functions?.includes(h.fn);
      return { key: f.key, handler: f.handler, bundled: true, loaded: Boolean(exported), error: exported ? null : `Handler "${h.fn}" is not an exported function of src/${h.module}` };
    });
    built = { functions: out, files: Object.fromEntries(Object.entries(files).map(([k, v]) => [k, { bundled: v.bundled, error: v.error, metafile: v.metafile }])) };
    return built;
  }

  async function invokeFunction(fnKey, { moduleKey, moduleType, event, asUser, lineage = {}, source = 'function' } = {}) {
    if (!built) await build();
    const fnDef = functions.find((f) => f.key === fnKey);
    if (!fnDef) return { ok: false, error: errorOf({ errorType: 'EmulatorError', errorMessage: `no function '${fnKey}' in the manifest` }), logs: [], ms: 0, calls: [] };
    const status = built.functions.find((f) => f.key === fnKey);
    const user = users[fnKey]?.find((u) => !moduleKey || u.key === moduleKey);
    const mType = moduleType ?? user?.type ?? null;
    const mKey = moduleKey ?? user?.key ?? null;
    const id = `inv-${(++invSeq).toString().padStart(5, '0')}-${crypto.randomBytes(3).toString('hex')}`;
    const timeoutSec = timeoutFor(fnDef, mType);
    // The invocation starts at the site's current time — after the change that triggered it was applied (the 1.0 stale
    // trigger clock started it at the emulator's older time, so 29 of 41 measured trigger runs exceeded 25 s on paper).
    const vStart = await syncClock();
    const record = { id, functionKey: fnKey, moduleType: mType, moduleKey: mKey, aaid: asUser ?? undefined, source,
      scheduledRun: lineage.scheduledRun ?? null, originChange: lineage.originChange ?? null, depth: lineage.depth ?? 0,
      vStart, vnow: vStart, deadline: vStart + timeoutSec * 1000 };
    invocations.set(id, record);
    const t0 = new Date(vStart).toISOString();
    if (!status?.loaded) {
      const out = { ok: false, invocationId: id, functionKey: fnKey, moduleType: mType, moduleKey: mKey, t0, t1: t0, vms: 0,
        error: errorOf({ errorType: 'FunctionNotLoaded', errorMessage: status?.error ?? 'function did not bundle' }), logs: [], ms: 0, calls: [], timedOut: false };
      onInvocation?.(record, out);
      return out;
    }
    const lambdaEvent = {
      body: event, handler: fnDef.handler, variables: [],
      _meta: {
        proxy: { url: proxyAddr.url, token: rt.fakeJwt({ inv: id, exp: Math.floor(vStart / 1000) + 86400 }), host: '127.0.0.1' },
        contextAri, appContext: appContext(fnKey, mKey, id),
        tracing: { traceId: crypto.randomBytes(8).toString('hex'), spanId: crypto.randomBytes(8).toString('hex') },
        ...(asUser ? { aaid: asUser } : {}), timeout: timeoutSec, featureFlags: [],
      },
    };
    const exec = () => rt.runInvocation({ bundleDir, lambdaEvent, timeoutSec, vStart, runtime, fence: fenceMode, proxyPort: proxyAddr.port });
    const r = aroundInvocation ? await aroundInvocation(exec) : await exec();
    // The clock the process reported; a process that died without one ends at the last completion the proxy answered.
    const vEnd = Math.max(r.vEnd ?? record.vnow, vStart);
    record.vnow = vEnd;
    await advanceTo(vEnd);
    const t1 = new Date(vEnd).toISOString();
    const calls = log.filter((c) => c.invocationId === id);
    let out;
    if (r.timedOut) {
      out = { ok: false, timedOut: true, error: errorOf({ errorType: 'TimeoutError', errorMessage: r.killedAtLimit
        ? `invocation exceeded the ${timeoutSec} s platform limit (virtual time) and was killed`
        : `invocation ran ${timeoutSec} s of real time without finishing (CPU-bound or hung) and was killed` }) };
    } else if (r.crash) out = { ok: false, error: errorOf({ errorType: r.crash.name ?? 'RunnerError', errorMessage: r.crash.message }) };
    else if (r.result?.success) out = { ok: true, result: r.result.body };
    else out = { ok: false, error: errorOf(r.result?.error ?? { errorType: 'Unknown', errorMessage: 'no result' }) };
    Object.assign(out, { invocationId: id, functionKey: fnKey, moduleType: mType, moduleKey: mKey, asUser: asUser ?? null, t0, t1, vms: vEnd - vStart, timeoutSec,
      logs: r.logs, stderr: r.stderr, ms: r.ms, calls, timedOut: Boolean(r.timedOut), realTimeout: Boolean(r.realTimeout) });
    onInvocation?.(record, out);
    return out;
  }

  // A retry decision shared by queue events and product-event triggers: the app's InvocationError, or a failure.
  const retryPlan = (r, attempt) => {
    if (r.ok && r.result && r.result._retry === true) {
      const opts = r.result.retryOptions ?? {};
      return { kind: 'retry', waitS: Math.min(Math.max(Number(opts.retryAfter ?? 1), 1), MAX_RETRY_AFTER_S),
        reason: opts.retryReason ?? 'FUNCTION_RETRY_REQUEST', data: opts.retryData ?? null };
    }
    if (r.ok) return null;
    // @forge/events InvocationErrorCode names a timeout FUNCTION_TIME_OUT; a thrown error has no code there (harness name).
    return { kind: r.timedOut ? 'timeout' : 'throw', waitS: (REDELIVERY_MINUTES[attempt] ?? REDELIVERY_CAP_MINUTES) * 60,
      reason: r.timedOut ? 'FUNCTION_TIME_OUT' : 'FUNCTION_ERROR', data: null };
  };

  // A product-event trigger that asked for a retry, threw or was killed is delivered again (≤ TRIGGER_MAX_RETRIES), from
  // the same pending list the queues use, so retries and queue events run in one virtual-time order.
  function scheduleTriggerRetry(t, event, r, attempt, lineage) {
    const plan = retryPlan(r, attempt);
    if (!plan) return null;
    if (attempt >= TRIGGER_MAX_RETRIES) return { ...plan, dropped: `retried ${TRIGGER_MAX_RETRIES} times` };
    const readyAt = Date.parse(r.t1) + plan.waitS * 1000;
    queueState.pending.push({ kind: 'trigger', seq: queueState.seq++, eventId: `trigger:${t.key}:${event.changelog?.id ?? event.eventType}:${attempt + 1}`,
      moduleKey: t.key, event, attempt: attempt + 1, retryReason: plan.reason, retryData: plan.data, readyAt, lineage });
    return { ...plan, redeliverAt: new Date(readyAt).toISOString() };
  }

  async function deliverTriggerRetry(ev) {
    const t = modules('trigger').find((x) => x.key === ev.moduleKey);
    const event = { ...ev.event, retryContext: { retryCount: ev.attempt, retryReason: ev.retryReason, retryData: ev.retryData } };
    const r = await invokeFunction(t.function, { moduleKey: t.key, moduleType: 'trigger', event, lineage: ev.lineage });
    const next = scheduleTriggerRetry(t, ev.event, r, ev.attempt, ev.lineage);
    return { kind: 'trigger', eventId: ev.eventId, queueName: null, attempt: ev.attempt, invocationId: r.invocationId, functionKey: r.functionKey, moduleKey: t.key,
      t: r.t0, t1: r.t1, lineage: ev.lineage, ok: r.ok, result: r.result ?? null, error: r.error ?? null, timedOut: r.timedOut, logs: r.logs,
      outcome: next ? next.kind : 'ok', ...(next ? { retryAfter: next.waitS, ...(next.dropped ? { dropped: next.dropped } : { redeliverAt: next.redeliverAt }) } : {}) };
  }

  async function drainQueues() {
    const deliveries = [];
    for (;;) {
      if (!queueState.pending.length) break;
      queueState.pending.sort((a, b) => a.readyAt - b.readyAt || a.seq - b.seq);
      const ev = queueState.pending.shift();
      await advanceTo(ev.readyAt);
      if (ev.kind === 'trigger') { deliveries.push(await deliverTriggerRetry(ev)); continue; }
      const job = queueState.jobs.get(ev.jobId);
      const consumer = consumerFor(ev.queueName);
      const fnKey = consumerFunction(consumer);
      if (!fnKey) {
        deliveries.push({ eventId: ev.eventId, queueName: ev.queueName, attempt: ev.attempt, outcome: 'no_consumer', ok: false, result: null, t: new Date(clock.now()).toISOString() });
        if (job) { job.inProgress -= 1; job.failed += 1; }
        continue;
      }
      await client.call('signal', { type: 'consumer-start', originChange: ev.lineage.originChange, scheduledRun: ev.lineage.scheduledRun });
      const meta = {
        queueName: ev.queueName, jobId: ev.jobId, eventId: ev.eventId,
        ...(ev.delayInSeconds !== undefined ? { delayInSeconds: ev.delayInSeconds } : {}), ...(ev.concurrency ? { concurrency: ev.concurrency } : {}),
        ...(ev.attempt > 0 ? { retryContext: { retryCount: ev.attempt, retryReason: ev.retryReason, retryData: ev.retryData,
          retentionWindow: { startTime: new Date(ev.enqueuedAt).toISOString(), remainingTimeMs: Math.max(0, ev.enqueuedAt + RETENTION_MS - clock.now()) } } } : {}),
      };
      // The resolver form is called the way @forge/resolver's getDefinitions() dispatches: {call: {functionKey, payload,
      // jobId}, context}; the event's metadata (retryContext included) rides in `context` (not documented: harness choice).
      const event = consumer.function ? { body: ev.body, ...meta } : { call: { functionKey: consumer.resolver.method, payload: ev.body, jobId: ev.jobId }, context: meta };
      const r = await invokeFunction(fnKey, { moduleKey: consumer.key, moduleType: 'consumer', event, lineage: ev.lineage });
      const base = { eventId: ev.eventId, queueName: ev.queueName, attempt: ev.attempt, invocationId: r.invocationId, functionKey: r.functionKey, moduleKey: consumer.key,
        t: r.t0, t1: r.t1, lineage: ev.lineage, ok: r.ok, result: r.result ?? null, error: r.error ?? null, timedOut: r.timedOut, logs: r.logs };
      const plan = retryPlan(r, ev.attempt);
      if (!plan) {
        deliveries.push({ ...base, outcome: 'ok' });
        if (job) { job.inProgress -= 1; job.success += 1; }
        continue;
      }
      ev.retryReason = plan.reason;
      ev.retryData = plan.data;
      const next = clock.now() + plan.waitS * 1000;
      if (next > ev.enqueuedAt + RETENTION_MS) {
        deliveries.push({ ...base, outcome: plan.kind, retryAfter: plan.waitS, dropped: 'retention window exceeded' });
        if (job) { job.inProgress -= 1; job.failed += 1; }
        continue;
      }
      deliveries.push({ ...base, outcome: plan.kind, retryAfter: plan.waitS, redeliverAt: new Date(next).toISOString() });
      queueState.pending.push({ ...ev, attempt: ev.attempt + 1, readyAt: next, seq: queueState.seq++ });
    }
    return deliveries;
  }

  async function triggerEvent(delivery) {
    const change = delivery.change;
    const event = {
      eventType: 'avi:jira:updated:issue', selfGenerated: change.authorId === info.appAccountId, jiraEventTypeName: 'issue_generic',
      issue: delivery.issue, atlassianId: change.authorId,
      changelog: { id: change.id, items: change.items }, associatedUsers: [{ accountId: change.authorId }],
    };
    const results = [];
    for (const t of modules('trigger')) {
      const events = (Array.isArray(t.events) ? t.events : []).map((e) => (typeof e === 'string' ? e : e?.eventType));
      if (!events.includes('avi:jira:updated:issue')) continue;
      if (t.filter?.ignoreSelf && event.selfGenerated) continue;
      const lineage = { originChange: change.id };
      const r = await invokeFunction(t.function, { moduleKey: t.key, moduleType: 'trigger', event, lineage });
      const retry = scheduleTriggerRetry(t, event, r, 0, lineage);
      results.push(retry ? { ...r, retry } : r);
    }
    return { changelogId: change.id, duplicate: Boolean(delivery.duplicate), slot: delivery.slot, event, triggers: results, invocations: results };
  }

  async function deliverProductEvent(change) {
    const changelogId = typeof change === 'string' ? change : change.changelogId ?? change.id;
    const delivery = await client.call('event', { changelogId });
    return triggerEvent(delivery);
  }

  async function deliverNext() {
    const d = await client.call('next');
    if (d.done) return null;
    return triggerEvent(d);
  }

  let scheduledRuns = 0;
  async function runScheduled(moduleKey) {
    const m = modules('scheduledTrigger').find((x) => x.key === moduleKey);
    if (!m) throw new Error(`no scheduledTrigger '${moduleKey}' in the manifest`);
    const run = ++scheduledRuns;
    const invocation = await invokeFunction(m.function, { moduleKey: m.key, moduleType: 'scheduledTrigger',
      event: { context: { cloudId: info.cloudId, moduleKey: m.key }, contextToken: crypto.randomBytes(16).toString('hex') }, lineage: { scheduledRun: run } });
    const deliveries = await drainQueues();
    return { run, invocation, deliveries };
  }

  async function invokeAction(actionKey, inputs = {}, { asUser } = {}) {
    const m = modules('action').find((x) => x.key === actionKey);
    if (!m) return { ok: false, error: errorOf({ errorType: 'EmulatorError', errorMessage: `no action '${actionKey}' in the manifest` }), calls: [] };
    if (!m.function) return { ok: false, error: errorOf({ errorType: 'EmulatorError', errorMessage: `action '${actionKey}' has no function` }), calls: [] };
    // developer.atlassian.com/platform/forge/manifest-reference/modules/rovo-action (fetched 2026-10-02): "the payload
    // will also include a context object with relevant Atlassian app identifiers ... The context contains the user's
    // accountId", with examples carrying {cloudId, moduleKey, jira|confluence}. function-reference/arguments: the
    // second argument's `principal.accountId` is "the Atlassian ID of the user that interacted with the component"
    // (the wrapper builds it from the invocation's aaid). Both documented places carry the user; neither was
    // measured on a live Rovo invocation.
    const context = { cloudId: info.cloudId, moduleKey: actionKey, ...(asUser ? { accountId: asUser } : {}) };
    return invokeFunction(m.function, { moduleKey: actionKey, moduleType: 'action', asUser, event: { ...inputs, context } });
  }

  async function invokeResolver(moduleKey, functionKey, payload, context, asUser) {
    const found = moduleByKey(moduleKey);
    const fnKey = found?.module?.resolver?.function ?? found?.module?.edit?.resolver?.function;
    if (!fnKey) return { ok: false, error: errorOf({ errorType: 'EmulatorError', errorMessage: `module '${moduleKey}' has no resolver function` }), calls: [] };
    // A resolver is invoked from the app frontend: its body carries the frontend's contextToken, which the wrapper
    // hands @forge/realtime (wrapper: `realtime:{contextToken: body?.contextToken}`), so `publish()` reaches that
    // module's subscribers in that product context.
    const { contextToken } = await client.call('rtcontext', { moduleKey, extension: context?.extension ?? null });
    return invokeFunction(fnKey, { moduleKey, moduleType: found.type, asUser, event: { call: { functionKey, payload: payload ?? {} }, context, contextToken }, source: 'resolver' });
  }

  // The probe's door to a resolver (admin / non-admin / forged payload): the context is the one the platform builds for
  // `asUser` on that module (the Custom UI host's contextFor), so identity can only come from asUser — a forged identity
  // can only travel in `payload`, exactly as on Forge, where a frontend controls the payload and never the context.
  async function callResolver({ moduleKey, functionKey, payload = {}, asUser = null, extension = {}, theme = 'light' } = {}) {
    const found = moduleByKey(moduleKey);
    if (!found) return { ok: false, error: errorOf({ errorType: 'EmulatorError', errorMessage: `no module '${moduleKey}' in the manifest` }), calls: [] };
    const context = host.contextFor(emu, { type: found.type, moduleKey, asUser, extension, theme, entry: 'view' });
    return invokeResolver(moduleKey, functionKey, payload, context, asUser);
  }

  // A web trigger's function with the request a web trigger receives (limit 55 s); the ingress (lib/webtrigger.cjs) maps
  // HTTP to this request and the function's answer (or a static output) back to HTTP.
  async function invokeWebtrigger(moduleKey, request) {
    const m = modules('webtrigger').find((x) => x.key === moduleKey);
    if (!m?.function) return { ok: false, error: errorOf({ errorType: 'EmulatorError', errorMessage: `no web trigger '${moduleKey}' with a function in the manifest` }), calls: [] };
    return invokeFunction(m.function, { moduleKey, moduleType: 'webtrigger', event: request, source: 'webtrigger' });
  }

  const emu = {
    get manifest() { return manifest; },
    manifestError,
    functions,
    modules,
    moduleByKey,
    build,
    invoke: (fnKey, { moduleKey, event, asUser } = {}) => invokeFunction(fnKey, { moduleKey, event, asUser }),
    invokeResolver,
    callResolver,
    invokeWebtrigger,
    webtriggerUrl: (moduleKey) => `${proxyAddr.url}/x/webtrigger/${encodeURIComponent(moduleKey)}`,
    deliverProductEvent,
    deliverNext,
    drainQueues,
    runScheduled,
    invokeAction,
    kvs,
    log,
    bridgeLog,
    harnessMissing,
    invocations,
    proxy,
    clock,
    advance,
    site: client,
    siteInfo: info,
    runtime,
    publishable: runtime === 'wrapper',
    wrapper,
    fence: fenceMode,
    paths,
    appDir,
    queueState,
    dumpState: () => ({ kvs: kvs.dump(), queue: queueState.pending }),
    close: async () => { await host.closeHost(emu); await proxy.close(); if (ownWorkDir) fs.rmSync(work, { recursive: true, force: true }); },
  };
  emu.stop = emu.close;
  emu.openSurface = (page, opts) => host.openSurface(emu, page, opts);
  emu.hostSave = (page) => host.hostSave(emu, page);
  emu.resizeSurface = (page, layout) => host.resize(emu, page, layout);
  emu.serveDev = (opts) => host.serveDev(emu, opts);
  emu.cspReports = () => host.cspReports(emu);
  emu.widgetConfigs = () => host.widgetConfigs(emu);
  // Forge LLM and Realtime live on the site (site/llm.cjs, site/realtime.cjs); these read them through it.
  emu.llm = {
    log: (since = 0) => client.call('llmlog', { since }),
    phase: (phase) => client.call('llmphase', { phase }),
  };
  emu.realtime = {
    log: (since = 0) => client.call('rtlog', { since }),
    deliveries: () => host.realtimeDeliveries(emu),
  };
  return emu;
}

module.exports = { createEmulator, functionUsers, TIMEOUTS, TRIGGER_MAX_RETRIES };
