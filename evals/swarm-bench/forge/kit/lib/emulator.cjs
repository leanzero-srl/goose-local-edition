'use strict';
// The Forge emulator (DESIGN.md §6, interface I2). One instance = one installed app on one site.
//
//   const emu = await createEmulator({ appDir, kitDir, site, runtime: 'wrapper' | 'shim' });
//   emu.manifest; emu.functions; emu.modules(type);
//   await emu.build();                       -> { functions: [{key, handler, bundled, loaded, error}], files }
//   await emu.invoke(fnKey, { moduleKey, event, asUser })   -> {ok, result, error, logs, ms, calls, ...}
//   await emu.deliverProductEvent(change)    -> the site applies it, every subscribed trigger runs (no drain)
//   await emu.drainQueues()                  -> [{eventId, attempt, result, retryAfter, ...}]
//   await emu.runScheduled(moduleKey)        -> invoke + drain
//   await emu.invokeAction(actionKey, inputs, { asUser })
//   await emu.openSurface(page, { moduleKey, entry, theme, layout, asUser, extension })   (bridge-host.cjs)
//   await emu.hostSave(page)
//   emu.kvs.snapshot(); emu.log; emu.bridgeLog; emu.harnessMissing; emu.close()
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
const CYCLIC_INVOCATION_LIMIT = 1000;
const LONG_RUNNING_PAYLOAD_BYTES = 100 * 1024;

function siteClient(site) {
  if (site.control) {
    const c = site.control;
    return { url: site.url, call: async (op, args = {}) => c[op](args) };
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
  let clockOffset = Date.parse(info.now) - Date.now();
  const syncClock = async () => { const c = await client.call('clock'); clockOffset = c.now - Date.now(); return c.now; };
  const clock = { now: () => Date.now() + clockOffset };
  const advance = async (ms) => { const r = await client.call('advance', { ms }); clockOffset = r.now - Date.now(); return r.now; };
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
  const queue = {
    push(inv, body) {
      const items = Array.isArray(body?.payload) ? body.payload : [];
      if ((inv.depth ?? 0) >= CYCLIC_INVOCATION_LIMIT) return { status: 405, body: { message: 'Cyclic invocation limit reached' } };
      const consumer = consumerFor(body.queueName);
      const fnDef = consumer && functions.find((f) => f.key === consumer.function);
      if (fnDef && Number(fnDef.timeoutSeconds) > TIMEOUTS.longRunningDefault && Buffer.byteLength(JSON.stringify(items)) > LONG_RUNNING_PAYLOAD_BYTES) {
        return { status: 413, body: { errorMessage: 'Payload size for long running functions exceeds 100 KB' } };
      }
      const job = queueState.jobs.get(body.jobId) ?? { success: 0, inProgress: 0, failed: 0 };
      queueState.jobs.set(body.jobId, job);
      items.forEach((item, i) => {
        const delay = Number(item.delayInSeconds ?? 0);
        queueState.pending.push({
          seq: queueState.seq++, eventId: `${body.jobId}#${i}`, jobId: body.jobId, queueName: body.queueName, body: item.body,
          delayInSeconds: item.delayInSeconds, concurrency: item.concurrency, enqueuedAt: clock.now(), readyAt: clock.now() + delay * 1000,
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

  const proxy = createProxy({ siteUrl: client.url, manifest, kvs, queue, invocations, clock, log, harnessMissing });
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
      const r = await rt.runInvocation({ bundleDir, lambdaEvent: null, timeoutSec: TIMEOUTS.standard, clockOffsetMs: clockOffset, runtime: 'probe', fence: fenceMode,
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
    const record = { id, functionKey: fnKey, moduleType: mType, moduleKey: mKey, aaid: asUser ?? undefined, source,
      scheduledRun: lineage.scheduledRun ?? null, originChange: lineage.originChange ?? null, depth: lineage.depth ?? 0 };
    invocations.set(id, record);
    const timeoutSec = timeoutFor(fnDef, mType);
    if (!status?.loaded) {
      const t = new Date(clock.now()).toISOString();
      const out = { ok: false, invocationId: id, functionKey: fnKey, moduleType: mType, moduleKey: mKey, t0: t, t1: t,
        error: errorOf({ errorType: 'FunctionNotLoaded', errorMessage: status?.error ?? 'function did not bundle' }), logs: [], ms: 0, calls: [], timedOut: false };
      onInvocation?.(record, out);
      return out;
    }
    const lambdaEvent = {
      body: event, handler: fnDef.handler, variables: [],
      _meta: {
        proxy: { url: proxyAddr.url, token: rt.fakeJwt({ inv: id, exp: Math.floor(clock.now() / 1000) + 86400 }), host: '127.0.0.1' },
        contextAri, appContext: appContext(fnKey, mKey, id),
        tracing: { traceId: crypto.randomBytes(8).toString('hex'), spanId: crypto.randomBytes(8).toString('hex') },
        ...(asUser ? { aaid: asUser } : {}), timeout: timeoutSec, featureFlags: [],
      },
    };
    const t0 = new Date(clock.now()).toISOString();
    const exec = () => rt.runInvocation({ bundleDir, lambdaEvent, timeoutSec, clockOffsetMs: clockOffset, runtime, fence: fenceMode, proxyPort: proxyAddr.port });
    const r = aroundInvocation ? await aroundInvocation(exec) : await exec();
    await syncClock();
    const t1 = new Date(clock.now()).toISOString();
    const calls = log.filter((c) => c.invocationId === id);
    let out;
    if (r.timedOut) out = { ok: false, timedOut: true, error: errorOf({ errorType: 'TimeoutError', errorMessage: `invocation exceeded the ${timeoutSec} s platform timeout` }) };
    else if (r.crash) out = { ok: false, error: errorOf({ errorType: r.crash.name ?? 'RunnerError', errorMessage: r.crash.message }) };
    else if (r.result?.success) out = { ok: true, result: r.result.body };
    else out = { ok: false, error: errorOf(r.result?.error ?? { errorType: 'Unknown', errorMessage: 'no result' }) };
    Object.assign(out, { invocationId: id, functionKey: fnKey, moduleType: mType, moduleKey: mKey, asUser: asUser ?? null, t0, t1, timeoutSec,
      logs: r.logs, stderr: r.stderr, ms: r.ms, calls, timedOut: Boolean(r.timedOut) });
    onInvocation?.(record, out);
    return out;
  }

  async function drainQueues() {
    const deliveries = [];
    for (;;) {
      if (!queueState.pending.length) break;
      queueState.pending.sort((a, b) => a.readyAt - b.readyAt || a.seq - b.seq);
      const ev = queueState.pending.shift();
      if (ev.readyAt > clock.now()) await advance(ev.readyAt - clock.now());
      const job = queueState.jobs.get(ev.jobId);
      const consumer = consumerFor(ev.queueName);
      if (!consumer) {
        deliveries.push({ eventId: ev.eventId, queueName: ev.queueName, attempt: ev.attempt, outcome: 'no_consumer', ok: false, result: null, t: new Date(clock.now()).toISOString() });
        if (job) { job.inProgress -= 1; job.failed += 1; }
        continue;
      }
      await client.call('signal', { type: 'consumer-start', originChange: ev.lineage.originChange, scheduledRun: ev.lineage.scheduledRun });
      const asyncEvent = {
        body: ev.body, queueName: ev.queueName, jobId: ev.jobId, eventId: ev.eventId,
        ...(ev.delayInSeconds !== undefined ? { delayInSeconds: ev.delayInSeconds } : {}), ...(ev.concurrency ? { concurrency: ev.concurrency } : {}),
        ...(ev.attempt > 0 ? { retryContext: { retryCount: ev.attempt, retryReason: ev.retryReason, retryData: ev.retryData,
          retentionWindow: { startTime: new Date(ev.enqueuedAt).toISOString(), remainingTimeMs: Math.max(0, ev.enqueuedAt + RETENTION_MS - clock.now()) } } } : {}),
      };
      const r = await invokeFunction(consumer.function, { moduleKey: consumer.key, moduleType: 'consumer', event: asyncEvent, lineage: ev.lineage });
      const base = { eventId: ev.eventId, queueName: ev.queueName, attempt: ev.attempt, invocationId: r.invocationId, functionKey: r.functionKey, moduleKey: consumer.key,
        t: r.t0, t1: r.t1, lineage: ev.lineage, ok: r.ok, result: r.result ?? null, error: r.error ?? null, timedOut: r.timedOut, logs: r.logs };
      const retry = r.ok && r.result && r.result._retry === true;
      if (r.ok && !retry) {
        deliveries.push({ ...base, outcome: 'ok' });
        if (job) { job.inProgress -= 1; job.success += 1; }
        continue;
      }
      let waitS;
      let kind;
      if (retry) {
        const opts = r.result.retryOptions ?? {};
        waitS = Math.min(Math.max(Number(opts.retryAfter ?? 1), 1), MAX_RETRY_AFTER_S);
        ev.retryReason = opts.retryReason ?? 'FUNCTION_RETRY_REQUEST';
        ev.retryData = opts.retryData ?? null;
        kind = 'retry';
      } else {
        waitS = (REDELIVERY_MINUTES[ev.attempt] ?? REDELIVERY_CAP_MINUTES) * 60;
        ev.retryReason = r.timedOut ? 'FUNCTION_TIMEOUT' : 'FUNCTION_ERROR';
        ev.retryData = null;
        kind = r.timedOut ? 'timeout' : 'throw';
      }
      const next = clock.now() + waitS * 1000;
      if (next > ev.enqueuedAt + RETENTION_MS) {
        deliveries.push({ ...base, outcome: kind, retryAfter: waitS, dropped: 'retention window exceeded' });
        if (job) { job.inProgress -= 1; job.failed += 1; }
        continue;
      }
      deliveries.push({ ...base, outcome: kind, retryAfter: waitS, redeliverAt: new Date(next).toISOString() });
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
      results.push(await invokeFunction(t.function, { moduleKey: t.key, moduleType: 'trigger', event, lineage: { originChange: change.id } }));
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
    return invokeFunction(m.function, { moduleKey: actionKey, moduleType: 'action', asUser, event: { ...inputs, context: { cloudId: info.cloudId, moduleKey: actionKey } } });
  }

  async function invokeResolver(moduleKey, functionKey, payload, context, asUser) {
    const found = moduleByKey(moduleKey);
    const fnKey = found?.module?.resolver?.function ?? found?.module?.edit?.resolver?.function;
    if (!fnKey) return { ok: false, error: errorOf({ errorType: 'EmulatorError', errorMessage: `module '${moduleKey}' has no resolver function` }), calls: [] };
    return invokeFunction(fnKey, { moduleKey, moduleType: found.type, asUser, event: { call: { functionKey, payload: payload ?? {} }, context }, source: 'resolver' });
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
  return emu;
}

module.exports = { createEmulator, functionUsers, TIMEOUTS };
