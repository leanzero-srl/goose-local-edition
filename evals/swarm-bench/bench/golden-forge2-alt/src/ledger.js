import { kvs, Filter } from '@forge/kvs';
import { isKvsCode } from './budget';
import { toMicro, creepTenths } from './numbers';

export const V1_LEDGER = 'scope-change'; // v1's ledger: read only, never changed (§9)
export const LEDGER = 'scope-ledger';
export const MEMBERS = 'scope-member';
export const DEPLOYS = 'scope-deploy';

// A change is keyed by changelog id + sprint (§1). v2 keeps v1's key, so a v1 row and its migrated copy share it.
export const rowKey = (changeId, sprintId) => `${changeId}:${sprintId}`;
export const memberKey = (sprintId, issueId) => `${sprintId}:${issueId}`;
export const tombstoneKey = (issueId) => `deleted:${issueId}`;
export const frozenKey = (sprintId) => `frozen:${sprintId}`;

export async function queryAll(entityName, indexName, partition, filters) {
  const out = [];
  let cursor;
  do {
    let q = kvs.entity(entityName).query().index(indexName, { partition }).limit(100);
    if (filters) q = q.filters(filters);
    if (cursor) q = q.cursor(cursor);
    const page = await q.getMany();
    out.push(...page.results.map((r) => r.value));
    cursor = page.nextCursor;
  } while (cursor);
  return out;
}

export const ledgerOfSprint = (sprintId) => queryAll(LEDGER, 'by-sprint', [String(sprintId)]);
export const ledgerOfIssue = (issueId) => queryAll(LEDGER, 'by-issue', [String(issueId)]);
export const ledgerOfIssueKey = (issueKey) => queryAll(LEDGER, 'by-issue-key', [String(issueKey)]);
export const v1OfSprint = (sprintId) => queryAll(V1_LEDGER, 'by-sprint', [String(sprintId)]);
export const membersOfSprint = (sprintId) => queryAll(MEMBERS, 'by-sprint', [String(sprintId)]);
export const membersOfIssue = (issueId) => queryAll(MEMBERS, 'by-issue', [String(issueId)]);

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

export const envList = (s) => (s ? String(s).split(',').map((x) => x.trim()).filter(Boolean) : []);
export const envString = (envs) => [...new Set(envs)].join(',');

// ---- the per-issue facts every row writer re-checks after writing --------------------------------
// A CI deployment or a deletion that lands while a row is being written would otherwise miss that row: the CI
// handler and the delete handler record their fact first, then (after the query staleness window) patch the rows
// they can see; each writer re-reads the facts after its write and patches its own row.

export async function deployedEnvsOf(issueKey) {
  if (!issueKey) return [];
  return envList((await kvs.entity(DEPLOYS).get(String(issueKey)))?.envs);
}

export async function isDeleted(issueId) {
  return Boolean(await kvs.get(tombstoneKey(issueId)));
}

// Row updates after the first write touch only `deleted` and `deployedEnvs`; both are conditions of the update, so
// two patchers never lose each other's change. Returns the stored row.
export async function patchRow(key, mutate) {
  for (;;) {
    const row = await kvs.entity(LEDGER).get(key);
    if (!row) return null;
    const next = mutate(row);
    if (!next || (next.deleted === row.deleted && next.deployedEnvs === row.deployedEnvs)) return row;
    const conditions = new Filter()
      .and('deployedEnvs', { condition: 'EQUAL_TO', values: [row.deployedEnvs ?? ''] })
      .and('deleted', { condition: 'EQUAL_TO', values: [row.deleted === true] });
    try {
      await kvs.transact().set(key, { ...row, deleted: next.deleted, deployedEnvs: next.deployedEnvs }, { entityName: LEDGER, conditions }).execute();
      return next;
    } catch (e) {
      if (!isKvsCode(e, 'CONDITIONAL_CHECK_FAILED')) throw e;
    }
  }
}

// Exactly one row per change: the first writer creates it (FAIL_IF_EXISTS) and keeps its `source`. Rows of a change
// v1 recorded take v1's change time, kind, issue, author and source whoever writes them, so the migration, the event
// path and the reconciliation all produce the same row. Returns the created row, or null when it already existed.
// `v1` may be passed by a caller that has already read v1's row.
export async function recordRow(row, v1Row) {
  const key = rowKey(row.changeId, row.sprintId);
  const v1 = v1Row === undefined ? await kvs.entity(V1_LEDGER).get(key) : v1Row;
  const value = v1
    ? { ...row, at: v1.at, created: v1.created, kind: v1.kind, issueId: String(v1.issueId), issueKey: v1.issueKey, authorId: v1.authorId ?? '', authorName: v1.authorName ?? '', source: v1.source, migrated: true }
    : { ...row, migrated: false };
  try {
    await kvs.entity(LEDGER).set(key, value, { keyPolicy: 'FAIL_IF_EXISTS' });
  } catch (e) {
    if (isKvsCode(e, 'KEY_CONFLICT')) return null;
    throw e;
  }
  const [envs, deleted] = await Promise.all([deployedEnvsOf(value.issueKey), isDeleted(value.issueId)]);
  const missing = envs.filter((env) => !envList(value.deployedEnvs).includes(env));
  if (missing.length || (deleted && !value.deleted)) {
    return patchRow(key, (r) => ({ ...r, deleted: r.deleted || deleted, deployedEnvs: envString([...envList(r.deployedEnvs), ...envs]) }));
  }
  return value;
}

// Member rows hold an issue's membership of one sprint and its current values of every tracked estimation field.
// The fresher Jira read wins: a write based on an older read than the stored one is dropped.
export async function writeMember(next, stored) {
  if (stored && sameMember(next, stored)) return false;
  const key = memberKey(next.sprintId, next.issueId);
  if (!stored) {
    try {
      await kvs.entity(MEMBERS).set(key, next, { keyPolicy: 'FAIL_IF_EXISTS' });
      return true;
    } catch (e) {
      if (!isKvsCode(e, 'KEY_CONFLICT')) throw e;
    }
    // The index had not caught up with an existing row: compare with the row itself.
    const current = await kvs.entity(MEMBERS).get(key);
    if (current && (sameMember(next, current) || current.syncedAt >= next.syncedAt)) return false;
  }
  try {
    await kvs
      .transact()
      .set(key, next, { entityName: MEMBERS, conditions: new Filter().and('syncedAt', { condition: 'LESS_THAN', values: [next.syncedAt] }) })
      .execute();
    return true;
  } catch (e) {
    if (isKvsCode(e, 'CONDITIONAL_CHECK_FAILED')) return false;
    throw e;
  }
}

const sameMember = (a, b) =>
  a.inSprint === b.inSprint && a.deleted === (b.deleted === true) && a.issueKey === b.issueKey && a.estimates === b.estimates;

export function estimatesOf(member) {
  try {
    return member?.estimates ? JSON.parse(member.estimates) : {};
  } catch {
    console.error(`member ${member?.sprintId}:${member?.issueId}: unreadable estimates ${JSON.stringify(member?.estimates)}`);
    return {};
  }
}

// An issue's value of one field (§1: no value counts as 0, and a deleted issue has no value).
export function valueOf(member, field) {
  if (!member || member.deleted || !field) return 0;
  const v = estimatesOf(member)[field];
  return typeof v === 'number' && Number.isFinite(v) ? v : 0;
}

// §1 from the ledger and the member rows: membership at startDate is the opposite of the first recorded change after
// it (first change `added` = not in S at start), or the current membership when nothing changed. Estimates are the
// values of `field`, the estimation field S's board uses now.
export function computeTotals(rows, members, field) {
  const memberByIssue = new Map(members.map((m) => [String(m.issueId), m]));
  const historyByIssue = new Map();
  for (const r of [...rows].sort(byTime)) {
    const id = String(r.issueId);
    if (!historyByIssue.has(id)) historyByIssue.set(id, []);
    historyByIssue.get(id).push(r);
  }
  let committed = 0;
  let added = 0;
  let removed = 0;
  for (const issueId of new Set([...memberByIssue.keys(), ...historyByIssue.keys()])) {
    const member = memberByIssue.get(issueId);
    const history = historyByIssue.get(issueId) ?? [];
    const deleted = member?.deleted === true || history.some((r) => r.deleted);
    const inNow = !deleted && member?.inSprint === true;
    const inAtStart = history.length ? history[0].kind === 'removed' : inNow;
    const everIn = inAtStart || history.some((r) => r.kind === 'added');
    const estimate = deleted ? 0 : toMicro(valueOf(member, field));
    if (inAtStart) committed += estimate;
    if (inNow && !inAtStart) added += estimate;
    if (everIn && !inNow) removed += estimate;
  }
  return { committed, added, removed, creepTenths: creepTenths(added, committed) };
}
