import { sprintReport } from './report';

const callerOf = (payload, context) =>
  (context && context.principal && context.principal.accountId) ||
  (context && context.accountId) ||
  (payload && payload.context && payload.context.accountId) ||
  null;

export async function getSprintScope(payload, context) {
  const raw = payload && payload.sprintId;
  const sprintId = raw == null ? '' : String(raw).trim();
  if (!/^\d+$/.test(sprintId)) return { error: 'sprintId is required: the numeric id of a Jira sprint, e.g. "41".' };
  try {
    const r = await sprintReport(sprintId, callerOf(payload, context));
    if (r.error) return { error: r.error };
    if (r.notStarted) return { error: `Sprint ${sprintId} (${r.sprint.name}) has not started yet, so it has no scope changes.` };
    return {
      sprintId: r.sprint.id,
      sprintName: r.sprint.name,
      committed: r.summary.committed,
      added: r.summary.added,
      removed: r.summary.removed,
      creepPercent: r.summary.creepPercent,
      hiddenChanges: r.hiddenChanges,
      changes: r.changes.map((c) => ({ changeId: c.changeId, issueKey: c.issueKey, kind: c.kind, points: c.points, at: c.at, by: c.by })),
    };
  } catch (err) {
    return { error: `Could not read sprint ${sprintId}: ${err.message}` };
  }
}
