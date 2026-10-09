'use strict';
// The benchmark's rate model (SPEC §2.1; RATE-MODEL.json must carry these numbers): a points quota per installation
// per virtual hour with a hard wall until the top of the next hour, a burst token bucket per endpoint (method + path
// template), a per-issue write window, the 429 shape and the X-RateLimit headers. Pure over virtual time: every call
// passes the request's instant; nothing here reads a clock.
//   const rate = createRate();
//   rate.check({ t, endpoint, kind, cost, issues })  -> null, or { reason, retryAfter } (the request is refused)
//   rate.charge({ t, endpoint, kind, cost, issues }) after a request was served
//   rate.headers(t)  the X-RateLimit headers every response carries
//   rate.draw({ t, points })  points drawn from this installation's hour by anything but the app (a world event)
//   rate.summary()  per virtual hour: points by kind, requests, refusals by kind and reason
//   node forge2/site/rate.cjs --model   (prints MODEL as JSON)
const HOUR_MS = 3_600_000;

const MODEL = {
  quotaPerHour: 2400,
  // Background (triggers, consumers, scheduled triggers, the web trigger) may use at most this share of an hour; it is
  // graded from the log, never enforced here.
  backgroundShare: 0.7,
  // Below this share of the hour's quota left, responses also carry X-RateLimit-Remaining and X-RateLimit-NearLimit.
  nearLimitShare: 0.2,
  burst: { capacity: 30, refillPerSecond: 5 },
  perIssueWriteMs: 2000,
  fieldUpdatesPerRequest: 200,
  // GET 1 · search/jql 1 + 1 per 50 issues returned (each started 50) · changelog/bulkfetch 2 · app field values
  // 1 + 1 per 50 updates (each started 50) · any other POST/PUT/DELETE 2. Agile and software GETs are GETs.
  costs: { get: 1, write: 2, bulkChangelog: 2, searchBase: 1, fieldValueBase: 1, perBlock: 50 },
  // SPEC §2.2: the virtual time one proxied request takes (the proxy advances the invocation's clock by it).
  latencyMs: { get: 120, search: 300, bulkfetch: 600, write: 200 },
  body429: { errorMessages: ['Rate limit exceeded'] },
  reasons: { quota: 'jira-quota-tenant-based', burst: 'jira-burst-based', perIssue: 'jira-per-issue-on-write' },
};

const SEARCH = new Set(['GET /rest/api/3/search/jql', 'POST /rest/api/3/search/jql']);
const BULK_CHANGELOG = 'POST /rest/api/3/changelog/bulkfetch';
// The shipped OpenAPI's app field-value writes (SPEC R7 names the bulk one; it is a POST in jira.json).
const FIELD_VALUES = new Set(['POST /rest/api/3/app/field/value', 'PUT /rest/api/3/app/field/{fieldIdOrKey}/value']);
// POSTs on an issue path that read rather than write.
const READ_POSTS = new Set(['POST /rest/api/3/issue/{issueIdOrKey}/changelog/list']);
const BACKGROUND = new Set(['trigger', 'consumer', 'scheduledTrigger', 'webtrigger']);

const methodOf = (opKey) => opKey.slice(0, opKey.indexOf(' '));
const fieldUpdateIssues = (body) => (Array.isArray(body?.updates) ? body.updates : [])
  .flatMap((u) => (Array.isArray(u?.issueIds) ? u.issueIds : []));

// Points one request costs; `returned` is the number of issues a search page served.
function costOf(opKey, body, returned = 0) {
  const c = MODEL.costs;
  if (SEARCH.has(opKey)) return c.searchBase + Math.ceil(returned / c.perBlock);
  if (opKey === BULK_CHANGELOG) return c.bulkChangelog;
  if (FIELD_VALUES.has(opKey)) return c.fieldValueBase + Math.ceil(fieldUpdateIssues(body).length / c.perBlock);
  return methodOf(opKey) === 'GET' ? c.get : c.write;
}

// The issue references a request writes (the per-issue window): a POST/PUT/DELETE on an issue path, and every issue
// of an app field-value update. The caller resolves them to issue ids.
function writtenRefs(opKey, params, body) {
  if (FIELD_VALUES.has(opKey)) return fieldUpdateIssues(body).map(String);
  if (methodOf(opKey) !== 'GET' && params.issueIdOrKey !== undefined && !READ_POSTS.has(opKey)) return [String(params.issueIdOrKey)];
  return [];
}

// The emulator's labels on a proxied request (x-forge-source, x-forge-module-type): a resolver invoked from a UI
// surface, the page's own bridge request or a Rovo action is person-facing; triggers, consumers, scheduled triggers and
// the web trigger are background (SPEC §2.1). An unlabelled request is reported as such, never guessed.
function kindOf(caller) {
  if (caller.source === 'resolver' || caller.source === 'frontend' || caller.moduleType === 'action') return 'person';
  if (BACKGROUND.has(caller.moduleType)) return 'background';
  return 'unlabelled';
}

function latencyOf(opKey) {
  if (SEARCH.has(opKey)) return MODEL.latencyMs.search;
  if (opKey === BULK_CHANGELOG) return MODEL.latencyMs.bulkfetch;
  return methodOf(opKey) === 'GET' ? MODEL.latencyMs.get : MODEL.latencyMs.write;
}

function createRate(model = MODEL) {
  let hours;
  let buckets;
  let lastWrite;
  const reset = () => { hours = new Map(); buckets = new Map(); lastWrite = new Map(); };
  reset();
  const hourOf = (t) => Math.floor(t / HOUR_MS);
  const hour = (t) => {
    const h = hourOf(t);
    if (!hours.has(h)) hours.set(h, { hour: h, person: 0, background: 0, unlabelled: 0, external: 0, requests: 0, refused: {} });
    return hours.get(h);
  };
  const spent = (h) => h.person + h.background + h.unlabelled + h.external;
  // Tokens refill continuously; a request older than the bucket's last update refills nothing.
  const bucket = (endpoint, t) => {
    const b = buckets.get(endpoint) ?? { tokens: model.burst.capacity, at: t };
    return { tokens: Math.min(model.burst.capacity, b.tokens + (Math.max(0, t - b.at) / 1000) * model.burst.refillPerSecond), at: Math.max(b.at, t) };
  };
  const refuse = (h, kind, reason, seconds) => {
    const byKind = (h.refused[kind] ??= {});
    byKind[reason] = (byKind[reason] ?? 0) + 1;
    return { reason, retryAfter: Math.max(1, Math.ceil(seconds)) };
  };

  // Quota first (the wall refuses everything), then the endpoint's bucket, then the per-issue window. `cost` is what is
  // known before serving (a search's per-issue points are charged after it).
  const check = ({ t, endpoint, kind, cost, issues = [] }) => {
    const h = hour(t);
    if (spent(h) >= model.quotaPerHour) return refuse(h, kind, model.reasons.quota, ((hourOf(t) + 1) * HOUR_MS - t) / 1000);
    const b = bucket(endpoint, t);
    if (b.tokens < cost) return refuse(h, kind, model.reasons.burst, (cost - b.tokens) / model.burst.refillPerSecond);
    for (const id of issues) {
      const last = lastWrite.get(id);
      if (last !== undefined && t - last < model.perIssueWriteMs) return refuse(h, kind, model.reasons.perIssue, (model.perIssueWriteMs - (t - last)) / 1000);
    }
    return null;
  };
  const charge = ({ t, endpoint, kind, cost, issues = [] }) => {
    const h = hour(t);
    h[kind] += cost;
    h.requests += 1;
    const b = bucket(endpoint, t);
    buckets.set(endpoint, { tokens: b.tokens - cost, at: b.at });
    for (const id of issues) lastWrite.set(id, Math.max(lastWrite.get(id) ?? -Infinity, t));
  };
  const headers = (t) => {
    const remaining = model.quotaPerHour - spent(hour(t));
    return { 'X-RateLimit-Limit': String(model.quotaPerHour),
      ...(remaining < model.quotaPerHour * model.nearLimitShare ? { 'X-RateLimit-Remaining': String(Math.max(0, remaining)), 'X-RateLimit-NearLimit': 'true' } : {}) };
  };
  const draw = ({ t, points }) => {
    if (!(Number.isFinite(points) && points > 0)) throw new Error(`draw needs a positive number of points, got ${points}`);
    const h = hour(t);
    h.external += points;
    return { hour: new Date(h.hour * HOUR_MS).toISOString(), spent: spent(h) };
  };
  const summary = () => [...hours.values()].sort((a, b) => a.hour - b.hour).map((h) => ({
    hour: new Date(h.hour * HOUR_MS).toISOString(), person: h.person, background: h.background, unlabelled: h.unlabelled,
    external: h.external, total: spent(h), requests: h.requests, backgroundShare: h.background / model.quotaPerHour,
    refused: JSON.parse(JSON.stringify(h.refused)) }));
  return { model, check, charge, headers, draw, summary, reset };
}

if (require.main === module && process.argv.includes('--model')) process.stdout.write(JSON.stringify(MODEL, null, 1) + '\n');

module.exports = { MODEL, createRate, costOf, writtenRefs, kindOf, latencyOf, HOUR_MS };
