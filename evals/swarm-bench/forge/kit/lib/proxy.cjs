'use strict';
// The Forge platform proxy, emulated: the HTTP server Atlassian's runtime wrapper talks to. The wrapper
// builds `${proxy.url}${route}` and sends the product path in `forge-proxy-target` with
// `forge-proxy-authorization: Bearer <token>` (wrapper.js nodeRuntimeFetch). Each invocation gets its own
// token, so every call is attributed to the invocation, its module and its user.
//
// Routes: /fpp/provider/{app|user|none}/remote/{jira|confluence|bitbucket|stargate}  (product + queue)
//         /fpp/as/app/provider/atlassian/capability/kvs                          (KVS, kvs.cjs)
//         /egress (permissions.external.fetch.backend) and /logs.
// Anything else answers 501 EMULATOR_NOT_MODELLED and is recorded as harness_missing, never a silent 200.
const http = require('http');

const REQUEST_STATS = /^\/webhook\/queue\/stats\//;
const REQUEST_CANCEL = /^\/webhook\/queue\/cancel\//;
const REQUEST_PUSH = /^\/webhook\/queue\/publish\//;

function decodeToken(header) {
  const m = /^Bearer\s+(.+)$/.exec(header ?? '');
  if (!m) return null;
  try { return JSON.parse(Buffer.from(m[1].split('.')[1], 'base64url').toString()); } catch { return null; }
}

function createProxy({ siteUrl, manifest, kvs, queue, invocations, clock, log = [], harnessMissing = [] }) {
  const scopes = manifest?.permissions?.scopes ?? [];
  const egressAllow = (manifest?.permissions?.external?.fetch?.backend ?? []).map((e) => (typeof e === 'string' ? e : e?.address)).filter(Boolean);
  const vnow = () => new Date(clock.now()).toISOString();
  const record = (entry) => { const e = { t_virtual: vnow(), ...entry }; log.push(e); return e; };
  const missing = (what, inv) => harnessMissing.push({ what, at: vnow(), invocationId: inv?.id ?? null, moduleType: inv?.moduleType ?? null });

  // One product request (Jira), shared by the wrapper route and the Custom UI host's fetchProduct.
  async function productFetch({ inv, provider, product, method, path, headers = {}, body }) {
    const entry = record({ invocationId: inv?.id ?? null, moduleType: inv?.moduleType ?? null, moduleKey: inv?.moduleKey ?? null,
      functionKey: inv?.functionKey ?? null, source: inv?.source ?? 'function', service: product, provider, method, path, body: parseMaybe(body) });
    if (product !== 'jira') {
      entry.status = 404;
      return { status: 404, headers: { 'content-type': 'application/json' }, body: JSON.stringify({ message: `This site has no ${product} product installed.` }) };
    }
    if (provider === 'user' && !inv?.aaid) {
      entry.status = 401;
      entry.needsAuthentication = true;
      return { status: 401, headers: { 'forge-proxy-error': 'NEEDS_AUTHENTICATION_ERR' }, body: '' };
    }
    if (!/^\/rest\//.test(path)) {
      entry.status = 404;
      return { status: 404, headers: { 'content-type': 'application/json' }, body: JSON.stringify({ errorMessages: [`No resource matches ${method} ${path}`], errors: {} }) };
    }
    const fwd = {
      'content-type': headers['content-type'] ?? 'application/json',
      'x-forge-as': provider,
      'x-forge-scopes': JSON.stringify(scopes),
      'x-forge-source': inv?.source ?? 'function',
    };
    if (provider === 'user') fwd['x-forge-account'] = inv.aaid;
    if (inv?.id) fwd['x-forge-invocation'] = inv.id;
    if (inv?.moduleType) fwd['x-forge-module-type'] = inv.moduleType;
    if (inv?.moduleKey) fwd['x-forge-module-key'] = inv.moduleKey;
    if (inv?.scheduledRun) fwd['x-forge-scheduled-run'] = String(inv.scheduledRun);
    if (inv?.originChange) fwd['x-forge-origin-change'] = inv.originChange;
    const started = Date.now();
    const r = await fetch(new URL(path, siteUrl), { method, headers: fwd, body: method === 'GET' || method === 'HEAD' ? undefined : body });
    const text = await r.text();
    entry.status = r.status;
    entry.ms = Date.now() - started;
    if (r.status === 429) entry.retryAfter = Number(r.headers.get('retry-after'));
    if (r.headers.get('x-forge-site-fault')) {
      entry.fault = r.headers.get('x-forge-site-fault');
      entry.earlyRetry = r.headers.get('x-forge-site-fault-kind') === 'early_retry';
    }
    if (r.headers.get('x-forge-site-scopes')) entry.scopes = JSON.parse(r.headers.get('x-forge-site-scopes'));
    if (r.headers.get('x-forge-site-op')) entry.op = r.headers.get('x-forge-site-op');
    if (r.status === 501) missing(`product ${method} ${path}`, inv);
    // The response as the app saw it (graders judge pagination and leaks from it) and a page summary.
    const parsed = parseMaybe(text);
    entry.response = parsed;
    if (parsed && typeof parsed === 'object') {
      const issues = Array.isArray(parsed.issues) ? parsed.issues : Array.isArray(parsed.values) ? parsed.values : null;
      entry.page = { nextPageToken: parsed.nextPageToken ?? null, isLast: parsed.isLast ?? null, startAt: parsed.startAt ?? null,
        maxResults: parsed.maxResults ?? null, total: parsed.total ?? null,
        ids: issues ? issues.map((x) => String(x.id ?? x.key ?? '')) : Array.isArray(parsed.issueChangeLogs) ? parsed.issueChangeLogs.map((x) => String(x.issueId)) : null,
        histories: Array.isArray(parsed.issueChangeLogs) ? parsed.issueChangeLogs.reduce((n, x) => n + (x.changeHistories?.length ?? 0), 0)
          : Array.isArray(parsed.histories) ? parsed.histories.length : null };
    }
    const out = {};
    for (const h of ['content-type', 'retry-after', 'ratelimit-reason']) if (r.headers.get(h)) out[h] = r.headers.get(h);
    return { status: r.status, headers: out, body: text };
  }

  function stargate(inv, target, body) {
    if (REQUEST_PUSH.test(target)) {
      const res = queue.push(inv, body);
      record({ invocationId: inv.id, moduleType: inv.moduleType, moduleKey: inv.moduleKey, functionKey: inv.functionKey, service: 'queue', provider: 'app',
        method: 'POST', path: target, body: { queueName: body.queueName, jobId: body.jobId, payload: body.payload }, status: res.status });
      return res;
    }
    if (REQUEST_STATS.test(target) || REQUEST_CANCEL.test(target)) {
      const res = REQUEST_STATS.test(target) ? queue.stats(body) : queue.cancel(body);
      record({ invocationId: inv.id, moduleType: inv.moduleType, moduleKey: inv.moduleKey, service: 'queue', provider: 'app', method: 'POST', path: target, body, status: res.status });
      return res;
    }
    missing(`stargate ${target}`, inv);
    record({ invocationId: inv.id, moduleType: inv.moduleType, moduleKey: inv.moduleKey, service: 'stargate', provider: 'app', method: 'POST', path: target, body, status: 501 });
    return { status: 501, body: { code: 'EMULATOR_NOT_MODELLED', message: `stargate ${target}` } };
  }

  function kvsCall(inv, op, body) {
    const entry = record({ invocationId: inv?.id ?? null, moduleType: inv?.moduleType ?? null, moduleKey: inv?.moduleKey ?? null,
      functionKey: inv?.functionKey ?? null, service: 'kvs', provider: 'app', method: 'POST', path: op, body });
    if (!scopes.includes('storage:app')) {
      entry.status = 403;
      entry.missingScope = 'storage:app';
      return { status: 403, body: { code: 'FORBIDDEN', message: "The app does not have the 'storage:app' scope required to use Forge storage." } };
    }
    const res = kvs.handle(op, body);
    entry.status = res.status;
    if (res.error) { entry.kvsError = res.error; if (res.error.limit) entry.limitError = res.error.code; }
    if (res.notModelled) missing(`kvs ${op}`, inv);
    return res;
  }

  const server = http.createServer((req, res) => {
    const chunks = [];
    req.on('data', (c) => chunks.push(c));
    req.on('end', async () => {
      const raw = Buffer.concat(chunks).toString('utf8');
      const send = (status, body, headers = {}) => {
        res.writeHead(status, { 'content-type': 'application/json', ...headers });
        res.end(body === undefined ? '' : typeof body === 'string' ? body : JSON.stringify(body));
      };
      try {
        const route = new URL(req.url, 'http://proxy').pathname;
        const target = req.headers['forge-proxy-target'];
        const claims = decodeToken(req.headers['forge-proxy-authorization']);
        const inv = claims ? invocations.get(claims.inv) : null;
        if (route === '/logs') return send(200, {});
        if (!inv) return send(401, { code: 'UNAUTHENTICATED', message: 'unknown or missing proxy token' }, { 'forge-proxy-error': 'PROXY_ERR' });
        let m;
        if ((m = route.match(/^\/fpp\/provider\/(app|user|none)\/remote\/(jira|confluence|bitbucket|stargate)(?:\/account\/(.+))?$/))) {
          const [, provider, remote] = m;
          if (remote === 'stargate') {
            const r = stargate(inv, target, parseMaybe(raw));
            return send(r.status, r.body);
          }
          const r = await productFetch({ inv, provider, product: remote, method: req.method, path: target, headers: req.headers, body: raw || undefined });
          return send(r.status, r.body, r.headers);
        }
        if (route === '/fpp/as/app/provider/atlassian/capability/kvs') {
          const r = kvsCall(inv, target, parseMaybe(raw));
          return send(r.status, r.body);
        }
        if (route === '/egress') {
          let host = null;
          try { host = new URL(target).host; } catch { host = null; }
          const allowed = host && egressAllow.some((a) => a === '*' || safeHost(a) === host || (a.startsWith('*.') && host.endsWith(a.slice(1))));
          record({ invocationId: inv.id, moduleType: inv.moduleType, moduleKey: inv.moduleKey, service: 'egress', provider: 'app', method: req.method, path: target, status: allowed ? 502 : 403 });
          if (!allowed) return send(403, '', { 'forge-proxy-error': 'REQUEST_EGRESS_ALLOWLIST_ERR' });
          return send(502, { message: 'The benchmark harness has no internet: declared egress cannot be reached.' });
        }
        missing(`proxy route ${route}`, inv);
        record({ invocationId: inv.id, moduleType: inv.moduleType, moduleKey: inv.moduleKey, service: 'unknown', provider: 'app', method: req.method, path: route, status: 501 });
        return send(501, { code: 'EMULATOR_NOT_MODELLED', message: `proxy route ${route}` });
      } catch (e) {
        send(500, { code: 'PROXY_CRASH', message: String(e.message) });
      }
    });
  });

  return {
    log, harnessMissing, productFetch, kvsCall,
    listen: () => new Promise((ok) => server.listen(0, '127.0.0.1', () => ok({ url: `http://127.0.0.1:${server.address().port}`, port: server.address().port }))),
    close: () => new Promise((ok) => server.close(() => ok())),
  };
}

function parseMaybe(raw) {
  if (raw === undefined || raw === null || raw === '') return undefined;
  if (typeof raw !== 'string') return raw;
  try { return JSON.parse(raw); } catch { return raw; }
}
function safeHost(a) { try { return new URL(a.includes('://') ? a : `https://${a}`).host; } catch { return null; } }

module.exports = { createProxy, decodeToken };
