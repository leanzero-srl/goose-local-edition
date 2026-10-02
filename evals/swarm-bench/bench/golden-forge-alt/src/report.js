import { route } from '@forge/api';
import { appClient, getSprint, browsableIssueIds, boardSprints } from './jira';
import { sprintMembers, sprintChanges } from './store';
import { summarize, tableOrder } from './model';

const sprintHead = (s) => ({ id: String(s.id), name: s.name, state: s.state, startDate: s.startDate || null });
const started = (s) => s.state !== 'future' && !!s.startDate;

async function teamSummary(sprintId) {
  const members = await sprintMembers(sprintId);
  return { members, summary: summarize(members) };
}

// One sprint as a person sees it: team totals for everyone, changes only on issues the
// person can browse (asApp + an explicit BROWSE_PROJECTS check for their accountId).
export async function sprintReport(sprintId, accountId) {
  const jira = appClient({ waitBudgetSeconds: 20 });
  const sprint = await getSprint(jira, sprintId);
  if (!sprint) return { error: `Sprint ${sprintId} was not found.` };
  if (!started(sprint)) return { sprint: sprintHead(sprint), notStarted: true };
  const [{ members, summary }, changes] = await Promise.all([teamSummary(sprint.id), sprintChanges(sprint.id)]);
  const byIssue = new Map(members.map((m) => [m.issueId, m]));
  const allowed = changes.length && accountId ? await browsableIssueIds(jira, accountId, changes.map((c) => c.issueId)) : new Set();
  const visible = changes
    .filter((c) => allowed.has(c.issueId))
    .sort(tableOrder)
    .map((c) => {
      const m = byIssue.get(c.issueId);
      return { changeId: c.changeId, issueKey: (m && m.issueKey) || c.issueKey, points: m ? m.points : 0, kind: c.kind, by: c.authorName, at: c.at, source: c.source };
    });
  return { sprint: sprintHead(sprint), summary, hiddenChanges: changes.length - visible.length, changes: visible };
}

export async function boardReport(boardId) {
  const jira = appClient({ waitBudgetSeconds: 20 });
  const sprints = (await boardSprints(jira, boardId, 'active'))
    .filter(started)
    .sort((a, b) => Date.parse(a.startDate) - Date.parse(b.startDate) || Number(a.id) - Number(b.id));
  const out = [];
  for (const s of sprints) out.push({ ...sprintHead(s), summary: (await teamSummary(String(s.id))).summary });
  return { boardId: String(boardId), sprints: out };
}

export async function listScrumBoards() {
  const jira = appClient({ waitBudgetSeconds: 20 });
  const boards = [];
  for (let startAt = 0; ; ) {
    const page = await jira.json(route`/rest/agile/1.0/board?type=scrum&startAt=${startAt}&maxResults=50`);
    for (const b of page.values || []) boards.push({ id: String(b.id), name: b.name });
    if (page.isLast || !(page.values || []).length) break;
    startAt += page.values.length;
  }
  return boards;
}
