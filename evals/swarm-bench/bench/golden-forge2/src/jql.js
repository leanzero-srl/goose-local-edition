import { kvs, WhereConditions } from '@forge/kvs';
import { Queue } from '@forge/events';
import { changesOfSprint, QUERY_LAG_MS } from './ledger';
import { getSprint } from './views';
import { jiraJson, route, postJson } from './jira';

export const SCOPE_FUNCTION_NAME = 'scopeChanged';
const REGISTRY_PREFIX = 'scope-jql:';
const REFRESH_CONCURRENCY = { key: 'scope-jql-refresh', limit: 1 };
const registryPrefix = (sprintId) => `${REGISTRY_PREFIX}${sprintId}:`;
const registryKey = (sprintId, id) => `${registryPrefix(sprintId)}${id}`;

// Updates to a shared precomputation are ordered across all queues in the installation. Relevant
// events can coalesce in the current ledger view; a later duplicate sees the last successful value.
export async function queueScopeRefresh(sprintIds) {
  const ids = [...new Set(sprintIds.map(String))];
  if (!ids.length) return;
  return new Queue({ key: 'scope-ledger' }).push({ body: { type: 'jql-refresh', sprintIds: ids },
    delayInSeconds: QUERY_LAG_MS / 1000, concurrency: REFRESH_CONCURRENCY });
}

async function changedIds(sprintId, work, validateSprint = true) {
  if (validateSprint) {
    const sprint = await getSprint(sprintId, work);
    if (!sprint) throw new Error(`No sprint with id ${sprintId} exists on this site.`);
    if (!sprint.started) return [];
  }
  const rows = await changesOfSprint(sprintId);
  return [...new Set(rows.filter((r) => r.deleted !== true).map((r) => String(r.issueId)))].sort((a, b) => Number(a) - Number(b));
}

function fragment(ids, operator) {
  // Both expressions are valid native clauses even when the sprint has no changes. Their logical
  // truth does not depend on whether Jira has an issue with ID 1.
  if (!ids.length) return operator === 'not in' ? '(id = 1 OR id != 1)' : '(id = 1 AND id != 1)';
  return `id ${operator} (${ids.join(',')})`;
}

function argument(clause) {
  if (!clause || !['key', 'issue', 'issuekey', 'id'].includes(clause.field)
      || !['in', 'not in'].includes(clause.operator) || clause.functionName !== SCOPE_FUNCTION_NAME
      || !Array.isArray(clause.arguments) || clause.arguments.length !== 1
      || typeof clause.arguments[0] !== 'string' || !/^\d+$/.test(clause.arguments[0])) {
    throw new Error('scopeChanged requires one numeric sprint ID with issue IN or NOT IN.');
  }
  return clause.arguments[0];
}

export async function scopeChanged(request) {
  try {
    const sprintId = argument(request?.clause);
    if (typeof request.precomputationId !== 'string' || !request.precomputationId) {
      throw new Error('Jira did not supply the precomputation ID.');
    }
    const value = fragment(await changedIds(sprintId, { maxWaitSeconds: 0 }), request.clause.operator);
    const key = registryKey(sprintId, request.precomputationId);
    try {
      await kvs.set(key, { id: request.precomputationId, sprintId, operator: request.clause.operator, value },
        { keyPolicy: 'FAIL_IF_EXISTS' });
    } catch (e) {
      if (e.code !== 'KEY_CONFLICT') throw e;
      const stored = await kvs.get(key);
      if (!stored || stored.sprintId !== sprintId || stored.operator !== request.clause.operator) throw e;
    }
    // A write can race this initial evaluation's eventual ledger query. Refresh after both the
    // registry and preceding ledger writes become queryable, even if no further issue event comes.
    await queueScopeRefresh([sprintId]);
    return { jql: value };
  } catch (e) {
    return { error: `Could not compute scopeChanged: ${e.message}` };
  }
}

async function registeredPrecomputations(sprintId) {
  const records = [];
  let cursor;
  do {
    let query = kvs.query().where('key', WhereConditions.beginsWith(registryPrefix(sprintId))).limit(100);
    if (cursor) query = query.cursor(cursor);
    const page = await query.getMany();
    for (const { key } of page.results) {
      // The index can still hold a value from before the last successful update or deletion.
      const value = await kvs.get(key);
      if (value) records.push({ key, ...value });
    }
    cursor = page.nextCursor;
  } while (cursor);
  return records;
}

// Jira supplies each precomputationId at initial evaluation. Keeping those IDs avoids a site-wide
// precomputation GET after every issue event. Shared team IDs are never filtered by the caller.
export async function refreshScopePrecomputations(sprintIds, work) {
  const updates = [];
  const records = new Map();
  for (const sprintId of new Set(sprintIds.map(String))) {
    const precomputations = await registeredPrecomputations(sprintId);
    if (!precomputations.length) continue;
    const ids = await changedIds(sprintId, work, false);
    for (const pre of precomputations) {
      const value = fragment(ids, pre.operator);
      if (pre.value !== value) {
        updates.push({ id: pre.id, value });
        records.set(pre.id, { key: pre.key, value: { id: pre.id, sprintId, operator: pre.operator, value } });
      }
    }
  }
  if (!updates.length) return;
  const result = await jiraJson('app', route`/rest/api/3/jql/function/computation?skipNotFoundPrecomputations=true`,
    postJson({ values: updates }), work);
  if (!result || !Array.isArray(result.notFoundPrecomputationIDs)) throw new Error('Jira did not confirm the precomputation update.');
  const missing = new Set(result.notFoundPrecomputationIDs);
  for (const [id, record] of records) {
    if (missing.has(id)) await kvs.delete(record.key);
    else await kvs.set(record.key, record.value);
  }
}
