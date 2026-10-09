import { kvs } from '@forge/kvs';
import { jiraJson, route } from './jira';

const CONFIG_KEY = 'config';
const SPRINT_FIELD_TYPE = 'com.pyxis.greenhopper.jira:gh-sprint';
export const SCOPE_FIELD_MODULE = 'scope-status';

// config = { sprintFieldId, scopeFieldId, sprints: { [sprintId]: { id, name, startDate, startMs, boardId, estimateFieldId } } }
// holding the ACTIVE sprints only. Written by the scheduled run, extended by the consumer when an event
// names an active sprint it has not seen yet, and corrected by the consumer when it reads a sprint or a
// board fresh (a sprint that closed, a board whose estimation field changed).
export const loadConfig = () => kvs.get(CONFIG_KEY);

export async function saveConfig(next, previous) {
  if (previous && JSON.stringify(previous) === JSON.stringify(next)) return false;
  await kvs.set(CONFIG_KEY, next);
  return true;
}

// The Sprint field, and this app's own read-only `scope-status` field. Jira lists a Forge custom field with
// schema.custom = the extension ARI (…/static/<module key>) and a key ending in __<module key>.
export async function discoverFields(work) {
  const fields = await jiraJson('app', route`/rest/api/3/field`, undefined, work);
  const sprint = fields.find((f) => f.schema?.custom === SPRINT_FIELD_TYPE);
  if (!sprint) throw new Error(`no field with schema.custom ${SPRINT_FIELD_TYPE} on this site`);
  const mine = (f) => String(f.schema?.custom ?? '').endsWith(`/${SCOPE_FIELD_MODULE}`) || String(f.key ?? '').endsWith(`__${SCOPE_FIELD_MODULE}`);
  const scope = fields.find(mine);
  if (!scope) console.error(`config: Jira lists no ${SCOPE_FIELD_MODULE} field of this app; scope statuses are not written until it does`);
  return { sprintFieldId: sprint.id, scopeFieldId: scope?.id ?? null };
}

async function pagedAgile(build, work) {
  const values = [];
  for (let startAt = 0; ; ) {
    const page = await jiraJson('app', build(startAt), undefined, work);
    values.push(...(page.values ?? []));
    if (page.isLast || !page.values?.length) return values;
    startAt += page.values.length;
  }
}

const readScrumBoards = (work) => pagedAgile((startAt) => route`/rest/agile/1.0/board?type=scrum&startAt=${startAt}&maxResults=50`, work);

// One background invocation reads the board list once (the migration plan and the discovery both need it).
export const listScrumBoards = (work) => (work.background ? (work.scrumBoards ??= readScrumBoards(work)) : readScrumBoards(work));

export const listSprints = (boardId, state, work) =>
  pagedAgile((startAt) => route`/rest/agile/1.0/board/${boardId}/sprint?state=${state}&startAt=${startAt}&maxResults=50`, work);

export const listActiveSprints = (boardId, work) => listSprints(boardId, 'active', work);

export async function boardEstimateField(boardId, work) {
  const conf = await jiraJson('app', route`/rest/agile/1.0/board/${boardId}/configuration`, undefined, work);
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

// The full picture the scheduled run starts from: the fields and every active sprint of every scrum
// board, each with the estimation field of the board that owns it (originBoardId).
export async function discoverConfig(work) {
  const { sprintFieldId, scopeFieldId } = await discoverFields(work);
  const boards = await listScrumBoards(work);
  const found = new Map();
  for (const board of boards) {
    for (const sprint of await listActiveSprints(board.id, work)) {
      if (!found.has(String(sprint.id))) found.set(String(sprint.id), { sprint, boardId: sprint.originBoardId ?? board.id });
    }
  }
  const estimateFields = new Map();
  const sprints = {};
  for (const [id, { sprint, boardId }] of [...found].sort(([a], [b]) => Number(a) - Number(b))) {
    if (!isStarted(sprint)) continue;
    if (!estimateFields.has(String(boardId))) estimateFields.set(String(boardId), await boardEstimateField(boardId, work));
    sprints[id] = sprintEntry(sprint, boardId, estimateFields.get(String(boardId)));
  }
  return { sprintFieldId, scopeFieldId, sprints };
}

// The sprints one event touches, as they are now: a sprint the issue's own Sprint field carries (state,
// dates and board come with the issue) or, for one it left, a fresh read of the sprint. Returns the sprint
// with `completeMs` (null while active), or null for a sprint that never started or does not exist. cfg
// follows what was read: closed sprints leave cfg.sprints (and are listed in cfg.closed until the next
// scheduled run settles them), active ones enter it. readBoard(boardId) reads a board's estimation field
// fresh, once per invocation, so a change recorded after the board switched fields uses the new one.
export function sprintReader(cfg, work) {
  const sprints = new Map();
  const known = new Map();
  const fresh = new Map();
  const readBoard = async (boardId) => {
    if (!fresh.has(boardId)) {
      const field = await boardEstimateField(boardId, work);
      fresh.set(boardId, field);
      for (const s of [...Object.values(cfg.sprints), ...sprints.values()]) if (s && s.boardId === boardId) s.estimateFieldId = field;
    }
    return fresh.get(boardId);
  };
  const boardField = async (boardId) => {
    if (fresh.has(boardId)) return fresh.get(boardId);
    const cached = Object.values(cfg.sprints).find((s) => s.boardId === boardId);
    return cached ? cached.estimateFieldId : readBoard(boardId);
  };
  const resolve = async (sprintId) => {
    if (sprints.has(sprintId)) return sprints.get(sprintId);
    const res =
      known.get(sprintId) ??
      (await jiraJson('app', route`/rest/agile/1.0/sprint/${sprintId}`, undefined, work).catch((e) => {
        if (e.status === 404) return null;
        throw e;
      }));
    let entry = null;
    if (res && isStarted(res)) {
      const boardId = String(res.originBoardId ?? res.boardId);
      entry = { ...sprintEntry(res, boardId, await boardField(boardId)), completeMs: res.state === 'closed' ? Date.parse(res.completeDate ?? res.endDate) : null };
      if (res.state === 'active') {
        const { completeMs, ...active } = entry;
        cfg.sprints[sprintId] = active;
      }
    }
    if ((!entry || entry.completeMs !== null) && cfg.sprints[sprintId]) {
      // Remembered until the next scheduled run, which recomputes the closed sprint's issues' statuses.
      delete cfg.sprints[sprintId];
      cfg.closed = [...new Set([...(cfg.closed ?? []), sprintId])];
    }
    sprints.set(sprintId, entry);
    return entry;
  };
  resolve.readBoard = readBoard;
  resolve.learn = (fieldValue) => {
    for (const s of Array.isArray(fieldValue) ? fieldValue : []) if (s !== null && typeof s === 'object' && s.id !== undefined) known.set(String(s.id), s);
  };
  return resolve;
}
