import { jiraJson, route, postJson } from './jira';
import { listScrumBoards, listActiveSprints, isStarted } from './config';
import { sprintTotals, visibleIssues, byTime, memberKey } from './ledger';
import { formatPoints, fromMicro, formatCreep, creepPercent } from './numbers';

const iso = (ms) => new Date(ms).toISOString();

function totalsView(totals) {
  return {
    text: {
      committed: formatPoints(totals.committed),
      added: formatPoints(totals.added),
      removed: formatPoints(totals.removed),
      creep: formatCreep(totals.creepTenths),
    },
    values: {
      committed: fromMicro(totals.committed),
      added: fromMicro(totals.added),
      removed: fromMicro(totals.removed),
      creepPercent: creepPercent(totals.creepTenths),
    },
  };
}

export async function boardsView(policy) {
  const boards = await listScrumBoards(policy);
  return boards.map((b) => ({ id: String(b.id), name: b.name }));
}

// Team totals per active sprint of one board, ordered by startDate. Sprint metadata is not issue data,
// and the totals are the same for everyone, so this reads as the app.
export async function widgetView(boardId, policy) {
  const sprints = (await listActiveSprints(boardId, policy)).filter(isStarted);
  sprints.sort((a, b) => Date.parse(a.startDate) - Date.parse(b.startDate) || a.id - b.id);
  const out = [];
  for (const sprint of sprints) {
    const { totals } = await sprintTotals(String(sprint.id));
    out.push({ id: String(sprint.id), name: sprint.name, startDate: sprint.startDate, ...totalsView(totals) });
  }
  return { boardId: String(boardId), sprints: out };
}

// null when Jira does not know the sprint.
export async function getSprint(sprintId, policy) {
  try {
    const sprint = await jiraJson('app', route`/rest/agile/1.0/sprint/${sprintId}`, undefined, policy);
    return { id: String(sprint.id), name: sprint.name, state: sprint.state, startDate: sprint.startDate ?? null, started: isStarted(sprint) };
  } catch (e) {
    if (e.status === 404) return null;
    throw e;
  }
}

// What one person sees of a sprint: team totals, and only the changes to issues they can browse
// (read as them) plus how many are hidden. Rows in table order: at ascending, then changelog id.
export async function personView(sprint, policy) {
  const { changes, members, totals } = await sprintTotals(sprint.id);
  const visible = changes.length ? await visibleIssues(changes.map((c) => c.issueId), policy) : new Map();
  const estimate = new Map(members.map((m) => [memberKey(m.sprintId, m.issueId), m.estimate]));
  const rows = changes
    .filter((c) => visible.has(c.issueId))
    .sort(byTime)
    .map((c) => {
      const points = estimate.get(memberKey(c.sprintId, c.issueId)) ?? 0;
      return {
        changeId: c.changeId,
        issueId: c.issueId,
        issueKey: visible.get(c.issueId),
        kind: c.kind,
        points,
        pointsText: formatPoints(Math.round(points * 1_000_000)),
        at: iso(c.at),
        by: c.authorName,
        source: c.source,
        deployedTo: c.deployedEnvs ? c.deployedEnvs.split(',') : [],
      };
    });
  return { sprint, ...totalsView(totals), hiddenCount: changes.length - rows.length, changes: rows };
}

export function summaryDoc(issueKey, sprintName, creepText, totalsText) {
  return {
    version: 1,
    type: 'doc',
    content: [
      {
        type: 'paragraph',
        content: [
          { type: 'text', text: 'Scope Ledger: ', marks: [{ type: 'strong' }] },
          { type: 'text', text: `${issueKey} is part of the scope change of sprint "${sprintName}". ` },
          { type: 'text', text: `Sprint scope creep: ${creepText} ` },
          { type: 'text', text: `(committed ${totalsText.committed}, added ${totalsText.added}, removed ${totalsText.removed} points).` },
        ],
      },
    ],
  };
}

// commentGroup (admin setting): when set, the comment is visible to that group only; empty = everyone who
// can browse the issue.
export async function postSummary(sprint, changeId, policy, commentGroup) {
  const { changes, totals } = await sprintTotals(sprint.id);
  const change = changes.find((c) => c.changeId === String(changeId));
  if (!change) return { ok: false, error: `Change ${changeId} is not in sprint ${sprint.name}.` };
  const visible = await visibleIssues([change.issueId], policy);
  if (!visible.has(change.issueId)) return { ok: false, error: 'You cannot browse this issue.' };
  const issueKey = visible.get(change.issueId);
  const view = totalsView(totals);
  const comment = { body: summaryDoc(issueKey, sprint.name, view.text.creep, view.text) };
  if (commentGroup) comment.visibility = { type: 'group', value: commentGroup };
  await jiraJson('user', route`/rest/api/3/issue/${change.issueId}/comment`, postJson(comment), policy);
  return { ok: true, issueKey };
}
