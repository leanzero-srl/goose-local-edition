import kvs, { Sort } from '@forge/kvs';

// Layout (different from a single bulk ledger):
//   entity scope-change  key "<sprintId>#<changelogId>"  index sprint-time  (sprintId | atMs)
//   entity scope-member  key "<sprintId>#<issueId>"      index sprint-issue (sprintId | issueNum)
//   plain key  registry                                   sprint field id, boards, active sprints
export const CHANGE = 'scope-change';
export const MEMBER = 'scope-member';
const REGISTRY = 'registry';

export const changeKey = (sprintId, changeId) => `${sprintId}#${changeId}`;
export const memberKey = (sprintId, issueId) => `${sprintId}#${issueId}`;

async function readIndex(entity, index, sprintId) {
  const rows = [];
  let cursor;
  do {
    let q = kvs.entity(entity).query().index(index, { partition: [String(sprintId)] }).sort(Sort.ASC).limit(100);
    if (cursor) q = q.cursor(cursor);
    const page = await q.getMany();
    for (const r of page.results) rows.push(r.value);
    cursor = page.nextCursor;
  } while (cursor);
  return rows;
}

export const sprintChanges = (sprintId) => readIndex(CHANGE, 'sprint-time', sprintId);
export const sprintMembers = (sprintId) => readIndex(MEMBER, 'sprint-issue', sprintId);

export const getRegistry = () => kvs.get(REGISTRY);
export const setRegistry = (value) => kvs.set(REGISTRY, value);

// First writer wins: the row's `source` is the path that recorded it first.
export async function recordChange(row) {
  try {
    await kvs.entity(CHANGE).set(changeKey(row.sprintId, row.changeId), row, { keyPolicy: 'FAIL_IF_EXISTS' });
    return true;
  } catch (err) {
    if (err && (err.code === 'CONDITIONAL_CHECK_FAILED' || /already exists/i.test(err.message || ''))) return false;
    throw err;
  }
}

export const getMember = (sprintId, issueId) => kvs.entity(MEMBER).get(memberKey(sprintId, issueId));
export const putMember = (row) => kvs.entity(MEMBER).set(memberKey(row.sprintId, row.issueId), row);
export const getChange = (sprintId, changeId) => kvs.entity(CHANGE).get(changeKey(sprintId, changeId));
