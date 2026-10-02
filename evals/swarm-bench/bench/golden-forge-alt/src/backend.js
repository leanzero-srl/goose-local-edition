import { Queue, InvocationError, InvocationErrorCode } from '@forge/events';
import { appClient, RetryLater, scrumBoards, boardSprints, boardEstimationField, sprintFieldId, sprintIssues, searchIssues, issueWithChangelog } from './jira';
import { getRegistry, setRegistry, sprintMembers, sprintChanges, getMember, putMember, getChange, recordChange } from './store';
import { issueFields, isTracked, learnSprints, rowsForIssue, sprintsTouching, memberDiffers } from './ledger';

const work = new Queue({ key: 'scope-ledger-work' });
const WRITE_CHUNK = 40;

function watchedFields(registry) {
  if (!registry) return null;
  return new Set([registry.sprintField, ...Object.values(registry.boards).map((b) => b.estField)].filter(Boolean));
}

// trigger: decide from the event's own changelog; only KVS is read here.
export async function onIssueUpdated(event) {
  const items = (event && event.changelog && event.changelog.items) || [];
  const watched = watchedFields(await getRegistry());
  const relevant = items.some((i) =>
    watched ? watched.has(i.fieldId) : i.field === 'Sprint' || /story point|estimate/i.test(i.field || ''),
  );
  if (!relevant || !event.issue) return { queued: false };
  await work.push([{ body: { kind: 'issue', issueId: String(event.issue.id) } }]);
  return { queued: true };
}

async function writeRows({ members, changes }) {
  let written = 0;
  for (const m of members) {
    if (memberDiffers(await getMember(m.sprintId, m.issueId), m)) {
      await putMember(m);
      written += 1;
    }
  }
  for (const c of changes) {
    if (!(await getChange(c.sprintId, c.changeId)) && (await recordChange(c))) written += 1;
  }
  return written;
}

async function syncOneIssue(issueId) {
  const jira = appClient({ waitBudgetSeconds: 20 });
  let registry = await getRegistry();
  if (!registry) registry = { sprintField: await sprintFieldId(jira), boards: {}, sprints: {} };
  const fields = issueFields(registry);
  let issue = await issueWithChangelog(jira, issueId, fields);
  if (!issue) return { written: 0, reason: 'issue not found' };
  if (await learnSprints(jira, registry, sprintsTouching(issue, registry.sprintField))) {
    await setRegistry(registry);
    if (issueFields(registry).some((f) => !fields.includes(f))) issue = await issueWithChangelog(jira, issueId, issueFields(registry));
  }
  return { written: await writeRows(rowsForIssue(issue, registry, 'event')) };
}

// consumer: every ledger write happens here.
export async function consume(event) {
  const body = (event && event.body) || {};
  try {
    if (body.kind === 'issue') return await syncOneIssue(body.issueId);
    if (body.kind === 'write') return { written: await writeRows(body) };
    return { ignored: body.kind || null };
  } catch (err) {
    if (err instanceof RetryLater) {
      return new InvocationError({
        retryAfter: Math.max(1, Math.ceil(err.seconds)),
        retryReason: InvocationErrorCode.FUNCTION_UPSTREAM_RATE_LIMITED,
        retryData: { kind: body.kind, issueId: body.issueId || null },
      });
    }
    throw err;
  }
}

function jqlInstant(ms) {
  const d = new Date(ms);
  const p = (n) => String(n).padStart(2, '0');
  return `${d.getUTCFullYear()}-${p(d.getUTCMonth() + 1)}-${p(d.getUTCDate())} ${p(d.getUTCHours())}:${p(d.getUTCMinutes())}`;
}

async function refreshRegistry(jira, previous) {
  const next = {
    sprintField: (previous && previous.sprintField) || (await sprintFieldId(jira)),
    boards: {},
    sprints: {},
  };
  for (const b of await scrumBoards(jira)) {
    next.boards[b.id] = { name: b.name, estField: await boardEstimationField(jira, b.id) };
    for (const s of await boardSprints(jira, b.id, 'active')) {
      next.sprints[String(s.id)] = { id: String(s.id), name: s.name, state: s.state, startDate: s.startDate || null, originBoardId: s.originBoardId != null ? String(s.originBoardId) : b.id };
    }
  }
  for (const s of Object.values(next.sprints)) {
    if (s.originBoardId && !next.boards[s.originBoardId]) next.boards[s.originBoardId] = { name: null, estField: await boardEstimationField(jira, s.originBoardId) };
  }
  if (JSON.stringify(previous) !== JSON.stringify(next)) await setRegistry(next);
  return next;
}

// scheduledTrigger: reads Jira and the ledger, computes what is missing, and hands only the
// difference to the consumer. Nothing new means no queue work and no writes.
export async function hourly() {
  const jira = appClient({ waitBudgetSeconds: 600 });
  const registry = await refreshRegistry(jira, await getRegistry());
  const active = Object.values(registry.sprints).filter(isTracked);
  if (!active.length) return { sprints: 0, queued: 0 };

  const fields = issueFields(registry);
  const issues = new Map();
  for (const s of active) for (const i of await sprintIssues(jira, s.id, fields)) issues.set(String(i.id), i);
  const since = Math.min(...active.map((s) => Date.parse(s.startDate))) - 24 * 3600 * 1000;
  const leavers = await searchIssues(jira, `updated >= "${jqlInstant(since)}" AND (sprint not in openSprints() OR sprint is EMPTY)`, fields);
  for (const i of leavers) if (!issues.has(String(i.id))) issues.set(String(i.id), i);

  const known = new Map();
  for (const s of active) {
    const [members, changes] = await Promise.all([sprintMembers(s.id), sprintChanges(s.id)]);
    known.set(s.id, { members: new Map(members.map((m) => [m.issueId, m])), changes: new Set(changes.map((c) => c.changeId)) });
  }
  const members = [];
  const changes = [];
  for (const issue of issues.values()) {
    const rows = rowsForIssue(issue, registry, 'reconcile');
    for (const m of rows.members) if (memberDiffers(known.get(m.sprintId).members.get(m.issueId), m)) members.push(m);
    for (const c of rows.changes) if (!known.get(c.sprintId).changes.has(c.changeId)) changes.push(c);
  }
  const rows = [...members.map((m) => ['m', m]), ...changes.map((c) => ['c', c])];
  const batch = [];
  for (let i = 0; i < rows.length; i += WRITE_CHUNK) {
    const slice = rows.slice(i, i + WRITE_CHUNK);
    batch.push({ body: { kind: 'write', members: slice.filter((r) => r[0] === 'm').map((r) => r[1]), changes: slice.filter((r) => r[0] === 'c').map((r) => r[1]) } });
  }
  for (let i = 0; i < batch.length; i += 50) await work.push(batch.slice(i, i + 50));
  return { sprints: active.length, issues: issues.size, members: members.length, changes: changes.length, queued: batch.length };
}
