// Local emulator of the Forge platform PROXY — the HTTP server the real Forge node runtime
// wrapper talks to (see README: wrapper.js `nodeRuntimeFetch` builds `${proxy.url}${route}` and
// puts the product path in the `forge-proxy-target` header). Both runtime tiers use it:
//   tier A (runtime-shim.cjs) installs a 40-line global.__forge_fetch__ that speaks this protocol;
//   tier B (real-wrapper.cjs) runs Atlassian's own wrapper.js pointed at this server.
// Anything the emulator does not model answers 501 with a named reason and is recorded — never a
// silent 200.
const http = require('http');
// Captured at load: tier A replaces globalThis.fetch with the egress door in the same process.
const nativeFetch = globalThis.fetch;

function createForgeProxy({ jiraUrl, manifest }) {
  const log = [];          // every proxied call, in order
  const kvs = new Map();   // key -> { value, createdAt, updatedAt }
  const secrets = new Map();
  const entities = new Map(); // `${entity}\u0000${key}` -> value
  const queue = [];        // pushed events awaiting the consumer
  const unmodelled = [];   // 501s — the scorer prints these loudly
  const egressAllow = (manifest?.permissions?.external?.fetch?.backend ?? []).map((e) => (typeof e === 'string' ? e : e.address));

  const json = (res, status, obj) => { res.writeHead(status, { 'content-type': 'application/json' }); res.end(obj === undefined ? '' : JSON.stringify(obj)); };
  const notModelled = (res, what) => { unmodelled.push(what); json(res, 501, { code: 'EMULATOR_NOT_MODELLED', message: what }); };

  function kvsOp(op, body, res) {
    const now = Date.now();
    const meta = (k, e) => ({ key: k, value: e.value, createdAt: e.createdAt, updatedAt: e.updatedAt });
    const notFound = () => json(res, 404, { code: 'KEY_NOT_FOUND', message: `key ${body.key} not found` });
    const put = (m, k, v) => { const prev = m.get(k); m.set(k, { value: v, createdAt: prev?.createdAt ?? now, updatedAt: now }); return meta(k, m.get(k)); };
    switch (op) {
      case '/api/v1/get': return kvs.has(body.key) ? json(res, 200, meta(body.key, kvs.get(body.key))) : notFound();
      case '/api/v1/set': return json(res, 200, put(kvs, body.key, body.value));
      case '/api/v1/delete': return kvs.delete(body.key) ? json(res, 204) : notFound();
      case '/api/v1/secret/get': return secrets.has(body.key) ? json(res, 200, meta(body.key, secrets.get(body.key))) : notFound();
      case '/api/v1/secret/set': return json(res, 200, put(secrets, body.key, body.value));
      case '/api/v1/secret/delete': return secrets.delete(body.key) ? json(res, 204) : notFound();
      case '/api/v1/entity/get': { const k = `${body.entityName}\u0000${body.key}`; return entities.has(k) ? json(res, 200, meta(body.key, entities.get(k))) : notFound(); }
      case '/api/v1/entity/set': return json(res, 200, put(entities, `${body.entityName}\u0000${body.key}`, body.value));
      case '/api/v1/entity/delete': return entities.delete(`${body.entityName}\u0000${body.key}`) ? json(res, 204) : notFound();
      case '/api/v1/query': {
        const w = body.where?.[0];
        let keys = [...kvs.keys()].sort();
        if (w?.property === 'key' && w.condition === 'BEGINS_WITH') keys = keys.filter((k) => k.startsWith(w.values[0]));
        const start = body.after ? keys.indexOf(body.after) + 1 : 0;
        const page = keys.slice(start, start + (body.limit ?? 10));
        const more = start + page.length < keys.length;
        return json(res, 200, { data: page.map((k) => ({ key: k, value: kvs.get(k).value })), cursor: more ? page.at(-1) : undefined });
      }
      default: return notModelled(res, `kvs ${op}`);
    }
  }

  function stargate(target, body, res, ctx) {
    if (target === '/graphql' && /createWebTriggerUrl/.test(body?.query ?? ''))
      return json(res, 200, { data: { createWebTriggerUrl: { url: `${ctx.selfUrl}/x1/${body.variables.input.triggerKey}` } } });
    if (target.startsWith('/webhook/queue/publish/')) {
      for (const p of body.payload) queue.push({ queueName: body.queueName, jobId: body.jobId, body: p.body ?? p });
      return json(res, 201, {}); // @forge/events validators.js:107 expects 201
    }
    return notModelled(res, `stargate ${target}`);
  }

  const server = http.createServer((req, res) => {
    let raw = '';
    req.on('data', (c) => (raw += c));
    req.on('end', async () => {
      const target = req.headers['forge-proxy-target'];
      const auth = req.headers['forge-proxy-authorization'];
      const body = raw ? (() => { try { return JSON.parse(raw); } catch { return raw; } })() : undefined;
      const route = new URL(req.url, 'http://x').pathname;
      log.push({ route, method: req.method, target, authed: Boolean(auth), body });
      const selfUrl = `http://127.0.0.1:${server.address().port}`;
      let m;
      if ((m = route.match(/^\/fpp\/provider\/(app|user|none)\/remote\/(jira|confluence|bitbucket|stargate)(?:\/account\/(.+))?$/))) {
        const [, as, remote] = m;
        if (remote === 'stargate') return stargate(target, body, res, { selfUrl });
        if (remote !== 'jira') return notModelled(res, `product ${remote}`);
        const r = await nativeFetch(new URL(target, jiraUrl), { method: req.method, headers: { 'content-type': req.headers['content-type'] ?? 'application/json', 'x-forge-as': as }, body: raw || undefined });
        res.writeHead(r.status, { 'content-type': r.headers.get('content-type') ?? 'application/json' });
        return res.end(Buffer.from(await r.arrayBuffer()));
      }
      if (route === '/fpp/as/app/provider/atlassian/capability/kvs') return kvsOp(target, body, res);
      if (route === '/egress') {
        const host = new URL(target).host;
        if (!egressAllow.some((a) => a === '*' || new URL(a.includes('://') ? a : `https://${a}`).host === host)) {
          res.writeHead(403, { 'forge-proxy-error': 'EGRESS_NOT_ALLOWED' }); return res.end();
        }
        return notModelled(res, `egress ${target}`);
      }
      if (route === '/logs') return json(res, 200, {});
      return notModelled(res, `route ${route}`);
    });
  });
  return {
    server, log, kvs, queue, unmodelled,
    listen: (port = 0) => new Promise((ok) => server.listen(port, '127.0.0.1', () => ok(`http://127.0.0.1:${server.address().port}`))),
  };
}

module.exports = { createForgeProxy };
