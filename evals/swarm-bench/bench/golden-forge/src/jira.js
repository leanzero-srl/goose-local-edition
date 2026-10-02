import api, { route } from '@forge/api';

export { route };

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
  }
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

// Retry-After is either delta-seconds or an HTTP-date (RFC 9110 §10.2.3). Without the header we back
// off exponentially from one second, never retrying immediately.
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

// policy.maxWaitSeconds: the longest single Retry-After this invocation may sleep through. A longer
// one is raised as RateLimited so the caller can hand the wait to something that outlives it (the
// queue's retry request, or the Custom UI re-invoking the resolver).
export async function jira(who, path, init = {}, policy = { maxWaitSeconds: Infinity }) {
  const client = who === 'user' ? api.asUser() : api.asApp();
  for (let attempt = 0; ; attempt += 1) {
    const res = await client.requestJira(path, init);
    if (res.status !== 429) return res;
    const wait = retryAfterSeconds(res, attempt);
    if (wait > policy.maxWaitSeconds) throw new RateLimited(wait);
    // Strictly more than the header asks for, so "at least that long" holds on any clock granularity.
    await sleep(wait * 1000 + 50);
  }
}

export async function jiraJson(who, path, init, policy) {
  const res = await jira(who, path, init, policy);
  if (!res.ok) throw new JiraError(res.status, path.value ?? String(path), await res.text());
  return res.status === 204 ? undefined : res.json();
}

export const postJson = (body) => ({
  method: 'POST',
  headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
  body: JSON.stringify(body),
});
