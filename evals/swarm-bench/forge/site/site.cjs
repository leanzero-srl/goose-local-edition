'use strict';
// The private mock Jira Cloud site (DESIGN.md §5). It answers ONLY the Forge proxy, which tells it who is
// calling through x-forge-* headers; a request without them is unauthenticated, as on Jira.
//
//   const site = await createSite({ seed, port: 0, trace })
//     -> { url, adminUrl, pack, state, clock, log, comments, faultLog, harnessMissing, stop, ... }
//   node forge/site/site.cjs --seed <16 hex> [--port N] [--token T] [--trace file]   (prints {"url","adminUrl"})
//
// Every request is matched against the pinned OpenAPI: not in it -> 404 like Jira (the app's defect);
// in it but not modelled -> 501 EMULATOR_NOT_MODELLED + a harness_missing entry (a harness gap: the
// verdict is held, never zeroed). Scopes are checked from the OpenAPI before any handler runs.
const http = require('http');
const fs = require('fs');
const crypto = require('crypto');
const { facts } = require('./fixtures.cjs');
const { createState } = require('./state.cjs');
const { createOpenApi } = require('./openapi.cjs');
const { createRenderer } = require('./rest/render.cjs');
const platform = require('./rest/platform.cjs');
const agile = require('./rest/agile.cjs');
const { NotModelledError } = require('./jql.cjs');

const HANDLERS = { ...platform.handlers, ...agile.handlers };
const COMMENT_POST = 'POST /rest/api/3/issue/{issueIdOrKey}/comment';
const WRITE_OPS = new Set([COMMENT_POST]);

function json(res, status, body, headers = {}) {
  const text = body === undefined ? '' : JSON.stringify(body);
  res.writeHead(status, { 'content-type': 'application/json;charset=UTF-8', ...headers });
  res.end(text);
}

async function createSite({ seed, port = 0, trace = null, token = crypto.randomBytes(12).toString('hex'), openapiDir, pack: givenPack } = {}) {
  const pack = givenPack ?? facts(seed);
  const state = createState(pack);
  const render = createRenderer(state);
  const openapi = createOpenApi(openapiDir);
  const log = [];
  const harnessMissing = [];
  const faultLog = [];
  const signals = [];
  let faults;
  let writes;
  const traceLine = (o) => { if (trace) fs.appendFileSync(trace, JSON.stringify({ t: new Date(state.now()).toISOString(), ...o }) + '\n'); };
  const resetFaults = () => {
    faults = pack.faults.map((f) => ({ ...f, armed: false, fired: false, count: 0, windowUntil: null }));
    writes = new Map();
  };
  resetFaults();

  const missing = (what, extra) => {
    const m = { what, at: new Date(state.now()).toISOString(), ...extra };
    harnessMissing.push(m);
    traceLine({ event: 'harness_missing', ...m });
  };

  const inScope = (f, caller, opKey) => {
    switch (f.match.scope) {
      case 'consumer-of-change': return caller.moduleType === 'consumer';
      case 'scheduled-run': return caller.scheduledRun === f.match.run;
      case 'comment-post': return opKey === COMMENT_POST;
      default: return false;
    }
  };
  // Scripted 429s, matched by WHO is calling (DESIGN.md §5.2 faults); an early retry inside an open
  // window gets another 429 and is recorded `early_retry`.
  const faultFor = (caller, opKey, path) => {
    const now = state.now();
    for (const f of faults) {
      if (!inScope(f, caller, opKey)) continue;
      if (f.windowUntil !== null && now < f.windowUntil) {
        const rec = { fault: f.id, kind: 'early_retry', at: new Date(now).toISOString(), invocationId: caller.invocationId, path, retryAfter: Math.ceil((f.windowUntil - now) / 1000) };
        faultLog.push(rec);
        return { f, retryAfter: rec.retryAfter, kind: 'early_retry' };
      }
      if (f.fired) continue;
      let fire = false;
      if (f.match.scope === 'consumer-of-change') fire = f.armed;
      else { f.count += 1; fire = f.count === f.match.nth; }
      if (fire) {
        f.fired = true;
        f.windowUntil = now + f.retryAfter * 1000;
        const rec = { fault: f.id, kind: 'fired', at: new Date(now).toISOString(), invocationId: caller.invocationId, moduleType: caller.moduleType, path, retryAfter: f.retryAfter, scope: f.match.scope };
        faultLog.push(rec);
        traceLine({ event: 'fault', ...rec });
        return { f, retryAfter: f.retryAfter, kind: 'fired' };
      }
    }
    return null;
  };
  // Per-issue write limit (RESEARCH §2 rate-limiting page): 20 writes per 2 s and 100 per 30 s.
  const writeLimit = (issueKeyOrId) => {
    const iss = state.issueByIdOrKey(issueKeyOrId);
    if (!iss) return null;
    const now = state.now();
    const list = (writes.get(iss.id) ?? []).filter((t) => now - t < 30_000);
    const last2 = list.filter((t) => now - t < 2_000);
    if (last2.length >= pack.limits.issueWritesPer2s.value) return Math.ceil((2_000 - (now - last2[0])) / 1000);
    if (list.length >= pack.limits.issueWritesPer30s.value) return Math.ceil((30_000 - (now - list[0])) / 1000);
    list.push(now);
    writes.set(iss.id, list);
    return null;
  };

  const callerOf = (h) => {
    const as = h['x-forge-as'];
    if (!as) return null;
    const accountId = as === 'app' ? pack.appAccountId : as === 'user' ? h['x-forge-account'] : null;
    let scopes = [];
    try { scopes = JSON.parse(h['x-forge-scopes'] ?? '[]'); } catch { scopes = []; }
    return { as, accountId, scopes, invocationId: h['x-forge-invocation'] ?? null, moduleType: h['x-forge-module-type'] ?? null,
      moduleKey: h['x-forge-module-key'] ?? null, source: h['x-forge-source'] ?? 'function',
      scheduledRun: h['x-forge-scheduled-run'] ? Number(h['x-forge-scheduled-run']) : null, originChange: h['x-forge-origin-change'] ?? null };
  };

  const handleProduct = (req, res, raw) => {
    const url = new URL(req.url, 'http://site');
    const method = req.method;
    const caller = callerOf(req.headers);
    const entry = { t: new Date(state.now()).toISOString(), method, path: url.pathname + url.search, as: caller?.as ?? null, accountId: caller?.accountId ?? null,
      invocationId: caller?.invocationId ?? null, moduleType: caller?.moduleType ?? null, moduleKey: caller?.moduleKey ?? null, source: caller?.source ?? null };
    log.push(entry);
    const send = (status, body, headers = {}) => {
      entry.status = status;
      // Harness-internal attribution for the proxy's call log; the proxy strips x-forge-site-* before the app sees the response.
      const internal = {};
      if (entry.fault) { internal['x-forge-site-fault'] = entry.fault; internal['x-forge-site-fault-kind'] = entry.faultKind; }
      if (entry.scopeAlternative) internal['x-forge-site-scopes'] = JSON.stringify(entry.scopeAlternative);
      if (entry.op) internal['x-forge-site-op'] = entry.op;
      json(res, status, body, { ...headers, ...internal });
    };
    if (!caller) return send(401, { errorMessages: ['Client must be authenticated to access this resource.'], errors: {} });
    if (!caller.accountId) return send(401, { code: 401, message: 'Unauthorized; no user or app identity on this request' });
    let body;
    if (raw) {
      try { body = JSON.parse(raw); } catch {
        return send(400, { errorMessages: ['Unexpected character in the request body: it is not valid JSON.'], errors: {} });
      }
    }
    let pathname = url.pathname;
    const legacy = pathname.match(/^\/rest\/api\/(2|latest)\//);
    if (legacy) {
      const v3 = pathname.replace(/^\/rest\/api\/(2|latest)\//, '/rest/api/3/');
      if (openapi.match(method, v3)) {
        missing(`REST ${method} ${pathname} (API v${legacy[1]} is valid Jira; the site models v3)`, { method, path: pathname });
        return send(501, { code: 'EMULATOR_NOT_MODELLED', message: `v${legacy[1]} REST is not modelled; the site serves /rest/api/3` });
      }
    }
    const m = openapi.match(method, pathname);
    if (!m) {
      if (openapi.pathExists(pathname)) return send(405, { errorMessages: [`Method ${method} is not allowed for ${pathname}`], errors: {} });
      return send(404, { errorMessages: [`No resource matches ${method} ${pathname}`], errors: {} });
    }
    const opKey = `${method} ${m.op.template}`;
    entry.op = opKey;
    const sc = openapi.scopeCheck(m.op, caller.scopes);
    entry.scopeAlternative = sc.ok ? sc.chosen : null;
    if (!sc.ok) return send(401, { code: 401, message: 'Unauthorized; scope does not match' });
    const handler = HANDLERS[opKey];
    if (!handler) {
      missing(`REST ${opKey}`, { method, path: pathname });
      return send(501, { code: 'EMULATOR_NOT_MODELLED', message: `${opKey} is a Jira Cloud operation the emulator does not model` });
    }
    const fault = faultFor(caller, opKey, pathname);
    if (fault) {
      entry.fault = fault.f.id;
      entry.faultKind = fault.kind;
      return send(429, { errorMessages: ['Rate limit exceeded.'], errors: {} }, { 'Retry-After': String(fault.retryAfter), 'RateLimit-Reason': fault.f.reason });
    }
    if (WRITE_OPS.has(opKey)) {
      const wait = writeLimit(m.params.issueIdOrKey);
      if (wait !== null) {
        entry.writeLimited = true;
        return send(429, { errorMessages: ['Rate limit exceeded.'], errors: {} }, { 'Retry-After': String(wait), 'RateLimit-Reason': 'jira-per-issue-on-write' });
      }
    }
    const ctx = { state, render, limits: pack.limits, caller, params: m.params,
      req: { method, pathname, query: url.searchParams, body },
      canBrowse: (iss) => state.canBrowse(caller.accountId, iss), canComment: (iss) => state.canComment(caller.accountId, iss) };
    try {
      const out = handler(ctx);
      send(out.status, out.body, out.headers);
    } catch (e) {
      if (e instanceof NotModelledError) {
        missing(e.message, { method, path: pathname });
        return send(501, { code: 'EMULATOR_NOT_MODELLED', message: e.message });
      }
      entry.error = String(e.stack ?? e);
      send(500, { errorMessages: [`site handler crashed: ${e.message}`], errors: {} });
    }
  };

  // ---- harness-only control surface (the proxy never forwards it: paths outside /rest are refused) ----
  const eventSnapshot = (iss) => {
    const f = iss.fields;
    const userRef = (u) => (u ? { accountId: u.accountId } : null);
    return { id: iss.id, key: iss.key, fields: {
      summary: f.summary, issuetype: render.fieldValue('issuetype', f.issuetype), creator: userRef(f.creator), created: render.jiraDate(f.created),
      project: render.fieldValue('project', f.project), reporter: userRef(f.reporter), assignee: userRef(f.assignee),
      updated: render.jiraDate(f.updated), status: render.fieldValue('status', f.status) } };
  };
  const delivery = (d) => d && ({ slot: d.slot, duplicate: d.duplicate, remaining: d.remaining, applied: d.applied,
    change: { id: d.change.changelogId, created: d.change.created, authorId: d.change.authorId,
      items: d.change.items.map(({ field, fieldId, from, fromString, to, toString }) => ({ field, fieldId, from: from === '' ? null : from, fromString: fromString === '' ? null : fromString, to: to === '' ? null : to, toString: toString === '' ? null : toString })) },
    issue: eventSnapshot(d.issue) });
  const control = {
    info: () => ({ cloudId: pack.cloudId, siteUrl: pack.siteUrl, appAccountId: pack.appAccountId, now: new Date(state.now()).toISOString(),
      users: pack.users.map(({ accountId, displayName }) => ({ accountId, displayName })), viewer: pack.viewer,
      // What the Agile REST API discloses anyway; the dev kit builds sprint-action contexts from it.
      projects: pack.projects.map(({ id, key }) => ({ id, key })),
      boards: pack.boards.map(({ id, type, projectKey }) => ({ id, type, projectKey })),
      sprints: pack.sprints.map(({ id, state, originBoardId }) => ({ id, state, originBoardId })),
      liveRemaining: state.plan.length - state.st.cursor }),
    clock: () => ({ now: state.now(), skippedMs: state.st.skipped }),
    advance: ({ ms }) => ({ now: state.advance(Number(ms)) }),
    next: () => delivery(state.nextDelivery()) ?? { done: true },
    apply: ({ changelogId }) => ({ applied: state.applyThrough(changelogId) }),
    // A delivery of one specific live change (emu.deliverProductEvent): applies through it, then the event.
    event: ({ changelogId }) => {
      const applied = state.applyThrough(changelogId);
      const change = pack.live.find((c) => c.changelogId === changelogId);
      return delivery({ slot: change.delivery.slot, duplicate: !applied.includes(changelogId), remaining: null, applied, change, issue: state.st.issues.get(change.issueId) });
    },
    flush: () => ({ applied: state.flush() }),
    signal: (s) => {
      signals.push({ ...s, at: new Date(state.now()).toISOString() });
      if (s.type === 'consumer-start' && s.originChange) {
        for (const f of faults) if (f.match.scope === 'consumer-of-change' && f.match.changelogId === s.originChange) f.armed = true;
      }
      return { ok: true };
    },
    reset: () => { state.reset(); resetFaults(); log.length = 0; faultLog.length = 0; signals.length = 0; return { ok: true }; },
    log: ({ since = 0 }) => ({ entries: log.slice(Number(since)), next: log.length }),
    comments: () => ({ comments: state.st.comments }),
    users: () => control.info().users,
  };
  const handleControl = (req, res, raw, op) => {
    const fn = control[op];
    if (!fn) return json(res, 404, { error: `no control op ${op}` });
    const url = new URL(req.url, 'http://site');
    const args = raw ? JSON.parse(raw) : Object.fromEntries(url.searchParams);
    try { json(res, 200, fn(args)); } catch (e) { json(res, 500, { error: String(e.message) }); }
  };

  const server = http.createServer((req, res) => {
    let raw = '';
    req.setEncoding('utf8');
    req.on('data', (c) => (raw += c));
    req.on('end', () => {
      const pathname = new URL(req.url, 'http://site').pathname;
      const ctl = pathname.match(/^\/__site\/([0-9a-f]+)\/([a-z]+)$/);
      if (ctl) {
        if (ctl[1] !== token) return json(res, 404, { errorMessages: [`No resource matches ${req.method} ${pathname}`], errors: {} });
        return handleControl(req, res, raw, ctl[2]);
      }
      handleProduct(req, res, raw);
    });
  });
  await new Promise((ok) => server.listen(port, '127.0.0.1', ok));
  const url = `http://127.0.0.1:${server.address().port}`;
  traceLine({ event: 'site_started', seed: pack.seed, url });
  return {
    url,
    adminUrl: `${url}/__site/${token}`,
    token,
    pack,
    state,
    clock: { now: state.now, advance: state.advance },
    log,
    get comments() { return state.st.comments; },
    faultLog,
    signals,
    harnessMissing,
    control,
    // A live change that happens in Jira without its event reaching the app (a dropped delivery).
    applyChange: (change) => state.applyThrough(typeof change === 'string' ? change : change.changelogId),
    flushLive: () => state.flush(),
    stop: () => new Promise((ok) => server.close(() => ok())),
  };
}

if (require.main === module) {
  const args = process.argv.slice(2);
  const get = (n) => { const i = args.indexOf(n); return i >= 0 ? args[i + 1] : undefined; };
  createSite({ seed: get('--seed'), port: Number(get('--port') ?? 0), trace: get('--trace') ?? null, ...(get('--token') ? { token: get('--token') } : {}) })
    .then((site) => {
      process.stdout.write(JSON.stringify({ url: site.url, adminUrl: site.adminUrl, pid: process.pid }) + '\n');
      const stop = () => site.stop().then(() => process.exit(0));
      process.on('SIGTERM', stop);
      process.on('SIGINT', stop);
    })
    .catch((e) => { process.stderr.write(`site failed: ${e.stack ?? e}\n`); process.exit(2); });
}

module.exports = { createSite };
