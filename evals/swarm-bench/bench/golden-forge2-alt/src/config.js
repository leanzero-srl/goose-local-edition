import { kvs } from '@forge/kvs';
import { jiraJson, route } from './jira';
import { HOUR_MS } from './time';

// v1 kept its own working config under 'config' (STARTER: do not count on finding it); v2's has its own key.
const CONFIG_KEY = 'config-v2';
const SPRINT_FIELD_TYPE = 'com.pyxis.greenhopper.jira:gh-sprint';
// The jira:customField module's `name` (manifest.yml): GET /rest/api/3/field lists the field under it (§15).
export const STATUS_FIELD_NAME = 'Scope status';

// config = {
//   sprintFieldId, statusFieldId,
//   sprints: { [id]: { id, name, startDate, startMs, boardId } }   the ACTIVE, started sprints
//   known:   { [id]: { state, boardId, name, completeDate } }        every sprint the boards list (any state)
//   boards:  { [boardId]: [{ field, from }] }                        each board's estimation field over time
// }
// Jira keeps no history of a board's estimation field, so the app keeps its own: a field first seen at an observation
// is taken to apply from just after the previous observation of the board.
export const loadConfig = () => kvs.get(CONFIG_KEY);

export async function saveConfig(next, previous) {
  if (previous && JSON.stringify(previous) === JSON.stringify(next)) return false;
  await kvs.set(CONFIG_KEY, next);
  return true;
}

async function pagedAgile(endpoint, build, policy) {
  const values = [];
  for (let startAt = 0; ; ) {
    const page = await jiraJson('app', endpoint, build(startAt), policy);
    values.push(...(page.values ?? []));
    if (page.isLast || !page.values?.length) return values;
    startAt += page.values.length;
  }
}

export const listScrumBoards = (policy) =>
  pagedAgile('GET /rest/agile/1.0/board', (startAt) => route`/rest/agile/1.0/board?type=scrum&startAt=${startAt}&maxResults=50`, policy);

export const listBoardSprints = (boardId, policy) =>
  pagedAgile('GET /rest/agile/1.0/board/{boardId}/sprint', (startAt) => route`/rest/agile/1.0/board/${boardId}/sprint?startAt=${startAt}&maxResults=50`, policy);

export const listActiveSprints = (boardId, policy) =>
  pagedAgile('GET /rest/agile/1.0/board/{boardId}/sprint', (startAt) => route`/rest/agile/1.0/board/${boardId}/sprint?state=active&startAt=${startAt}&maxResults=50`, policy);

export async function boardEstimateField(boardId, policy) {
  const conf = await jiraJson('app', 'GET /rest/agile/1.0/board/{boardId}/configuration', route`/rest/agile/1.0/board/${boardId}/configuration`, policy);
  return conf?.estimation?.field?.fieldId ?? null;
}

export async function getSprintFromJira(sprintId, policy) {
  try {
    return await jiraJson('app', 'GET /rest/agile/1.0/sprint/{sprintId}', route`/rest/agile/1.0/sprint/${sprintId}`, policy);
  } catch (e) {
    if (e.status === 404) return null;
    throw e;
  }
}

export const isStarted = (sprint) => sprint.state !== 'future' && Boolean(sprint.startDate);

export const sprintEntry = (sprint, boardId) => ({
  id: String(sprint.id),
  name: sprint.name,
  startDate: sprint.startDate,
  startMs: Date.parse(sprint.startDate),
  boardId: String(boardId),
});

// ---- estimation fields over time ----------------------------------------------------------------

// Records what a board uses now. Returns true when that is a change. The previous look at the board was the hourly
// discovery: at the top of this hour (an event-time look) or of the previous one (the discovery itself).
export function observeField(cfg, boardId, field, { discovery = false, now = Date.now() } = {}) {
  const id = String(boardId);
  cfg.boards = cfg.boards ?? {};
  const history = cfg.boards[id] ?? [];
  if (history.length && history[history.length - 1].field === (field ?? null)) return false;
  const lastLook = now - (now % HOUR_MS) - (discovery ? HOUR_MS : 0);
  const from = history.length ? Math.max(history[history.length - 1].from + 1, Math.min(now, lastLook + 1)) : 0;
  cfg.boards[id] = [...history, { field: field ?? null, from }];
  return true;
}

export function fieldAt(cfg, boardId, at) {
  const history = cfg.boards?.[String(boardId)] ?? [];
  let field = history.length ? history[0].field : null;
  for (const entry of history) if (entry.from <= at) field = entry.field;
  return field;
}

export const currentField = (cfg, boardId) => {
  const history = cfg.boards?.[String(boardId)] ?? [];
  return history.length ? history[history.length - 1].field : null;
};

// Every estimation field any board uses or used: the issue state keeps all of their values.
export const trackedFields = (cfg) => [...new Set(Object.values(cfg.boards ?? {}).flatMap((h) => h.map((e) => e.field)).filter(Boolean))].sort();

// ---- discovery ------------------------------------------------------------------------------------

export async function discoverFields(policy) {
  const fields = await jiraJson('app', 'GET /rest/api/3/field', route`/rest/api/3/field`, policy);
  const sprint = fields.find((f) => f.schema?.custom === SPRINT_FIELD_TYPE);
  if (!sprint) throw new Error(`no field with schema.custom ${SPRINT_FIELD_TYPE} on this site`);
  const status = fields.find((f) => f.custom !== false && f.name === STATUS_FIELD_NAME);
  return { sprintFieldId: sprint.id, statusFieldId: status?.id ?? null };
}

// The scheduled run's full picture: the fields, every scrum board's sprints (all states, one listing per board) and
// every board's estimation field now, merged into the previous config's field history.
export async function discoverConfig(policy, previous) {
  const now = Date.now();
  const { sprintFieldId, statusFieldId } = await discoverFields(policy);
  const boards = await listScrumBoards(policy);
  const listed = new Map();
  for (const board of boards) {
    for (const sprint of await listBoardSprints(board.id, policy)) {
      if (!listed.has(String(sprint.id))) listed.set(String(sprint.id), { sprint, boardId: String(sprint.originBoardId ?? board.id) });
    }
  }
  const cfg = { sprintFieldId, statusFieldId, boards: structuredClone(previous?.boards ?? {}), sprints: {}, known: {} };
  const boardIds = new Set(boards.map((b) => String(b.id)));
  for (const { boardId } of listed.values()) boardIds.add(boardId);
  for (const boardId of [...boardIds].sort((a, b) => Number(a) - Number(b))) observeField(cfg, boardId, await boardEstimateField(boardId, policy), { discovery: true, now });
  for (const [id, { sprint, boardId }] of [...listed].sort(([a], [b]) => Number(a) - Number(b))) {
    cfg.known[id] = knownEntry(sprint, boardId);
    if (sprint.state === 'active' && isStarted(sprint)) cfg.sprints[id] = sprintEntry(sprint, boardId);
  }
  return cfg;
}

const knownEntry = (sprint, boardId) => ({ state: sprint.state, boardId: String(boardId), name: sprint.name, completeDate: sprint.completeDate ?? null });

// An event names a sprint the config does not hold as active. Returns its entry when Jira says it is active and
// started (and adds it to cfg), else null. `hint` is the state the issue's Sprint field reports, which can be newer
// than the config's (a sprint that started since the last scheduled run).
export async function resolveSprint(cfg, sprintId, policy, hint) {
  if (cfg.sprints[sprintId]) return cfg.sprints[sprintId];
  const known = cfg.known?.[sprintId];
  if (known && known.state !== 'active' && hint !== 'active') return null;
  const res = await getSprintFromJira(sprintId, policy);
  cfg.known = cfg.known ?? {};
  if (!res) {
    cfg.known[sprintId] = { state: 'deleted', boardId: null, name: '', completeDate: null };
    return null;
  }
  const boardId = String(res.originBoardId);
  cfg.known[sprintId] = knownEntry(res, boardId);
  if (res.state !== 'active' || !isStarted(res)) return null;
  if (!cfg.boards?.[boardId]?.length) observeField(cfg, boardId, await boardEstimateField(boardId, policy));
  cfg.sprints[sprintId] = sprintEntry(res, boardId);
  return cfg.sprints[sprintId];
}
