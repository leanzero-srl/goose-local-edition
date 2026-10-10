import { createHmac, timingSafeEqual } from 'node:crypto';
import { kvs, Filter } from '@forge/kvs';
import { DEPLOYS, envList, envString, ledgerOfIssueKey, patchRow, rowKey } from './ledger';
import { isKvsCode } from './budget';
import { sleep } from './time';
import { QUERY_STALENESS_MS } from './sync';

export const CI_SECRET_KEY = 'ci-secret';
const MAX_SKEW_SECONDS = 300;
const ENVIRONMENTS = new Set(['staging', 'production']);

const header = (headers, name) => {
  for (const [k, v] of Object.entries(headers ?? {})) {
    if (k.toLowerCase() === name) return Array.isArray(v) ? String(v[0] ?? '') : String(v ?? '');
  }
  return '';
};

function signatureValid(secret, timestamp, body, given) {
  const expected = Buffer.from(`sha256=${createHmac('sha256', Buffer.from(secret, 'utf8')).update(`${timestamp}.${body}`, 'utf8').digest('hex')}`, 'utf8');
  const actual = Buffer.from(given, 'utf8');
  return actual.length === expected.length && timingSafeEqual(actual, expected);
}

function parseEvent(body) {
  let event;
  try {
    event = JSON.parse(body);
  } catch {
    return null;
  }
  if (!event || typeof event !== 'object') return null;
  if (typeof event.eventId !== 'string' || !event.eventId) return null;
  if (!ENVIRONMENTS.has(event.environment)) return null;
  if (!Array.isArray(event.issueKeys) || !event.issueKeys.every((k) => typeof k === 'string' && k)) return null;
  return event;
}

async function addEnvironment(issueKey, env) {
  for (;;) {
    const row = await kvs.entity(DEPLOYS).get(issueKey);
    const envs = envList(row?.envs);
    if (envs.includes(env)) return;
    const next = { kind: 'deploy', issueKey, envs: envString([...envs, env]), version: (row?.version ?? 0) + 1 };
    try {
      if (!row) await kvs.entity(DEPLOYS).set(issueKey, next, { keyPolicy: 'FAIL_IF_EXISTS' });
      else await kvs.transact().set(issueKey, next, { entityName: DEPLOYS, conditions: new Filter().and('version', { condition: 'EQUAL_TO', values: [row.version] }) }).execute();
      return;
    } catch (e) {
      if (!isKvsCode(e, 'KEY_CONFLICT', 'CONDITIONAL_CHECK_FAILED')) throw e;
    }
  }
}

// Records a deployment: each issue's environments first (rows written from now on pick them up), then, once the index
// shows every row written before that, the existing rows.
export async function applyDeployment(environment, issueKeys) {
  const keys = [...new Set(issueKeys)];
  for (const key of keys) await addEnvironment(key, environment);
  await sleep(QUERY_STALENESS_MS + 100);
  for (const key of keys) {
    for (const row of await ledgerOfIssueKey(key)) {
      if (envList(row.deployedEnvs).includes(environment)) continue;
      await patchRow(rowKey(row.changeId, row.sprintId), (r) => ({ ...r, deployedEnvs: envString([...envList(r.deployedEnvs), environment]) }));
    }
  }
}

// §14. Checks in order: signature (timing-safe), timestamp (±300 s), then the eventId (applied at most once, ever).
export async function handleDeployment(request) {
  const body = typeof request?.body === 'string' ? request.body : '';
  const signature = header(request?.headers, 'x-lz-signature');
  const timestamp = header(request?.headers, 'x-lz-timestamp');
  const secret = await kvs.getSecret(CI_SECRET_KEY);
  if (!secret || !signature || !timestamp) return { outputKey: 'unauthorized' };
  if (!signatureValid(secret, timestamp, body, signature)) return { outputKey: 'unauthorized' };
  const ts = Number(timestamp);
  if (!/^\d+$/.test(timestamp.trim()) || Math.abs(Date.now() / 1000 - ts) > MAX_SKEW_SECONDS) return { outputKey: 'unauthorized' };
  const event = parseEvent(body);
  if (!event) return { outputKey: 'invalid' };
  const claimKey = `ci-event:${event.eventId}`;
  try {
    await kvs.set(claimKey, { at: Date.now(), environment: event.environment, issueKeys: event.issueKeys, applied: false }, { keyPolicy: 'FAIL_IF_EXISTS' });
  } catch (e) {
    if (isKvsCode(e, 'KEY_CONFLICT')) return { outputKey: 'duplicate' };
    throw e;
  }
  await applyDeployment(event.environment, event.issueKeys);
  await kvs.set(claimKey, { at: Date.now(), environment: event.environment, issueKeys: event.issueKeys, applied: true });
  return { outputKey: 'accepted' };
}
