import { assess, currentSprintIds, estimateOf, sprintEntries } from './model';
import { boardEstimationField, getSprint } from './jira';

export function issueFields(registry) {
  const est = new Set(Object.values(registry.boards).map((b) => b.estField).filter(Boolean));
  return [registry.sprintField, 'created', 'project', ...est];
}

export function estimateFieldOf(registry, sprint) {
  const board = registry.boards[String(sprint.originBoardId)];
  return board ? board.estField : null;
}

// The sprints an issue could hold changes for: where it is now, and every sprint its
// Sprint-field history ever named.
export function sprintsTouching(issue, sprintFieldId) {
  const ids = new Set(currentSprintIds(issue, sprintFieldId));
  for (const e of sprintEntries(issue, sprintFieldId)) for (const id of [...e.from, ...e.to]) ids.add(id);
  return ids;
}

export const isTracked = (s) => s && s.state === 'active' && !!s.startDate;

// An event can name a sprint the last hourly run did not know (started since, or on a board
// that is not scrum-listed): learn it from Jira instead of dropping its changes.
export async function learnSprints(jira, registry, ids) {
  let changed = false;
  for (const id of ids) {
    if (registry.sprints[id]) continue;
    const s = await getSprint(jira, id);
    if (!s) continue;
    registry.sprints[id] = { id: String(s.id), name: s.name, state: s.state, startDate: s.startDate || null, originBoardId: s.originBoardId != null ? String(s.originBoardId) : null };
    const boardId = registry.sprints[id].originBoardId;
    if (boardId && !registry.boards[boardId]) registry.boards[boardId] = { name: null, estField: await boardEstimationField(jira, boardId) };
    changed = true;
  }
  return changed;
}

// Rows for one issue against every tracked sprint it touches.
export function rowsForIssue(issue, registry, source) {
  const members = [];
  const changes = [];
  const projectId = issue.fields && issue.fields.project ? String(issue.fields.project.id) : null;
  for (const id of sprintsTouching(issue, registry.sprintField)) {
    const sprint = registry.sprints[id];
    if (!isTracked(sprint)) continue;
    const a = assess(issue, sprint, registry.sprintField);
    if (!a.everIn && !a.atStart) continue;
    members.push({
      sprintId: sprint.id,
      issueId: String(issue.id),
      issueNum: Number(issue.id),
      issueKey: issue.key,
      projectId,
      points: estimateOf(issue, estimateFieldOf(registry, sprint)),
      atStart: a.atStart,
      inNow: a.inNow,
      everIn: a.everIn,
    });
    for (const c of a.changes) {
      changes.push({
        sprintId: sprint.id,
        changeId: c.changeId,
        changeNum: Number(c.changeId),
        atMs: c.atMs,
        at: new Date(c.atMs).toISOString(),
        issueId: String(issue.id),
        issueKey: issue.key,
        projectId,
        kind: c.kind,
        authorId: c.authorId,
        authorName: c.authorName,
        source,
      });
    }
  }
  return { members, changes };
}

const MEMBER_FIELDS = ['issueKey', 'projectId', 'points', 'atStart', 'inNow', 'everIn'];
export const memberDiffers = (a, b) => !a || MEMBER_FIELDS.some((f) => a[f] !== b[f]);
