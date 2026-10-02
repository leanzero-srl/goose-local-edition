// WP3's in-process Forge platform for the golden's own tests: bundles src/ the way `forge deploy`
// does (esbuild, one CJS file per handler module), installs the two globals the real @forge packages
// read (__forge_runtime__, __forge_fetch__ — the spike's proven seam) and routes:
//   fpp  -> the mock site (asApp / asUser with the invocation's aaid; asUser without one fails)
//   kvs  -> an in-memory KVS with custom entities, indexes and FAIL_IF_EXISTS
//   queue publish -> an in-memory queue the test drains into the consumer.
'use strict';

const path = require('path');
const fs = require('fs');
const crypto = require('crypto');
const { AsyncLocalStorage } = require('async_hooks');
const YAML = require(path.join(__dirname, '..', '..', '..', 'forge', 'spike', 'node_modules', 'yaml'));
const esbuild = require('esbuild');

const APP = path.join(__dirname, '..');
const als = new AsyncLocalStorage();

function loadManifest() {
  return YAML.parse(fs.readFileSync(path.join(APP, 'manifest.yml'), 'utf8'));
}

async function bundle(outDir) {
  const manifest = loadManifest();
  fs.mkdirSync(outDir, { recursive: true });
  const files = new Set(manifest.modules.function.map((f) => f.handler.split('.')[0]));
  for (const f of files) {
    await esbuild.build({ entryPoints: [path.join(APP, 'src', `${f}.js`)], bundle: true, platform: 'node', format: 'cjs', target: 'node22', outfile: path.join(outDir, `${f}.cjs`), logLevel: 'error' });
  }
  return outDir;
}

function createKvs(manifest) {
  const plain = new Map();
  const entities = new Map(); // name -> Map(key -> value)
  const defs = Object.fromEntries((manifest.app.storage?.entities ?? []).map((e) => [e.name, e]));
  const ops = [];
  const typeOk = (t, v) => (t === 'any' ? true : t === 'string' ? typeof v === 'string' : t === 'boolean' ? typeof v === 'boolean' : t === 'integer' ? Number.isInteger(v) : t === 'float' ? typeof v === 'number' : false);
  const err = (status, code, message) => ({ status, json: { code, message } });
  function handle(op, body) {
    ops.push({ op, body });
    switch (op) {
      case '/api/v1/get':
        return plain.has(body.key) ? { status: 200, json: { key: body.key, value: plain.get(body.key) } } : err(404, 'KEY_NOT_FOUND', 'not found');
      case '/api/v1/set':
        plain.set(body.key, body.value);
        return { status: 204 };
      case '/api/v1/delete':
        return plain.delete(body.key) ? { status: 204 } : err(404, 'KEY_NOT_FOUND', 'not found');
      case '/api/v1/entity/get': {
        const m = entities.get(body.entityName);
        return m?.has(body.key) ? { status: 200, json: { key: body.key, value: m.get(body.key) } } : err(404, 'KEY_NOT_FOUND', 'not found');
      }
      case '/api/v1/entity/set': {
        const def = defs[body.entityName];
        if (!def) return err(400, 'ENTITY_NOT_DEFINED', `entity ${body.entityName} is not in the manifest`);
        for (const [k, v] of Object.entries(body.value)) {
          if (!def.attributes[k]) return err(400, 'INVALID_ATTRIBUTE', `attribute ${k} is not declared on ${body.entityName}`);
          if (!typeOk(def.attributes[k].type, v)) return err(400, 'INVALID_ATTRIBUTE_TYPE', `${body.entityName}.${k} must be ${def.attributes[k].type}, got ${JSON.stringify(v)}`);
        }
        if (!entities.has(body.entityName)) entities.set(body.entityName, new Map());
        const m = entities.get(body.entityName);
        if (body.options?.keyPolicy === 'FAIL_IF_EXISTS' && m.has(body.key)) return err(409, 'KEY_ALREADY_EXISTS', `${body.key} exists`);
        m.set(body.key, body.value);
        return { status: 204 };
      }
      case '/api/v1/entity/query': {
        const def = defs[body.entityName];
        const index = def?.indexes?.find((i) => (typeof i === 'string' ? i : i.name) === body.indexName);
        if (!index) return err(400, 'INDEX_NOT_DEFINED', `index ${body.indexName} is not declared on ${body.entityName}`);
        const partition = index.partition ?? [];
        if ((body.partition ?? []).length !== partition.length) return err(400, 'INVALID_PARTITION', 'partition values must match the index');
        const rangeAttr = index.range[0];
        let rows = [...(entities.get(body.entityName) ?? new Map())].filter(([, v]) => partition.every((a, n) => v[a] === body.partition[n]));
        if (body.range) {
          const [a, b] = body.range.values;
          const test = { BETWEEN: (x) => x >= a && x <= b, EQUAL_TO: (x) => x === a, GREATER_THAN: (x) => x > a, GREATER_THAN_EQUAL_TO: (x) => x >= a, LESS_THAN: (x) => x < a, LESS_THAN_EQUAL_TO: (x) => x <= a, BEGINS_WITH: (x) => String(x).startsWith(a) }[body.range.condition];
          rows = rows.filter(([, v]) => test(v[rangeAttr]));
        }
        rows.sort(([ka, va], [kb, vb]) => (va[rangeAttr] < vb[rangeAttr] ? -1 : va[rangeAttr] > vb[rangeAttr] ? 1 : ka < kb ? -1 : 1));
        if (body.sort === 'DESC') rows.reverse();
        const start = body.cursor ? Number(body.cursor) : 0;
        const limit = Math.min(body.limit ?? 10, 100);
        const page = rows.slice(start, start + limit);
        const more = start + page.length < rows.length;
        return { status: 200, json: { data: page.map(([key, value]) => ({ key, value })), ...(more ? { cursor: String(start + page.length) } : {}) } };
      }
      default:
        return err(501, 'NOT_MODELLED', `kvs ${op}`);
    }
  }
  const writes = () => ops.filter((o) => /\/(set|delete|transaction)$|batch\/(set|delete)/.test(o.op));
  return { plain, entities, ops, writes, handle };
}

function createPlatform({ site, manifest = loadManifest() }) {
  const kvs = createKvs(manifest);
  const queue = [];
  const queuePushes = [];
  const scopes = manifest.permissions?.scopes ?? [];
  const toResponse = ({ status, json, headers = {} }) =>
    new Response(json === undefined || status === 204 ? null : JSON.stringify(json), { status, headers: { 'content-type': 'application/json', ...headers } });

  global.__forge_fetch__ = async (target, p, init = {}) => {
    const store = als.getStore();
    const body = init.body ? JSON.parse(init.body) : undefined;
    if (target.type === 'kvs') {
      if (!scopes.includes('storage:app')) return toResponse({ status: 403, json: { code: 'SCOPE_MISSING', message: 'storage:app is not declared' } });
      return toResponse(kvs.handle(p, body));
    }
    if (target.type === 'fpp' && target.remote === 'stargate' && p.startsWith('/webhook/queue/publish/')) {
      for (const e of body.payload) queue.push({ queueName: body.queueName, jobId: body.jobId, eventId: crypto.randomUUID(), body: e.body });
      queuePushes.push(body);
      return toResponse({ status: 201, json: {} });
    }
    if (target.type === 'fpp' && target.remote === 'jira') {
      if (target.provider === 'user' && !store?.aaid) return toResponse({ status: 401, json: { code: 'NEEDS_AUTHENTICATION_ERR', message: 'asUser() has no user in this invocation' } });
      return toResponse(site.handle({ as: target.provider === 'user' ? 'user' : 'app', accountId: store?.aaid, method: (init.method ?? 'GET').toUpperCase(), path: p, body }));
    }
    return toResponse({ status: 501, json: { code: 'NOT_MODELLED', message: `${JSON.stringify(target)} ${p}` } });
  };
  globalThis.__forgeNativeFetch ??= globalThis.fetch;
  Object.defineProperty(global, '__forge_runtime__', { configurable: true, get: () => als.getStore()?.runtime });

  let bundleDir;
  const mods = {};
  async function build(outDir) {
    bundleDir = await bundle(outDir);
  }
  function handlerFor(functionKey) {
    const fn = manifest.modules.function.find((f) => f.key === functionKey);
    const [file, exportName] = fn.handler.split('.');
    mods[file] ??= require(path.join(bundleDir, `${file}.cjs`));
    if (typeof mods[file][exportName] !== 'function') throw new Error(`${fn.handler} is not exported`);
    return mods[file][exportName];
  }
  function invoke(functionKey, event, { aaid, moduleKey } = {}, context = {}) {
    const runtime = {
      appContext: { appId: 'golden', environmentId: 'dev', environmentType: 'DEVELOPMENT', appVersion: '1.0.0', invocationId: crypto.randomUUID(), installationId: 'inst', moduleKey, functionKey },
      contextAri: 'ari:cloud:jira::site/golden',
      proxy: { token: 'golden', url: 'golden', host: 'golden' },
      tracing: { traceId: 't', spanId: 's' },
      lambdaContext: { getRemainingTimeInMillis: () => 25000 },
      metrics: { counter: () => ({ incr() {} }), timing: () => ({ measure: () => ({ stop() {} }) }) },
      aaid,
    };
    return als.run({ runtime, aaid }, () => handlerFor(functionKey)(event, context));
  }
  const fnOf = (type, key) => {
    const m = manifest.modules[type].find((x) => x.key === key);
    return m.function ?? m.resolver?.function;
  };

  async function resolver(moduleType, moduleKey, functionKey, payload, { aaid, extension }) {
    const context = { accountId: aaid, cloudId: 'golden', localId: `${moduleKey}-local`, moduleKey, extension };
    const res = await invoke(fnOf(moduleType, moduleKey), { call: { functionKey, payload }, context }, { aaid, moduleKey }, { principal: { accountId: aaid }, installContext: 'ari:cloud:jira::site/golden' });
    return JSON.parse(JSON.stringify(res ?? {}));
  }

  // Drain the queue into the consumer. Retry requests are redelivered (virtual clock: recorded,
  // not slept); thrown errors are redelivered too.
  async function drain({ onDeliver } = {}) {
    const deliveries = [];
    const consumer = manifest.modules.consumer[0];
    let guard = 0;
    while (queue.length) {
      if ((guard += 1) > 5000) throw new Error('queue never drained');
      const ev = queue.shift();
      const attempt = (ev.attempt ?? 0) + 1;
      const event = { body: ev.body, queueName: ev.queueName, jobId: ev.jobId, eventId: ev.eventId, ...(attempt > 1 ? { retryContext: { retryCount: attempt - 1, retryReason: ev.retryReason } } : {}) };
      let result;
      let error;
      try {
        result = await invoke(consumer.function, event, { moduleKey: consumer.key });
      } catch (e) {
        error = e;
      }
      const d = { eventId: ev.eventId, attempt, body: ev.body, result, error: error?.message };
      deliveries.push(d);
      onDeliver?.(d);
      if (result && result._retry) queue.push({ ...ev, attempt, retryReason: result.retryOptions.retryReason, retryAfter: result.retryOptions.retryAfter });
      else if (error) queue.push({ ...ev, attempt });
    }
    return deliveries;
  }

  return { manifest, kvs, queue, queuePushes, build, invoke, resolver, drain, fnOf };
}

module.exports = { createPlatform, loadManifest, bundle, APP };
