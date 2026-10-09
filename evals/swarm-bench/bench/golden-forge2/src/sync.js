import { kvs } from '@forge/kvs';
import { jiraJson, route, postJson, RateLimited } from './jira';
import { sprintReader } from './config';
import {
  changesOfSprint,
  membersOfSprint,
  membersOfIssue,
  rowsOfIssue,
  recordChange,
  writeMember,
  changeKey,
  memberKey,
  markIssueDeleted,
  scopeStatus,
} from './ledger';
import { estimateOf } from './estimate';
import { writeStatuses, currentStatus } from './status';

const RECONCILE_KEY = 'reconcile-state';

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

export const issueFields = (cfg) => [cfg.sprintFieldId, ...estimateFieldIds(cfg), ...(cfg.scopeFieldId ? [cfg.scopeFieldId] : [])];

// Jira types changelog `created` as an ISO-8601 date-time; an epoch-millisecond number is accepted too.
// Anything else is logged and the entry skipped, never silently dropped.
export function instantOf(created) {
  if (typeof created === 'number' && Number.isFinite(created)) return created;
  if (typeof created === 'string') return Date.parse(created);
  return NaN;
}

// The row of one change, with the estimate the issue carries for the sprint's board right now (rows keep
// it: a later switch of the board's estimation field does not touch them).
export function changeRow(sprint, history, issue, kind, source) {
  return {
    sprintId: sprint.id,
    changeId: String(history.id),
    at: instantOf(history.created),
    kind,
    issueId: String(issue.id),
    issueKey: issue.key ?? '',
    estimate: estimateOf(issue.fields, sprint.estimateFieldId),
    boardId: sprint.boardId,
    estimateField: sprint.estimateFieldId ?? '',
    deleted: false,
    deployedEnvs: '',
    authorId: history.author?.accountId ?? '',
    authorName: history.author?.displayName ?? '',
    source,
  };
}

// A sprint's ledger takes changes after its start and, once it closed, none after its completion.
const openAt = (sprint, at) => at > sprint.startMs && !(sprint.completeMs > 0 && at > sprint.completeMs);

async function rowsOf(cfg, issue, histories, source, resolve) {
  const rows = [];
  for (const history of histories) {
    const at = instantOf(history.created);
    for (const move of sprintMoves(history, cfg.sprintFieldId)) {
      const sprint = await resolve(move.sprintId);
      if (!sprint) continue;
      if (Number.isNaN(at)) {
        console.error(`changelog ${history.id} of issue ${issue.key ?? issue.id}: unreadable created ${JSON.stringify(history.created)}; change not recorded`);
        continue;
      }
      if (!openAt(sprint, at)) continue;
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
// `source`), refresh the issue's membership and estimate in every active sprint it touches, and its
// scope status. Sprints and boards are read fresh, so a sprint that closed or a board whose estimation
// field changed is honoured at once. Reads current Jira state, so duplicates and reordering converge.
// A 404 is a deleted issue: `onDeleted` handles it.
export async function applyIssueEvent(cfg, { issueId, changelogId, sprintChange }, work, onDeleted) {
  const resolve = sprintReader(cfg, work);
  const wantsLog = Boolean(sprintChange && changelogId);
  const getIssue = () => {
    const fields = issueFields(cfg).join(',');
    const path = wantsLog
      ? route`/rest/api/3/issue/${issueId}?fields=${fields}&expand=changelog`
      : route`/rest/api/3/issue/${issueId}?fields=${fields}`;
    return jiraJson('app', path, undefined, work).catch((e) => {
      if (e.status === 404) return null;
      throw e;
    });
  };
  let issue = await getIssue();
  if (!issue) return onDeleted(String(issueId));

  let histories = [];
  if (wantsLog) {
    const embedded = issue.changelog?.histories ?? [];
    histories = embedded.filter((h) => String(h.id) === String(changelogId));
    // The embedded changelog is the most recent page only; an entry older than it is read in full.
    if (!histories.length && (issue.changelog?.total ?? 0) > embedded.length) {
      const all = (await sprintHistories([String(issueId)], cfg.sprintFieldId, work)).get(String(issueId)) ?? [];
      histories = all.filter((h) => String(h.id) === String(changelogId));
    }
    for (const history of histories) for (const move of sprintMoves(history, cfg.sprintFieldId)) await resolve(move.sprintId);
  }
  const current = sprintsOfField(issue.fields?.[cfg.sprintFieldId]);
  const stored = new Map((await membersOfIssue(issue.id)).map((m) => [m.sprintId, m]));
  const knownFields = issueFields(cfg).join(',');
  for (const s of current) if (s.state === undefined || s.state === 'active') await resolve(s.id);
  for (const sprintId of stored.keys()) if (cfg.sprints[sprintId]) await resolve(sprintId);
  if (issueFields(cfg).join(',') !== knownFields) issue = (await getIssue()) ?? issue;

  const rows = await rowsOf(cfg, issue, histories, 'event', resolve);
  const changed = new Set();
  let written = 0;
  for (const row of rows) {
    if (await recordChange(row)) {
      written += 1;
      changed.add(row.sprintId);
    }
  }

  const currentIds = new Set(current.map((s) => s.id));
  const touched = new Set([...currentIds, ...rows.map((r) => r.sprintId), ...stored.keys()]);
  let members = 0;
  for (const sprintId of touched) {
    if (!cfg.sprints[sprintId]) continue;
    if (await writeMember(memberRow(cfg, sprintId, issue, currentIds), stored.get(sprintId))) {
      members += 1;
      changed.add(sprintId);
    }
  }

  let statuses = 0;
  if (cfg.scopeFieldId) {
    const issueMembers = await membersOfIssue(issue.id);
    const value = scopeStatus(new Set(Object.keys(cfg.sprints)), issueMembers, await rowsOfIssue(issue.id, issueMembers.map((m) => m.sprintId)));
    if (currentStatus(issue.fields, cfg.scopeFieldId) !== value) statuses = await writeStatuses(cfg.scopeFieldId, new Map([[String(issue.id), value]]), work);
  }
  return { rows: written, members, statuses, sprintIds: [...changed] };
}

// ---- scheduled path --------------------------------------------------------------------------

function jqlDate(ms) {
  const d = new Date(ms);
  const pad = (n) => String(n).padStart(2, '0');
  return `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())}`;
}

// Every issue that can hold a change of an active sprint: those in one now, and every issue updated since
// the last complete run started (a sprint change updates the issue, even one that has left every sprint
// since). The first run, and a run that finds a sprint the previous one did not know, reads everything
// updated since the earliest start (one day early, so the site's JQL time zone cannot cut it).
export async function searchCandidates(cfg, sinceMs, work) {
  const sprints = Object.values(cfg.sprints);
  const updated = sinceMs
    ? `updated >= -${Math.ceil((Date.now() - sinceMs) / 60000) + 1}m`
    : `updated >= "${jqlDate(Math.min(...sprints.map((s) => s.startMs)) - 24 * 3600 * 1000)}"`;
  const jql = `sprint in (${sprints.map((s) => s.id).join(', ')}) OR ${updated}`;
  const issues = [];
  let nextPageToken;
  do {
    const body = { jql, fields: issueFields(cfg), maxResults: 100 };
    if (nextPageToken) body.nextPageToken = nextPageToken;
    const page = await jiraJson('app', route`/rest/api/3/search/jql`, postJson(body), work);
    issues.push(...(page.issues ?? []));
    nextPageToken = page.isLast === true ? undefined : page.nextPageToken ?? undefined;
  } while (nextPageToken);
  return issues;
}

export async function sprintHistories(issueIds, sprintFieldId, work) {
  const byIssue = new Map();
  for (let i = 0; i < issueIds.length; i += 1000) {
    let nextPageToken;
    do {
      const body = { issueIdsOrKeys: issueIds.slice(i, i + 1000), fieldIds: [sprintFieldId], maxResults: 1000 };
      if (nextPageToken) body.nextPageToken = nextPageToken;
      const page = await jiraJson('app', route`/rest/api/3/changelog/bulkfetch`, postJson(body), work);
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

// The issues of this list Jira no longer has (asApp sees every issue that exists).
async function missingIssues(issueIds, work) {
  const found = new Set();
  for (let i = 0; i < issueIds.length; i += 100) {
    const page = await jiraJson('app', route`/rest/api/3/issue/bulkfetch`, postJson({ issueIdsOrKeys: issueIds.slice(i, i + 100), fields: ['key'] }), work);
    for (const issue of page.issues ?? []) found.add(String(issue.id));
  }
  return issueIds.filter((id) => !found.has(id));
}

const push = (map, key, value) => {
  if (!map.has(key)) map.set(key, []);
  map.get(key).push(value);
};

// Backfill and heal: compute every change and membership of every active sprint from Jira, then write
// only what the ledger does not hold yet, then the scope statuses that differ from what Jira shows. A run
// with nothing new writes nothing. `previous` is the config before this run's discovery: its sprints that
// are no longer active have closed, and their issues' statuses are recomputed.
export async function reconcileAll(cfg, previous, work) {
  const startedAt = Date.now();
  const sprintIds = Object.keys(cfg.sprints);
  const active = new Set(sprintIds);
  const last = await kvs.get(RECONCILE_KEY);
  const knewAll = sprintIds.every((id) => previous?.sprints?.[id]);
  const issues = sprintIds.length ? await searchCandidates(cfg, last && knewAll ? last.startedAt : null, work) : [];
  const histories = issues.length ? await sprintHistories(issues.map((i) => String(i.id)), cfg.sprintFieldId, work) : new Map();

  const storedRows = new Set();
  const rowsBySprintIssue = new Map();
  const membersNow = new Map();
  for (const sprintId of sprintIds) {
    for (const row of await changesOfSprint(sprintId)) {
      storedRows.add(changeKey(row.changeId, row.sprintId));
      push(rowsBySprintIssue, memberKey(row.sprintId, row.issueId), row);
    }
    for (const m of await membersOfSprint(sprintId)) membersNow.set(memberKey(m.sprintId, m.issueId), m);
  }

  const resolve = async (sprintId) => cfg.sprints[sprintId] ?? null;
  const changed = new Set();
  let rows = 0;
  let members = 0;
  for (const issue of issues) {
    // Out of time: stop here; the caller hands the rest to the queue (every write so far stays valid).
    if (!work.hasTimeFor(0)) throw new RateLimited(1, 'time');
    const issueRows = await rowsOf(cfg, issue, histories.get(String(issue.id)) ?? [], 'reconcile', resolve);
    for (const row of issueRows) {
      const key = changeKey(row.changeId, row.sprintId);
      if (storedRows.has(key)) continue;
      if (await recordChange(row, true)) {
        rows += 1;
        changed.add(row.sprintId);
      }
      storedRows.add(key);
      push(rowsBySprintIssue, memberKey(row.sprintId, row.issueId), row);
    }
    const currentIds = new Set(sprintsOfField(issue.fields?.[cfg.sprintFieldId]).map((s) => s.id));
    const touched = new Set([...currentIds, ...issueRows.map((r) => r.sprintId)]);
    for (const sprintId of sprintIds) if (membersNow.has(memberKey(sprintId, issue.id))) touched.add(sprintId);
    for (const sprintId of touched) {
      if (!active.has(sprintId)) continue;
      const next = memberRow(cfg, sprintId, issue, currentIds);
      if (await writeMember(next, membersNow.get(memberKey(sprintId, issue.id)))) {
        members += 1;
        changed.add(sprintId);
      }
      membersNow.set(memberKey(sprintId, issue.id), next);
    }
  }

  // Every issue in an active sprint is a candidate, so a member still "in" a sprint that the search did
  // not return has either left it unseen or been deleted: Jira says which.
  const candidateIds = new Set(issues.map((i) => String(i.id)));
  const orphans = [...new Set([...membersNow.values()].filter((m) => m.inSprint && m.deleted !== true && !candidateIds.has(m.issueId)).map((m) => m.issueId))];
  const deleted = orphans.length ? await missingIssues(orphans, work) : [];
  for (const issueId of deleted) for (const sprintId of await markIssueDeleted(issueId)) changed.add(sprintId);

  let statuses = 0;
  if (cfg.scopeFieldId) {
    const values = new Map();
    for (const issue of issues) {
      const id = String(issue.id);
      const issueMembers = sprintIds.map((s) => membersNow.get(memberKey(s, id))).filter(Boolean);
      const issueRows = sprintIds.flatMap((s) => rowsBySprintIssue.get(memberKey(s, id)) ?? []);
      const value = scopeStatus(active, issueMembers, issueRows);
      if (currentStatus(issue.fields, cfg.scopeFieldId) !== value) values.set(id, value);
    }
    const goneDeleted = new Set(deleted);
    const closed = new Set([...Object.keys(previous?.sprints ?? {}), ...(previous?.closed ?? [])]);
    for (const sprintId of closed) {
      if (active.has(sprintId)) continue;
      for (const m of await membersOfSprint(sprintId)) {
        if (candidateIds.has(m.issueId) || values.has(m.issueId) || goneDeleted.has(m.issueId) || m.deleted === true) continue;
        const issueMembers = await membersOfIssue(m.issueId);
        const issueRows = await rowsOfIssue(m.issueId, issueMembers.map((x) => x.sprintId));
        if (issueRows.some((r) => r.deleted)) continue;
        values.set(m.issueId, scopeStatus(active, issueMembers, issueRows));
      }
    }
    if (values.size) statuses = await writeStatuses(cfg.scopeFieldId, values, work);
  }
  await kvs.set(RECONCILE_KEY, { startedAt });
  return { issues: issues.length, rows, members, statuses, deleted: deleted.length, sprintIds: [...changed] };
}
