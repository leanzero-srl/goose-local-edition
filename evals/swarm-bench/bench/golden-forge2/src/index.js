import Resolver from '@forge/resolver';
import { Queue, InvocationError, InvocationErrorCode } from '@forge/events';
import { JiraError, RateLimited, Background } from './jira';
import { loadConfig, saveConfig, discoverConfig } from './config';
import { applyIssueEvent, estimateFieldIds, reconcileAll, settleClosedSprints } from './sync';
import { boardsView, widgetView, getSprint, personView, postSummary } from './views';
import { announce, CHANNEL } from './realtime';
import { explainSprint } from './explain';
import { migrateSome } from './migrate';
import { markIssueDeleted, migrationComplete } from './ledger';
import { loadSettings } from './settings';
import { handleCiEvent, applyDeployment } from './ci';

export { adminResolver } from './admin';

const QUEUE_KEY = 'scope-ledger';
const MAX_RETRY_AFTER = 900; // @forge/events: InvocationError retryAfter and push delay are at most 900 s
const LONG_LIMIT_SECONDS = 900; // consume-change and reconcile declare timeoutSeconds: 900
const WEBTRIGGER_LIMIT_SECONDS = 55;

const queue = () => new Queue({ key: QUEUE_KEY });
const enqueue = (body, delaySeconds = 0) =>
  queue().push([delaySeconds > 0 ? { body, delayInSeconds: Math.min(delaySeconds, MAX_RETRY_AFTER) } : { body }]);

// Background work (triggers, the consumer, the scheduled run, the web trigger) runs dosed: at most the
// admin's background share of the hour's points, paused after a quota 429, paced per endpoint, and never
// waiting past its own time limit.
async function backgroundWork(limitSeconds) {
  const { backgroundShare } = await loadSettings();
  return new Background({ sharePercent: backgroundShare, limitSeconds });
}

// retryData is not optional in practice: for a function with timeoutSeconds > 55 the Forge runtime wrapper
// measures Buffer.byteLength(JSON.stringify(retryData)), which throws on undefined (measured 2026-10-02).
const retryLater = (e) =>
  new InvocationError({
    retryAfter: Math.min(Math.max(e.retryAfterSeconds, 1), MAX_RETRY_AFTER),
    retryReason: InvocationErrorCode.FUNCTION_UPSTREAM_RATE_LIMITED,
    retryData: { retryAfterSeconds: e.retryAfterSeconds, reason: e.reason },
  });

// v1's config has no scopeFieldId: v2 rediscovers before its first event.
const isV2Config = (cfg) => Boolean(cfg) && 'scopeFieldId' in cfg;

// ---- triggers: hand the work to the queue and return ------------------------------------------------------

// Only updates whose changelog touches the Sprint field or an estimation field become queue work;
// anything else does no Jira or queue work.
export async function onIssueUpdated(event) {
  const items = event?.changelog?.items ?? [];
  if (!items.length || !event.issue?.id) return;
  const cfg = await loadConfig();
  const estimateFields = new Set(cfg ? estimateFieldIds(cfg) : []);
  const sprintChange = items.some((i) => (cfg && i.fieldId ? i.fieldId === cfg.sprintFieldId : i.field === 'Sprint'));
  const estimateChange = items.some((i) => estimateFields.has(i.fieldId) || estimateFields.has(i.field));
  if (!sprintChange && !estimateChange) return;
  await enqueue({ issueId: String(event.issue.id), changelogId: String(event.changelog.id ?? ''), sprintChange, estimateChange });
}

export async function onIssueDeleted(event) {
  if (!event?.issue?.id) return;
  await enqueue({ type: 'deleted', issueId: String(event.issue.id) });
}

// The v1 -> v2 upgrade: start copying the v1 ledger at once rather than at the next scheduled run.
export async function onAppUpgraded() {
  await enqueue({ type: 'migrate' });
}

// ---- the consumer --------------------------------------------------------------------------------------

async function issueEvent(body, work) {
  const previous = await loadConfig();
  const cfg = isV2Config(previous) ? structuredClone(previous) : await discoverConfig(work);
  const closedBefore = new Set(cfg.closed ?? []);
  const result = await applyIssueEvent(cfg, body, work, async (issueId) => ({ rows: 0, members: 0, sprintIds: await markIssueDeleted(issueId) }));
  await settleClosedSprints(cfg, (cfg.closed ?? []).filter((id) => !closedBefore.has(id)), work, new Set([String(body.issueId)]));
  await saveConfig(cfg, previous);
  await announce(result.sprintIds);
  return result;
}

async function continueMigration(work) {
  const state = await migrateSome(work);
  if (!state.complete) await enqueue({ type: 'migrate' });
  return { migration: state.complete ? 'complete' : 'continued' };
}

// Deployment marks are written on v2 rows, so they wait for the migration to finish.
async function deployment(body, work) {
  if (!(await migrationComplete())) {
    const state = await migrateSome(work);
    if (!state.complete) throw new RateLimited(1, 'migration');
  }
  const result = await applyDeployment(body, work);
  await announce(result.sprintIds);
  return result;
}

// The scheduled pass, also run from the queue when a pass was cut off: the migration first, then a fresh
// view of the active sprints and boards, then the backfill/heal.
async function runReconcile(work) {
  const migration = await migrateSome(work);
  if (!migration.complete) await enqueue({ type: 'migrate' });
  const previous = await loadConfig();
  const cfg = await discoverConfig(work);
  const result = await reconcileAll(cfg, isV2Config(previous) ? previous : null, work);
  await saveConfig(cfg, previous);
  await announce(result.sprintIds);
  console.log(
    `reconcile: ${Object.keys(cfg.sprints).length} active sprints, ${result.issues} issues read, ${result.rows} rows, ${result.members} memberships, ${result.statuses} statuses, ${result.deleted} deleted`,
  );
  return result;
}

// A 429, a spent background share, a pause or the time limit becomes a retry request carrying the wait,
// so the queue — not this invocation — waits.
export async function consumeChange(event) {
  const work = await backgroundWork(LONG_LIMIT_SECONDS);
  const body = event?.body ?? {};
  try {
    if (body.type === 'migrate') return await continueMigration(work);
    if (body.type === 'reconcile') return await runReconcile(work);
    if (body.type === 'deploy') return await deployment(body, work);
    if (body.type === 'deleted') {
      const sprintIds = await markIssueDeleted(body.issueId);
      await announce(sprintIds);
      return { sprintIds };
    }
    return await issueEvent(body, work);
  } catch (e) {
    if (e instanceof RateLimited) return retryLater(e);
    throw e;
  } finally {
    await work.flush();
  }
}

// ---- scheduled trigger (hourly) ---------------------------------------------------------------

// Scheduled runs are not retried: a pass that has to stop hands the rest to the queue after the wait.
export async function reconcile() {
  const work = await backgroundWork(LONG_LIMIT_SECONDS);
  try {
    await runReconcile(work);
  } catch (e) {
    if (!(e instanceof RateLimited)) throw e;
    await enqueue({ type: 'reconcile' }, Math.max(e.retryAfterSeconds, 1));
    console.log(`reconcile: continues from the queue in ${e.retryAfterSeconds}s (${e.reason})`);
  } finally {
    await work.flush();
  }
}

// ---- web trigger: CI deployments --------------------------------------------------------------------

export async function ciDeploy(request) {
  const work = await backgroundWork(WEBTRIGGER_LIMIT_SECONDS);
  try {
    return await handleCiEvent(request, work, (body) => enqueue(body));
  } finally {
    await work.flush();
  }
}

// ---- Custom UI resolvers (dashboard widget + sprint action) ------------------------------------

// Resolvers live 25 s and are person-facing (never held back for the background share). A Retry-After
// this short is slept through; a longer one goes back to the page, which waits and calls again.
const UI_POLICY = { maxWaitSeconds: 5 };

const resolver = new Resolver();

// Every resolver answers; none throws. A long Retry-After goes back to the page to wait out; any other
// failure (a Jira 4xx/5xx such as a forbidden comment, a KVS or LLM error) becomes {ok:false, error},
// which the page shows as an error flag or message while staying usable.
const rateLimitedAware = (fn) => async (req) => {
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

resolver.define('boards', rateLimitedAware(async () => ({ boards: await boardsView(UI_POLICY) })));

resolver.define(
  'widget',
  rateLimitedAware(async ({ payload, context }) => {
    const boardId = payload?.boardId ?? context?.extension?.config?.boardId;
    if (boardId === undefined || boardId === null || boardId === '') return { needsConfig: true };
    const view = await widgetView(String(boardId), UI_POLICY);
    return { ...view, realtime: { channel: CHANNEL } };
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

resolver.define(
  'explain',
  rateLimitedAware(async ({ context }) => {
    const sprintId = contextSprintId(context);
    const sprint = sprintId && (await getSprint(sprintId, UI_POLICY));
    if (!sprint) return { ok: false, error: 'This action was opened without a known sprint.' };
    return explainSprint(await personView(sprint, UI_POLICY), await loadSettings());
  }),
);

resolver.define(
  'postSummary',
  rateLimitedAware(async ({ payload, context }) => {
    const sprintId = contextSprintId(context);
    const sprint = sprintId && (await getSprint(sprintId, UI_POLICY));
    if (!sprint) return { ok: false, error: 'This action was opened without a known sprint.' };
    return postSummary(sprint, payload?.changeId, UI_POLICY, (await loadSettings()).commentGroup);
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
