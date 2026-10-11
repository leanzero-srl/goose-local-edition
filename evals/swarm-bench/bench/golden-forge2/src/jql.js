import { changesOfSprint } from './ledger';
import { getSprint } from './views';
import { jiraJson, route, postJson } from './jira';

export const SCOPE_FUNCTION_NAME = 'scopeChanged';

async function changedIds(sprintId, work) {
  const sprint = await getSprint(sprintId, work);
  if (!sprint) throw new Error(`No sprint with id ${sprintId} exists on this site.`);
  if (!sprint.started) return [];
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
    return { jql: fragment(await changedIds(sprintId, { maxWaitSeconds: 0 }), request.clause.operator) };
  } catch (e) {
    return { error: `Could not compute scopeChanged: ${e.message}` };
  }
}

// Jira does not invoke a cached clause again. Consumers and reconciliation call this after their
// ledger writes become queryable; mappings are shared by every person and never browse-filtered here.
export async function refreshScopePrecomputations(sprintIds, work) {
  const touched = new Set(sprintIds.map(String));
  if (!touched.size) return;
  const idsBySprint = new Map();
  for (let startAt = 0; ; ) {
    const page = await jiraJson('app', route`/rest/api/3/jql/function/computation?startAt=${startAt}&maxResults=100`, undefined, work);
    const values = Array.isArray(page?.values) ? page.values : [];
    const updates = [];
    for (const pre of values) {
      if (pre.functionName !== SCOPE_FUNCTION_NAME || !touched.has(pre.arguments?.[0])) continue;
      const sprintId = argument({ field: pre.field, operator: pre.operator, functionName: pre.functionName, arguments: pre.arguments });
      if (!idsBySprint.has(sprintId)) idsBySprint.set(sprintId, await changedIds(sprintId, work));
      const value = fragment(idsBySprint.get(sprintId), pre.operator);
      if (pre.value !== value || pre.error !== undefined) updates.push({ id: pre.id, value });
    }
    if (updates.length) await jiraJson('app', route`/rest/api/3/jql/function/computation`, postJson({ values: updates }), work);
    if (page.isLast || !values.length) return;
    startAt += values.length;
  }
}
