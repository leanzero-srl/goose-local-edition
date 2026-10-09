'use strict';
// The private mock Jira Cloud site (DESIGN.md §5). It answers ONLY the Forge proxy, which tells it who is
// calling through x-forge-* headers; a request without them is unauthenticated, as on Jira.
//
//   const site = await createSite({ seed, port: 0, trace })
//     -> { url, adminUrl, pack, state, rate, clock, log, comments, faultLog, harnessMissing, stop, ... }
//   node forge2/site/site.cjs --seed <16 hex> [--port N] [--token T] [--trace file] [--scoring]   (prints {"url","adminUrl"})
//
// Every request is matched against the pinned OpenAPI: not in it -> 404 like Jira (the app's defect);
// in it but not modelled -> 501 EMULATOR_NOT_MODELLED + a harness_missing entry (a harness gap: the
// verdict is held, never zeroed). Scopes are checked from the OpenAPI before any handler runs, then the rate model
// (rate.cjs, SPEC §2.1) admits or refuses the request and charges it. Each log entry carries the request's virtual
// instant `t`, its `kind` (person / background / unlabelled, from the emulator's labels), the `points` charged and,
// for a refusal, `rateLimited` (the RateLimit-Reason) and `retryAfter`.
const http = require('http');
const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { worldPack } = require('./fixtures.cjs');
const { createWorld } = require('./world.cjs');
const fieldsRest = require('./rest/fields.cjs');
const { createState } = require('./state.cjs');
const { createOpenApi } = require('./openapi.cjs');
const { createRenderer } = require('./rest/render.cjs');
const { NotModelledError } = require('./jql.cjs');
const { createLlm } = require('./llm.cjs');
const { createRealtime } = require('./realtime.cjs');
const rateModel = require('./rate.cjs');

// Every rest/*.cjs module that exports `handlers` ({ 'METHOD /template': (ctx) => {status, body, headers?} }) serves
// its operations: platform.cjs, agile.cjs, and site-world's fields.cjs. One operation has one handler.
const HANDLERS = {};
for (const file of fs.readdirSync(path.join(__dirname, 'rest')).filter((f) => f.endsWith('.cjs') && !f.endsWith('.test.cjs')).sort()) {
  for (const [op, fn] of Object.entries(require(path.join(__dirname, 'rest', file)).handlers ?? {})) {
    if (HANDLERS[op]) throw new Error(`rest/${file} handles ${op} a second time`);
    HANDLERS[op] = fn;
  }
}
const COMMENT_POST = 'POST /rest/api/3/issue/{issueIdOrKey}/comment';
const UI_MODULE_TYPES = new Set(['jira:sprintAction', 'dashboards:widget']);
// A page after the first: a nextPageToken, or a startAt past 0, in the query or the JSON body.
const isContinuation = (query, body) => Boolean(query.get('nextPageToken') || Number(query.get('startAt')) > 0
  || (body && typeof body === 'object' && (body.nextPageToken || Number(body.startAt) > 0)));

function json(res, status, body, headers = {}) {
  const text = body === undefined ? '' : JSON.stringify(body);
  res.writeHead(status, { 'content-type': 'application/json;charset=UTF-8', ...headers });
  res.end(text);
}

async function createSite({ seed, port = 0, trace = null, token = crypto.randomBytes(12).toString('hex'), openapiDir, pack: givenPack, scoring = false } = {}) {
  const pack = givenPack ?? worldPack(seed, { scoring });
  const state = createState(pack);
  const rate = rateModel.createRate();
  const render = createRenderer(state);
  const openapi = createOpenApi(openapiDir);
  const log = [];
  const harnessMissing = [];
  const faultLog = [];
  const signals = [];
  let faults;
  const traceLine = (o) => { if (trace) fs.appendFileSync(trace, JSON.stringify({ t: new Date(state.now()).toISOString(), ...o }) + '\n'); };
  const resetFaults = () => {
    faults = pack.faults.map((f) => ({ ...f, armed: false, fired: false, count: 0, windowUntil: null }));
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
      // A Custom UI resolver's own Jira read (never the page's direct requestJira): DESIGN §5.2, armed by the probe.
      case 'resolver-read': return UI_MODULE_TYPES.has(caller.moduleType) && caller.source !== 'frontend';
      default: return false;
    }
  };
  // Scripted 429s, matched by WHO is calling (DESIGN.md §5.2 faults); an early retry inside an open
  // window gets another 429 and is recorded `early_retry`.
  const faultFor = (caller, opKey, path, query, body, now) => {
    for (const f of faults) {
      if (!inScope(f, caller, opKey)) continue;
      if (f.windowUntil !== null && now < f.windowUntil) {
        const rec = { fault: f.id, kind: 'early_retry', at: new Date(now).toISOString(), invocationId: caller.invocationId, path, retryAfter: Math.ceil((f.windowUntil - now) / 1000) };
        faultLog.push(rec);
        return { f, retryAfter: rec.retryAfter, kind: 'early_retry' };
      }
      if (f.fired) continue;
      if (f.match.continuation && !isContinuation(query, body)) continue;
      let fire = false;
      if (f.match.scope === 'consumer-of-change' || f.match.scope === 'resolver-read') fire = f.armed;
      else { f.count += 1; fire = f.count === f.match.nth; }
      if (fire) {
        f.fired = true;
        // A 429 opens a Retry-After window; any other scripted status (the resolver's 500) answers once.
        f.windowUntil = f.status === 429 ? now + f.retryAfter * 1000 : null;
        const rec = { fault: f.id, kind: 'fired', at: new Date(now).toISOString(), invocationId: caller.invocationId, moduleType: caller.moduleType, path, retryAfter: f.retryAfter, scope: f.match.scope };
        faultLog.push(rec);
        traceLine({ event: 'fault', ...rec });
        return { f, retryAfter: f.retryAfter, kind: 'fired' };
      }
    }
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
  // The request's virtual instant: the invocation's clock as the proxy sends it (x-forge-vtime, epoch ms), else the
  // site's clock (a request outside any invocation's clock).
  const instantOf = (h) => (/^\d+$/.test(h['x-forge-vtime'] ?? '') ? Number(h['x-forge-vtime']) : state.now());

  const handleProduct = (req, res, raw) => {
    const url = new URL(req.url, 'http://site');
    const method = req.method;
    const caller = callerOf(req.headers);
    const t = instantOf(req.headers);
    const kind = caller ? rateModel.kindOf(caller) : 'unlabelled';
    const entry = { t: new Date(t).toISOString(), method, path: url.pathname + url.search, as: caller?.as ?? null, accountId: caller?.accountId ?? null,
      invocationId: caller?.invocationId ?? null, moduleType: caller?.moduleType ?? null, moduleKey: caller?.moduleKey ?? null, source: caller?.source ?? null,
      kind, points: 0 };
    log.push(entry);
    const send = (status, body, headers = {}) => {
      entry.status = status;
      // Harness-internal attribution for the proxy's call log; the proxy strips x-forge-site-* before the app sees the response.
      const internal = {};
      if (entry.fault) { internal['x-forge-site-fault'] = entry.fault; internal['x-forge-site-fault-kind'] = entry.faultKind; }
      if (entry.scopeAlternative) internal['x-forge-site-scopes'] = JSON.stringify(entry.scopeAlternative);
      if (entry.op) { internal['x-forge-site-op'] = entry.op; internal['x-forge-site-latency-ms'] = String(rateModel.latencyOf(entry.op)); }
      internal['x-forge-site-points'] = String(entry.points);
      json(res, status, body, { ...rate.headers(t), ...headers, ...internal });
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
    // The rate model (SPEC §2.1): the wall, the endpoint's bucket, the per-issue write window. A page's own bridge
    // request is not charged to the quota (rate.cjs quotaCharged).
    const written = [...new Set(rateModel.writtenRefs(opKey, m.params, body).map((k) => state.issueByIdOrKey(k)?.id).filter(Boolean))];
    const charged = rateModel.quotaCharged(caller);
    const refusal = rate.check({ t, endpoint: opKey, kind, cost: rateModel.costOf(opKey, body, 0), issues: written, charged });
    if (refusal) {
      entry.rateLimited = refusal.reason;
      entry.retryAfter = refusal.retryAfter;
      traceLine({ event: 'rate_limited', kind, op: opKey, reason: refusal.reason, retryAfter: refusal.retryAfter, invocationId: caller.invocationId });
      return send(429, rate.model.body429, { 'Retry-After': String(refusal.retryAfter), 'RateLimit-Reason': refusal.reason });
    }
    const fault = faultFor(caller, opKey, pathname, url.searchParams, body, t);
    if (fault) {
      entry.fault = fault.f.id;
      entry.faultKind = fault.kind;
      if (fault.kind === 'fired' && fault.f.status !== 429) {
        return send(fault.f.status, { errorMessages: ['Internal server error'], errors: {} });
      }
      return send(429, rate.model.body429, { 'Retry-After': String(fault.retryAfter), 'RateLimit-Reason': fault.f.reason });
    }
    const ctx = { state, render, limits: pack.limits, paging: pack.paging, caller, params: m.params,
      req: { method, pathname, query: url.searchParams, body },
      canBrowse: (iss) => state.canBrowse(caller.accountId, iss), canComment: (iss) => state.canComment(caller.accountId, iss),
      canBrowseProject: (key) => state.canBrowseProject(caller.accountId, key) };
    // Every served request is charged (a refused one never is); a write counts in its issues' window only when it
    // succeeded. `points` is what the quota was charged (0 for a bridge request, whose cost still drains the bucket).
    const serve = (out) => {
      const cost = rateModel.costOf(opKey, body, Array.isArray(out.body?.issues) ? out.body.issues.length : 0);
      entry.points = charged ? cost : 0;
      rate.charge({ t, endpoint: opKey, kind, cost, issues: out.status < 400 ? written : [], charged });
      send(out.status, out.body, out.headers);
    };
    try {
      serve(handler(ctx));
    } catch (e) {
      if (e instanceof NotModelledError) {
        missing(e.message, { method, path: pathname });
        return send(501, { code: 'EMULATOR_NOT_MODELLED', message: e.message });
      }
      entry.error = String(e.stack ?? e);
      serve({ status: 500, body: { errorMessages: [`site handler crashed: ${e.message}`], errors: {} } });
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
  const worldEvent = (change) => delivery({ slot: null, duplicate: false, remaining: null, applied: [], change, issue: state.st.issues.get(change.issueId) });
  // Platform services the site hosts for every emulator attached to it: Forge LLM (llm.cjs) and Realtime
  // (realtime.cjs). The realtime token key derives from the control token, so only this site's emulators can sign.
  // The world that changes mid-run (world.cjs, SPEC §2.5): its scheduled events apply as the site's time reaches them
  // (state.cjs calls the hook before each live change and at flush); a deletion queues the avi:jira:deleted:issue
  // event the emulator delivers (control `worldevents`).
  const world = pack.world ? createWorld({ pack, state }) : null;
  const worldLog = [];
  const worldPending = [];
  let worldReported = 0;
  if (world) {
    state.setWorldHook((t) => {
      for (const rec of world.applyDue(t)) {
        const { result, event, ...e } = rec;
        worldLog.push({ t_ms: e.atMs, ...e });
        if (event) worldPending.push({ ...event, issue: eventSnapshot(event.issue) });
      }
    });
  }
  const llm = createLlm({ pack, now: () => state.now() });
  const realtime = createRealtime({ now: () => state.now(), secret: crypto.createHash('sha256').update(`realtime:${token}`).digest() });
  const control = {
    llm: (req) => llm.handle(req),
    llmphase: ({ phase, script }) => llm.phase(phase, script),
    llmlog: ({ since = 0 }) => ({ entries: llm.log.slice(Number(since)), next: llm.log.length, state: llm.state(), models: llm.models() }),
    rtsign: (a) => realtime.signToken(a),
    rtcontext: (ctx) => ({ contextToken: realtime.mintContext(ctx) }),
    rtsubscribe: (a) => realtime.subscribe(a),
    rtunsubscribe: ({ subscriptionId }) => ({ removed: realtime.unsubscribe(subscriptionId) }),
    rtpublish: (a) => realtime.publish(a),
    rtdeliveries: ({ since = 0, origin = null }) => ({ deliveries: realtime.deliveriesSince(Number(since), origin) }),
    rtlog: ({ since = 0 }) => ({ ...realtime.eventsSince(Number(since)), subscriptions: realtime.subscriptions() }),
    info: () => ({ cloudId: pack.cloudId, siteUrl: pack.siteUrl, appAccountId: pack.appAccountId, now: new Date(state.now()).toISOString(),
      users: pack.users.map(({ accountId, displayName }) => ({ accountId, displayName })), viewer: pack.viewer, admins: pack.admins, admin: pack.admin,
      // Who may not comment where (the default viewer on one issue): Jira answers such a comment 400.
      commentForbidden: pack.issues.filter((i) => i.commentForbiddenFor.length).map((i) => ({ issueKey: i.key, accountIds: i.commentForbiddenFor })),
      // What the Agile REST API discloses anyway; the dev kit builds sprint-action contexts from it (live: the world
      // closes sprints and switches estimation fields).
      projects: pack.projects.map(({ id, key }) => ({ id, key })),
      boards: state.boards().map(({ id, type, projectKey }) => ({ id, type, projectKey })),
      sprints: state.sprints().map(({ id, state: s, originBoardId }) => ({ id, state: s, originBoardId })),
      scopeStatusFieldId: pack.scopeStatusFieldId,
      liveRemaining: state.plan.length - state.st.cursor }),
    // The rate model's ledger (SPEC §2.1): per virtual hour, points by kind, requests, refusals by kind and reason.
    rate: () => ({ model: rate.model, hours: rate.summary() }),
    // Points drawn from this installation's hour by the rest of the world (other tenants on the shared pool).
    draw: ({ points, at }) => rate.draw({ t: at ?? state.now(), points: Number(points) }),
    // The world mutation API (state.cjs, SPEC §2.5); `at` is a virtual epoch ms. A changelog entry the world writes
    // comes back with `events`: the issue-updated payloads in the shape `next`/`event` deliver live changes; a deletion
    // comes back with the deleted issue's snapshot.
    moveissue: ({ issue, sprintId, at, authorId, estimate }) => {
      const w = state.moveIssue(issue, sprintId, { at: at ?? state.now(), authorId, estimate });
      return { ...w, events: [worldEvent(w.change)] };
    },
    deleteissue: ({ issue, at }) => {
      const w = state.deleteIssue(issue, { at: at ?? state.now() });
      return { ...w, snapshot: eventSnapshot(w.issue) };
    },
    closesprint: ({ sprintId, at, carryTo = null, authorId }) => {
      const w = state.closeSprint(sprintId, { at: at ?? state.now(), carryTo, authorId });
      return { ...w, events: w.carried.map(worldEvent) };
    },
    estimationfield: ({ boardId, fieldId, at }) => state.setBoardEstimationField(boardId, fieldId, { at: at ?? state.now() }),
    revokebrowse: ({ accountId, projectKey, at }) => state.revokeBrowse(accountId, projectKey, { at: at ?? state.now() }),
    addfield: (def) => state.addField(def),
    // With `until` (virtual epoch ms): the site as Jira stands then (live changes created by then, world events due by
    // then) -> the world events applied since the last such call. Without it: the state's mutation log since `since`.
    world: ({ since = 0, until }) => {
      if (until === undefined) return { entries: state.st.world.slice(Number(since)), next: state.st.world.length };
      state.applyUntil(Number(until));
      const applied = worldLog.slice(worldReported);
      worldReported = worldLog.length;
      return { applied, pending: world ? world.pending().map((e) => e.id) : [] };
    },
    worldplan: () => ({ world: pack.world ?? null, applied: worldLog }),
    worldevents: () => ({ events: worldPending.splice(0) }),
    // The app's scope-status field (SPEC R7): installed with v2 (idempotent: every emulator of the app installs it).
    installfield: ({ key, name, description }) => {
      if (key !== 'scope-status') return { installed: null, reason: `the site names no field id for custom field '${key}'` };
      const id = pack.scopeStatusFieldId;
      if (!state.field(id)) {
        state.addField({ id, key: id, name: name ?? key, custom: true, orderable: true, navigable: true, searchable: true,
          clauseNames: [`cf[${id.replace('customfield_', '')}]`, name ?? key], ...(description ? { description } : {}),
          schema: { type: 'string', custom: `ari:cloud:ecosystem::extension/${pack.cloudId}/forge-app/static/${key}`, customId: Number(id.replace('customfield_', '')) } });
      }
      return { installed: id };
    },
    fieldvalues: () => ({ values: state.field(pack.scopeStatusFieldId) ? fieldsRest.fieldValues(state) : {} }),
    fieldwrites: ({ since = 0 }) => ({ writes: fieldsRest.fieldWrites(state).slice(Number(since)) }),
    // Every /rest request the rate model priced, in the probe's shape (SPEC §2.1).
    ratelog: ({ since = 0 }) => ({ entries: log.slice(Number(since)).filter((e) => e.op).map((e) => ({ t_ms: Date.parse(e.t), invocation: e.invocationId,
      kind: e.kind, method: e.method, path_tpl: e.op.slice(e.op.indexOf(' ') + 1), cost: e.points, status: e.status,
      reason: e.rateLimited ?? null, retry_after_s: e.retryAfter ?? null, module_type: e.moduleType, source: e.source })), next: log.length }),
    // v1's KVS content at the upgrade (SPEC §2.4): the dev kit lays it down when its storage is empty.
    preload: () => ({ v1Preload: pack.v1Preload ?? null }),
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
    // The probe arms a resolver-read fault for one extra surface open (DESIGN §5.2): it fires on the next resolver's
    // first Jira read; disarm withdraws it when that open made no Jira read.
    arm: ({ id }) => { const f = faults.find((x) => x.id === id); if (!f) throw new Error(`no fault ${id}`); f.armed = true; return { ok: true }; },
    disarm: ({ id }) => { const f = faults.find((x) => x.id === id); if (f) f.armed = false; return { ok: true, fired: Boolean(f?.fired) }; },
    signal: (s) => {
      signals.push({ ...s, at: new Date(state.now()).toISOString() });
      if (s.type === 'consumer-start' && s.originChange) {
        for (const f of faults) if (f.match.scope === 'consumer-of-change' && f.match.changelogId === s.originChange) f.armed = true;
      }
      return { ok: true };
    },
    reset: () => {
      state.reset(); rate.reset(); resetFaults(); log.length = 0; faultLog.length = 0; signals.length = 0; llm.reset(); realtime.reset();
      world?.reset(); worldLog.length = 0; worldPending.length = 0; worldReported = 0;
      return { ok: true };
    },
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
    rate,
    clock: { now: state.now, advance: state.advance },
    log,
    get comments() { return state.st.comments; },
    faultLog,
    signals,
    harnessMissing,
    control,
    llm,
    realtime,
    // A live change that happens in Jira without its event reaching the app (a dropped delivery).
    applyChange: (change) => state.applyThrough(typeof change === 'string' ? change : change.changelogId),
    flushLive: () => state.flush(),
    stop: () => new Promise((ok) => server.close(() => ok())),
  };
}

if (require.main === module) {
  const args = process.argv.slice(2);
  const get = (n) => { const i = args.indexOf(n); return i >= 0 ? args[i + 1] : undefined; };
  createSite({ seed: get('--seed'), port: Number(get('--port') ?? 0), trace: get('--trace') ?? null, scoring: args.includes('--scoring'),
    ...(get('--token') ? { token: get('--token') } : {}) })
    .then((site) => {
      process.stdout.write(JSON.stringify({ url: site.url, adminUrl: site.adminUrl, pid: process.pid }) + '\n');
      const stop = () => site.stop().then(() => process.exit(0));
      process.on('SIGTERM', stop);
      process.on('SIGINT', stop);
    })
    .catch((e) => { process.stderr.write(`site failed: ${e.stack ?? e}\n`); process.exit(2); });
}

module.exports = { createSite };
