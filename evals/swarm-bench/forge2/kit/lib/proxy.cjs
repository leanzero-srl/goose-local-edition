'use strict';
// The Forge platform proxy, emulated: the HTTP server Atlassian's runtime wrapper talks to. The wrapper
// builds `${proxy.url}${route}` and sends the product path in `forge-proxy-target` with
// `forge-proxy-authorization: Bearer <token>` (wrapper.js nodeRuntimeFetch). Each invocation gets its own
// token, so every call is attributed to the invocation, its module and its user.
//
// Routes: /fpp/provider/{app|user|none}/remote/{jira|confluence|bitbucket|stargate}  (product + queue)
//         /fpp/as/app/provider/atlassian/capability/kvs                          (KVS, kvs.cjs)
//         /egress (permissions.external.fetch.backend) and /logs.
//         /llm/ and /llm/<model>                                                 (Forge LLM -> the site's llm.cjs)
//         /fpp/as/app/provider/atlassian/capability/realtime                     (Realtime GraphQL -> realtime.cjs)
// The last two routes are what the pinned wrapper was MEASURED to call (2026-10-03, see site/llm.cjs and
// site/realtime.cjs for the recorded requests).
//         /x/webtrigger/<moduleKey>   PUBLIC (no token): a web trigger's URL; the request goes to the emulator's
//                                     ingress (lib/webtrigger.cjs). `webTrigger.getUrl(key)` answers this URL.
// Anything else answers 501 EMULATOR_NOT_MODELLED and is recorded as harness_missing, never a silent 200.
//
// Virtual time (clock.cjs): every request of an invocation carries its virtual send time (`forge-vclock`); the proxy
// charges the request's class cost, answers the completion time in the same header, keeps the site's clock at least at
// the send time before a product request reaches it (so rate windows and per-issue write spacing see the invocation's
// time), and logs `t_virtual` (send) and `vcostMs` for every call.
const http = require('http');
const { costOf, VCLOCK_HEADER } = require('./clock.cjs');

const KVS_ROUTE = '/fpp/as/app/provider/atlassian/capability/kvs';
const WEBTRIGGER_ROUTE = /^\/x\/webtrigger\/([^/]+)$/;

const STREAM_READ_GAP_MS = 20; // transport: a pause long enough for two writes to arrive as two socket reads
const REQUEST_STATS = /^\/webhook\/queue\/stats\//;
const REQUEST_CANCEL = /^\/webhook\/queue\/cancel\//;
const REQUEST_PUSH = /^\/webhook\/queue\/publish\//;

function decodeToken(header) {
  const m = /^Bearer\s+(.+)$/.exec(header ?? '');
  if (!m) return null;
  try { return JSON.parse(Buffer.from(m[1].split('.')[1], 'base64url').toString()); } catch { return null; }
}

function createProxy({ siteUrl, siteCall, manifest, kvs, queue, invocations, clock, log = [], harnessMissing = [], webtrigger = null }) {
  const llmModules = manifest?.modules?.llm ?? [];
  const webtriggerKeys = new Set((manifest?.modules?.webtrigger ?? []).map((w) => w?.key).filter(Boolean));
  const scopes = manifest?.permissions?.scopes ?? [];
  const egressAllow = (manifest?.permissions?.external?.fetch?.backend ?? []).map((e) => (typeof e === 'string' ? e : e?.address)).filter(Boolean);
  let selfUrl = null;
  const iso = (t) => new Date(t ?? clock.now()).toISOString();
  // `tv`: the virtual time the call was made (an invocation's send time); frontend calls use the emulator's clock.
  const record = (entry, tv) => { const e = { t_virtual: iso(tv), ...entry }; log.push(e); return e; };
  const missing = (what, inv) => harnessMissing.push({ what, at: iso(), invocationId: inv?.id ?? null, moduleType: inv?.moduleType ?? null });

  // One product request (Jira), shared by the wrapper route and the Custom UI host's fetchProduct.
  async function productFetch({ inv, provider, product, method, path, headers = {}, body, tv, vcostMs }) {
    const entry = record({ invocationId: inv?.id ?? null, moduleType: inv?.moduleType ?? null, moduleKey: inv?.moduleKey ?? null,
      functionKey: inv?.functionKey ?? null, source: inv?.source ?? 'function', service: product, provider, method, path, body: parseMaybe(body),
      ...(vcostMs !== undefined ? { vcostMs } : {}) }, tv);
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
    if (tv !== undefined) await clock.advanceTo(tv);
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

  function stargate(inv, target, body, tv, vcostMs) {
    if (REQUEST_PUSH.test(target)) {
      const res = queue.push(inv, body, tv);
      record({ invocationId: inv.id, moduleType: inv.moduleType, moduleKey: inv.moduleKey, functionKey: inv.functionKey, service: 'queue', provider: 'app',
        method: 'POST', path: target, body: { queueName: body.queueName, jobId: body.jobId, payload: body.payload }, status: res.status, vcostMs }, tv);
      return res;
    }
    if (REQUEST_STATS.test(target) || REQUEST_CANCEL.test(target)) {
      const res = REQUEST_STATS.test(target) ? queue.stats(body) : queue.cancel(body);
      record({ invocationId: inv.id, moduleType: inv.moduleType, moduleKey: inv.moduleKey, service: 'queue', provider: 'app', method: 'POST', path: target, body, status: res.status, vcostMs }, tv);
      return res;
    }
    // @forge/api webTrigger.getUrl(key): the GraphQL mutation createWebTriggerUrl (api/out/webTrigger.js). The URL is this
    // emulator's public web-trigger route, so a URL the app shows can be called.
    if (target === '/graphql' && /\bcreateWebTriggerUrl\b/.test(String(body?.query ?? ''))) {
      const key = body?.variables?.input?.triggerKey;
      const ok = webtriggerKeys.has(key);
      const res = ok ? { status: 200, body: { data: { createWebTriggerUrl: { url: `${selfUrl}/x/webtrigger/${encodeURIComponent(key)}` } } } }
        : { status: 200, body: { data: null, errors: [{ message: `No web trigger module with key '${key}' in the manifest` }] } };
      record({ invocationId: inv.id, moduleType: inv.moduleType, moduleKey: inv.moduleKey, service: 'webtrigger', provider: 'app', method: 'POST', path: target, op: 'createWebTriggerUrl',
        body: { triggerKey: key }, status: res.status, ...(ok ? {} : { refused: 'unknown web trigger key' }), vcostMs }, tv);
      return res;
    }
    missing(`stargate ${target}`, inv);
    record({ invocationId: inv.id, moduleType: inv.moduleType, moduleKey: inv.moduleKey, service: 'stargate', provider: 'app', method: 'POST', path: target, body, status: 501, vcostMs }, tv);
    return { status: 501, body: { code: 'EMULATOR_NOT_MODELLED', message: `stargate ${target}` } };
  }

  function kvsCall(inv, op, body, tv, vcostMs) {
    const entry = record({ invocationId: inv?.id ?? null, moduleType: inv?.moduleType ?? null, moduleKey: inv?.moduleKey ?? null,
      functionKey: inv?.functionKey ?? null, service: 'kvs', provider: 'app', method: 'POST', path: op, body, ...(vcostMs !== undefined ? { vcostMs } : {}) }, tv);
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
      let timing = {};
      const send = (status, body, headers = {}) => {
        res.writeHead(status, { 'content-type': 'application/json', ...timing, ...headers });
        res.end(body === undefined ? '' : typeof body === 'string' ? body : JSON.stringify(body));
      };
      try {
        const url = new URL(req.url, 'http://proxy');
        const route = url.pathname;
        let m;
        if ((m = route.match(WEBTRIGGER_ROUTE))) {
          const moduleKey = decodeURIComponent(m[1]);
          if (!webtrigger) {
            missing(`webtrigger ingress for ${moduleKey}`, null);
            return send(501, { code: 'EMULATOR_NOT_MODELLED', message: 'no web-trigger ingress is mounted in this emulator (lib/webtrigger.cjs handle)' });
          }
          // Header values as arrays and the raw body string: the shapes a web trigger function receives.
          const headers = {};
          for (let i = 0; i < req.rawHeaders.length; i += 2) (headers[req.rawHeaders[i].toLowerCase()] ??= []).push(req.rawHeaders[i + 1]);
          const queryParameters = {};
          for (const [k, v] of url.searchParams) (queryParameters[k] ??= []).push(v);
          const r = await webtrigger(moduleKey, { method: req.method, path: route, headers, queryParameters, body: raw });
          const out = {};
          for (const [k, v] of Object.entries(r?.headers ?? {})) out[k] = Array.isArray(v) ? v.join(', ') : String(v);
          res.writeHead(Number(r?.statusCode ?? 500), out);
          return res.end(r?.body === undefined || r?.body === null ? '' : typeof r.body === 'string' ? r.body : JSON.stringify(r.body));
        }
        const target = req.headers['forge-proxy-target'];
        const claims = decodeToken(req.headers['forge-proxy-authorization']);
        const inv = claims ? invocations.get(claims.inv) : null;
        if (route === '/logs') return send(200, {});
        if (!inv) return send(401, { code: 'UNAUTHENTICATED', message: 'unknown or missing proxy token' }, { 'forge-proxy-error': 'PROXY_ERR' });
        // The invocation's virtual clock: its send time (or, for a request the agent did not stamp, the latest time the
        // proxy knows for it), plus this request's class cost.
        const stamped = Number(req.headers[VCLOCK_HEADER]);
        const tv = Number.isFinite(stamped) && stamped > 0 ? stamped : (inv.vnow ?? clock.now());
        const vcostMs = costOf({ kvs: route === KVS_ROUTE, method: req.method, path: target ?? route });
        inv.vnow = Math.max(inv.vnow ?? tv, tv + vcostMs);
        clock.observe(tv);
        timing = { [VCLOCK_HEADER]: String(tv + vcostMs) };
        if ((m = route.match(/^\/fpp\/provider\/(app|user|none)\/remote\/(jira|confluence|bitbucket|stargate)(?:\/account\/(.+))?$/))) {
          const [, provider, remote] = m;
          if (remote === 'stargate') {
            const r = stargate(inv, target, parseMaybe(raw), tv, vcostMs);
            return send(r.status, r.body);
          }
          const r = await productFetch({ inv, provider, product: remote, method: req.method, path: target, headers: req.headers, body: raw || undefined, tv, vcostMs });
          return send(r.status, r.body, r.headers);
        }
        if (route === KVS_ROUTE) {
          const r = kvsCall(inv, target, parseMaybe(raw), tv, vcostMs);
          return send(r.status, r.body);
        }
        if (route === '/llm/' || route.startsWith('/llm/')) {
          const model = route === '/llm/' ? null : decodeURIComponent(route.slice('/llm/'.length));
          const body = parseMaybe(raw);
          const entry = record({ invocationId: inv.id, moduleType: inv.moduleType, moduleKey: inv.moduleKey, functionKey: inv.functionKey, asUser: inv.aaid ?? null,
            service: 'llm', provider: 'app', method: req.method, path: route, op: req.method === 'GET' ? 'list' : body?.stream ? 'stream' : 'chat', model, body, vcostMs }, tv);
          if (!llmModules.length) {
            // The docs: "If the SDK is used without declaring this module, linting will fail with an error like:
            // Error: LLM package is used but 'llm' module is not defined in the manifest". What the runtime answers is
            // not documented; the harness refuses the call loudly (DESIGN §6.2).
            entry.status = 403;
            entry.refused = 'no llm module';
            return send(403, { code: 'LLM_MODULE_NOT_DEFINED', message: "LLM package is used but 'llm' module is not defined in the manifest" });
          }
          const r = await siteCall('llm', { method: req.method, model, body, caller: { invocationId: inv.id, moduleType: inv.moduleType, moduleKey: inv.moduleKey, functionKey: inv.functionKey, asUser: inv.aaid ?? null } });
          Object.assign(entry, { status: r.status, response: r.body });
          if (Array.isArray(r.stream) && r.status === 200) {
            // Newline-delimited ChatResponse chunks (site/llm.cjs STREAMING). The tool-call line is cut in two writes,
            // with a pause so they arrive as two reads, which drives @forge/llm's fragment path (llm-stream-parser.js:25-32).
            entry.streamedChunks = r.stream.length;
            res.writeHead(200, { 'content-type': 'application/x-ndjson', ...timing });
            const cut = Math.max(0, r.stream.findIndex((c) => c.choices?.[0]?.message?.tool_calls));
            for (const [i, c] of r.stream.entries()) {
              const line = `${JSON.stringify(c)}\n`;
              if (i === cut) {
                res.write(line.slice(0, Math.floor(line.length / 2)));
                await new Promise((ok) => setTimeout(ok, STREAM_READ_GAP_MS));
                res.write(line.slice(Math.floor(line.length / 2)));
              } else res.write(line);
              await new Promise((ok) => setTimeout(ok, STREAM_READ_GAP_MS));
            }
            return res.end();
          }
          return send(r.status, r.body, r.headers ?? {});
        }
        if (route === '/fpp/as/app/provider/atlassian/capability/realtime') {
          const gql = parseMaybe(raw) ?? {};
          const v = gql.variables ?? {};
          const origin = { source: 'function', invocationId: inv.id, moduleType: inv.moduleType, moduleKey: inv.moduleKey, functionKey: inv.functionKey };
          const query = String(gql.query ?? '');
          if (/\bpublishRealtimeChannel\b/.test(query)) {
            let overrides = null;
            try { overrides = v.context ? JSON.parse(v.context).contextOverrides ?? null : null; } catch { overrides = null; }
            const contextToken = req.headers['x-forge-context-token'];
            const r = await siteCall('rtpublish', { channelName: v.name, payload: v.payload, isGlobal: Boolean(v.isGlobal), token: v.token ?? null,
              contextToken, contextOverrides: overrides, origin });
            record({ ...origin, service: 'realtime', provider: 'app', method: 'POST', path: route, op: v.isGlobal ? 'publishGlobal' : 'publish',
              body: { channel: v.name, payload: v.payload, isGlobal: Boolean(v.isGlobal), token: Boolean(v.token), contextToken: contextToken ?? null, contextOverrides: overrides },
              status: 200, response: r, vcostMs }, tv);
            if (r.errors) return send(200, { data: null, errors: r.errors });
            return send(200, { data: { ecosystem: { publishRealtimeChannel: { eventId: r.eventId, eventTimestamp: r.eventTimestamp } } } });
          }
          if (/\bsignRealtimeToken\b/.test(query)) {
            const r = await siteCall('rtsign', { channelName: v.channelName, claims: v.claims, permissions: v.permissions ?? null });
            record({ ...origin, service: 'realtime', provider: 'app', method: 'POST', path: route, op: 'signRealtimeToken', body: v, status: 200, response: { expiresAt: r.expiresAt }, vcostMs }, tv);
            return send(200, { data: { ecosystem: { signRealtimeToken: { errors: null, forgeRealtimeToken: { jwt: r.jwt, expiresAt: r.expiresAt }, success: true } } } });
          }
          missing(`realtime operation ${query.slice(0, 60)}`, inv);
          record({ ...origin, service: 'realtime', provider: 'app', method: 'POST', path: route, body: gql, status: 400, vcostMs }, tv);
          return send(400, { errors: [{ message: 'EMULATOR_NOT_MODELLED: unknown realtime operation' }] });
        }
        if (route === '/egress') {
          let host = null;
          try { host = new URL(target).host; } catch { host = null; }
          const allowed = host && egressAllow.some((a) => a === '*' || safeHost(a) === host || (a.startsWith('*.') && host.endsWith(a.slice(1))));
          record({ invocationId: inv.id, moduleType: inv.moduleType, moduleKey: inv.moduleKey, service: 'egress', provider: 'app', method: req.method, path: target, status: allowed ? 502 : 403, vcostMs }, tv);
          if (!allowed) return send(403, '', { 'forge-proxy-error': 'REQUEST_EGRESS_ALLOWLIST_ERR' });
          return send(502, { message: 'The benchmark harness has no internet: declared egress cannot be reached.' });
        }
        missing(`proxy route ${route}`, inv);
        // The whole request is kept so an unmodelled platform capability can be read off the log exactly.
        record({ invocationId: inv.id, moduleType: inv.moduleType, moduleKey: inv.moduleKey, service: 'unknown', provider: 'app', method: req.method, path: route,
          headers: Object.fromEntries(Object.entries(req.headers).filter(([k]) => k !== 'forge-proxy-authorization')), body: parseMaybe(raw), status: 501, vcostMs }, tv);
        return send(501, { code: 'EMULATOR_NOT_MODELLED', message: `proxy route ${route}` });
      } catch (e) {
        send(500, { code: 'PROXY_CRASH', message: String(e.message) });
      }
    });
  });

  return {
    log, harnessMissing, productFetch, kvsCall,
    listen: () => new Promise((ok) => server.listen(0, '127.0.0.1', () => {
      selfUrl = `http://127.0.0.1:${server.address().port}`;
      ok({ url: selfUrl, port: server.address().port });
    })),
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
