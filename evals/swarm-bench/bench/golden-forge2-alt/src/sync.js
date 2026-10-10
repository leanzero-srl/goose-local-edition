import { kvs } from '@forge/kvs';
import { jiraJson, route } from './jira';
import { resolveSprint, fieldAt, trackedFields, observeField, boardEstimateField, getSprintFromJira } from './config';
import {
  recordRow,
  writeMember,
  patchRow,
  membersOfIssue,
  membersOfSprint,
  ledgerOfIssue,
  ledgerOfSprint,
  rowKey,
  memberKey,
  tombstoneKey,
  LEDGER,
  envString,
  envList,
  DEPLOYS,
  queryAll,
} from './ledger';
import { freezeSprint } from './scope';
import { statusFor, writtenStatus, writtenStatuses, writeStatuses, keysWithPrefix } from './status';
import { remainingMs, sleep } from './time';

// Index queries miss writes younger than this (§3); a handler that records a fact and then patches what it can
// see waits it out first.
export const QUERY_STALENESS_MS = 5_000;

// ---- Jira shapes ------------------------------------------------------------------------------------

// The Sprint field's issue value is an array of sprint objects (or ids); its changelog `from`/`to` hold the
// comma-separated sprint ids before and after the change.
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

// Jira types changelog `created` as an ISO-8601 date-time; an epoch-millisecond number is accepted too.
export function instantOf(created) {
  if (typeof created === 'number' && Number.isFinite(created)) return created;
  if (typeof created === 'string') return Date.parse(created);
  return NaN;
}

export const numberOrNull = (v) => {
  if (v === null || v === undefined || v === '') return null;
  const n = typeof v === 'number' ? v : Number(v);
  return Number.isFinite(n) ? n : null;
};

// Every tracked estimation field's value (null = no value), as a stable string so equal states compare equal.
export const estimatesJson = (fields, tracked) => JSON.stringify(Object.fromEntries(tracked.map((f) => [f, numberOrNull(fields?.[f])])));

function buildRow(cfg, sprint, history, issue, kind, source) {
  const at = instantOf(history.created);
  const field = fieldAt(cfg, sprint.boardId, at);
  return {
    sprintId: sprint.id,
    changeId: String(history.id),
    at,
    created: new Date(at).toISOString(),
    kind,
    issueId: String(issue.id),
    issueKey: issue.key ?? '',
    authorId: history.author?.accountId ?? '',
    authorName: history.author?.displayName ?? '',
    source,
    estimate: numberOrNull(issue.fields?.[field]) ?? 0,
    estimateField: field ?? '',
    boardId: sprint.boardId,
    deleted: false,
    deployedEnvs: '',
  };
}

// The rows a set of changelog histories hold for active sprints, after each sprint's start.
async function candidateRows(cfg, issue, histories, source, resolve) {
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
      if (!(at > sprint.startMs)) continue;
      rows.push({ sprint, history, kind: move.kind });
    }
  }
  return rows;
}

export async function sprintHistories(issueIds, sprintFieldId, policy) {
  const byIssue = new Map();
  for (let i = 0; i < issueIds.length; i += 1000) {
    let nextPageToken;
    do {
      const body = { issueIdsOrKeys: issueIds.slice(i, i + 1000), fieldIds: [sprintFieldId], maxResults: 10000 };
      if (nextPageToken) body.nextPageToken = nextPageToken;
      const page = await jiraJson('app', 'POST /rest/api/3/changelog/bulkfetch', route`/rest/api/3/changelog/bulkfetch`, policy, { body });
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

// Issues as the app, by id: { found: Map(id -> issue), missing: Set(id) }. The app can see every issue, so an id
// Jira does not return is a deleted issue.
export async function fetchIssues(issueIds, fields, policy) {
  const found = new Map();
  const ids = [...new Set(issueIds.map(String))];
  for (let i = 0; i < ids.length; i += 100) {
    const page = await jiraJson('app', 'POST /rest/api/3/issue/bulkfetch', route`/rest/api/3/issue/bulkfetch`, policy, {
      body: { issueIdsOrKeys: ids.slice(i, i + 100), fields },
    });
    for (const issue of page.issues ?? []) found.set(String(issue.id), issue);
  }
  return { found, missing: new Set(ids.filter((id) => !found.has(id))) };
}

// ---- deletion -------------------------------------------------------------------------------------

// §12: the issue's rows stay as history with deleted: true and it counts in no current scope. The tombstone is
// written first; every row writer re-checks it after writing, and after the staleness window every row the index
// shows is patched.
export async function markDeleted(issueIds) {
  const ids = [...new Set(issueIds.map(String))];
  if (!ids.length) return [];
  for (const id of ids) await kvs.set(tombstoneKey(id), { at: Date.now() });
  await sleep(QUERY_STALENESS_MS + 100);
  const changed = new Set();
  for (const id of ids) {
    for (const row of await ledgerOfIssue(id)) {
      if (row.deleted) continue;
      await patchRow(rowKey(row.changeId, row.sprintId), (r) => ({ ...r, deleted: true }));
      changed.add(row.sprintId);
    }
    for (const member of await membersOfIssue(id)) {
      if (member.deleted) continue;
      if (await writeMember({ ...member, inSprint: false, deleted: true, estimates: '{}', syncedAt: Date.now() }, member)) changed.add(member.sprintId);
    }
  }
  return [...changed];
}

// ---- closed sprints --------------------------------------------------------------------------------

// A sprint the config held as active is closed now: freeze its ledger as it stands and stop treating it as active.
export async function closeSprint(cfg, sprintId, sprint) {
  const known = cfg.known?.[sprintId] ?? {};
  const meta = { name: sprint?.name ?? cfg.sprints[sprintId]?.name ?? known.name, completeDate: sprint?.completeDate ?? known.completeDate ?? new Date().toISOString(), boardId: cfg.sprints[sprintId]?.boardId ?? known.boardId };
  await freezeSprint(sprintId, meta, cfg);
  delete cfg.sprints[sprintId];
  cfg.known = { ...(cfg.known ?? {}), [sprintId]: { ...known, state: 'closed', name: meta.name, boardId: meta.boardId, completeDate: meta.completeDate } };
}

// ---- event path -------------------------------------------------------------------------------------

async function getIssue(issueId, fields, withLog, policy) {
  const path = withLog ? route`/rest/api/3/issue/${issueId}?fields=${fields}&expand=changelog` : route`/rest/api/3/issue/${issueId}?fields=${fields}`;
  try {
    return await jiraJson('app', 'GET /rest/api/3/issue/{issueIdOrKey}', path, policy);
  } catch (e) {
    if (e.status === 404) return null;
    throw e;
  }
}

function groupRows(rows) {
  const bySprint = new Map();
  for (const r of rows) {
    if (!bySprint.has(r.sprintId)) bySprint.set(r.sprintId, []);
    bySprint.get(r.sprintId).push(r);
  }
  return bySprint;
}

// One delivered issue update. Reads current Jira state, so duplicates, reordering and concurrent deliveries converge:
// rows are created once (first writer keeps `source`), member rows keep the fresher read, and the field value follows
// what storage then says. Records every Sprint change of the issue's embedded changelog the ledger lacks (event work
// may record siblings whose own event was lost). Returns the sprints whose numbers may have moved.
export async function applyIssueEvent(cfg, body, policy) {
  const issueId = String(body.issueId);
  const withLog = Boolean(body.sprintChange);
  let tracked = trackedFields(cfg);
  const readAt = Date.now();
  let issue = await getIssue(issueId, [cfg.sprintFieldId, ...tracked].join(','), withLog, policy);
  if (!issue) return { sprintIds: await markDeleted([issueId]), rows: 0, deleted: true };

  let histories = [];
  if (withLog) {
    const embedded = issue.changelog?.histories ?? [];
    histories = embedded.filter((h) => (h.items ?? []).some((i) => isSprintItem(i, cfg.sprintFieldId)));
    const named = body.changelogId && embedded.some((h) => String(h.id) === String(body.changelogId));
    if (body.changelogId && !named && (issue.changelog?.total ?? 0) > embedded.length) {
      histories = (await sprintHistories([issueId], cfg.sprintFieldId, policy)).get(issueId) ?? [];
    }
  }

  const current = sprintsOfField(issue.fields?.[cfg.sprintFieldId]);
  const negative = new Set();
  const resolve = async (sprintId, hint) => {
    if (negative.has(sprintId)) return null;
    if (cfg.sprints[sprintId] && hint === 'closed') {
      await closeSprint(cfg, sprintId);
      negative.add(sprintId);
      return null;
    }
    const s = await resolveSprint(cfg, sprintId, policy, hint);
    if (!s) negative.add(sprintId);
    return s;
  };
  for (const s of current) await resolve(s.id, s.state);

  const [storedRows, storedMembers] = await Promise.all([ledgerOfIssue(issueId), membersOfIssue(issueId)]);
  const have = new Set(storedRows.map((r) => rowKey(r.changeId, r.sprintId)));
  const found = (await candidateRows(cfg, issue, histories, 'event', resolve)).filter((c) => !have.has(rowKey(String(c.history.id), c.sprint.id)));

  // A new row's estimation field is the one its board used at the change; Jira sends no event when a board switches
  // fields, so the board is read now, when a change needs it.
  const observed = new Set();
  for (const c of found) {
    if (observed.has(c.sprint.boardId)) continue;
    observed.add(c.sprint.boardId);
    observeField(cfg, c.sprint.boardId, await boardEstimateField(c.sprint.boardId, policy));
  }
  if (trackedFields(cfg).join(',') !== tracked.join(',')) {
    tracked = trackedFields(cfg);
    issue = (await getIssue(issueId, [cfg.sprintFieldId, ...tracked].join(','), false, policy)) ?? issue;
  }

  const changed = new Set();
  const newRows = [];
  const seenRows = [];
  for (const c of found) {
    const row = buildRow(cfg, c.sprint, c.history, issue, c.kind, 'event');
    const written = await recordRow(row);
    if (written) {
      newRows.push(written);
      changed.add(written.sprintId);
    } else {
      // A concurrent delivery wrote it moments ago (too fresh for the index): read it by key.
      const existing = await kvs.entity(LEDGER).get(rowKey(row.changeId, row.sprintId));
      if (existing) seenRows.push(existing);
    }
  }

  const rowsBySprint = groupRows([...storedRows, ...newRows, ...seenRows]);
  const currentIds = new Set(current.map((s) => s.id));
  const storedBySprint = new Map(storedMembers.map((m) => [m.sprintId, m]));
  const touched = new Set([...currentIds, ...rowsBySprint.keys(), ...storedBySprint.keys()]);
  const estimates = estimatesJson(issue.fields, tracked);
  const members = new Map();
  for (const sprintId of touched) {
    if (!cfg.sprints[sprintId]) continue;
    const stored = storedBySprint.get(sprintId);
    const inSprint = currentIds.has(sprintId);
    // Left the sprint with no Sprint change recorded: a sprint close moves unfinished issues out without a changelog
    // entry. Ask Jira before touching a sprint that may have closed.
    if (stored?.inSprint && !inSprint) {
      const last = (rowsBySprint.get(sprintId) ?? []).sort((a, b) => a.at - b.at).pop();
      if (!last || last.kind !== 'removed') {
        const sprint = await getSprintFromJira(sprintId, policy);
        if (!sprint || sprint.state === 'closed') {
          await closeSprint(cfg, sprintId, sprint);
          changed.add(sprintId);
          continue;
        }
      }
    }
    const next = { sprintId, issueId, issueKey: issue.key ?? '', inSprint, estimates, deleted: false, syncedAt: readAt };
    members.set(sprintId, next);
    if (await writeMember(next, stored)) changed.add(sprintId);
  }

  await updateStatus(cfg, issueId, members, rowsBySprint, policy);
  return { sprintIds: [...changed], rows: newRows.length };
}

// The issue's field value from what this invocation knows (its own writes included), written only when it changed.
async function updateStatus(cfg, issueId, members, rowsBySprint, policy) {
  const entries = [];
  for (const [sprintId, member] of members) if (cfg.sprints[sprintId]) entries.push({ member, rows: rowsBySprint.get(sprintId) ?? [] });
  const value = statusFor(entries);
  if (value === null || value === (await writtenStatus(issueId))) return;
  await writeStatuses(cfg, new Map([[issueId, value]]), policy);
}

// ---- scheduled path ---------------------------------------------------------------------------------

function jqlDate(ms) {
  const d = new Date(ms);
  const pad = (n) => String(n).padStart(2, '0');
  return `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())}`;
}

// Every issue that can hold a change of an active sprint: those in one now, and every issue updated since the
// earliest start (an issue taken out of a sprint was updated by that change). The date is a day early so the site's
// JQL time zone cannot cut it.
export async function searchCandidates(cfg, policy) {
  const sprints = Object.values(cfg.sprints);
  const since = Math.min(...sprints.map((s) => s.startMs)) - 24 * 3600 * 1000;
  const jql = `sprint in (${sprints.map((s) => s.id).join(', ')}) OR updated >= "${jqlDate(since)}"`;
  const fields = [cfg.sprintFieldId, ...trackedFields(cfg)];
  const issues = [];
  let nextPageToken;
  do {
    const readAt = Date.now();
    const body = { jql, fields, maxResults: 1000 };
    if (nextPageToken) body.nextPageToken = nextPageToken;
    const page = await jiraJson('app', 'POST /rest/api/3/search/jql', route`/rest/api/3/search/jql`, policy, { body });
    for (const issue of page.issues ?? []) issues.push({ ...issue, readAt });
    nextPageToken = page.isLast === true ? undefined : page.nextPageToken ?? undefined;
  } while (nextPageToken);
  return issues;
}

export class OutOfTime extends Error {}

const checkTime = (marginMs) => {
  if (remainingMs() < marginMs) throw new OutOfTime(`less than ${Math.round(marginMs / 1000)} s left`);
};

// Backfill and heal: every change and membership of every active sprint from Jira; write only what storage lacks.
// A run with nothing new writes nothing.
export async function reconcileAll(cfg, policy, { marginMs, migrate }) {
  const sprintIds = Object.keys(cfg.sprints);
  const changed = new Set();
  const tracked = trackedFields(cfg);
  const issues = sprintIds.length ? await searchCandidates(cfg, policy) : [];
  checkTime(marginMs);
  const histories = issues.length ? await sprintHistories(issues.map((i) => String(i.id)), cfg.sprintFieldId, policy) : new Map();
  checkTime(marginMs);
  const byId = new Map(issues.map((i) => [String(i.id), i]));
  if (migrate) await migrate(byId);

  const storedRows = new Map();
  const storedMembers = new Map();
  for (const sprintId of sprintIds) {
    for (const row of await ledgerOfSprint(sprintId)) storedRows.set(rowKey(row.changeId, row.sprintId), row);
    for (const m of await membersOfSprint(sprintId)) storedMembers.set(memberKey(m.sprintId, m.issueId), m);
  }

  const resolve = async (sprintId) => cfg.sprints[sprintId] ?? null;
  let rows = 0;
  let members = 0;
  const allRows = new Map(storedRows);
  const memberState = new Map(storedMembers);
  const consumerFresh = new Set();
  for (const issue of issues) {
    checkTime(marginMs);
    const issueId = String(issue.id);
    for (const c of await candidateRows(cfg, issue, histories.get(issueId) ?? [], 'reconcile', resolve)) {
      const key = rowKey(String(c.history.id), c.sprint.id);
      if (allRows.has(key)) continue;
      const row = buildRow(cfg, c.sprint, c.history, issue, c.kind, 'reconcile');
      const written = await recordRow(row);
      if (written) {
        rows += 1;
        changed.add(row.sprintId);
      }
      allRows.set(key, written ?? (await kvs.entity(LEDGER).get(key)) ?? row);
    }
    const currentIds = new Set(sprintsOfField(issue.fields?.[cfg.sprintFieldId]).map((s) => s.id));
    const touched = new Set(currentIds);
    for (const r of allRows.values()) if (r.issueId === issueId) touched.add(r.sprintId);
    for (const sprintId of sprintIds) if (storedMembers.has(memberKey(sprintId, issueId))) touched.add(sprintId);
    const estimates = estimatesJson(issue.fields, tracked);
    for (const sprintId of touched) {
      if (!cfg.sprints[sprintId]) continue;
      const key = memberKey(sprintId, issueId);
      const stored = storedMembers.get(key);
      const next = { sprintId, issueId, issueKey: issue.key ?? '', inSprint: currentIds.has(sprintId), estimates, deleted: false, syncedAt: issue.readAt };
      if (stored && stored.syncedAt > issue.readAt) {
        consumerFresh.add(issueId);
        continue;
      }
      if (await writeMember(next, stored)) {
        members += 1;
        changed.add(sprintId);
        memberState.set(key, next);
      } else if (!stored || stored.inSprint !== next.inSprint || stored.estimates !== next.estimates || stored.deleted) {
        consumerFresh.add(issueId); // a fresher read won the write
      }
    }
  }

  // Issues storage knows in an active sprint that Jira no longer returns: deleted, if Jira confirms.
  const known = new Set([...[...memberState.values()].filter((m) => !m.deleted).map((m) => String(m.issueId)), ...[...allRows.values()].filter((r) => !r.deleted).map((r) => String(r.issueId))]);
  const unseen = [...known].filter((id) => !byId.has(id));
  if (unseen.length) {
    const { missing } = await fetchIssues(unseen, ['key'], policy);
    for (const s of await markDeleted([...missing])) changed.add(s);
    for (const id of missing) {
      for (const [key, m] of memberState) if (String(m.issueId) === id) memberState.set(key, { ...m, deleted: true, inSprint: false });
    }
  }
  checkTime(marginMs);

  const statuses = await syncStatuses(cfg, [...memberState.values()], [...allRows.values()], consumerFresh, policy);
  await healDeployments([...allRows.values()]);
  return { issues: issues.length, rows, members, statuses, sprintIds: [...changed] };
}

// Every issue's scope-status from storage's view of the active sprints; issues a consumer refreshed after this run's
// read are left to that consumer.
async function syncStatuses(cfg, memberRows, rows, skip, policy) {
  const written = await writtenStatuses();
  for (const issueId of (await keysWithPrefix('deleted:')).keys()) skip.add(issueId); // never compared
  const membersByIssue = new Map();
  for (const m of memberRows) {
    if (!cfg.sprints[m.sprintId]) continue;
    const id = String(m.issueId);
    if (!membersByIssue.has(id)) membersByIssue.set(id, []);
    membersByIssue.get(id).push(m);
  }
  const rowsByIssueSprint = new Map();
  for (const r of rows) {
    const k = `${r.issueId}|${r.sprintId}`;
    if (!rowsByIssueSprint.has(k)) rowsByIssueSprint.set(k, []);
    rowsByIssueSprint.get(k).push(r);
  }
  const changes = new Map();
  for (const issueId of new Set([...membersByIssue.keys(), ...written.keys()])) {
    if (skip.has(issueId)) continue;
    const entries = (membersByIssue.get(issueId) ?? []).map((member) => ({ member, rows: rowsByIssueSprint.get(`${issueId}|${member.sprintId}`) ?? [] }));
    const value = statusFor(entries);
    if (value === null) continue;
    if (value !== (written.get(issueId) ?? '')) changes.set(issueId, value);
  }
  return writeStatuses(cfg, changes, policy);
}

// Safety net for §14: every row of a deployed issue carries its environments.
async function healDeployments(rows) {
  const records = await queryAll(DEPLOYS, 'all', ['deploy']);
  const envsByKey = new Map(records.map((d) => [d.issueKey, envList(d.envs)]));
  for (const row of rows) {
    const envs = envsByKey.get(row.issueKey) ?? [];
    if (!envs.length || envs.every((e) => envList(row.deployedEnvs).includes(e))) continue;
    await patchRow(rowKey(row.changeId, row.sprintId), (r) => ({ ...r, deployedEnvs: envString([...envList(r.deployedEnvs), ...envs]) }));
  }
}
