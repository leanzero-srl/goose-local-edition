import api, { route } from '@forge/api';
import { kvs, WhereConditions } from '@forge/kvs';
import { randomUUID } from 'node:crypto';

export { route };

// Raised instead of making (or repeating) a request: Jira answered 429, or background work must not spend
// now. `reason` is Jira's RateLimit-Reason, or one of the app's own: `background-share` (this hour's
// background points are spent), `paused` (a quota 429 paused all background work), `time` (the wait does
// not fit in what is left of this invocation).
export class RateLimited extends Error {
  constructor(retryAfterSeconds, reason = '') {
    super(`Jira rate limit (${reason || 'no reason'}): retry after ${retryAfterSeconds}s`);
    this.retryAfterSeconds = retryAfterSeconds;
    this.reason = reason;
  }
}

export class JiraError extends Error {
  constructor(status, path, body) {
    super(`Jira ${status} on ${path}: ${body.slice(0, 300)}`);
    this.status = status;
  }
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

// ---- the rate model (RATE-MODEL.json) ---------------------------------------------------------------

export const QUOTA = 2400; // points per installation per hour, reset at the top of the hour
const HOUR_MS = 3_600_000;
const BUCKET_CAPACITY = 30; // per endpoint (method + path template)
const BUCKET_REFILL = 5; // points per second
const PER_ISSUE_WRITE_MS = 2000; // at most one write per issue per 2 s
const QUOTA_REASON = 'jira-quota-tenant-based';
const BURST_REASON = 'jira-burst-based';
const ISSUE_REASON = 'jira-per-issue-on-write';

export const hourOf = (ms) => Math.floor(ms / HOUR_MS);
export const secondsToNextHour = (ms = Date.now()) => Math.max(1, Math.ceil(((hourOf(ms) + 1) * HOUR_MS - ms) / 1000));

const pathOf = (path) => String(path?.value ?? path).split('?')[0];
const queryOf = (path) => new URLSearchParams(String(path?.value ?? path).split('?')[1] ?? '');

function bodyOf(init) {
  try {
    return init?.body ? JSON.parse(init.body) : undefined;
  } catch {
    return undefined;
  }
}

// The points a request costs. For a search page this is the most it can cost (maxResults issues); the
// real cost is known once the page is read (chargeSearchPage).
export function pointsOf(method, path, init) {
  const p = pathOf(path);
  if (p === '/rest/api/3/search/jql') {
    const max = Number(method === 'GET' ? queryOf(path).get('maxResults') : bodyOf(init)?.maxResults) || 50;
    return 1 + Math.ceil(max / 50);
  }
  if (p === '/rest/api/3/changelog/bulkfetch') return 2;
  if (p === '/rest/api/3/app/field/value') {
    const updates = (bodyOf(init)?.updates ?? []).reduce((n, u) => n + (u.issueIds?.length ?? 0), 0);
    return 1 + Math.ceil(updates / 50);
  }
  return method === 'GET' ? 1 : 2;
}

const endpointOf = (method, path) =>
  `${method} ${pathOf(path)
    .replace(/\/[A-Z][A-Z0-9_]*-\d+(?=\/|$)/g, '/{key}')
    .replace(/\/\d+(?=\/|$)/g, '/{id}')}`;

// Issues a write touches, for the per-issue write limit.
function writtenIssues(method, path, init) {
  if (method === 'GET') return [];
  const p = pathOf(path);
  if (p === '/rest/api/3/app/field/value') return (bodyOf(init)?.updates ?? []).flatMap((u) => (u.issueIds ?? []).map(String));
  const m = p.match(/^\/rest\/api\/3\/issue\/([^/]+)\/(comment|properties)/);
  return m ? [m[1]] : [];
}

// ---- the background dose ------------------------------------------------------------------------------

const SPEND_PREFIX = 'dose:';
const PAUSED_KEY = 'dose-paused-until';

// Each background invocation records its own spend under dose:<hour>:<invocation>, so invocations running
// at the same time never overwrite each other's numbers; the hour's spend is the sum of those keys.
async function spentThisHour(hour, ownKey) {
  let total = 0;
  let cursor;
  do {
    let q = kvs.query().where('key', WhereConditions.beginsWith(`${SPEND_PREFIX}${hour}:`)).limit(100);
    if (cursor) q = q.cursor(cursor);
    const page = await q.getMany();
    for (const r of page.results) if (r.key !== ownKey) total += Number(r.value) || 0;
    cursor = page.nextCursor;
  } while (cursor);
  return total;
}

// One per background invocation: the hourly share (settings.backgroundShare % of the quota), the pause a
// quota 429 sets, the per-endpoint pacing and the per-issue write spacing, and the invocation's own time
// limit (no wait is started that would not end before it).
export class Background {
  constructor({ sharePercent, limitSeconds }) {
    this.cap = Math.floor((QUOTA * sharePercent) / 100);
    this.limitMs = limitSeconds * 1000;
    this.startedAt = Date.now();
    this.reserveMs = this.limitMs / 10;
    this.id = randomUUID();
    this.hour = null;
    this.others = 0;
    this.mine = 0;
    this.flushedMine = 0;
    this.pausedUntil = 0;
    this.buckets = new Map();
    this.lastWrite = new Map();
    this.background = true;
  }

  timeLeftMs() {
    return this.limitMs - (Date.now() - this.startedAt);
  }

  // Seconds this invocation may still spend waiting in-function.
  get maxWaitSeconds() {
    return Math.max(0, Math.floor((this.timeLeftMs() - this.reserveMs) / 1000));
  }

  hasTimeFor(ms) {
    return this.timeLeftMs() - this.reserveMs > ms;
  }

  key() {
    return `${SPEND_PREFIX}${this.hour}:${this.id}`;
  }

  async sync() {
    const hour = hourOf(Date.now());
    if (this.hour !== hour) {
      if (this.hour !== null) await this.flush();
      this.hour = hour;
      this.mine = 0;
      this.flushedMine = 0;
    }
    this.others = await spentThisHour(hour, this.key());
    this.pausedUntil = Number(await kvs.get(PAUSED_KEY)) || 0;
  }

  async flush() {
    if (this.hour === null || this.mine === this.flushedMine) return;
    await kvs.set(this.key(), this.mine);
    this.flushedMine = this.mine;
  }

  async admit(method, path, init) {
    if (this.hour !== hourOf(Date.now()) || this.mine - this.flushedMine >= 50) {
      await this.flush();
      await this.sync();
    }
    const now = Date.now();
    if (this.pausedUntil > now) throw new RateLimited(Math.ceil((this.pausedUntil - now) / 1000), 'paused');
    const cost = pointsOf(method, path, init);
    if (this.others + this.mine + cost > this.cap) throw new RateLimited(secondsToNextHour(now), 'background-share');
    await this.pace(endpointOf(method, path), cost);
    await this.spaceWrites(writtenIssues(method, path, init));
    this.mine += cost;
    return cost;
  }

  refund(points) {
    this.mine = Math.max(0, this.mine - points);
  }

  async wait(ms, reason) {
    if (!this.hasTimeFor(ms)) throw new RateLimited(Math.max(1, Math.ceil(ms / 1000)), reason);
    await sleep(ms);
  }

  // A local mirror of Jira's per-endpoint token bucket: wait for the tokens instead of drawing a burst 429.
  async pace(endpoint, cost) {
    const now = Date.now();
    const b = this.buckets.get(endpoint) ?? { tokens: BUCKET_CAPACITY, at: now, rate: BUCKET_REFILL };
    b.tokens = Math.min(BUCKET_CAPACITY, b.tokens + ((now - b.at) / 1000) * b.rate);
    b.at = now;
    this.buckets.set(endpoint, b);
    if (b.tokens < cost) {
      await this.wait(Math.ceil(((cost - b.tokens) / b.rate) * 1000), 'time');
      b.tokens = cost;
      b.at = Date.now();
    }
    b.tokens -= cost;
  }

  slow(endpoint) {
    const b = this.buckets.get(endpoint) ?? { tokens: 0, at: Date.now(), rate: BUCKET_REFILL };
    b.rate = Math.max(b.rate / 2, BUCKET_REFILL / 10);
    b.tokens = 0;
    b.at = Date.now();
    this.buckets.set(endpoint, b);
  }

  async spaceWrites(issueIds) {
    if (!issueIds.length) return;
    const now = Date.now();
    const ready = Math.max(...issueIds.map((id) => (this.lastWrite.get(id) ?? -Infinity) + PER_ISSUE_WRITE_MS));
    if (ready > now) await this.wait(ready - now, 'time');
    for (const id of issueIds) this.lastWrite.set(id, Date.now());
  }

  delayIssues(issueIds, seconds) {
    const until = Date.now() + seconds * 1000;
    for (const id of issueIds) this.lastWrite.set(id, until - PER_ISSUE_WRITE_MS);
  }

  async pause(seconds) {
    this.pausedUntil = Date.now() + seconds * 1000;
    await kvs.set(PAUSED_KEY, this.pausedUntil);
  }
}

// ---- requests -----------------------------------------------------------------------------------------

// Retry-After is either delta-seconds or an HTTP-date (RFC 9110 §10.2.3). Without the header we back off
// exponentially from one second, never retrying immediately.
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

// `work` is a Background (background work: dosed, paced, bounded by the invocation's time) or a plain
// { maxWaitSeconds } for person-facing requests, which are never held back for the background share.
export async function jira(who, path, init = {}, work = { maxWaitSeconds: Infinity }) {
  const client = who === 'user' ? api.asUser() : api.asApp();
  const method = (init.method ?? 'GET').toUpperCase();
  for (let attempt = 0; ; attempt += 1) {
    if (work.background) await work.admit(method, path, init);
    const res = await client.requestJira(path, init);
    if (res.status !== 429) return res;
    const wait = retryAfterSeconds(res, attempt);
    const reason = res.headers.get('ratelimit-reason') ?? '';
    if (work.background) {
      if (reason === QUOTA_REASON) {
        await work.pause(wait);
        throw new RateLimited(wait, reason);
      }
      if (reason === BURST_REASON) work.slow(endpointOf(method, path));
      if (reason === ISSUE_REASON) work.delayIssues(writtenIssues(method, path, init), wait);
    }
    if (wait > work.maxWaitSeconds) throw new RateLimited(wait, reason);
    // Strictly more than the header asks for, so "at least that long" holds on any clock granularity.
    await sleep(wait * 1000 + 50);
  }
}

export async function jiraJson(who, path, init, work) {
  const res = await jira(who, path, init, work);
  if (!res.ok) throw new JiraError(res.status, path.value ?? String(path), await res.text());
  const body = res.status === 204 ? undefined : await res.json();
  // A search page costs 1 + 1 per 50 issues returned: give back what the worst case over-reserved.
  if (work?.background && pathOf(path) === '/rest/api/3/search/jql') {
    work.refund(pointsOf((init?.method ?? 'GET').toUpperCase(), path, init) - (1 + Math.ceil((body?.issues?.length ?? 0) / 50)));
  }
  return body;
}

export const postJson = (body) => ({
  method: 'POST',
  headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
  body: JSON.stringify(body),
});
