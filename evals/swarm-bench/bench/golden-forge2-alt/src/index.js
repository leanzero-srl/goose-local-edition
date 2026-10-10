import Resolver from '@forge/resolver';
import { Queue, InvocationError, InvocationErrorCode } from '@forge/events';
import { JiraError, RateLimited, Deferred } from './jira';
import { backgroundPolicy, personPolicy } from './budget';
import { loadConfig, saveConfig, discoverConfig, trackedFields } from './config';
import { applyIssueEvent, markDeleted, reconcileAll, closeSprint, OutOfTime } from './sync';
import { runMigration } from './migrate';
import { boardsView, widgetView, getSprint, personView, postSummary } from './views';
import { announce, CHANNEL } from './realtime';
import { explainSprint } from './explain';
import { handleDeployment } from './ci';
import { adminLoad, adminSave, adminRotate } from './admin';
import { remainingMs } from './time';

const QUEUE_KEY = 'scope-ledger';
const MAX_RETRY_AFTER = 900; // @forge/events: InvocationError retryAfter is at most 900 s

const queue = () => new Queue({ key: QUEUE_KEY });

// ---- trigger: avi:jira:updated:issue, avi:jira:deleted:issue ------------------------------------
// Hands relevant work to the queue and returns: never writes storage, and an update that touches neither the Sprint
// field nor an estimation field does no Jira or queue work.
export async function onIssueUpdated(event) {
  const issueId = event?.issue?.id;
  if (issueId === undefined || issueId === null) return;
  if (event.eventType === 'avi:jira:deleted:issue') {
    await queue().push([{ body: { kind: 'deleted', issueId: String(issueId) } }]);
    return;
  }
  const items = event?.changelog?.items ?? [];
  if (!items.length) return;
  const cfg = await loadConfig();
  const estimateFields = new Set(cfg ? trackedFields(cfg) : []);
  const sprintChange = items.some((i) => (cfg && i.fieldId ? i.fieldId === cfg.sprintFieldId : i.field === 'Sprint'));
  const estimateChange = items.some((i) => estimateFields.has(i.fieldId) || estimateFields.has(i.field));
  if (!sprintChange && !estimateChange) return;
  await queue().push([{ body: { kind: 'issue', issueId: String(issueId), changelogId: String(event.changelog?.id ?? ''), sprintChange, estimateChange } }]);
}

// ---- consumer ---------------------------------------------------------------------------------------

// A Retry-After or a spent share becomes a retry request carrying the wait, so the queue (not this invocation)
// waits. retryData is required: for timeoutSeconds > 55 the runtime measures it and throws on undefined.
const retryLater = (e) =>
  new InvocationError({
    retryAfter: Math.min(Math.max(Math.ceil(e.waitMs / 1000), 1), MAX_RETRY_AFTER),
    retryReason: InvocationErrorCode.FUNCTION_UPSTREAM_RATE_LIMITED,
    retryData: { reason: e.reason },
  });

export async function consumeChange(event) {
  const body = event?.body ?? {};
  try {
    if (body.kind === 'reconcile') {
      await scheduledWork(await backgroundPolicy({ maxWaitMs: 30_000, marginMs: 60_000 }));
      return { continued: true };
    }
    const policy = await backgroundPolicy({ maxWaitMs: 10_000, marginMs: 20_000 });
    if (body.kind === 'deleted') {
      const sprintIds = await markDeleted([body.issueId]);
      await announce(sprintIds);
      return { deleted: body.issueId, sprintIds };
    }
    const previous = await loadConfig();
    const cfg = previous ? structuredClone(previous) : await discoverConfig(policy, null);
    const result = await applyIssueEvent(cfg, body, policy);
    await saveConfig(cfg, previous);
    await announce(result.sprintIds);
    return result;
  } catch (e) {
    if (e instanceof Deferred) return retryLater(e);
    throw e;
  }
}

// ---- scheduled trigger (hourly), and its continuation through the queue --------------------------

async function scheduledWork(policy) {
  try {
    const previous = await loadConfig();
    const cfg = await discoverConfig(policy, previous);
    const closed = [];
    for (const [id, entry] of Object.entries(previous?.sprints ?? {})) {
      if (cfg.sprints[id] || cfg.known[id]?.state === 'active') continue;
      cfg.sprints[id] = entry;
      await closeSprint(cfg, id, { name: cfg.known[id]?.name ?? entry.name, completeDate: cfg.known[id]?.completeDate });
      closed.push(id);
    }
    await saveConfig(cfg, previous);
    const checkTime = () => {
      if (remainingMs() < policy.marginMs) throw new OutOfTime('scheduled work out of time');
    };
    const result = await reconcileAll(cfg, policy, { marginMs: policy.marginMs, migrate: (byId) => runMigration(cfg, policy, byId, checkTime) });
    await announce([...result.sprintIds, ...closed]);
    console.log(`reconcile: ${Object.keys(cfg.sprints).length} active sprints, ${result.issues} issues read, ${result.rows} rows, ${result.members} members, ${result.statuses} field values written; closed ${closed.length}`);
  } catch (e) {
    if (e instanceof OutOfTime) {
      await queue().push([{ body: { kind: 'reconcile' } }]);
      console.log('reconcile: continued in a queue invocation (time limit)');
      return;
    }
    if (e instanceof Deferred) {
      if (e.reason === 'quota' || e.reason === 'background share') {
        console.log(`reconcile: stopped until the next hour (${e.reason})`);
        return;
      }
      await queue().push([{ body: { kind: 'reconcile' }, delayInSeconds: Math.min(Math.max(Math.ceil(e.waitMs / 1000), 1), MAX_RETRY_AFTER) }]);
      console.log(`reconcile: continued later (${e.reason})`);
      return;
    }
    throw e;
  }
}

export async function reconcile() {
  await scheduledWork(await backgroundPolicy({ maxWaitMs: 30_000, marginMs: 60_000 }));
}

// ---- Custom UI resolvers (dashboard widget + sprint action) ------------------------------------------

// Resolvers live 25 s: a short Retry-After is sat out, a longer one goes back to the page, which waits and calls
// again. Every resolver answers; none throws.
const UI = personPolicy(5000);

const answer = (fn) => async (req) => {
  try {
    return await fn(req);
  } catch (e) {
    if (e instanceof RateLimited) return { rateLimited: true, retryAfter: e.retryAfterSeconds };
    console.error(`resolver ${req?.call?.functionKey ?? ''} failed: ${e?.message ?? e}`);
    return { ok: false, error: e instanceof JiraError ? `Jira refused the request (${e.status}).` : `The request failed: ${e?.message ?? e}` };
  }
};

const contextSprintId = (context) => {
  const id = context?.extension?.sprint?.id;
  return id === undefined || id === null ? null : String(id);
};

const resolver = new Resolver();

resolver.define('boards', answer(async () => ({ boards: await boardsView(UI) })));

resolver.define(
  'widget',
  answer(async ({ payload, context }) => {
    const boardId = payload?.boardId ?? context?.extension?.config?.boardId;
    if (boardId === undefined || boardId === null || boardId === '') return { needsConfig: true };
    return { ...(await widgetView(String(boardId), UI)), realtime: { channel: CHANNEL } };
  }),
);

resolver.define(
  'sprintLedger',
  answer(async ({ context }) => {
    const sprintId = contextSprintId(context);
    if (!sprintId) return { ok: false, error: 'This action was opened without a sprint.' };
    const sprint = await getSprint(sprintId, UI);
    if (!sprint) return { ok: false, error: `Sprint ${sprintId} was not found.` };
    if (!sprint.started) return { notStarted: true, sprint };
    return personView(sprint, UI);
  }),
);

resolver.define(
  'explain',
  answer(async ({ context }) => {
    const sprintId = contextSprintId(context);
    const sprint = sprintId && (await getSprint(sprintId, UI));
    if (!sprint || !sprint.started) return { ok: false, error: 'This action was opened without a started sprint.' };
    return explainSprint(await personView(sprint, UI));
  }),
);

resolver.define(
  'postSummary',
  answer(async ({ payload, context }) => {
    const sprintId = contextSprintId(context);
    const sprint = sprintId && (await getSprint(sprintId, UI));
    if (!sprint || !sprint.started) return { ok: false, error: 'This action was opened without a started sprint.' };
    return postSummary(sprint, payload?.changeId, context?.accountId, UI);
  }),
);

export const uiResolver = resolver.getDefinitions();

// ---- admin page (UI Kit) -------------------------------------------------------------------------------

const admin = new Resolver();
admin.define('load', adminLoad);
admin.define('save', adminSave);
admin.define('rotate', adminRotate);
export const adminResolver = admin.getDefinitions();

// ---- Rovo action get-sprint-scope ----------------------------------------------------------------------

const ACTION = personPolicy(30_000); // actions live 55 s

export async function getSprintScope(payload) {
  const raw = payload?.sprintId;
  const sprintId = raw === undefined || raw === null ? '' : String(raw).trim();
  if (!sprintId) return { error: 'sprintId is required: pass the numeric id of a Jira sprint.' };
  if (!/^\d+$/.test(sprintId)) return { error: `"${sprintId}" is not a sprint id: sprint ids are numbers, for example 41.` };
  try {
    const sprint = await getSprint(sprintId, ACTION);
    if (!sprint) return { error: `No sprint with id ${sprintId} exists on this site.` };
    const view = await personView(sprint, ACTION);
    return {
      sprintId: sprint.id,
      sprintName: sprint.name,
      committed: view.values.committed,
      added: view.values.added,
      removed: view.values.removed,
      creepPercent: view.values.creepPercent,
      hiddenChanges: view.hiddenCount,
      changes: view.changes.map((c) => ({ changeId: c.changeId, issueKey: c.issueKey, kind: c.kind, points: c.points, at: c.at, by: c.by })),
    };
  } catch (e) {
    if (e instanceof RateLimited) return { error: `Jira is rate limiting this app; try again in ${e.retryAfterSeconds} seconds.` };
    return { error: `Could not read the sprint scope: ${e.message}` };
  }
}

// ---- web trigger: CI deployment events --------------------------------------------------------------------

export async function ciDeployment(request) {
  try {
    return await handleDeployment(request);
  } catch (e) {
    console.error(`ci deployment failed: ${e?.message ?? e}`);
    return { outputKey: 'error' };
  }
}
