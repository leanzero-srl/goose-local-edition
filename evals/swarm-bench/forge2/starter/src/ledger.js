import { kvs } from '@forge/kvs';
import { jiraJson, route, postJson } from './jira';
import { toMicro, creepTenths } from './numbers';

export const CHANGES = 'scope-change';
export const MEMBERS = 'sprint-issue';

// A change is keyed by changelog id + sprint (§1): one changelog entry moving an issue between two
// sprints is two rows.
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

export const changesOfSprint = (sprintId) => queryAll(CHANGES, 'by-sprint', [String(sprintId)]);
export const membersOfSprint = (sprintId) => queryAll(MEMBERS, 'by-sprint', [String(sprintId)]);
export const membersOfIssue = (issueId) => queryAll(MEMBERS, 'by-issue', [String(issueId)]);

// Exactly one row per change, whatever the delivery history: the first writer wins and keeps its
// `source`. Returns true when this call created the row. `known` skips the read when the caller has
// already listed the sprint's rows; FAIL_IF_EXISTS still settles a race between two writers.
export async function recordChange(row, known = false) {
  const key = changeKey(row.changeId, row.sprintId);
  if (!known && (await kvs.entity(CHANGES).get(key))) return false;
  try {
    await kvs.entity(CHANGES).set(key, row, { keyPolicy: 'FAIL_IF_EXISTS' });
    return true;
  } catch (e) {
    if (await kvs.entity(CHANGES).get(key)) return false;
    throw e;
  }
}

const sameMember = (a, b) =>
  a && b && a.inSprint === b.inSprint && a.estimate === b.estimate && a.issueKey === b.issueKey;

// Writes the member row only when it differs from what is stored, so a run with nothing new writes nothing.
export async function writeMember(next, stored) {
  if (sameMember(next, stored)) return false;
  await kvs.entity(MEMBERS).set(memberKey(next.sprintId, next.issueId), next);
  return true;
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

// §1, reconstructed from the ledger: membership at startDate is the opposite of the first recorded
// change after it (an issue whose first change is `added` was not in S at start), or the current
// membership when nothing changed.
export function computeTotals(changes, members) {
  const memberByIssue = new Map(members.map((m) => [m.issueId, m]));
  const changesByIssue = new Map();
  for (const c of [...changes].sort(byTime)) {
    if (!changesByIssue.has(c.issueId)) changesByIssue.set(c.issueId, []);
    changesByIssue.get(c.issueId).push(c);
  }
  const issueIds = new Set([...memberByIssue.keys(), ...changesByIssue.keys()]);
  let committed = 0;
  let added = 0;
  let removed = 0;
  for (const issueId of issueIds) {
    const member = memberByIssue.get(issueId);
    const history = changesByIssue.get(issueId) ?? [];
    const inNow = member?.inSprint === true;
    const inAtStart = history.length ? history[0].kind === 'removed' : inNow;
    const everInAfterStart = inAtStart || history.some((c) => c.kind === 'added');
    const estimate = toMicro(member?.estimate);
    if (inAtStart) committed += estimate;
    if (inNow && !inAtStart) added += estimate;
    if (everInAfterStart && !inNow) removed += estimate;
  }
  return { committed, added, removed, creepTenths: creepTenths(added, committed) };
}

export async function sprintTotals(sprintId) {
  const [changes, members] = await Promise.all([changesOfSprint(sprintId), membersOfSprint(sprintId)]);
  return { changes, members, totals: computeTotals(changes, members) };
}

// Which of these issues can the invoking person browse? Read as them: issue bulkfetch leaves out issues
// that "aren't found or that the user doesn't have permission to view". Returns issueId -> current key.
export async function visibleIssues(issueIds, policy) {
  const visible = new Map();
  const ids = [...new Set(issueIds)];
  for (let i = 0; i < ids.length; i += 100) {
    const page = await jiraJson('user', route`/rest/api/3/issue/bulkfetch`, postJson({ issueIdsOrKeys: ids.slice(i, i + 100), fields: ['key'] }), policy);
    for (const issue of page.issues ?? []) visible.set(String(issue.id), issue.key);
  }
  return visible;
}
