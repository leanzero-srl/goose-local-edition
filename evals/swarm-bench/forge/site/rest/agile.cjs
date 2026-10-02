'use strict';
// Jira Software (Agile) REST 1.0 handlers. Shapes and errors measured on a Jira Cloud site 2026-10-02:
// kanban /sprint -> 400 "The board does not support sprints"; unknown sprint -> 404 "We could not find the sprint".
const { compile, JqlError, NotModelledError } = require('../jql.cjs');
const { err, listParam, intParam } = require('./platform.cjs');

const board = (c, id) => c.state.pack.boards.find((b) => String(b.id) === String(id));
const boardBody = (c, b) => {
  const p = c.state.pack.projects.find((x) => x.key === b.projectKey);
  return { id: b.id, self: `${c.state.pack.siteUrl}/rest/agile/1.0/board/${b.id}`, name: b.name, type: b.type,
    location: { projectId: Number(p.id), displayName: `${p.name} (${p.key})`, projectName: p.name, projectKey: p.key, projectTypeKey: 'software',
      avatarURI: `${c.state.pack.siteUrl}/rest/api/2/universal_avatar/view/type/project/avatar/10412?size=small`, name: `${p.name} (${p.key})` }, isPrivate: false };
};
const sprintBody = (c, s) => ({ id: s.id, self: `${c.state.pack.siteUrl}/rest/agile/1.0/sprint/${s.id}`, state: s.state, name: s.name,
  ...(s.startDate ? { startDate: s.startDate, endDate: s.endDate } : {}), ...(s.completeDate ? { completeDate: s.completeDate } : {}),
  createdDate: s.createdDate, originBoardId: s.originBoardId, goal: s.goal ?? '' });
const page = (q, all, cap, dflt) => {
  const startAt = intParam(q.get('startAt'), 0);
  const maxResults = Math.min(intParam(q.get('maxResults'), dflt), cap);
  const values = all.slice(startAt, startAt + maxResults);
  return { maxResults, startAt, total: all.length, isLast: startAt + values.length >= all.length, values };
};

function issuesFor(c, base, jql) {
  let filter = () => true;
  if (jql) {
    try {
      const pack = c.state.pack;
      const q = compile(String(jql), { fields: pack.fields, sprints: pack.sprints, statuses: pack.statuses, issueTypes: pack.issueTypes,
        projects: pack.projects, sprintFieldId: pack.sprintFieldId, now: c.state.now, currentUser: c.caller.accountId });
      filter = q.matches;
    } catch (e) {
      if (e instanceof NotModelledError) throw e;
      if (e instanceof JqlError) return { error: err(400, e.message) };
      throw e;
    }
  }
  return { hits: c.state.allIssues().filter((i) => c.canBrowse(i) && base(i) && filter(i)).sort((a, b) => Number(a.id) - Number(b.id)) };
}

function issuePage(c, hits) {
  const q = c.req.query;
  const startAt = intParam(q.get('startAt'), 0);
  const cap = c.limits.agileIssuePage.value;
  const maxResults = Math.min(intParam(q.get('maxResults'), c.limits.agileIssueDefault.value), cap);
  const sel = c.render.selectFields(listParam(q, 'fields'), true);
  sel.add('key');
  const issues = hits.slice(startAt, startAt + maxResults).map((i) => c.render.issue(i, sel, { expand: listParam(q, 'expand'), apiBase: '/rest/agile/1.0' }));
  return { status: 200, body: { expand: 'names,schema', startAt, maxResults, total: hits.length, issues } };
}

const handlers = {
  'GET /rest/agile/1.0/board': (c) => {
    const q = c.req.query;
    let list = c.state.pack.boards.slice().sort((a, b) => a.id - b.id);
    const type = listParam(q, 'type');
    if (type.length) list = list.filter((b) => type.includes(b.type));
    const p = q.get('projectKeyOrId');
    if (p) list = list.filter((b) => { const pr = c.state.pack.projects.find((x) => x.key === b.projectKey); return pr.key === p.toUpperCase() || pr.id === p; });
    const name = q.get('name');
    if (name) list = list.filter((b) => b.name.toLowerCase().includes(name.toLowerCase()));
    return { status: 200, body: page(q, list.map((b) => boardBody(c, b)), c.limits.agileBoardPage.value, c.limits.agileBoardPage.value) };
  },

  'GET /rest/agile/1.0/board/{boardId}': (c) => {
    const b = board(c, c.params.boardId);
    return b ? { status: 200, body: boardBody(c, b) } : err(404, `Board does not exist or you do not have permission to see it.`);
  },

  'GET /rest/agile/1.0/board/{boardId}/configuration': (c) => {
    const b = board(c, c.params.boardId);
    if (!b) return err(404, `Board does not exist or you do not have permission to see it.`);
    const site = c.state.pack.siteUrl;
    const p = c.state.pack.projects.find((x) => x.key === b.projectKey);
    const body = { id: b.id, name: b.name, type: b.type, self: `${site}/rest/agile/1.0/board/${b.id}/configuration`,
      location: { type: 'project', key: p.key, id: p.id, self: `${site}/rest/api/2/project/${p.id}`, name: p.name },
      filter: { id: String(10000 + b.id), self: `${site}/rest/api/2/filter/${10000 + b.id}` },
      columnConfig: { columns: c.state.pack.statuses.map((s) => ({ name: s.name, statuses: [{ id: s.id, self: `${site}/rest/api/2/status/${s.id}` }] })), constraintType: 'none' },
      ...(b.estimationFieldId ? { estimation: { type: 'field', field: { fieldId: b.estimationFieldId, displayName: c.state.pack.fields.find((f) => f.id === b.estimationFieldId).name } } } : {}),
      ranking: { rankCustomFieldId: Number(c.state.pack.fields.find((f) => f.name === 'Rank').id.replace('customfield_', '')) } };
    return { status: 200, body };
  },

  'GET /rest/agile/1.0/board/{boardId}/sprint': (c) => {
    const b = board(c, c.params.boardId);
    if (!b) return err(404, `Board does not exist or you do not have permission to see it.`);
    if (b.type !== 'scrum') return err(400, 'The board does not support sprints');
    const states = listParam(c.req.query, 'state');
    const list = c.state.pack.sprints.filter((s) => s.originBoardId === b.id && (!states.length || states.includes(s.state))).sort((x, y) => x.id - y.id);
    return { status: 200, body: page(c.req.query, list.map((s) => sprintBody(c, s)), c.limits.agileSprintPage.value, c.limits.agileSprintPage.value) };
  },

  'GET /rest/agile/1.0/board/{boardId}/issue': (c) => {
    const b = board(c, c.params.boardId);
    if (!b) return err(404, `Board does not exist or you do not have permission to see it.`);
    const r = issuesFor(c, (i) => i.projectKey === b.projectKey, c.req.query.get('jql'));
    return r.error ?? issuePage(c, r.hits);
  },

  'GET /rest/agile/1.0/sprint/{sprintId}': (c) => {
    const s = c.state.pack.sprints.find((x) => String(x.id) === c.params.sprintId);
    return s ? { status: 200, body: sprintBody(c, s) } : err(404, 'We could not find the sprint');
  },

  'GET /rest/agile/1.0/sprint/{sprintId}/issue': (c) => {
    const s = c.state.pack.sprints.find((x) => String(x.id) === c.params.sprintId);
    if (!s) return err(404, 'We could not find the sprint');
    const fid = c.state.pack.sprintFieldId;
    const r = issuesFor(c, (i) => (i.fields[fid] ?? []).some((x) => x.id === s.id), c.req.query.get('jql'));
    return r.error ?? issuePage(c, r.hits);
  },
};

module.exports = { handlers };
