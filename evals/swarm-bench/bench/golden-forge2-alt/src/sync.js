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
  MEMBERS,
  envString,
  envList,
  DEPLOYS,
  queryAll,
  fresherThan,
  getMany,
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
      if ((await writeMember({ ...member, inSprint: false, deleted: true, estimates: '{}', syncedAt: Date.now() }, member)) === 'written') changed.add(member.sprintId);
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
// rows are created once (first writer keeps `source`), member rows keep the fresher read, and the field value is
// written only by the freshest read. The issue's changes come from its own changelog (one GET, the same cost with
// expand=changelog) and each row is looked up by key, so nothing here depends on an index query that may not show a
// write of the last seconds. Records every Sprint change the ledger lacks (event work may record siblings whose own
// event was lost). Returns the sprints whose numbers may have moved.
export async function applyIssueEvent(cfg, body, policy) {
  const issueId = String(body.issueId);
  let tracked = trackedFields(cfg);
  const readAt = Date.now();
  let issue = await getIssue(issueId, [cfg.sprintFieldId, ...tracked].join(','), true, policy);
  if (!issue) return { sprintIds: await markDeleted([issueId]), rows: 0, deleted: true };

  const embedded = issue.changelog?.histories ?? [];
  let histories = embedded.filter((h) => (h.items ?? []).some((i) => isSprintItem(i, cfg.sprintFieldId)));
  if ((issue.changelog?.total ?? 0) > embedded.length) {
    // The embedded changelog is the most recent page only: read the issue's Sprint changelog in full.
    histories = (await sprintHistories([issueId], cfg.sprintFieldId, policy)).get(issueId) ?? [];
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

  const candidates = await candidateRows(cfg, issue, histories, 'event', resolve);
  const known = [];
  const found = [];
  for (const c of candidates) {
    const row = await kvs.entity(LEDGER).get(rowKey(String(c.history.id), c.sprint.id));
    if (row) known.push(row);
    else found.push(c);
  }
  const storedMembers = await membersOfIssue(issueId);

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
  for (const c of found) {
    const row = buildRow(cfg, c.sprint, c.history, issue, c.kind, 'event');
    const written = await recordRow(row);
    if (written) {
      newRows.push(written);
      changed.add(written.sprintId);
    } else {
      // A concurrent delivery wrote it in between: read it by key.
      const existing = await kvs.entity(LEDGER).get(rowKey(row.changeId, row.sprintId));
      if (existing) known.push(existing);
    }
  }

  const rowsBySprint = groupRows([...known, ...newRows]);
  const currentIds = new Set(current.map((s) => s.id));
  const storedBySprint = new Map(storedMembers.map((m) => [m.sprintId, m]));
  const touched = new Set([...currentIds, ...rowsBySprint.keys(), ...storedBySprint.keys()]);
  const estimates = estimatesJson(issue.fields, tracked);
  const members = new Map();
  let superseded = false;
  for (const sprintId of touched) {
    if (!cfg.sprints[sprintId]) continue;
    const stored = storedBySprint.get(sprintId);
    const inSprint = currentIds.has(sprintId);
    // Left the sprint with no Sprint change recorded: a sprint close moves unfinished issues out without a changelog
    // entry. Ask Jira before touching a sprint that may have closed.
    if (stored?.inSprint && !inSprint) {
      const last = [...(rowsBySprint.get(sprintId) ?? [])].sort((a, b) => a.at - b.at).at(-1);
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
    const outcome = await writeMember(next, stored);
    if (outcome === 'written') changed.add(sprintId);
    if (outcome === 'stale') superseded = true;
  }

  // A delivery that read Jira later owns the field value; only the freshest read writes it.
  if (!superseded) await updateStatus(cfg, issueId, members, rowsBySprint, readAt, policy);
  return { sprintIds: [...changed], rows: newRows.length };
}

// The issue's field value from what this invocation knows (its own writes included), written only when it changed
// and when no fresher read of the issue has been stored meanwhile.
async function updateStatus(cfg, issueId, members, rowsBySprint, readAt, policy) {
  const entries = [];
  for (const [sprintId, member] of members) if (cfg.sprints[sprintId]) entries.push({ member, rows: rowsBySprint.get(sprintId) ?? [] });
  const value = statusFor(entries);
  if (value === null || value === (await writtenStatus(issueId))) return;
  if (await fresherThan([...members.keys()].map((s) => memberKey(s, issueId)), readAt)) return;
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
      const outcome = await writeMember(next, stored);
      if (outcome === 'stale') {
        consumerFresh.add(issueId); // a delivery read the issue after this run did and owns it
        continue;
      }
      memberState.set(key, next);
      if (outcome === 'written') {
        members += 1;
        changed.add(sprintId);
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

  const statuses = await syncStatuses(cfg, [...memberState.values()], [...allRows.values()], consumerFresh, (id) => byId.get(id)?.readAt, policy);
  await healDeployments([...allRows.values()]);
  return { issues: issues.length, rows, members, statuses, sprintIds: [...changed] };
}

// Every issue's scope-status from this run's view of the active sprints. An issue a delivery has read after this run
// did is left to that delivery: checked by key just before the write.
async function syncStatuses(cfg, memberRows, rows, skip, readAtOf, policy) {
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
  const candidates = new Map();
  for (const issueId of new Set([...membersByIssue.keys(), ...written.keys()])) {
    if (skip.has(issueId)) continue;
    const entries = (membersByIssue.get(issueId) ?? []).map((member) => ({ member, rows: rowsByIssueSprint.get(`${issueId}|${member.sprintId}`) ?? [] }));
    const value = statusFor(entries);
    if (value !== null && value !== (written.get(issueId) ?? '')) candidates.set(issueId, value);
  }
  // Read by key what the index may not show yet: a value written, or a member refreshed, in the last seconds.
  const items = [];
  for (const issueId of candidates.keys()) {
    items.push({ key: `status:${issueId}` });
    for (const m of membersByIssue.get(issueId) ?? []) items.push({ entityName: MEMBERS, key: memberKey(m.sprintId, issueId) });
  }
  const now = await getMany(items);
  const changes = new Map();
  for (const [issueId, value] of candidates) {
    if (value === (now.get(`|status:${issueId}`)?.v ?? '')) continue;
    const readAt = readAtOf(issueId);
    const fresher = (membersByIssue.get(issueId) ?? []).some((m) => (now.get(`${MEMBERS}|${memberKey(m.sprintId, issueId)}`)?.syncedAt ?? 0) > (readAt ?? Infinity));
    if (!fresher) changes.set(issueId, value);
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
