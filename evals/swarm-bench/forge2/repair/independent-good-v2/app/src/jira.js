import api, { route, getAppContext } from '@forge/api';
import { kvs } from '@forge/kvs';
export { route };
export const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
export const remaining = () => getAppContext().invocationRemainingTimeInMillis();
export class RateLimited extends Error {
  constructor(seconds, reason = 'continuation') { super(`Continue after ${seconds}s (${reason})`); this.retryAfterSeconds = Math.max(1, Math.ceil(seconds)); this.reason = reason; }
}
export class JiraError extends Error {
  constructor(status, path, body) { super(`Jira ${status} on ${path}: ${body.slice(0, 300)}`); this.status = status; }
}
export const postJson = (body) => ({ method: 'POST', headers: { Accept: 'application/json', 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
export function retryAfterSeconds(res, attempt) {
  const value = res.headers.get('retry-after');
  if (value !== null && value.trim() !== '') {
    const n = Number(value); if (Number.isFinite(n)) return Math.max(1, Math.ceil(n));
    const at = Date.parse(value); if (Number.isFinite(at)) return Math.max(1, Math.ceil((at - Date.now()) / 1000));
  }
  return 2 ** Math.min(attempt, 6);
}
const nextHour = () => (Math.floor(Date.now() / 3600000) + 1) * 3600000;
function endpoint(path) { return String(path.value ?? path).split('?')[0].replace(/\/issue\/[^/]+/, '/issue/{issue}').replace(/\/board\/[^/]+/, '/board/{board}').replace(/\/sprint\/[^/]+/, '/sprint/{sprint}'); }
function estimateCost(path, init) {
  const p = String(path.value ?? path); const method = init.method ?? 'GET';
  const read = method === 'GET' || /\/(search\/jql|changelog\/bulkfetch|issue\/bulkfetch)$/.test(p);
  if (!read) return 1;
  if (/\/field$/.test(p)) return 100;
  let body = {}; if (init.body) body = JSON.parse(init.body);
  if (body.issueIdsOrKeys) return 1 + body.issueIdsOrKeys.length;
  const max = body.maxResults ?? Number(new URLSearchParams(p.split('?')[1]).get('maxResults'));
  return max > 0 ? max + 1 : /\/user\/groups/.test(p) ? 50 : 2;
}
function actualCost(body, path, init) {
  const p = String(path.value ?? path); const m = init.method ?? 'GET';
  if (m !== 'GET' && !/\/(search\/jql|changelog\/bulkfetch|issue\/bulkfetch)$/.test(p)) return 1;
  const n = Array.isArray(body) ? body.length : ['issues','values','issueChangeLogs','comments','histories','users','groups','projects','precomputations'].reduce((v,k) => Array.isArray(body?.[k]) ? Math.max(v, body[k].length) : v, -1);
  return 1 + (n < 0 ? 1 : n) * (/\/user\/groups/.test(p) ? 2 : 1);
}
export async function jiraJson(who, path, init = {}, policy = {}) {
  const key = `${init.method ?? 'GET'} ${endpoint(path)}`;
  for (let attempt = 0; ; attempt += 1) {
    if (remaining() < 3000) throw new RateLimited(1);
    const waits = await kvs.get('jira-waits') ?? {};
    const until = Math.max(waits[key] ?? 0, policy.background ? waits.global ?? 0 : 0);
    if (until > Date.now()) {
      const seconds = Math.ceil((until-Date.now())/1000);
      if (policy.background || seconds * 1000 + 3000 > remaining()) throw new RateLimited(seconds);
      await sleep(seconds*1000);
    }
    let budget;
    if (policy.background) {
      const settings = await kvs.get('settings') ?? { share: 70 };
      const stored = await kvs.get('jira-budget');
      const hour = Math.floor(Date.now()/3600000);
      budget = stored?.hour === hour ? stored : { hour, spent: 0 };
      if (budget.spent + estimateCost(path, init) > 2400 * settings.share / 100) throw new RateLimited((nextHour()-Date.now())/1000, 'background-share');
    }
    const client = who === 'user' ? api.asUser() : api.asApp();
    const res = await client.requestJira(path, init);
    if (res.status === 429) {
      const wait = retryAfterSeconds(res, attempt); const reason = res.headers.get('ratelimit-reason') ?? 'jira-burst-based';
      if (/jira-quota-(global|tenant)-based/.test(reason)) waits.global = Date.now() + wait*1000;
      else waits[key] = Date.now()+wait*1000;
      await kvs.set('jira-waits', waits);
      if (policy.background || wait*1000+3000 > remaining()) throw new RateLimited(wait, reason);
      await sleep(wait*1000); continue;
    }
    if (!res.ok) throw new JiraError(res.status, path.value ?? String(path), await res.text());
    const result = res.status === 204 ? undefined : await res.json();
    if (budget) { budget.spent += actualCost(result, path, init); await kvs.set('jira-budget', budget); }
    return result;
  }
}
