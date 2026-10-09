import { kvs } from '@forge/kvs';
import { jiraJson, route, postJson } from './jira';
import { toMicro, creepTenths, formatPoints } from './numbers';

// v2 keeps the ledger in `scope-ledger`; v1's `scope-change` rows are copied into it by the migration
// (migrate.js) and are never changed. `sprint-issue` (membership + current estimate) is carried over as is.
export const LEDGER = 'scope-ledger';
export const V1_CHANGES = 'scope-change';
export const MEMBERS = 'sprint-issue';
export const MIGRATION_KEY = 'migration';

// A change is keyed by changelog id + sprint: one changelog entry moving an issue between two sprints is
// two rows. v1 used the same key, so a v1 change and its v2 copy can never be two rows.
export const changeKey = (changeId, sprintId) => `${changeId}:${sprintId}`;
export const memberKey = (sprintId, issueId) => `${sprintId}:${issueId}`;

async function queryAll(entityName, indexName, partition) {
  const out = [];
  let cursor;
  do {
    let q = kvs.entity(entityName).query().index(indexName, { partition }).limit(100);
    if (cursor) q = q.cursor(cursor);
    const page = await q.getMany();
    out.push(...page.results.map((r) => r.value));
    cursor = page.nextCursor;
  } while (cursor);
  return out;
}

export const ledgerRowsOfSprint = (sprintId) => queryAll(LEDGER, 'by-sprint', [String(sprintId)]);
export const ledgerRowsOfIssue = (issueId) => queryAll(LEDGER, 'by-issue', [String(issueId)]);
export const v1RowsOfSprint = (sprintId) => queryAll(V1_CHANGES, 'by-sprint', [String(sprintId)]);
export const membersOfSprint = (sprintId) => queryAll(MEMBERS, 'by-sprint', [String(sprintId)]);
export const membersOfIssue = (issueId) => queryAll(MEMBERS, 'by-issue', [String(issueId)]);

export const migrationComplete = async () => (await kvs.get(MIGRATION_KEY))?.complete === true;

// A v1 row in the v2 shape. `extra` carries what v1 never stored: the estimate, the board and its
// estimation field, and whether the issue still exists.
export function fromV1(v1, extra) {
  return {
    sprintId: String(v1.sprintId),
    at: v1.at,
    changeId: String(v1.changeId),
    kind: v1.kind,
    issueId: String(v1.issueId),
    issueKey: v1.issueKey ?? '',
    estimate: extra.estimate,
    boardId: extra.boardId,
    estimateField: extra.estimateField,
    deleted: extra.deleted,
    deployedEnvs: '',
    authorId: v1.authorId ?? '',
    authorName: v1.authorName ?? '',
    source: v1.source ?? '',
  };
}

// The ledger of one sprint: its v2 rows plus, until the migration is complete, the v1 rows not copied yet
// (so v1's numbers stay whole while the copy runs). Pending v1 rows carry no v2 extras; nothing reads them.
export async function changesOfSprint(sprintId) {
  const rows = await ledgerRowsOfSprint(sprintId);
  if (await migrationComplete()) return rows;
  const have = new Set(rows.map((r) => changeKey(r.changeId, r.sprintId)));
  for (const v1 of await v1RowsOfSprint(sprintId)) {
    if (!have.has(changeKey(v1.changeId, v1.sprintId))) rows.push(fromV1(v1, { estimate: 0, boardId: '', estimateField: '', deleted: false }));
  }
  return rows;
}

// One issue's rows (all sprints), plus — until the migration is complete — its v1 rows not copied yet in
// the given sprints.
export async function rowsOfIssue(issueId, sprintIds) {
  const rows = await ledgerRowsOfIssue(issueId);
  if (await migrationComplete()) return rows;
  const have = new Set(rows.map((r) => changeKey(r.changeId, r.sprintId)));
  for (const sprintId of new Set(sprintIds)) {
    for (const v1 of await v1RowsOfSprint(sprintId)) {
      if (String(v1.issueId) === String(issueId) && !have.has(changeKey(v1.changeId, v1.sprintId))) {
        rows.push(fromV1(v1, { estimate: 0, boardId: '', estimateField: '', deleted: false }));
      }
    }
  }
  return rows;
}

async function createRow(key, value) {
  try {
    await kvs.entity(LEDGER).set(key, value, { keyPolicy: 'FAIL_IF_EXISTS' });
    return true;
  } catch (e) {
    if (await kvs.entity(LEDGER).get(key)) return false;
    throw e;
  }
}

// Exactly one row per change, whatever the delivery history: the first writer wins and keeps its
// `source`. A change v1 already recorded keeps v1's time, author and source (the event path can reach it
// before the migration does). `known` skips the v2 read when the caller has listed the sprint's rows.
export async function recordChange(row, known = false) {
  const key = changeKey(row.changeId, row.sprintId);
  if (!known && (await kvs.entity(LEDGER).get(key))) return false;
  const v1 = await kvs.entity(V1_CHANGES).get(key);
  const value = v1 ? { ...row, at: v1.at, kind: v1.kind, authorId: v1.authorId ?? '', authorName: v1.authorName ?? '', source: v1.source ?? row.source } : row;
  return createRow(key, value);
}

export const copyV1 = (v1, extra) => createRow(changeKey(v1.changeId, v1.sprintId), fromV1(v1, extra));

export const sameMember = (a, b) =>
  a && b && a.inSprint === b.inSprint && a.estimate === b.estimate && a.estimates === b.estimates && a.issueKey === b.issueKey && (a.deleted === true) === (b.deleted === true);

// A change's points (contract §1): the issue's current value of the estimation field its sprint's board used at the
// time of the change, the field its row records — a later switch of the board's field leaves it alone. The member
// row carries every estimation field's current value (`estimates`, JSON); a row without a field (a v1 row not copied
// yet) reads the board's.
export function changePoints(member, fieldId) {
  if (!member) return 0;
  const values = member.estimates ? JSON.parse(member.estimates) : {};
  return fieldId && Object.hasOwn(values, fieldId) ? values[fieldId] : member.estimate ?? 0;
}

// Writes the member row only when it differs from what is stored, so a run with nothing new writes nothing.
export async function writeMember(next, stored) {
  if (sameMember(next, stored)) return false;
  await kvs.entity(MEMBERS).set(memberKey(next.sprintId, next.issueId), next);
  return true;
}

// A deleted issue: its rows stay as history marked `deleted`, and its memberships are marked `deleted`
// (keeping the last known membership and estimate), so it no longer counts in current scope and leaves it
// as a removal. Its v1 rows not copied yet are marked by the migration, which finds the issue gone.
export async function markIssueDeleted(issueId) {
  const sprintIds = new Set();
  for (const row of await ledgerRowsOfIssue(issueId)) {
    if (row.deleted) continue;
    await kvs.entity(LEDGER).set(changeKey(row.changeId, row.sprintId), { ...row, deleted: true });
    sprintIds.add(row.sprintId);
  }
  for (const m of await membersOfIssue(issueId)) {
    if (m.deleted === true) continue;
    await kvs.entity(MEMBERS).set(memberKey(m.sprintId, m.issueId), { ...m, deleted: true });
    sprintIds.add(m.sprintId);
  }
  return [...sprintIds];
}

// A CI deployment reached these issues: every ledger row of each names the environment. Idempotent.
export async function markDeployed(issueIds, environment) {
  const sprintIds = new Set();
  for (const issueId of issueIds) {
    for (const row of await ledgerRowsOfIssue(issueId)) {
      const envs = new Set(row.deployedEnvs ? row.deployedEnvs.split(',') : []);
      if (envs.has(environment)) continue;
      envs.add(environment);
      await kvs.entity(LEDGER).set(changeKey(row.changeId, row.sprintId), { ...row, deployedEnvs: [...envs].sort().join(',') });
      sprintIds.add(row.sprintId);
    }
  }
  return [...sprintIds];
}

const isNumeric = (s) => /^\d+$/.test(s);
export function compareIds(a, b) {
  if (isNumeric(a) && isNumeric(b)) {
    const x = BigInt(a);
    const y = BigInt(b);
    return x < y ? -1 : x > y ? 1 : 0;
  }
  return a < b ? -1 : a > b ? 1 : 0;
}

export const byTime = (a, b) => a.at - b.at || compareIds(a.changeId, b.changeId);

// Membership of one issue in one sprint: at startDate it was the opposite of its first recorded change
// after it (an issue whose first change is `added` was not in S at start), or its last known membership
// when nothing changed. A deleted issue is in no sprint now.
export function issueInSprint(history, member) {
  const sorted = [...history].sort(byTime);
  const lastKnown = member?.inSprint === true;
  const inNow = lastKnown && member?.deleted !== true;
  const inAtStart = sorted.length ? sorted[0].kind === 'removed' : lastKnown;
  const everInAfterStart = inAtStart || sorted.some((c) => c.kind === 'added');
  return { inNow, inAtStart, everInAfterStart };
}

export function computeTotals(changes, members) {
  const memberByIssue = new Map(members.map((m) => [m.issueId, m]));
  const changesByIssue = new Map();
  for (const c of changes) {
    if (!changesByIssue.has(c.issueId)) changesByIssue.set(c.issueId, []);
    changesByIssue.get(c.issueId).push(c);
  }
  const issueIds = new Set([...memberByIssue.keys(), ...changesByIssue.keys()]);
  let committed = 0;
  let added = 0;
  let removed = 0;
  for (const issueId of issueIds) {
    const member = memberByIssue.get(issueId);
    const { inNow, inAtStart, everInAfterStart } = issueInSprint(changesByIssue.get(issueId) ?? [], member);
    // Contract §1: a deleted issue has no value — it counts nowhere.
    const estimate = member?.deleted === true ? 0 : toMicro(member?.estimate);
    if (inAtStart) committed += estimate;
    if (inNow && !inAtStart) added += estimate;
    if (everInAfterStart && !inNow) removed += estimate;
  }
  return { committed, added, removed, creepTenths: creepTenths(added, committed) };
}

// The `scope-status` value of one issue (R7): `committed` or `added +<points>` (the points of the change that
// last added it) in the active sprint it is in now, `removed` when it left an active sprint after its start,
// empty otherwise. `activeSprintIds` are the active sprints; members and rows are this issue's.
export function scopeStatus(activeSprintIds, members, rows) {
  let removed = false;
  for (const m of members) {
    if (!activeSprintIds.has(m.sprintId)) continue;
    const history = rows.filter((r) => r.sprintId === m.sprintId);
    const { inNow, inAtStart, everInAfterStart } = issueInSprint(history, m);
    if (inNow) {
      if (inAtStart) return 'committed';
      const lastAdd = history.filter((r) => r.kind === 'added').sort(byTime).at(-1);
      return `added +${formatPoints(toMicro(changePoints(m, lastAdd?.estimateField)))}`;
    }
    if (everInAfterStart) removed = true;
  }
  return removed ? 'removed' : '';
}

export async function sprintTotals(sprintId) {
  const [changes, members] = await Promise.all([changesOfSprint(sprintId), membersOfSprint(sprintId)]);
  return { changes, members, totals: computeTotals(changes, members) };
}

// Which of these issues can the invoking person browse? Read as them: issue bulkfetch leaves out issues
// that "aren't found or that the user doesn't have permission to view". Returns issueId -> current key.
export async function visibleIssues(issueIds, work) {
  const visible = new Map();
  const ids = [...new Set(issueIds)];
  for (let i = 0; i < ids.length; i += 100) {
    const page = await jiraJson('user', route`/rest/api/3/issue/bulkfetch`, postJson({ issueIdsOrKeys: ids.slice(i, i + 100), fields: ['key'] }), work);
    for (const issue of page.issues ?? []) visible.set(String(issue.id), issue.key);
  }
  return visible;
}
