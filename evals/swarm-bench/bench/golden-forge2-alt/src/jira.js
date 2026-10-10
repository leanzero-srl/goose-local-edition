import api, { route } from '@forge/api';
import { admit, refund, recordHold, recordQuotaBlock, Deferred, QUOTA_POINTS } from './budget';
import { msToNextHour } from './time';

export { route, Deferred };

// A person-facing request met a Retry-After it cannot sit out: the surface waits and calls again.
export class RateLimited extends Error {
  constructor(retryAfterSeconds) {
    super(`Jira rate limit: retry after ${retryAfterSeconds}s`);
    this.retryAfterSeconds = retryAfterSeconds;
  }
}

export class JiraError extends Error {
  constructor(status, path, body) {
    super(`Jira ${status} on ${path}: ${body.slice(0, 300)}`);
    this.status = status;
    this.body = body;
  }
}

// Retry-After is delta-seconds or an HTTP-date (RFC 9110 §10.2.3). Without one, back off exponentially from 1 s.
export function retryAfterSeconds(res, attempt) {
  const header = res.headers.get('retry-after');
  if (header !== null && header.trim() !== '') {
    const seconds = Number(header);
    if (Number.isFinite(seconds)) return Math.max(0, Math.ceil(seconds));
    const at = Date.parse(header);
    if (Number.isFinite(at)) return Math.max(0, Math.ceil((at - Date.now()) / 1000));
  }
  return 2 ** Math.min(attempt, 6);
}

const blocks = (n, per) => Math.ceil(n / per);

// RATE-MODEL.json: what one request costs, reserved before it is sent (a search is reserved at its page size and the
// unreturned part handed back).
export function reservedPoints(endpoint, body) {
  if (endpoint.endsWith(' /rest/api/3/search/jql')) return 1 + blocks(body?.maxResults ?? 50, 50);
  if (endpoint === 'POST /rest/api/3/changelog/bulkfetch') return 2;
  if (endpoint.startsWith('POST /rest/api/3/app/field/value') || endpoint.startsWith('PUT /rest/api/3/app/field/')) {
    const updates = (body?.updates ?? [{ issueIds: body?.issueIds ?? [] }]).reduce((n, u) => n + (u.issueIds?.length ?? 0), 0);
    return 1 + blocks(Math.max(updates, 1), 50);
  }
  return endpoint.startsWith('GET ') ? 1 : 2;
}

// Background reacts to the site's own warning too: when fewer points remain than person-facing work is promised
// (the share background leaves free), background stops for the hour.
async function watchRemaining(res, policy) {
  if (res.headers.get('x-ratelimit-nearlimit') !== 'true') return;
  const remaining = Number(res.headers.get('x-ratelimit-remaining'));
  const personReserve = QUOTA_POINTS - policy.limit;
  if (Number.isFinite(remaining) && remaining < personReserve) await recordQuotaBlock();
}

// endpoint: "<METHOD> <path template>" — the burst bucket a Retry-After applies to. opts.issueId names the issue a
// write lands on (per-issue Retry-After); opts.body is the JSON body (cost and serialisation).
export async function jira(who, endpoint, path, policy, opts = {}) {
  const client = who === 'user' ? api.asUser() : api.asApp();
  const method = endpoint.split(' ')[0];
  const init = { method, headers: { Accept: 'application/json' } };
  if (opts.body !== undefined) {
    init.headers['Content-Type'] = 'application/json';
    init.body = JSON.stringify(opts.body);
  }
  const points = reservedPoints(endpoint, opts.body);
  for (let attempt = 0; ; attempt += 1) {
    await admit(policy, endpoint, points, opts.issueId);
    const res = await client.requestJira(path, init);
    if (policy.background) await watchRemaining(res, policy);
    if (res.status !== 429) return { res, points };
    const reason = (res.headers.get('ratelimit-reason') ?? '').trim();
    const waitMs = retryAfterSeconds(res, attempt) * 1000;
    if (reason === 'jira-quota-tenant-based') {
      await recordQuotaBlock();
      if (policy.background) throw new Deferred(msToNextHour(), 'quota');
      throw new RateLimited(Math.ceil(msToNextHour() / 1000));
    }
    const holdKey = reason === 'jira-per-issue-on-write' && opts.issueId ? `i:${opts.issueId}` : `e:${endpoint}`;
    await recordHold(holdKey, Date.now() + waitMs);
  }
}

export async function jiraJson(who, endpoint, path, policy, opts) {
  try {
    const { res, points } = await jira(who, endpoint, path, policy, opts);
    if (!res.ok) throw new JiraError(res.status, path.value ?? String(path), await res.text());
    const data = res.status === 204 ? undefined : await res.json();
    if (policy.background && endpoint.endsWith(' /rest/api/3/search/jql')) {
      await refund(points - (1 + blocks(data?.issues?.length ?? 0, 50)));
    }
    return data;
  } catch (e) {
    if (e instanceof Deferred && !policy.background) throw new RateLimited(Math.ceil(e.waitMs / 1000));
    throw e;
  }
}
