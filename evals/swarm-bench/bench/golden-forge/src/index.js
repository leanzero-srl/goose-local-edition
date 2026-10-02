import Resolver from '@forge/resolver';
import { Queue, InvocationError, InvocationErrorCode } from '@forge/events';
import { RateLimited } from './jira';
import { loadConfig, saveConfig, discoverConfig } from './config';
import { applyIssueEvent, estimateFieldIds, reconcileAll } from './sync';
import { boardsView, widgetView, getSprint, personView, postSummary } from './views';
import { announce, subscribeToken } from './realtime';
import { explainSprint } from './explain';

const QUEUE_KEY = 'scope-ledger';
const MAX_RETRY_AFTER = 900; // @forge/events: InvocationError retryAfter is at most 900 s

// ---- trigger: avi:jira:updated:issue ------------------------------------------------------------

// Hands the update to the queue only when its changelog touches the Sprint field or an estimation
// field; anything else does no Jira or queue work.
export async function onIssueUpdated(event) {
  const items = event?.changelog?.items ?? [];
  if (!items.length || !event.issue?.id) return;
  const cfg = await loadConfig();
  const estimateFields = new Set(cfg ? estimateFieldIds(cfg) : []);
  const sprintChange = items.some((i) => (cfg && i.fieldId ? i.fieldId === cfg.sprintFieldId : i.field === 'Sprint'));
  const estimateChange = items.some((i) => estimateFields.has(i.fieldId) || estimateFields.has(i.field));
  if (!sprintChange && !estimateChange) return;
  await new Queue({ key: QUEUE_KEY }).push([
    { body: { issueId: String(event.issue.id), changelogId: String(event.changelog.id ?? ''), sprintChange, estimateChange } },
  ]);
}

// ---- consumer --------------------------------------------------------------------------------

// Any 429 becomes a retry request carrying Retry-After, so the queue (not this invocation) waits.
export async function consumeChange(event) {
  const policy = { maxWaitSeconds: 0 };
  try {
    const previous = await loadConfig();
    const cfg = previous ? structuredClone(previous) : await discoverConfig(policy);
    const result = await applyIssueEvent(cfg, event.body, policy);
    await saveConfig(cfg, previous);
    await announce(result.sprintIds);
    return result;
  } catch (e) {
    if (e instanceof RateLimited) {
      // retryData is not optional in practice: for a function with timeoutSeconds > 55 the Forge runtime
      // wrapper measures Buffer.byteLength(JSON.stringify(retryData)), which throws on undefined and turns
      // the retry request into a function error (measured on the real wrapper, 2026-10-02).
      return new InvocationError({
        retryAfter: Math.min(Math.max(e.retryAfterSeconds, 1), MAX_RETRY_AFTER),
        retryReason: InvocationErrorCode.FUNCTION_UPSTREAM_RATE_LIMITED,
        retryData: { retryAfterSeconds: e.retryAfterSeconds },
      });
    }
    throw e;
  }
}

// ---- scheduled trigger (hourly) ---------------------------------------------------------------

export async function reconcile() {
  const policy = { maxWaitSeconds: Infinity };
  const previous = await loadConfig();
  const cfg = await discoverConfig(policy);
  await saveConfig(cfg, previous);
  const result = await reconcileAll(cfg, policy);
  await announce(result.sprintIds);
  console.log(`reconcile: ${Object.keys(cfg.sprints).length} active sprints, ${result.issues} issues read, ${result.rows} rows and ${result.members} memberships written`);
}

// ---- Custom UI resolvers (dashboard widget + sprint action) ------------------------------------

// Resolvers live 25 s. A Retry-After this short is slept through; a longer one goes back to the page,
// which waits and calls again.
const UI_POLICY = { maxWaitSeconds: 5 };

const resolver = new Resolver();

const rateLimitedAware = (fn) => async (req) => {
  try {
    return await fn(req);
  } catch (e) {
    if (e instanceof RateLimited) return { rateLimited: true, retryAfter: e.retryAfterSeconds };
    throw e;
  }
};

const contextSprintId = (context) => {
  const id = context?.extension?.sprint?.id;
  return id === undefined || id === null ? null : String(id);
};

resolver.define('boards', rateLimitedAware(async () => ({ boards: await boardsView(UI_POLICY) })));

resolver.define(
  'widget',
  rateLimitedAware(async ({ payload, context }) => {
    const boardId = payload?.boardId ?? context?.extension?.config?.boardId;
    if (boardId === undefined || boardId === null || boardId === '') return { needsConfig: true };
    const view = await widgetView(String(boardId), UI_POLICY);
    return payload?.withRealtime ? { ...view, realtime: await subscribeToken() } : view;
  }),
);

resolver.define(
  'sprintLedger',
  rateLimitedAware(async ({ context }) => {
    const sprintId = contextSprintId(context);
    if (!sprintId) return { error: 'This action was opened without a sprint.' };
    const sprint = await getSprint(sprintId, UI_POLICY);
    if (!sprint) return { error: `Sprint ${sprintId} was not found.` };
    if (!sprint.started) return { notStarted: true, sprint };
    return personView(sprint, UI_POLICY);
  }),
);

resolver.define('realtimeToken', async () => subscribeToken());

resolver.define(
  'explain',
  rateLimitedAware(async ({ context }) => {
    const sprintId = contextSprintId(context);
    const sprint = sprintId && (await getSprint(sprintId, UI_POLICY));
    if (!sprint) return { ok: false, error: 'This action was opened without a known sprint.' };
    return explainSprint(await personView(sprint, UI_POLICY));
  }),
);

resolver.define(
  'postSummary',
  rateLimitedAware(async ({ payload, context }) => {
    const sprintId = contextSprintId(context);
    const sprint = sprintId && (await getSprint(sprintId, UI_POLICY));
    if (!sprint) return { ok: false, error: 'This action was opened without a known sprint.' };
    return postSummary(sprint, payload?.changeId, UI_POLICY);
  }),
);

export const uiResolver = resolver.getDefinitions();

// ---- Rovo action get-sprint-scope ---------------------------------------------------------------

// Actions live 55 s; leave room for the ledger reads after a wait.
const ACTION_POLICY = { maxWaitSeconds: 30 };

export async function getSprintScope(payload) {
  const raw = payload?.sprintId;
  const sprintId = raw === undefined || raw === null ? '' : String(raw).trim();
  if (!sprintId) return { error: 'sprintId is required: pass the numeric id of a Jira sprint.' };
  if (!/^\d+$/.test(sprintId)) return { error: `"${sprintId}" is not a sprint id: sprint ids are numbers, for example 41.` };
  try {
    const sprint = await getSprint(sprintId, ACTION_POLICY);
    if (!sprint) return { error: `No sprint with id ${sprintId} exists on this site.` };
    const view = await personView(sprint, ACTION_POLICY);
    return {
      sprintId: sprint.id,
      sprintName: sprint.name,
      committed: view.values.committed,
      added: view.values.added,
      removed: view.values.removed,
      creepPercent: view.values.creepPercent,
      hiddenChanges: view.hiddenCount,
      changes: view.changes.map((c) => ({
        changeId: c.changeId,
        issueKey: c.issueKey,
        kind: c.kind,
        points: c.points,
        at: c.at,
        by: c.by,
      })),
    };
  } catch (e) {
    if (e instanceof RateLimited) return { error: `Jira is rate limiting this app; try again in ${e.retryAfterSeconds} seconds.` };
    return { error: `Could not read the sprint scope: ${e.message}` };
  }
}
