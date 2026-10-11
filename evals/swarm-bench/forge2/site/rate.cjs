'use strict';
// Jira's published cost classes and rate-limit semantics, on the benchmark's virtual clock. The 2,400-point
// installation allowance and one-second burst-buffer capacities are benchmark workloads, not Atlassian quotas.
// https://developer.atlassian.com/cloud/jira/platform/rate-limiting/ (verified 2026-10-11)
const HOUR_MS = 3_600_000;

const MODEL = {
  quotaPerHour: 2400,
  quotaScope: 'global',
  backgroundShare: 0.7,
  nearLimitShare: 0.2,
  burst: {
    requestsPerSecond: { GET: 100, POST: 100, PUT: 50, PATCH: 50, DELETE: 50 },
    endpointRequestsPerSecond: {
      'GET /rest/api/3/issue/{issueIdOrKey}': 150,
      'GET /rest/api/3/issue/{issueIdOrKey}/changelog': 200,
    },
    // Atlassian publishes refill rates, not bucket capacities. The harness gives one second of initial capacity.
    capacitySeconds: 1,
  },
  perIssueWrites: [{ maxWrites: 20, windowMs: 2000 }, { maxWrites: 100, windowMs: 30_000 }],
  fieldUpdatesPerRequest: 200,
  costs: { base: 1, coreObject: 1, identityObject: 2, write: 1 },
  latencyMs: { get: 120, search: 300, bulkfetch: 600, write: 200 },
  body429: { errorMessages: ['Rate limit exceeded'] },
  reasons: { quota: 'jira-quota-global-based', quotaGlobal: 'jira-quota-global-based', quotaTenant: 'jira-quota-tenant-based',
    burst: 'jira-burst-based', perIssue: 'jira-per-issue-on-write' },
};

const SEARCH = new Set(['GET /rest/api/3/search/jql', 'POST /rest/api/3/search/jql']);
const BULK_CHANGELOG = 'POST /rest/api/3/changelog/bulkfetch';
const FIELD_VALUES = new Set(['POST /rest/api/3/app/field/value', 'PUT /rest/api/3/app/field/{fieldIdOrKey}/value']);
const READ_POSTS = new Set([...SEARCH, BULK_CHANGELOG, 'POST /rest/api/3/issue/bulkfetch',
  'POST /rest/api/3/issue/{issueIdOrKey}/changelog/list', 'POST /rest/api/3/permissions/check',
  'POST /rest/api/3/permissions/project', 'POST /rest/api/3/jql/match']);
const BACKGROUND = new Set(['trigger', 'consumer', 'scheduledTrigger', 'webtrigger']);
const methodOf = (opKey) => opKey.slice(0, opKey.indexOf(' '));
const isRead = (opKey) => methodOf(opKey) === 'GET' || READ_POSTS.has(opKey);
const fieldUpdateIssues = (body) => (Array.isArray(body?.updates) ? body.updates : [])
  .flatMap((u) => (Array.isArray(u?.issueIds) ? u.issueIds : []));
const identityRead = (opKey) => / \/rest\/api\/3\/(?:myself(?:$|\/)|user(?:$|\/)|group(?:$|\/)|mypermissions(?:$|\/)|permissions(?:$|\/)|project\/[^/]+\/role(?:$|\/))/.test(opKey);

// Count returned domain records, not nested fields, changelog items, pagination metadata or the request envelope.
// Atlassian's catalogue gives cost classes rather than a complete endpoint tariff; this count is the disclosed
// benchmark interpretation of those classes. Read-like POST operations are reads, irrespective of their HTTP verb.
function returnedObjects(response) {
  if (Number.isInteger(response) && response >= 0) return response;
  if (Array.isArray(response)) return response.length;
  if (!response || typeof response !== 'object' || response.errorMessages || response.errors) return 0;
  for (const key of ['issues', 'values', 'issueChangeLogs', 'comments', 'histories', 'users', 'groups', 'projects']) {
    if (Array.isArray(response[key])) return response[key].length;
  }
  if (response.permissions && typeof response.permissions === 'object') return Object.keys(response.permissions).length;
  if (Array.isArray(response.globalPermissions) || Array.isArray(response.projectPermissions)) {
    return (response.globalPermissions?.length ?? 0) + (response.projectPermissions?.length ?? 0);
  }
  return Object.keys(response).length ? 1 : 0;
}

function costOf(opKey, _request, response = 0) {
  const c = MODEL.costs;
  return isRead(opKey) ? c.base + returnedObjects(response) * (identityRead(opKey) ? c.identityObject : c.coreObject) : c.write;
}

function writtenRefs(opKey, params, body) {
  if (FIELD_VALUES.has(opKey)) return fieldUpdateIssues(body).map(String);
  if (!isRead(opKey) && params.issueIdOrKey !== undefined) return [String(params.issueIdOrKey)];
  return [];
}

function kindOf(caller) {
  if (caller.source === 'resolver' || caller.source === 'frontend' || caller.moduleType === 'action') return 'person';
  if (BACKGROUND.has(caller.moduleType)) return 'background';
  return 'unlabelled';
}
const quotaCharged = (caller) => caller.source !== 'frontend';

function latencyOf(opKey) {
  if (SEARCH.has(opKey)) return MODEL.latencyMs.search;
  if (opKey === BULK_CHANGELOG) return MODEL.latencyMs.bulkfetch;
  return methodOf(opKey) === 'GET' ? MODEL.latencyMs.get : MODEL.latencyMs.write;
}

function issueWriteWait(history, t, windows = MODEL.perIssueWrites) {
  let waitMs = 0;
  for (const w of windows) {
    const recent = history.filter((at) => at <= t && t - at < w.windowMs).sort((a, b) => a - b);
    if (recent.length >= w.maxWrites) waitMs = Math.max(waitMs, recent[recent.length - w.maxWrites] + w.windowMs - t);
  }
  return waitMs;
}

function createRate(model = MODEL) {
  let hours;
  let buckets;
  let writes;
  const reset = () => { hours = new Map(); buckets = new Map(); writes = new Map(); };
  reset();
  const hourOf = (t) => Math.floor(t / HOUR_MS);
  const hour = (t) => {
    const h = hourOf(t);
    if (!hours.has(h)) hours.set(h, { hour: h, person: 0, background: 0, unlabelled: 0, external: 0, requests: 0, refused: {} });
    return hours.get(h);
  };
  const spent = (h) => h.person + h.background + h.unlabelled + h.external;
  const refillRate = (endpoint) => model.burst.endpointRequestsPerSecond[endpoint] ?? model.burst.requestsPerSecond[methodOf(endpoint)];
  const bucket = (endpoint, t) => {
    const refill = refillRate(endpoint);
    if (!(refill > 0)) throw new Error(`No burst request rate for ${endpoint}`);
    const capacity = refill * model.burst.capacitySeconds;
    const b = buckets.get(endpoint) ?? { tokens: capacity, at: t };
    return { tokens: Math.min(capacity, b.tokens + (Math.max(0, t - b.at) / 1000) * refill), at: Math.max(b.at, t), refill };
  };
  const refuse = (h, kind, reason, seconds) => {
    const byKind = (h.refused[kind] ??= {});
    byKind[reason] = (byKind[reason] ?? 0) + 1;
    return { reason, retryAfter: Math.max(1, Math.ceil(seconds)) };
  };

  const check = ({ t, endpoint, kind, issues = [], charged = true }) => {
    const h = hour(t);
    if (charged && spent(h) >= model.quotaPerHour) {
      const reason = model.quotaScope === 'tenant' ? model.reasons.quotaTenant : model.reasons.quotaGlobal;
      return refuse(h, kind, reason, ((hourOf(t) + 1) * HOUR_MS - t) / 1000);
    }
    const b = bucket(endpoint, t);
    if (b.tokens < 1) return refuse(h, kind, model.reasons.burst, (1 - b.tokens) / b.refill);
    let waitMs = 0;
    for (const id of new Set(issues)) {
      waitMs = Math.max(waitMs, issueWriteWait(writes.get(id) ?? [], t, model.perIssueWrites));
    }
    if (waitMs > 0) return refuse(h, kind, model.reasons.perIssue, waitMs / 1000);
    return null;
  };
  const charge = ({ t, endpoint, kind, cost, issues = [], charged = true }) => {
    const h = hour(t);
    if (charged) h[kind] += cost;
    h.requests += 1;
    const b = bucket(endpoint, t);
    buckets.set(endpoint, { tokens: b.tokens - 1, at: b.at });
    for (const id of new Set(issues)) {
      const history = writes.get(id) ?? [];
      history.push(t);
      history.sort((a, b) => a - b);
      writes.set(id, history);
    }
  };
  const headers = (t, endpoint = null) => {
    const remaining = Math.max(0, model.quotaPerHour - spent(hour(t)));
    const until = Math.ceil(((hourOf(t) + 1) * HOUR_MS - t) / 1000);
    const policy = model.quotaScope === 'tenant' ? 'tenant-app-quota' : 'global-app-quota';
    const near = remaining < model.quotaPerHour * model.nearLimitShare;
    return {
      'RateLimit-Policy': `"${policy}";q=${model.quotaPerHour};w=3600`,
      'RateLimit': `"${policy}";${near ? `r=${remaining};` : ''}t=${until}`,
      ...(near ? { 'X-RateLimit-NearLimit': 'true' } : {}),
      ...(endpoint ? { 'X-RateLimit-Limit': String(refillRate(endpoint)),
        'X-RateLimit-Remaining': String(Math.max(0, Math.floor(bucket(endpoint, t).tokens))) } : {}),
    };
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
module.exports = { MODEL, createRate, costOf, returnedObjects, issueWriteWait, writtenRefs, kindOf, quotaCharged, latencyOf, HOUR_MS };
