import { kvs } from '@forge/kvs';
import { jiraJson, route } from './jira';

const CONFIG_KEY = 'config';
const SPRINT_FIELD_TYPE = 'com.pyxis.greenhopper.jira:gh-sprint';

// config = { sprintFieldId, sprints: { [sprintId]: { id, name, startDate, startMs, boardId, estimateFieldId } } }
// holding the ACTIVE sprints only. Written by the scheduled run, extended by the consumer when an event
// names an active sprint it has not seen yet.
export const loadConfig = () => kvs.get(CONFIG_KEY);

export async function saveConfig(next, previous) {
  if (previous && JSON.stringify(previous) === JSON.stringify(next)) return false;
  await kvs.set(CONFIG_KEY, next);
  return true;
}

export async function discoverSprintField(policy) {
  const fields = await jiraJson('app', route`/rest/api/3/field`, undefined, policy);
  const sprint = fields.find((f) => f.schema?.custom === SPRINT_FIELD_TYPE);
  if (!sprint) throw new Error(`no field with schema.custom ${SPRINT_FIELD_TYPE} on this site`);
  return sprint.id;
}

async function pagedAgile(build, policy) {
  const values = [];
  for (let startAt = 0; ; ) {
    const page = await jiraJson('app', build(startAt), undefined, policy);
    values.push(...(page.values ?? []));
    if (page.isLast || !page.values?.length) return values;
    startAt += page.values.length;
  }
}

export const listScrumBoards = (policy) =>
  pagedAgile((startAt) => route`/rest/agile/1.0/board?type=scrum&startAt=${startAt}&maxResults=50`, policy);

export const listActiveSprints = (boardId, policy) =>
  pagedAgile((startAt) => route`/rest/agile/1.0/board/${boardId}/sprint?state=active&startAt=${startAt}&maxResults=50`, policy);

export async function boardEstimateField(boardId, policy) {
  const conf = await jiraJson('app', route`/rest/agile/1.0/board/${boardId}/configuration`, undefined, policy);
  return conf.estimation?.field?.fieldId ?? null;
}

export const isStarted = (sprint) => sprint.state !== 'future' && Boolean(sprint.startDate);

function sprintEntry(sprint, boardId, estimateFieldId) {
  return {
    id: String(sprint.id),
    name: sprint.name,
    startDate: sprint.startDate,
    startMs: Date.parse(sprint.startDate),
    boardId: String(boardId),
    estimateFieldId,
  };
}

// The full picture the scheduled run starts from: the sprint field and every active sprint of every
// scrum board, each with the estimation field of the board that owns it (originBoardId).
export async function discoverConfig(policy) {
  const sprintFieldId = await discoverSprintField(policy);
  const boards = await listScrumBoards(policy);
  const found = new Map();
  for (const board of boards) {
    for (const sprint of await listActiveSprints(board.id, policy)) {
      if (!found.has(String(sprint.id))) found.set(String(sprint.id), { sprint, boardId: sprint.originBoardId ?? board.id });
    }
  }
  const estimateFields = new Map();
  const sprints = {};
  for (const [id, { sprint, boardId }] of [...found].sort(([a], [b]) => Number(a) - Number(b))) {
    if (!isStarted(sprint)) continue;
    if (!estimateFields.has(String(boardId))) estimateFields.set(String(boardId), await boardEstimateField(boardId, policy));
    sprints[id] = sprintEntry(sprint, boardId, estimateFields.get(String(boardId)));
  }
  return { sprintFieldId, sprints };
}

// An event can name a sprint that started after the last scheduled run. Returns the sprint's entry
// when it is active (and adds it to cfg), or null for future/closed sprints.
// A sprint found not active is remembered in cfg.inactive (saved with the config, so later events read it from KVS,
// not from Jira); the hourly run rebuilds the config from Jira, which forgets it again should it have started since.
export async function resolveSprint(cfg, sprintId, policy) {
  if (cfg.sprints[sprintId]) return cfg.sprints[sprintId];
  if (cfg.inactive?.includes(sprintId)) return null;
  const res = await jiraJson('app', route`/rest/agile/1.0/sprint/${sprintId}`, undefined, policy).catch((e) => {
    if (e.status === 404) return null;
    throw e;
  });
  if (!res || res.state !== 'active' || !isStarted(res)) {
    cfg.inactive = [...(cfg.inactive ?? []), sprintId];
    return null;
  }
  const boardId = res.originBoardId;
  const known = Object.values(cfg.sprints).find((s) => s.boardId === String(boardId));
  const estimateFieldId = known ? known.estimateFieldId : await boardEstimateField(boardId, policy);
  cfg.sprints[sprintId] = sprintEntry(res, boardId, estimateFieldId);
  return cfg.sprints[sprintId];
}
