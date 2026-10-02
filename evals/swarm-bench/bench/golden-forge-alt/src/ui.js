import Resolver from '@forge/resolver';
import { sprintReport, boardReport, listScrumBoards } from './report';

const resolver = new Resolver();

resolver.define('boards', () => listScrumBoards());

resolver.define('board-scope', ({ payload }) => {
  if (!payload || payload.boardId == null) return { error: 'No board configured.' };
  return boardReport(String(payload.boardId));
});

resolver.define('sprint-scope', ({ payload, context }) => {
  const ext = (context && context.extension) || {};
  const sprintId = (payload && payload.sprintId) || (ext.sprint && ext.sprint.id);
  if (sprintId == null) return { error: 'No sprint in this context.' };
  return sprintReport(String(sprintId), context.accountId);
});

export const handler = resolver.getDefinitions();
