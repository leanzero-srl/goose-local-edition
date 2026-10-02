import { jiraJson, route, postJson } from './jira';
import { resolveSprint } from './config';
import { changesOfSprint, membersOfSprint, membersOfIssue, recordChange, writeMember, changeKey } from './ledger';

// The Sprint field's issue value is an array of sprint objects (or ids); its changelog `from`/`to`
// hold the comma-separated sprint ids before and after the change.
export function sprintsOfField(value) {
  if (!Array.isArray(value)) return [];
  return value.map((s) => (s !== null && typeof s === 'object' ? { id: String(s.id), state: s.state } : { id: String(s), state: undefined }));
}

const idList = (s) => (s === null || s === undefined ? [] : String(s).split(',').map((x) => x.trim()).filter(Boolean));

const isSprintItem = (item, sprintFieldId) => (item.fieldId ? item.fieldId === sprintFieldId : item.field === 'Sprint');

export function sprintMoves(history, sprintFieldId) {
  const moves = [];
  for (const item of history.items ?? []) {
    if (!isSprintItem(item, sprintFieldId)) continue;
    const from = new Set(idList(item.from));
    const to = new Set(idList(item.to));
    for (const id of to) if (!from.has(id)) moves.push({ sprintId: id, kind: 'added' });
    for (const id of from) if (!to.has(id)) moves.push({ sprintId: id, kind: 'removed' });
  }
  return moves;
}

export const estimateFieldIds = (cfg) => [...new Set(Object.values(cfg.sprints).map((s) => s.estimateFieldId).filter(Boolean))];

export const issueFields = (cfg) => [cfg.sprintFieldId, ...estimateFieldIds(cfg)];

function estimateOf(fields, fieldId) {
  if (!fieldId) return 0;
  const v = fields?.[fieldId];
  const n = typeof v === 'number' ? v : v === null || v === undefined || v === '' ? 0 : Number(v);
  return Number.isFinite(n) ? n : 0;
}

export function changeRow(sprint, history, issue, kind, source) {
  const at = Date.parse(history.created);
  return {
    sprintId: sprint.id,
    changeId: String(history.id),
    at,
    created: new Date(at).toISOString(),
    issueId: String(issue.id),
    issueKey: issue.key ?? '',
    kind,
    authorId: history.author?.accountId ?? '',
    authorName: history.author?.displayName ?? '',
    source,
  };
}

// Rows for the active sprints this changelog entry moved the issue into or out of, after their start.
async function rowsOf(cfg, issue, histories, source, resolve) {
  const rows = [];
  for (const history of histories) {
    for (const move of sprintMoves(history, cfg.sprintFieldId)) {
      const sprint = await resolve(move.sprintId);
      if (!sprint) continue;
      if (!(Date.parse(history.created) > sprint.startMs)) continue;
      rows.push(changeRow(sprint, history, issue, move.kind, source));
    }
  }
  return rows;
}

export function memberRow(cfg, sprintId, issue, currentSprintIds) {
  const sprint = cfg.sprints[sprintId];
  return {
    sprintId,
    issueId: String(issue.id),
    issueKey: issue.key ?? '',
    inSprint: currentSprintIds.has(sprintId),
    estimate: estimateOf(issue.fields, sprint.estimateFieldId),
  };
}

// ---- event path ------------------------------------------------------------------------------

// One delivered issue update: record the rows of exactly this changelog entry (first writer keeps
// `source`), and refresh the issue's membership and estimate in every active sprint it touches.
// Reads current Jira state, so duplicates and reordering converge on the same result.
export async function applyIssueEvent(cfg, { issueId, changelogId, sprintChange }, policy) {
  const negative = new Set();
  const resolve = async (sprintId) => {
    if (negative.has(sprintId)) return null;
    const s = await resolveSprint(cfg, sprintId, policy);
    if (!s) negative.add(sprintId);
    return s;
  };

  let histories = [];
  if (sprintChange && changelogId) {
    const res = await jiraJson('app', route`/rest/api/3/issue/${issueId}/changelog/list`, postJson({ changelogIds: [Number(changelogId)] }), policy);
    histories = res.histories ?? [];
    for (const history of histories) for (const move of sprintMoves(history, cfg.sprintFieldId)) await resolve(move.sprintId);
  }

  const getIssue = () =>
    jiraJson('app', route`/rest/api/3/issue/${issueId}?fields=${issueFields(cfg).join(',')}`, undefined, policy).catch((e) => {
      if (e.status === 404) return null;
      throw e;
    });
  let issue = await getIssue();
  if (!issue) return { rows: 0, members: 0 };
  const current = sprintsOfField(issue.fields?.[cfg.sprintFieldId]);
  const knownFields = issueFields(cfg).join(',');
  for (const s of current) if (s.state === undefined || s.state === 'active') await resolve(s.id);
  if (issueFields(cfg).join(',') !== knownFields) issue = (await getIssue()) ?? issue;

  const rows = await rowsOf(cfg, issue, histories, 'event', resolve);
  let written = 0;
  for (const row of rows) if (await recordChange(row)) written += 1;

  const currentIds = new Set(current.map((s) => s.id));
  const stored = new Map((await membersOfIssue(issue.id)).map((m) => [m.sprintId, m]));
  const touched = new Set([...currentIds, ...rows.map((r) => r.sprintId), ...stored.keys()]);
  let members = 0;
  for (const sprintId of touched) {
    if (!cfg.sprints[sprintId]) continue;
    if (await writeMember(memberRow(cfg, sprintId, issue, currentIds), stored.get(sprintId))) members += 1;
  }
  return { rows: written, members };
}

// ---- scheduled path --------------------------------------------------------------------------

function jqlDate(ms) {
  const d = new Date(ms);
  const pad = (n) => String(n).padStart(2, '0');
  return `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())}`;
}

// Every issue that can hold a change of an active sprint: those in one now, and every issue updated
// since the earliest start (an issue taken out of a sprint was updated by that change, even if it has
// left every sprint since). The date is one day early so the site's JQL time zone cannot cut it.
export async function searchCandidates(cfg, policy) {
  const sprints = Object.values(cfg.sprints);
  const since = Math.min(...sprints.map((s) => s.startMs)) - 24 * 3600 * 1000;
  const jql = `sprint in (${sprints.map((s) => s.id).join(', ')}) OR updated >= "${jqlDate(since)}"`;
  const issues = [];
  let nextPageToken;
  do {
    const body = { jql, fields: issueFields(cfg), maxResults: 100 };
    if (nextPageToken) body.nextPageToken = nextPageToken;
    const page = await jiraJson('app', route`/rest/api/3/search/jql`, postJson(body), policy);
    issues.push(...(page.issues ?? []));
    nextPageToken = page.isLast === true ? undefined : page.nextPageToken ?? undefined;
  } while (nextPageToken);
  return issues;
}

export async function sprintHistories(issueIds, sprintFieldId, policy) {
  const byIssue = new Map();
  for (let i = 0; i < issueIds.length; i += 1000) {
    let nextPageToken;
    do {
      const body = { issueIdsOrKeys: issueIds.slice(i, i + 1000), fieldIds: [sprintFieldId], maxResults: 1000 };
      if (nextPageToken) body.nextPageToken = nextPageToken;
      const page = await jiraJson('app', route`/rest/api/3/changelog/bulkfetch`, postJson(body), policy);
      for (const log of page.issueChangeLogs ?? []) {
        const id = String(log.issueId);
        if (!byIssue.has(id)) byIssue.set(id, []);
        byIssue.get(id).push(...(log.changeHistories ?? []));
      }
      nextPageToken = page.nextPageToken ?? undefined;
    } while (nextPageToken);
  }
  return byIssue;
}

// Backfill and heal: compute every change and membership of every active sprint from Jira, then write
// only what the ledger does not hold yet. A run with nothing new writes nothing.
export async function reconcileAll(cfg, policy) {
  const sprintIds = Object.keys(cfg.sprints);
  if (!sprintIds.length) return { issues: 0, rows: 0, members: 0 };
  const issues = await searchCandidates(cfg, policy);
  const histories = await sprintHistories(issues.map((i) => String(i.id)), cfg.sprintFieldId, policy);

  const storedRows = new Set();
  const storedMembers = new Map();
  for (const sprintId of sprintIds) {
    for (const row of await changesOfSprint(sprintId)) storedRows.add(changeKey(row.changeId, row.sprintId));
    for (const m of await membersOfSprint(sprintId)) storedMembers.set(`${m.sprintId}:${m.issueId}`, m);
  }

  const resolve = async (sprintId) => cfg.sprints[sprintId] ?? null;
  let rows = 0;
  let members = 0;
  for (const issue of issues) {
    const issueRows = await rowsOf(cfg, issue, histories.get(String(issue.id)) ?? [], 'reconcile', resolve);
    for (const row of issueRows) {
      if (storedRows.has(changeKey(row.changeId, row.sprintId))) continue;
      if (await recordChange(row, true)) rows += 1;
    }
    const currentIds = new Set(sprintsOfField(issue.fields?.[cfg.sprintFieldId]).map((s) => s.id));
    const touched = new Set([...currentIds, ...issueRows.map((r) => r.sprintId)]);
    for (const sprintId of sprintIds) if (storedMembers.has(`${sprintId}:${issue.id}`)) touched.add(sprintId);
    for (const sprintId of touched) {
      if (!cfg.sprints[sprintId]) continue;
      if (await writeMember(memberRow(cfg, sprintId, issue, currentIds), storedMembers.get(`${sprintId}:${issue.id}`))) members += 1;
    }
  }
  return { issues: issues.length, rows, members };
}
