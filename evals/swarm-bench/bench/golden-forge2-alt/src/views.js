import { kvs } from '@forge/kvs';
import { jiraJson, route } from './jira';
import { listScrumBoards, listActiveSprints, isStarted, boardEstimateField, getSprintFromJira, loadConfig } from './config';
import { liveScope, loadFrozen, freezeSprint, sprintRows } from './scope';
import { byTime, envList, queryAll, DEPLOYS } from './ledger';
import { formatPoints, fromMicro, formatCreep, creepPercent, toMicro } from './numbers';
import { loadSettings } from './settings';
import { isKvsCode } from './budget';

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

// Team totals per active sprint of one board, ordered by startDate (ties by id). Sprint metadata and team totals are
// the same for everyone, so this reads as the app; each sprint's estimates use the field its board uses now.
export async function widgetView(boardId, policy) {
  const cfg = await loadConfig();
  const sprints = (await listActiveSprints(boardId, policy)).filter(isStarted);
  sprints.sort((a, b) => Date.parse(a.startDate) - Date.parse(b.startDate) || Number(a.id) - Number(b.id));
  const fields = new Map();
  const out = [];
  for (const sprint of sprints) {
    const origin = String(sprint.originBoardId ?? boardId);
    if (!fields.has(origin)) fields.set(origin, await boardEstimateField(origin, policy));
    const scope = await liveScope(String(sprint.id), fields.get(origin), cfg);
    out.push({ id: String(sprint.id), name: sprint.name, startDate: sprint.startDate, ...totalsView(scope.totals) });
  }
  return { boardId: String(boardId), sprints: out };
}

// null when Jira does not know the sprint.
export async function getSprint(sprintId, policy) {
  const s = await getSprintFromJira(sprintId, policy);
  if (!s) return null;
  return {
    id: String(s.id),
    name: s.name,
    state: s.state,
    startDate: s.startDate ?? null,
    completeDate: s.completeDate ?? null,
    boardId: s.originBoardId === undefined || s.originBoardId === null ? null : String(s.originBoardId),
    started: isStarted(s),
  };
}

// Which of these issues can the invoking person browse, right now? Read as them: issue bulkfetch leaves out issues
// that "aren't found or that the user doesn't have permission to view". Returns issueId -> current key.
export async function visibleIssues(issueIds, policy) {
  const visible = new Map();
  const ids = [...new Set(issueIds.map(String))];
  for (let i = 0; i < ids.length; i += 100) {
    const page = await jiraJson('user', 'POST /rest/api/3/issue/bulkfetch', route`/rest/api/3/issue/bulkfetch`, policy, {
      body: { issueIdsOrKeys: ids.slice(i, i + 100), fields: ['key'] },
    });
    for (const issue of page.issues ?? []) visible.set(String(issue.id), issue.key);
  }
  return visible;
}

async function deploymentsByKey() {
  const records = await queryAll(DEPLOYS, 'all', ['deploy']);
  return new Map(records.map((d) => [d.issueKey, envList(d.envs)]));
}

// A sprint's scope for its viewer: an active sprint live, a closed one as at its close (§12).
async function sprintScope(sprint, policy) {
  const cfg = await loadConfig();
  if (sprint.state === 'closed') {
    const frozen = (await loadFrozen(sprint.id)) ?? (await freezeSprint(sprint.id, { name: sprint.name, completeDate: sprint.completeDate, boardId: sprint.boardId }, cfg));
    const rows = (await sprintRows(sprint.id, cfg)).filter((r) => Object.hasOwn(frozen.points, r.changeId)).sort(byTime);
    return { rows, totals: frozen.totals, points: frozen.points };
  }
  return liveScope(sprint.id, await boardEstimateField(sprint.boardId, policy), cfg);
}

// What one person sees of a sprint: team totals, and only the changes to issues they can browse now, plus how many
// are hidden. Changes of deleted issues are listed to nobody and counted as hidden for nobody. Table order: at
// ascending, then changelog id.
export async function personView(sprint, policy) {
  const scope = await sprintScope(sprint, policy);
  const live = scope.rows.filter((r) => !r.deleted);
  const visible = live.length ? await visibleIssues(live.map((r) => r.issueId), policy) : new Map();
  const shown = live.filter((r) => visible.has(String(r.issueId)));
  const deployments = shown.length ? await deploymentsByKey() : new Map();
  const changes = shown.map((r) => {
    const issueKey = visible.get(String(r.issueId));
    const points = scope.points[r.changeId] ?? 0;
    return {
      changeId: r.changeId,
      issueId: String(r.issueId),
      issueKey,
      kind: r.kind,
      points,
      pointsText: formatPoints(toMicro(points)),
      at: iso(r.at),
      by: r.authorName || r.authorId,
      source: r.source,
      deployed: [...new Set([...envList(r.deployedEnvs), ...(deployments.get(issueKey) ?? []), ...(deployments.get(r.issueKey) ?? [])])],
    };
  });
  return { sprint, ...totalsView(scope.totals), hiddenCount: live.length - shown.length, changes };
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

async function inGroup(accountId, groupName, policy) {
  const groups = await jiraJson('app', 'GET /rest/api/3/user/groups', route`/rest/api/3/user/groups?accountId=${accountId}`, policy);
  const want = groupName.toLowerCase();
  return (groups ?? []).some((g) => String(g.name ?? '').toLowerCase() === want);
}

// One click (or a double click) is one comment: concurrent requests for the same viewer and change share one lock;
// only its holder posts. A lock older than any resolver can live is a dead holder's and is taken over.
const LOCK_STALE_MS = 30_000;
async function acquirePostLock(key) {
  for (let attempt = 0; attempt < 2; attempt += 1) {
    try {
      await kvs.set(key, { at: Date.now() }, { keyPolicy: 'FAIL_IF_EXISTS' });
      return true;
    } catch (e) {
      if (!isKvsCode(e, 'KEY_CONFLICT')) throw e;
      const held = await kvs.get(key);
      if (held && Date.now() - held.at < LOCK_STALE_MS) return false;
      await kvs.delete(key);
    }
  }
  return false;
}

export async function postSummary(sprint, changeId, accountId, policy) {
  const settings = await loadSettings();
  if (settings.commentGroup && !(await inGroup(accountId, settings.commentGroup, policy))) {
    return { ok: false, error: `Only members of the group "${settings.commentGroup}" may post the summary.` };
  }
  const view = await personView(sprint, policy);
  const change = view.changes.find((c) => c.changeId === String(changeId));
  if (!change) return { ok: false, error: 'Select a change you can see in this sprint first.' };
  const lockKey = `posting:${accountId}:${sprint.id}:${change.changeId}`;
  if (!(await acquirePostLock(lockKey))) return { ok: true, duplicate: true, issueKey: change.issueKey };
  try {
    await jiraJson('user', 'POST /rest/api/3/issue/{issueIdOrKey}/comment', route`/rest/api/3/issue/${change.issueId}/comment`, policy, {
      body: { body: summaryDoc(change.issueKey, sprint.name, view.text.creep, view.text) },
      issueId: change.issueId,
    });
  } finally {
    await kvs.delete(lockKey);
  }
  return { ok: true, issueKey: change.issueKey };
}
