import Resolver from '@forge/resolver';
import { sprintReport, boardReport, listScrumBoards } from './report';
import { explainSprint } from './explain';

const base = new Resolver();
// Contract §2: "Every resolver returns a value and never throws: a failure (a Jira error, storage, Forge LLM) returns a
// value describing it". A Jira 500 on a read came back as a rejected invoke before (2026-10-03 scoring site).
const resolver = {
  define: (key, fn) => base.define(key, async (req) => {
    try {
      return await fn(req);
    } catch (err) {
      console.error(`resolver ${key} failed: ${err?.message ?? err}`);
      const message = `Could not read the sprint scope: ${err?.message ?? err}`;
      return key === 'explain' ? { error: 'error', message } : { error: message };
    }
  }),
  getDefinitions: () => base.getDefinitions(),
};

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
