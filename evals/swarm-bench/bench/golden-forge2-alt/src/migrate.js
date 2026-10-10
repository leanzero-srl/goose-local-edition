import { kvs, Filter } from '@forge/kvs';
import { fieldAt, trackedFields } from './config';
import { recordRow, rowKey, v1OfSprint, queryAll, LEDGER } from './ledger';
import { MIGRATION_KEY, loadMigration } from './scope';
import { fetchIssues, numberOrNull } from './sync';

// §9: every v1 row (entity scope-change, never changed) gets exactly one scope-ledger copy under the same key, with
// v1's change time, kind, issue, author and source. The plan counts v1's rows once: v2 never writes scope-change,
// so that count is what v1 left at the upgrade.
export async function migrationPlan(cfg) {
  const existing = await loadMigration();
  if (existing) return existing;
  const ids = new Set([...Object.keys(cfg.known ?? {}), ...Object.keys(cfg.sprints ?? {})]);
  const v1Config = await kvs.get('config'); // v1's working config, when it is still there
  for (const id of Object.keys(v1Config?.sprints ?? {})) ids.add(String(id));
  const counts = {};
  for (const id of [...ids].sort((a, b) => Number(a) - Number(b))) {
    const n = (await v1OfSprint(id)).length;
    if (n) counts[id] = n;
  }
  const total = Object.values(counts).reduce((a, b) => a + b, 0);
  const plan = { total, sprints: Object.keys(counts), complete: total === 0, plannedAt: Date.now() };
  await kvs.set(MIGRATION_KEY, plan);
  return plan;
}

// Copies the v1 rows that have no copy yet. `issuesById` holds issues the caller already read (fields included);
// only the others are read from Jira. Idempotent and resumable: a cut-off run leaves only whole rows behind and the
// next run picks up the rest.
export async function runMigration(cfg, policy, issuesById, checkTime) {
  const plan = await migrationPlan(cfg);
  if (plan.complete) return { plan, copied: 0 };
  const pending = [];
  for (const sprintId of plan.sprints) {
    for (const v1 of await v1OfSprint(sprintId)) {
      checkTime();
      if (!(await kvs.entity(LEDGER).get(rowKey(v1.changeId, v1.sprintId)))) pending.push(v1);
    }
  }
  const tracked = trackedFields(cfg);
  const need = [...new Set(pending.map((v) => String(v.issueId)).filter((id) => !issuesById.has(id)))];
  const { found, missing } = need.length ? await fetchIssues(need, tracked, policy) : { found: new Map(), missing: new Set() };
  let copied = 0;
  for (const v1 of pending) {
    checkTime();
    const issueId = String(v1.issueId);
    const issue = issuesById.get(issueId) ?? found.get(issueId);
    const boardId = cfg.known?.[String(v1.sprintId)]?.boardId ?? cfg.sprints?.[String(v1.sprintId)]?.boardId ?? '';
    const field = fieldAt(cfg, boardId, v1.at);
    const row = {
      sprintId: String(v1.sprintId),
      changeId: String(v1.changeId),
      at: v1.at,
      created: v1.created,
      kind: v1.kind,
      issueId,
      issueKey: v1.issueKey,
      authorId: v1.authorId ?? '',
      authorName: v1.authorName ?? '',
      source: v1.source,
      estimate: numberOrNull(issue?.fields?.[field]) ?? 0,
      estimateField: field ?? '',
      boardId,
      deleted: missing.has(issueId),
      deployedEnvs: '',
    };
    if (await recordRow(row, v1)) copied += 1;
  }
  const done = { ...plan, complete: true, completedAt: Date.now() };
  await kvs.set(MIGRATION_KEY, done);
  return { plan: done, copied };
}

// "Migrated <n> of <total> v1 rows": n counts the scope-ledger rows that are copies of v1 rows.
export async function migrationProgress(plan) {
  let n = 0;
  for (const sprintId of plan.sprints) {
    n += (await queryAll(LEDGER, 'by-sprint', [sprintId], new Filter().and('migrated', { condition: 'EQUAL_TO', values: [true] }))).length;
  }
  const text = `Migrated ${n} of ${plan.total} v1 rows${n >= plan.total ? ' — complete' : ''}`;
  return { n, total: plan.total, complete: n >= plan.total, text };
}
