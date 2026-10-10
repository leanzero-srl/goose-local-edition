import { kvs } from '@forge/kvs';
import { fieldAt } from './config';
import { computeTotals, frozenKey, ledgerOfSprint, membersOfSprint, rowKey, v1OfSprint, valueOf, byTime } from './ledger';
import { isKvsCode } from './budget';

export const MIGRATION_KEY = 'migration';
export const loadMigration = () => kvs.get(MIGRATION_KEY);

// v1's rows that have no scope-ledger copy yet, shaped as v2 rows (no estimate snapshot yet). While the migration
// runs, every surface reads them too, so the numbers stay right before the copy is complete (§9).
function v1AsRow(v1, cfg) {
  const boardId = cfg?.known?.[String(v1.sprintId)]?.boardId ?? cfg?.sprints?.[String(v1.sprintId)]?.boardId ?? '';
  return {
    sprintId: String(v1.sprintId),
    changeId: String(v1.changeId),
    at: v1.at,
    kind: v1.kind,
    issueId: String(v1.issueId),
    issueKey: v1.issueKey,
    authorId: v1.authorId ?? '',
    authorName: v1.authorName ?? '',
    source: v1.source,
    estimate: 0,
    estimateField: cfg ? fieldAt(cfg, boardId, v1.at) ?? '' : '',
    boardId,
    deleted: false,
    deployedEnvs: '',
    migrated: true,
  };
}

export async function sprintRows(sprintId, cfg) {
  const [rows, migration] = await Promise.all([ledgerOfSprint(sprintId), loadMigration()]);
  if (migration?.complete) return rows;
  const have = new Set(rows.map((r) => rowKey(r.changeId, r.sprintId)));
  const pending = (await v1OfSprint(sprintId)).filter((v) => !have.has(rowKey(v.changeId, v.sprintId)));
  return [...rows, ...pending.map((v) => v1AsRow(v, cfg))];
}

// The change's points (§1): the issue's current value of the field its board used at the change.
export const pointsOf = (row, member) => valueOf(member, row.estimateField);

export async function liveScope(sprintId, field, cfg) {
  const [rows, members] = await Promise.all([sprintRows(sprintId, cfg), membersOfSprint(sprintId)]);
  const memberByIssue = new Map(members.map((m) => [String(m.issueId), m]));
  const points = Object.fromEntries(rows.map((r) => [r.changeId, pointsOf(r, memberByIssue.get(String(r.issueId)))]));
  return { rows: rows.sort(byTime), totals: computeTotals(rows, members, field), points, field };
}

export const loadFrozen = (sprintId) => kvs.get(frozenKey(sprintId));

// A closed sprint's ledger is final (§12): its totals and change points as at the close are kept once and served from
// then on. The first freezer wins; nothing after the close moves them.
export async function freezeSprint(sprintId, meta, cfg) {
  const existing = await loadFrozen(sprintId);
  if (existing) return existing;
  const field = cfg ? fieldAt(cfg, meta.boardId, Date.parse(meta.completeDate ?? '') || Date.now()) : null;
  const scope = await liveScope(sprintId, field, cfg);
  const snapshot = {
    sprintId: String(sprintId),
    name: meta.name,
    completeDate: meta.completeDate ?? null,
    boardId: meta.boardId ?? null,
    field,
    totals: scope.totals,
    points: scope.points,
    frozenAt: Date.now(),
  };
  try {
    await kvs.set(frozenKey(sprintId), snapshot, { keyPolicy: 'FAIL_IF_EXISTS' });
    return snapshot;
  } catch (e) {
    if (isKvsCode(e, 'KEY_CONFLICT')) return loadFrozen(sprintId);
    throw e;
  }
}
