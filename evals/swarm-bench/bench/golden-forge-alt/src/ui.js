import Resolver from '@forge/resolver';
import { sprintReport, boardReport, listScrumBoards } from './report';
import { explainSprint } from './explain';

const resolver = new Resolver();

resolver.define('boards', () => listScrumBoards());

resolver.define('board-scope', ({ payload }) => {
  if (!payload || payload.boardId == null) return { error: 'No board configured.' };
  return boardReport(String(payload.boardId));
});

const sprintOf = (payload, context) => {
  const ext = (context && context.extension) || {};
  const id = (payload && payload.sprintId) || (ext.sprint && ext.sprint.id);
  return id == null ? null : String(id);
};

resolver.define('sprint-scope', ({ payload, context }) => {
  const sprintId = sprintOf(payload, context);
  if (sprintId == null) return { error: 'No sprint in this context.' };
  return sprintReport(sprintId, context.accountId);
});

resolver.define('explain', ({ payload, context }) => {
  const sprintId = sprintOf(payload, context);
  if (sprintId == null) return { error: 'error', message: 'No sprint in this context.' };
  return explainSprint(sprintId, context.accountId);
});

export const handler = resolver.getDefinitions();
