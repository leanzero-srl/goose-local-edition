import { createHash, createHmac, timingSafeEqual } from 'node:crypto';
import { kvs } from '@forge/kvs';
import { jiraJson, route, postJson } from './jira';
import { markDeployed, migrationComplete, DEPLOY_ENVIRONMENTS } from './ledger';
import { SECRET_KEY } from './settings';

const MAX_SKEW_SECONDS = 300;
const ENVIRONMENTS = new Set(DEPLOY_ENVIRONMENTS);
const MAX_KEYS = 100; // one issue bulkfetch

// The static web trigger's declared outputs (manifest webtrigger.response.outputs). The dynamic-shape
// fields mirror the same answer for a host that reads them.
const OUTPUTS = {
  accepted: { statusCode: 202, body: '{"status":"accepted"}' },
  duplicate: { statusCode: 200, body: '{"status":"duplicate"}' },
  unauthorized: { statusCode: 401, body: '{"error":"invalid signature"}' },
  invalid: { statusCode: 400, body: '{"error":"invalid event"}' },
};
const answer = (key) => ({ outputKey: key, ...OUTPUTS[key], headers: { 'Content-Type': ['application/json'] } });

// Header names are case-insensitive; Forge hands each header as an array of strings.
function header(request, name) {
  for (const [k, v] of Object.entries(request?.headers ?? {})) {
    if (k.toLowerCase() === name) return Array.isArray(v) ? (v.length === 1 ? String(v[0]) : null) : String(v);
  }
  return null;
}

// HMAC-SHA256(secret, "<timestamp>.<raw body>") over the body exactly as received, compared in constant time.
function signatureValid(secret, timestamp, rawBody, given) {
  const expected = Buffer.from(`sha256=${createHmac('sha256', secret).update(`${timestamp}.${rawBody}`, 'utf8').digest('hex')}`);
  const actual = Buffer.from(given.trim().toLowerCase());
  return actual.length === expected.length && timingSafeEqual(actual, expected);
}

function parseEvent(rawBody) {
  let e;
  try {
    e = JSON.parse(rawBody);
  } catch {
    return null;
  }
  const ok =
    e !== null &&
    typeof e === 'object' &&
    typeof e.eventId === 'string' &&
    e.eventId.length > 0 &&
    Number.isFinite(e.sentAt) &&
    ENVIRONMENTS.has(e.environment) &&
    Array.isArray(e.issueKeys) &&
    e.issueKeys.length <= MAX_KEYS &&
    e.issueKeys.every((k) => typeof k === 'string' && /^[A-Z][A-Z0-9_]*-\d+$/.test(k));
  return ok ? e : null;
}

// Verify, then claim the eventId atomically (FAIL_IF_EXISTS: a replay, sequential or concurrent, finds the
// claim and changes nothing), then apply. Nothing is read or written before the signature holds.
// `enqueue(body)` hands the application to the queue when it cannot run now.
export async function handleCiEvent(request, work, enqueue) {
  const rawBody = typeof request?.body === 'string' ? request.body : '';
  const timestamp = header(request, 'x-lz-timestamp');
  const signature = header(request, 'x-lz-signature');
  if (!timestamp || !signature || !/^\d+$/.test(timestamp)) return answer('unauthorized');
  if (Math.abs(Math.floor(Date.now() / 1000) - Number(timestamp)) > MAX_SKEW_SECONDS) return answer('unauthorized');
  const secret = await kvs.getSecret(SECRET_KEY);
  if (typeof secret !== 'string' || !secret) return answer('unauthorized');
  if (!signatureValid(secret, timestamp, rawBody, signature)) return answer('unauthorized');

  const event = parseEvent(rawBody);
  if (!event) return answer('invalid');
  const claim = `ci-event:${createHash('sha256').update(event.eventId).digest('hex')}`;
  if (await kvs.get(claim)) return answer('duplicate');
  try {
    await kvs.set(claim, { environment: event.environment, at: Date.now() }, { keyPolicy: 'FAIL_IF_EXISTS' });
  } catch (e) {
    if (await kvs.get(claim)) return answer('duplicate');
    throw e;
  }
  const deployment = { type: 'deploy', eventId: event.eventId, environment: event.environment, issueKeys: [...new Set(event.issueKeys)] };
  if (!(await migrationComplete())) {
    await enqueue(deployment);
    return answer('accepted');
  }
  try {
    await applyDeployment(deployment, work);
  } catch (e) {
    console.error(`ci: deployment ${event.eventId} handed to the queue: ${e.message}`);
    await enqueue(deployment);
  }
  return answer('accepted');
}

// Marks every ledger row of the named issues "Deployed to <environment>". Keys are resolved by Jira (asApp:
// a web trigger has no user); keys Jira does not know are logged and skipped.
export async function applyDeployment({ eventId, environment, issueKeys }, work) {
  if (!issueKeys.length) return { sprintIds: [] };
  const page = await jiraJson('app', route`/rest/api/3/issue/bulkfetch`, postJson({ issueIdsOrKeys: issueKeys, fields: ['key'] }), work);
  const ids = (page.issues ?? []).map((i) => String(i.id));
  if (ids.length < issueKeys.length) console.error(`ci: event ${eventId} names ${issueKeys.length - ids.length} issue(s) Jira does not know`);
  return { sprintIds: await markDeployed(ids, environment) };
}
