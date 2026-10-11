import { kvs } from '@forge/kvs';
import { jiraJson, route } from './jira';
export const loadConfig = () => kvs.get('config-v2');
export const isStarted = s => s.state !== 'future' && Boolean(s.startDate);
export const saveConfig = cfg => kvs.set('config-v2', cfg);
async function list(path, policy) {
  const all = [];
  for (let offset = 0; ;) {
    const page = await jiraJson('app', path(offset), undefined, policy);
    all.push(...page.values);
    if (page.isLast || !page.values.length) return all;
    offset += page.values.length;
  }
}
export const listScrumBoards = policy => list(n => route`/rest/agile/1.0/board?type=scrum&startAt=${n}&maxResults=50`, policy);
export const listActiveSprints = (id, policy) => list(n => route`/rest/agile/1.0/board/${id}/sprint?state=active&startAt=${n}&maxResults=50`, policy);
export async function boardEstimateField(id, policy) {
  const configuration = await jiraJson('app', route`/rest/agile/1.0/board/${id}/configuration`, undefined, policy);
  return configuration.estimation?.field?.fieldId ?? null;
}
export async function refreshBoard(cfg, boardId, policy) {
  const field = await boardEstimateField(boardId, policy);
  cfg.estimateFields = [...new Set([...(cfg.estimateFields ?? []), field].filter(Boolean))];
  for (const sprint of Object.values(cfg.sprints)) {
    if (sprint.boardId !== String(boardId) || sprint.state !== 'active') continue;
    sprint.initialEstimateFieldId ??= sprint.estimateFieldId;
    sprint.estimateFields = [...new Set([...(sprint.estimateFields ?? []), sprint.estimateFieldId, field].filter(Boolean))];
    sprint.estimateFieldId = field;
  }
  return field;
}
export function markClosed(sprint, current) {
  if (sprint.state === current.state) return;
  sprint.state = current.state;
  if (current.state === 'closed') {
    sprint.closeMs = Date.parse(current.completeDate ?? current.endDate);
    if (!Number.isFinite(sprint.closeMs)) throw new Error(`Sprint ${sprint.id} has no valid close time.`);
    sprint.completeDate = current.completeDate;
    sprint.closureReconciled = false;
  }
}
export async function discoverConfig(policy) {
  const cfg = await loadConfig() ?? { sprints: {}, boards: [], estimateFields: [] };
  const fields = await jiraJson('app', route`/rest/api/3/field`, undefined, policy);
  const sprintField = fields.find(f => f.schema?.custom === 'com.pyxis.greenhopper.jira:gh-sprint');
  if (!sprintField) throw new Error('Sprint field was not found.');
  cfg.sprintFieldId = sprintField.id;
  cfg.statusFieldId = fields.find(f => f.name === 'Scope status')?.id;
  cfg.boards = (await listScrumBoards(policy)).map(board => ({ id: String(board.id), name: board.name }));
  const active = new Set(), byBoard = new Map();
  for (const board of cfg.boards) {
    const sprints = await listActiveSprints(board.id, policy);
    byBoard.set(board.id, sprints);
    for (const sprint of sprints) active.add(String(sprint.id));
  }
  for (const sprint of Object.values(cfg.sprints)) {
    if (sprint.state === 'active' && !active.has(sprint.id)) {
      markClosed(sprint, await jiraJson('app', route`/rest/agile/1.0/sprint/${sprint.id}`, undefined, policy));
    }
  }
  for (const board of cfg.boards) {
    const field = await refreshBoard(cfg, board.id, policy);
    for (const sprint of byBoard.get(board.id)) {
      const id = String(sprint.id), old = cfg.sprints[id];
      cfg.sprints[id] = {
        ...old, id, name: sprint.name, state: sprint.state, startDate: sprint.startDate,
        startMs: Date.parse(sprint.startDate), boardId: String(sprint.originBoardId ?? board.id),
        estimateFieldId: field, initialEstimateFieldId: old?.initialEstimateFieldId ?? old?.estimateFieldId ?? field,
        estimateFields: [...new Set([...(old?.estimateFields ?? []), field].filter(Boolean))],
      };
    }
  }
  await saveConfig(cfg);
  return cfg;
}
export async function resolveSprint(cfg, id, policy) {
  if (cfg.sprints[id]) return cfg.sprints[id];
  let sprint;
  try { sprint = await jiraJson('app', route`/rest/agile/1.0/sprint/${id}`, undefined, policy); }
  catch (error) { if (error.status === 404) return null; throw error; }
  if (sprint.state !== 'active') return null;
  const boardId = String(sprint.originBoardId), field = await boardEstimateField(boardId, policy);
  const entry = {
    id: String(id), name: sprint.name, state: sprint.state, startDate: sprint.startDate,
    startMs: Date.parse(sprint.startDate), boardId, estimateFieldId: field,
    initialEstimateFieldId: field, estimateFields: field ? [field] : [],
  };
  cfg.estimateFields = [...new Set([...(cfg.estimateFields ?? []), field].filter(Boolean))];
  cfg.sprints[id] = entry;
  return entry;
}
