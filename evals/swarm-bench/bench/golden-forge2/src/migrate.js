import { kvs } from '@forge/kvs';
import { jiraJson, route, postJson } from './jira';
import { listScrumBoards, listSprints, boardEstimateField, loadConfig } from './config';
import { V1_CHANGES, MIGRATION_KEY, copyV1, compareIds } from './ledger';
import { estimateOf } from './estimate';

const PAGE = 100;

// state = { total, sprints: [{ id, boardId, count }], index, cursor, done, complete }: the sprints that hold
// v1 rows, walked in order, `cursor` the v1 query cursor inside sprints[index] and `done` the rows of that
// sprint already copied. Every copy is keyed like the v1 row (FAIL_IF_EXISTS), so a step that is cut off
// and repeated, or two steps racing, copy each row exactly once; the counts follow the cursor, never an
// increment, so they stay exact under repetition too.
const migratedOf = (s) => s.sprints.slice(0, s.index).reduce((n, x) => n + x.count, 0) + s.done;

export async function migrationProgress() {
  const s = await kvs.get(MIGRATION_KEY);
  if (!s) return { started: false, migrated: 0, total: null, complete: false };
  return { started: true, migrated: s.complete ? s.total : migratedOf(s), total: s.total, complete: s.complete };
}

async function countV1(sprintId) {
  let n = 0;
  let cursor;
  do {
    let q = kvs.entity(V1_CHANGES).query().index('by-sprint', { partition: [sprintId] }).limit(PAGE);
    if (cursor) q = q.cursor(cursor);
    const page = await q.getMany();
    n += page.results.length;
    cursor = page.nextCursor;
  } while (cursor);
  return n;
}

// v1 only ever recorded changes of sprints that were active, which may have closed since: every active and
// closed sprint of every scrum board (plus whatever v1's config still names) is a place v1 rows can be.
async function plan(work) {
  const boardOf = new Map();
  for (const board of await listScrumBoards(work)) {
    for (const s of await listSprints(board.id, 'active,closed', work)) {
      if (!boardOf.has(String(s.id))) boardOf.set(String(s.id), String(s.originBoardId ?? board.id));
    }
  }
  for (const s of Object.values((await loadConfig())?.sprints ?? {})) if (!boardOf.has(String(s.id))) boardOf.set(String(s.id), String(s.boardId));
  const sprints = [];
  for (const id of [...boardOf.keys()].sort(compareIds)) {
    const count = await countV1(id);
    if (count) sprints.push({ id, boardId: boardOf.get(id), count });
  }
  const total = sprints.reduce((n, s) => n + s.count, 0);
  const state = { total, sprints, index: 0, cursor: null, done: 0, complete: sprints.length === 0 };
  await kvs.set(MIGRATION_KEY, state);
  console.log(`migration: ${total} v1 rows in ${sprints.length} sprints`);
  return state;
}

// The current estimate of each issue still in Jira; an issue Jira no longer returns (asApp sees every
// issue) has been deleted.
async function readIssues(issueIds, fieldId, work) {
  const found = new Map();
  const ids = [...new Set(issueIds)];
  for (let i = 0; i < ids.length; i += 100) {
    const page = await jiraJson('app', route`/rest/api/3/issue/bulkfetch`, postJson({ issueIdsOrKeys: ids.slice(i, i + 100), fields: fieldId ? [fieldId] : ['key'] }), work);
    for (const issue of page.issues ?? []) found.set(String(issue.id), issue);
  }
  return found;
}

// Copies v1 rows one page at a time until done or until this invocation has no room for another page
// (measured by the page it just did). Returns the state; the caller schedules the rest.
export async function migrateSome(work) {
  let state = (await kvs.get(MIGRATION_KEY)) ?? (await plan(work));
  const fields = new Map();
  while (!state.complete) {
    const t0 = Date.now();
    const sprint = state.sprints[state.index];
    let q = kvs.entity(V1_CHANGES).query().index('by-sprint', { partition: [sprint.id] }).limit(PAGE);
    if (state.cursor) q = q.cursor(state.cursor);
    const page = await q.getMany();
    const rows = page.results.map((r) => r.value);
    if (!fields.has(sprint.boardId)) fields.set(sprint.boardId, await boardEstimateField(sprint.boardId, work));
    const fieldId = fields.get(sprint.boardId);
    const issues = await readIssues(rows.map((r) => String(r.issueId)), fieldId, work);
    for (const v1 of rows) {
      const issue = issues.get(String(v1.issueId));
      await copyV1(v1, { estimate: issue ? estimateOf(issue.fields, fieldId) : 0, boardId: sprint.boardId, estimateField: fieldId ?? '', deleted: !issue });
    }
    state = page.nextCursor
      ? { ...state, cursor: page.nextCursor, done: state.done + rows.length }
      : { ...state, index: state.index + 1, cursor: null, done: 0 };
    if (state.index >= state.sprints.length) state = { ...state, complete: true };
    await kvs.set(MIGRATION_KEY, state);
    if (!state.complete && !work.hasTimeFor(2 * (Date.now() - t0))) break;
  }
  if (state.complete) console.log(`migration: complete, ${state.total} v1 rows in scope-ledger`);
  return state;
}
