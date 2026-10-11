import api, { route } from '@forge/api';
import { kvs, Filter, FilterConditions } from '@forge/kvs';

export { route };

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
export const QUOTA = 2400;
const HOUR_MS = 3_600_000;
const DOSE_ENTITY = 'rate-dose';
const QUOTA_REASONS = new Set(['jira-quota-global-based', 'jira-quota-tenant-based']);
const BURST_REASON = 'jira-burst-based';
const ISSUE_REASON = 'jira-per-issue-on-write';
const WRITE_WINDOWS = [{ count: 20, ms: 2000 }, { count: 100, ms: 30_000 }];
const READ_POSTS = new Set(['/rest/api/3/search/jql', '/rest/api/3/changelog/bulkfetch', '/rest/api/3/issue/bulkfetch',
  '/rest/api/3/permissions/check', '/rest/api/3/permissions/project', '/rest/api/3/jql/match']);

export const hourOf = (ms) => Math.floor(ms / HOUR_MS);
export const secondsToNextHour = (ms = Date.now()) => Math.max(1, Math.ceil(((hourOf(ms) + 1) * HOUR_MS - ms) / 1000));
const pathOf = (path) => String(path?.value ?? path).split('?')[0];
const queryOf = (path) => new URLSearchParams(String(path?.value ?? path).split('?')[1] ?? '');
const bodyOf = (init) => init?.body ? JSON.parse(init.body) : undefined;
const isRead = (method, p) => method === 'GET' || READ_POSTS.has(p) || /^\/rest\/api\/3\/issue\/[^/]+\/changelog\/list$/.test(p);
const identityRead = (p) => /^\/rest\/api\/3\/(?:myself(?:$|\/)|user(?:$|\/)|group(?:$|\/)|mypermissions(?:$|\/)|permissions(?:$|\/)|project\/[^/]+\/role(?:$|\/))/.test(p);

export function returnedObjects(body) {
  if (Array.isArray(body)) return body.length;
  if (!body || typeof body !== 'object' || body.errorMessages || body.errors) return 0;
  for (const key of ['issues', 'values', 'issueChangeLogs', 'comments', 'histories', 'users', 'groups', 'projects']) {
    if (Array.isArray(body[key])) return body[key].length;
  }
  if (body.permissions && typeof body.permissions === 'object') return Object.keys(body.permissions).length;
  if (Array.isArray(body.globalPermissions) || Array.isArray(body.projectPermissions)) return (body.globalPermissions?.length ?? 0) + (body.projectPermissions?.length ?? 0);
  return Object.keys(body).length ? 1 : 0;
}

export function actualPoints(method, path, body) {
  const p = pathOf(path);
  return isRead(method, p) ? 1 + returnedObjects(body) * (identityRead(p) ? 2 : 1) : 1;
}

// Reserve the bounded response before sending, then settle against the records Jira actually returned.
// Unpaginated collection APIs need a caller-provided bound; a one-object default would undercharge them.
export function pointsOf(method, path, init) {
  const p = pathOf(path);
  if (!isRead(method, p)) return 1;
  const body = bodyOf(init);
  const q = queryOf(path);
  let objects = 1;
  if (p === '/rest/api/3/search/jql') objects = Number(method === 'GET' ? q.get('maxResults') : body?.maxResults) || 50;
  else if (p === '/rest/api/3/changelog/bulkfetch') objects = Math.min(body?.issueIdsOrKeys?.length ?? 0, body?.maxResults ?? 1000);
  else if (p === '/rest/api/3/issue/bulkfetch') objects = body?.issueIdsOrKeys?.length ?? 0;
  else if (/\/changelog\/list$/.test(p)) objects = body?.changelogIds?.length ?? 0;
  else if (p === '/rest/api/3/permissions/check') objects = (body?.globalPermissions?.length ?? 0) + (body?.projectPermissions ?? []).reduce((n, x) => n + (x.permissions?.length ?? 0), 0);
  else if (p === '/rest/api/3/mypermissions') objects = (q.get('permissions') ?? '').split(',').filter(Boolean).length;
  else if (q.has('maxResults')) objects = Number(q.get('maxResults'));
  else if (p === '/rest/api/3/field' || /\/user\/groups$/.test(p)) throw new Error(`Background read ${p} has no response bound; use a paginated API.`);
  if (!(Number.isInteger(objects) && objects >= 0)) throw new Error(`Invalid background response bound on ${p}`);
  return 1 + objects * (identityRead(p) ? 2 : 1);
}

const endpointOf = (method, path) => `${method} ${pathOf(path).replace(/\/[A-Z][A-Z0-9_]*-\d+(?=\/|$)/g, '/{key}').replace(/\/\d+(?=\/|$)/g, '/{id}')}`;
function writtenIssues(method, path, init) {
  const p = pathOf(path);
  if (isRead(method, p)) return [];
  if (p === '/rest/api/3/app/field/value' || /^\/rest\/api\/3\/app\/field\/[^/]+\/value$/.test(p)) return (bodyOf(init)?.updates ?? []).flatMap((u) => (u.issueIds ?? []).map(String));
  const m = p.match(/^\/rest\/api\/3\/issue\/([^/]+)(?:\/|$)/);
  return m ? [m[1]] : [];
}
const isConflict = (e) => e.code === 'KEY_CONFLICT' || e.code === 'CONDITIONAL_CHECK_FAILED';

// A conditional transaction on a strongly read entity is the admission boundary. Query indexes can
// lag five seconds; concurrent invocations cannot reserve against a query-summed spend.
export class Background {
  constructor({ sharePercent, limitSeconds }) {
    this.cap = Math.floor((QUOTA * sharePercent) / 100);
    this.limitMs = limitSeconds * 1000;
    this.startedAt = Date.now();
    this.reserveMs = this.limitMs / 10;
    this.buckets = new Map();
    this.writes = new Map();
    this.delayedIssues = new Map();
    this.background = true;
  }
  timeLeftMs() { return this.limitMs - (Date.now() - this.startedAt); }
  get maxWaitSeconds() { return Math.max(0, Math.floor((this.timeLeftMs() - this.reserveMs) / 1000)); }
  hasTimeFor(ms) { return this.timeLeftMs() - this.reserveMs > ms; }
  async flush() {}

  async mutateDose(hour, update) {
    const key = String(hour);
    for (;;) {
      const stored = await kvs.entity(DOSE_ENTITY).get(key);
      const current = stored ?? { spent: 0, pausedUntil: 0 };
      const next = update(current);
      try {
        if (!stored) await kvs.entity(DOSE_ENTITY).set(key, next, { keyPolicy: 'FAIL_IF_EXISTS' });
        else {
          const conditions = new Filter().and('spent', FilterConditions.equalTo(current.spent)).and('pausedUntil', FilterConditions.equalTo(current.pausedUntil));
          await kvs.transact().set(key, next, { entityName: DOSE_ENTITY, conditions }).execute();
        }
        return next;
      } catch (e) {
        if (!isConflict(e)) throw e;
      }
    }
  }

  async admit(method, path, init) {
    await this.pace(endpointOf(method, path), method);
    await this.spaceWrites(writtenIssues(method, path, init));
    const hour = hourOf(Date.now());
    const points = pointsOf(method, path, init);
    await this.mutateDose(hour, (current) => {
      const now = Date.now();
      if (!this.hasTimeFor(0)) throw new RateLimited(1, 'time');
      if (current.pausedUntil > now) throw new RateLimited(Math.ceil((current.pausedUntil - now) / 1000), 'paused');
      if (current.spent + points > this.cap) throw new RateLimited(secondsToNextHour(now), 'background-share');
      return { ...current, spent: current.spent + points };
    });
    return { hour, points };
  }

  async settle(reservation, actual) {
    if (actual > reservation.points) throw new Error(`Jira returned more records than the admitted bound (${actual} > ${reservation.points})`);
    const refund = reservation.points - actual;
    if (refund) await this.mutateDose(reservation.hour, (current) => ({ ...current, spent: current.spent - refund }));
  }

  async wait(ms, reason) {
    if (!this.hasTimeFor(ms)) throw new RateLimited(Math.max(1, Math.ceil(ms / 1000)), reason);
    await sleep(ms);
  }
  async pace(endpoint, method) {
    const rate = ['PUT', 'PATCH', 'DELETE'].includes(method) ? 50 : 100;
    const now = Date.now();
    const b = this.buckets.get(endpoint) ?? { tokens: rate, at: now, rate };
    b.tokens = Math.min(rate, b.tokens + ((now - b.at) / 1000) * b.rate);
    b.at = now;
    this.buckets.set(endpoint, b);
    if (b.tokens < 1) {
      await this.wait(Math.ceil(((1 - b.tokens) / b.rate) * 1000), 'time');
      b.tokens = 1;
      b.at = Date.now();
    }
    b.tokens -= 1;
  }
  slow(endpoint) {
    const b = this.buckets.get(endpoint) ?? { tokens: 0, at: Date.now(), rate: 100 };
    b.rate /= 2;
    b.tokens = 0;
    b.at = Date.now();
    this.buckets.set(endpoint, b);
  }
  async spaceWrites(ids) {
    if (!ids.length) return;
    const now = Date.now();
    let ready = now;
    for (const id of new Set(ids)) {
      const times = (this.writes.get(id) ?? []).filter((at) => now - at < WRITE_WINDOWS.at(-1).ms);
      for (const w of WRITE_WINDOWS) {
        const recent = times.filter((at) => now - at < w.ms);
        if (recent.length >= w.count) ready = Math.max(ready, recent[recent.length - w.count] + w.ms);
      }
      ready = Math.max(ready, this.delayedIssues.get(id) ?? now);
      this.writes.set(id, times);
    }
    if (ready > now) await this.wait(ready - now, 'time');
    for (const id of new Set(ids)) this.writes.get(id).push(Date.now());
  }
  delayIssues(ids, seconds) { for (const id of ids) this.delayedIssues.set(id, Date.now() + seconds * 1000); }
  async pause(seconds) {
    const until = Date.now() + seconds * 1000;
    await this.mutateDose(hourOf(Date.now()), (current) => ({ ...current, pausedUntil: Math.max(current.pausedUntil, until) }));
  }
}

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

const bodies = new WeakMap();
export async function jira(who, path, init = {}, work = { maxWaitSeconds: Infinity }) {
  const client = who === 'user' ? api.asUser() : api.asApp();
  const method = (init.method ?? 'GET').toUpperCase();
  for (let attempt = 0; ; attempt += 1) {
    const reservation = work.background ? await work.admit(method, path, init) : null;
    const res = await client.requestJira(path, init);
    if (res.status !== 429) {
      if (reservation) {
        const body = res.status === 204 ? undefined : await res.json();
        bodies.set(res, body);
        await work.settle(reservation, actualPoints(method, path, body));
      }
      return res;
    }
    if (reservation) await work.settle(reservation, 0);
    const wait = retryAfterSeconds(res, attempt);
    const reason = res.headers.get('ratelimit-reason') ?? '';
    if (work.background) {
      if (QUOTA_REASONS.has(reason)) {
        await work.pause(wait);
        throw new RateLimited(wait, reason);
      }
      if (reason === BURST_REASON) work.slow(endpointOf(method, path));
      if (reason === ISSUE_REASON) work.delayIssues(writtenIssues(method, path, init), wait);
    }
    if (wait > work.maxWaitSeconds) throw new RateLimited(wait, reason);
    await sleep(wait * 1000 + 50);
  }
}

export async function jiraJson(who, path, init, work) {
  const res = await jira(who, path, init, work);
  const cached = bodies.has(res);
  const body = cached ? bodies.get(res) : undefined;
  if (!res.ok) throw new JiraError(res.status, path.value ?? String(path), cached ? JSON.stringify(body) : await res.text());
  return cached ? body : res.status === 204 ? undefined : res.json();
}
export const postJson = (body) => ({ method: 'POST', headers: { Accept: 'application/json', 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
