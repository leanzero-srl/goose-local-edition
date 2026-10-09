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
const YAML = require(process.env.GOLDEN_YAML_MODULE ?? path.join(__dirname, '..', '..', '..', 'forge', 'spike', 'node_modules', 'yaml'));
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
  const secrets = new Map();
  const entities = new Map(); // name -> Map(key -> value)
  const defs = Object.fromEntries((manifest.app.storage?.entities ?? []).map((e) => [e.name, e]));
  const ops = [];
  const typeOk = (t, v) => (t === 'any' ? true : t === 'string' ? typeof v === 'string' : t === 'boolean' ? typeof v === 'boolean' : t === 'integer' ? Number.isInteger(v) && v >= -2147483648 && v <= 2147483647 : t === 'float' ? typeof v === 'number' : false);
  const err = (status, code, message) => ({ status, json: { code, message } });
  function handle(op, body) {
    ops.push({ op, body });
    switch (op) {
      case '/api/v1/get':
        return plain.has(body.key) ? { status: 200, json: { key: body.key, value: plain.get(body.key) } } : err(404, 'KEY_NOT_FOUND', 'not found');
      case '/api/v1/set':
        if (body.options?.keyPolicy === 'FAIL_IF_EXISTS' && plain.has(body.key)) return err(409, 'KEY_CONFLICT', `${body.key} exists`);
        plain.set(body.key, body.value);
        return { status: 204 };
      case '/api/v1/secret/get':
        return secrets.has(body.key) ? { status: 200, json: { key: body.key, value: secrets.get(body.key) } } : err(404, 'KEY_NOT_FOUND', 'not found');
      case '/api/v1/secret/set':
        secrets.set(body.key, body.value);
        return { status: 204 };
      case '/api/v1/query': {
        const w = body.where?.[0];
        let rows = [...plain].filter(([k]) => !w || (w.condition === 'BEGINS_WITH' ? k.startsWith(w.values[0]) : true)).sort(([a], [b]) => (a < b ? -1 : 1));
        const start = body.cursor ? Number(body.cursor) : 0;
        const limit = Math.min(body.limit ?? 10, 100);
        const page = rows.slice(start, start + limit);
        const more = start + page.length < rows.length;
        return { status: 200, json: { data: page.map(([key, value]) => ({ key, value })), ...(more ? { cursor: String(start + page.length) } : {}) } };
      }
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
        if (body.options?.keyPolicy === 'FAIL_IF_EXISTS' && m.has(body.key)) return err(409, 'KEY_CONFLICT', `${body.key} exists`);
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
  const entityWrites = () => ops.filter((o) => /^\/api\/v1\/entity\/(set|delete)$/.test(o.op));
  return { plain, secrets, entities, ops, writes, entityWrites, handle };
}

// Default scripted model: explains with words only and names the first three changes it was sent.
function defaultLlmReply(req) {
  const input = JSON.parse(req.messages.find((m) => m.role === 'user').content[0].text);
  return {
    choices: [
      {
        index: 0,
        finish_reason: 'tool_use',
        message: {
          role: 'assistant',
          content: [],
          tool_calls: [
            {
              id: 'toolu_1',
              type: 'function',
              index: 0,
              function: { name: 'report_scope', arguments: { summary: 'Most of the growth came from work pulled in mid-sprint.', changeIds: input.changes.slice(0, 3).map((c) => c.changeId) } },
            },
          ],
        },
      },
    ],
  };
}

function createPlatform({ site, manifest = loadManifest(), clock = null }) {
  const kvs = createKvs(manifest);
  // Forge LLM (scripted, like the dev site's): models the list() answer, then each chat() takes the next
  // scripted reply (a function of the request) or a default that calls report_scope.
  const llm = {
    models: [
      { model: 'claude-opus-4-6', status: 'deprecated' },
      { model: 'claude-sonnet-4-7', status: 'active' },
      { model: 'claude-haiku-4-7', status: 'active' },
    ],
    script: [],
    calls: [],
  };
  // Forge Realtime: signed tokens are unsigned JWT-shaped strings @forge/realtime can decode; every
  // publish is recorded with the module type that made it. publish() (context channel) is refused from
  // async invocations, as the realtime docs state.
  const realtime = { published: [], signed: [], subscribers: [] };
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
    if (target.type === 'llm') {
      if (!manifest.modules.llm?.length) return toResponse({ status: 403, json: { code: 'LLM_MODULE_MISSING', message: 'no llm module in the manifest' } });
      if ((init.method ?? 'GET').toUpperCase() === 'GET') return toResponse({ status: 200, json: { models: llm.models } });
      const model = decodeURIComponent(p.split('/').pop());
      llm.calls.push({ model, body, functionKey: store?.runtime?.appContext?.functionKey });
      const next = llm.script.shift() ?? ((req) => defaultLlmReply(req));
      const reply = next(body, model);
      return toResponse(reply.status ? reply : { status: 200, json: reply });
    }
    if (target.type === 'realtime') {
      const b64 = (o) => Buffer.from(JSON.stringify(o)).toString('base64url');
      if (/signRealtimeToken/.test(body.query)) {
        if (['consumer', 'scheduledTrigger', 'trigger'].includes(store?.moduleType)) return toResponse({ status: 200, json: { errors: [{ message: 'signRealtimeToken is available in resolvers only' }] } });
        const { channelName, claims, permissions } = body.variables;
        const exp = Math.floor(Date.now() / 1000) + 3600;
        const jwt = `${b64({ alg: 'none' })}.${b64({ channel: { name: channelName }, claims, permissions, exp })}.sig`;
        realtime.signed.push({ channelName, claims, permissions, moduleType: store?.moduleType });
        return toResponse({ status: 200, json: { data: { ecosystem: { signRealtimeToken: { success: true, forgeRealtimeToken: { jwt, expiresAt: exp } } } } } });
      }
      const v = body.variables;
      const asyncCtx = ['consumer', 'scheduledTrigger', 'trigger'].includes(store?.moduleType);
      if (!v.isGlobal && asyncCtx) return toResponse({ status: 200, json: { errors: [{ message: 'publish() needs a product context; use publishGlobal() in async functions' }] } });
      const ev = { channel: v.name, payload: JSON.parse(v.payload), isGlobal: v.isGlobal, token: v.token, moduleType: store?.moduleType, at: Date.now() };
      realtime.published.push(ev);
      for (const s of realtime.subscribers) s(ev);
      return toResponse({ status: 200, json: { data: { ecosystem: { publishRealtimeChannel: { eventId: String(realtime.published.length), eventTimestamp: new Date().toISOString() } } } } });
    }
    if (target.type === 'fpp' && target.remote === 'stargate' && p.startsWith('/webhook/queue/publish/')) {
      for (const e of body.payload) queue.push({ queueName: body.queueName, jobId: body.jobId, eventId: crypto.randomUUID(), body: e.body, notBefore: Date.now() + (e.delayInSeconds ?? 0) * 1000 });
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
  const moduleTypeOf = (moduleKey) => Object.entries(manifest.modules).find(([, es]) => es.some((e) => e.key === moduleKey))?.[0];
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
    return als.run({ runtime, aaid, moduleType: moduleTypeOf(moduleKey) }, () => handlerFor(functionKey)(event, context));
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
      // Deliver the earliest due event; a retry request or a delayed push is due after its wait, and the
      // virtual clock jumps over it (as the benchmark's kit does).
      queue.sort((a, b) => (a.notBefore ?? 0) - (b.notBefore ?? 0));
      const ev = queue.shift();
      if ((ev.notBefore ?? 0) > Date.now()) clock?.set(ev.notBefore);
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
      if (result && result._retry) queue.push({ ...ev, attempt, retryReason: result.retryOptions.retryReason, retryAfter: result.retryOptions.retryAfter, notBefore: Date.now() + result.retryOptions.retryAfter * 1000 });
      else if (error) queue.push({ ...ev, attempt, notBefore: Date.now() + 60000 });
    }
    return deliveries;
  }

  return { manifest, kvs, queue, queuePushes, build, invoke, resolver, drain, fnOf, llm, realtime };
}

module.exports = { createPlatform, loadManifest, bundle, APP };
