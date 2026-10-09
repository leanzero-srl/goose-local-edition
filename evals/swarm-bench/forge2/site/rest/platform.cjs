'use strict';
// Jira platform REST v3 handlers. Each returns {status, body, headers?}. Error bodies and messages are
// the ones measured on a Jira Cloud site on 2026-10-02 unless noted.
const crypto = require('crypto');
const { compile, JqlError, NotModelledError } = require('../jql.cjs');
const { servedPageSize } = require('../limits.cjs');

const err = (status, messages, errors = {}) => ({ status, body: { errorMessages: [].concat(messages), errors } });
const NOT_FOUND_ISSUE = 'Issue does not exist or you do not have permission to see it.';

// Opaque page tokens bound to the query that produced them.
const token = (offset, bind) => Buffer.from(JSON.stringify({ o: offset, b: bind })).toString('base64url');
const readToken = (t, bind) => {
  if (t === undefined || t === null || t === '') return 0;
  try {
    const v = JSON.parse(Buffer.from(String(t), 'base64url').toString());
    if (v.b !== bind || !Number.isInteger(v.o)) return null;
    return v.o;
  } catch { return null; }
};
const hash = (x) => crypto.createHash('sha256').update(JSON.stringify(x)).digest('hex').slice(0, 16);
const listParam = (q, name) => q.getAll(name).flatMap((v) => v.split(',')).map((v) => v.trim()).filter(Boolean);
const intParam = (v, dflt) => (v === undefined || v === null || v === '' ? dflt : Number.parseInt(v, 10));

// The JQL model reads the LIVE site (sprints close, fields join), never the pack.
function jqlModel(c) {
  const { pack } = c.state;
  return { fields: c.state.fields(), sprints: c.state.sprints(), statuses: pack.statuses, issueTypes: pack.issueTypes, projects: pack.projects,
    sprintFieldId: pack.sprintFieldId, now: c.state.now, currentUser: c.caller.accountId };
}

// Hits are filtered and sorted once per (caller, query, state version); every page of a walk slices the same list.
function runSearch(c, jql) {
  if (jql === undefined || jql === null) return { error: err(400, 'Unbounded JQL queries are not allowed here. Please add a search restriction to your query.') };
  let q;
  try { q = compile(String(jql), jqlModel(c)); } catch (e) {
    if (e instanceof NotModelledError) throw e;
    if (e instanceof JqlError) return { error: err(400, e.message) };
    throw e;
  }
  if (!q.bounded) return { error: err(400, 'Unbounded JQL queries are not allowed here. Please add a search restriction to your query.') };
  const hits = c.state.cached(`search\u0000${c.caller.accountId}\u0000${jql}`, () => c.state.allIssues().filter((i) => c.canBrowse(i) && q.matches(i)).sort(q.compare));
  return { hits };
}

const handlers = {
  'GET /rest/api/3/field': (c) => ({ status: 200, body: c.state.fields() }),

  'GET /rest/api/3/field/search': (c) => {
    const q = c.req.query;
    let list = c.state.fields().filter((f) => f.custom || q.get('type') !== 'custom');
    if (q.get('type') === 'system') list = list.filter((f) => !f.custom);
    const ids = listParam(q, 'id');
    if (ids.length) list = list.filter((f) => ids.includes(f.id));
    const text = q.get('query');
    if (text) list = list.filter((f) => f.name.toLowerCase().includes(text.toLowerCase()) || f.id.toLowerCase().includes(text.toLowerCase()));
    const startAt = intParam(q.get('startAt'), 0);
    const maxResults = Math.min(intParam(q.get('maxResults'), c.limits.fieldSearchPage.value), c.limits.fieldSearchPage.value);
    const values = list.slice(startAt, startAt + maxResults).map((f) => ({ id: f.id, name: f.name, schema: f.schema, description: '', key: f.key, isLocked: false, searcherKey: '' }));
    return { status: 200, body: { maxResults, startAt, total: list.length, isLast: startAt + values.length >= list.length, values } };
  },

  'GET /rest/api/3/myself': (c) => {
    const me = c.render.user(c.caller.accountId);
    if (!listParam(c.req.query, 'expand').includes('groups')) return { status: 200, body: me };
    const items = groupsOf(c, c.caller.accountId);
    return { status: 200, body: { ...me, groups: { size: items.length, items } } };
  },

  'GET /rest/api/3/user/groups': (c) => {
    const id = c.req.query.get('accountId');
    if (!c.state.userById.has(id) && id !== c.state.pack.appAccountId) return err(404, `Specified user does not exist or you do not have required permissions`);
    return { status: 200, body: groupsOf(c, id) };
  },

  'GET /rest/api/3/user': (c) => {
    const u = c.render.user(c.req.query.get('accountId'));
    return u ? { status: 200, body: u } : err(404, `Specified user does not exist or you do not have required permissions`);
  },

  'GET /rest/api/3/user/bulk': (c) => {
    const ids = listParam(c.req.query, 'accountId');
    const startAt = intParam(c.req.query.get('startAt'), 0);
    const maxResults = intParam(c.req.query.get('maxResults'), 10);
    const users = ids.map((id) => c.render.user(id)).filter(Boolean);
    const values = users.slice(startAt, startAt + maxResults);
    return { status: 200, body: { maxResults, startAt, total: users.length, isLast: startAt + values.length >= users.length, values } };
  },

  'GET /rest/api/3/project/{projectIdOrKey}': (c) => {
    const k = c.params.projectIdOrKey;
    const p = c.state.pack.projects.find((x) => x.key === k.toUpperCase() || x.id === k);
    if (!p || !c.canBrowseProject(p.key)) return err(404, `No project could be found with key '${k}'.`);
    return { status: 200, body: { self: `${c.state.pack.siteUrl}/rest/api/3/project/${p.id}`, id: p.id, key: p.key, name: p.name, projectTypeKey: 'software', simplified: false, style: 'classic', isPrivate: false } };
  },

  'GET /rest/api/3/project/search': (c) => {
    const values = c.state.pack.projects.filter((p) => c.canBrowseProject(p.key)).map((p) => ({ self: `${c.state.pack.siteUrl}/rest/api/3/project/${p.id}`, id: p.id, key: p.key, name: p.name, projectTypeKey: 'software', simplified: false, style: 'classic', isPrivate: false }));
    return { status: 200, body: { self: `${c.state.pack.siteUrl}/rest/api/3/project/search`, maxResults: 50, startAt: 0, total: values.length, isLast: true, values } };
  },

  'GET /rest/api/3/issue/{issueIdOrKey}': (c) => {
    const iss = c.state.issueByIdOrKey(c.params.issueIdOrKey);
    if (!iss || !c.canBrowse(iss)) return err(404, NOT_FOUND_ISSUE);
    const sel = c.render.selectFields(listParam(c.req.query, 'fields'), true);
    sel.add('key');
    return { status: 200, body: c.render.issue(iss, sel, { expand: listParam(c.req.query, 'expand') }) };
  },

  'GET /rest/api/3/issue/{issueIdOrKey}/changelog': (c) => {
    const iss = c.state.issueByIdOrKey(c.params.issueIdOrKey);
    if (!iss || !c.canBrowse(iss)) return err(404, NOT_FOUND_ISSUE);
    const all = c.state.st.histories.get(iss.id);
    const cap = c.limits.issueChangelogPage.value;
    const startAt = intParam(c.req.query.get('startAt'), 0);
    const maxResults = servedPageSize(c.paging, Math.min(intParam(c.req.query.get('maxResults'), cap), cap), all.length);
    const values = all.slice(startAt, startAt + maxResults).map((h) => c.render.history(h));
    const base = `${c.state.pack.siteUrl}/rest/api/3/issue/${iss.key}/changelog`;
    const isLast = startAt + values.length >= all.length;
    return { status: 200, body: { self: `${base}?maxResults=${maxResults}&startAt=${startAt}`, ...(isLast ? {} : { nextPage: `${base}?maxResults=${maxResults}&startAt=${startAt + maxResults}` }), maxResults, startAt, total: all.length, isLast, values } };
  },

  'POST /rest/api/3/issue/{issueIdOrKey}/changelog/list': (c) => {
    const iss = c.state.issueByIdOrKey(c.params.issueIdOrKey);
    if (!iss || !c.canBrowse(iss)) return err(404, NOT_FOUND_ISSUE);
    const ids = c.req.body?.changelogIds;
    if (!Array.isArray(ids)) return err(400, 'changelogIds must not be null.');
    const wanted = new Set(ids.map(String));
    const histories = c.state.st.histories.get(iss.id).filter((h) => wanted.has(String(h.changelogId))).map((h) => c.render.history(h));
    return { status: 200, body: { startAt: 0, maxResults: histories.length, total: histories.length, histories } };
  },

  'POST /rest/api/3/issue/bulkfetch': (c) => {
    const b = c.req.body ?? {};
    const ids = Array.isArray(b.issueIdsOrKeys) ? b.issueIdsOrKeys : null;
    if (!ids || !ids.length) return err(400, 'issueIdsOrKeys must not be empty.');
    const fields = Array.isArray(b.fields) ? b.fields : b.fields ? String(b.fields).split(',') : [];
    const expand = Array.isArray(b.expand) ? b.expand : b.expand ? String(b.expand).split(',') : [];
    const heavy = expand.some((e) => ['changelog', 'editmeta', 'operations', 'renderedFields', 'transitions', 'versionedRepresentations'].includes(e));
    const named = fields.some((f) => !f.startsWith('-') && !f.startsWith('*'));
    const cap = named && !heavy ? c.limits.issueBulkNamedFields.value : c.limits.issueBulkDefault.value;
    if (ids.length > cap) return err(400, `The request can contain at most ${cap} issues.`);
    const sel = c.render.selectFields(fields, true);
    sel.add('key');
    const issues = [];
    for (const k of ids) {
      const iss = c.state.issueByIdOrKey(k);
      if (iss && c.canBrowse(iss)) issues.push(c.render.issue(iss, sel, { expand, changelogCap: 40 }));
    }
    return { status: 200, body: { expand: 'names,schema', issues, issueErrors: [] } };
  },

  'POST /rest/api/3/changelog/bulkfetch': (c) => {
    const b = c.req.body ?? {};
    const ids = Array.isArray(b.issueIdsOrKeys) ? b.issueIdsOrKeys : null;
    if (!ids || !ids.length) return err(400, 'issueIdsOrKeys must not be empty.');
    if (ids.length > c.limits.changelogBulkIssues.value) return err(400, `issueIdsOrKeys: size must be between 1 and ${c.limits.changelogBulkIssues.value}`);
    const fieldIds = Array.isArray(b.fieldIds) ? b.fieldIds : [];
    if (fieldIds.length > c.limits.changelogBulkFields.value) return err(400, `fieldIds: size must be between 0 and ${c.limits.changelogBulkFields.value}`);
    const maxResults = intParam(b.maxResults, c.limits.changelogBulkPageDefault.value);
    if (!(maxResults >= 1 && maxResults <= c.limits.changelogBulkPageMax.value)) return err(400, `maxResults: must be between 1 and ${c.limits.changelogBulkPageMax.value}`);
    const bind = hash({ ids, fieldIds });
    const offset = readToken(b.nextPageToken, bind);
    if (offset === null) return err(400, 'The provided next page token is invalid or expired.');
    const rows = [];
    for (const k of ids) {
      const iss = c.state.issueByIdOrKey(k);
      if (!iss || !c.canBrowse(iss)) continue;
      for (const h of c.state.st.histories.get(iss.id)) {
        const items = fieldIds.length ? h.items.filter((it) => fieldIds.includes(it.fieldId)) : h.items;
        if (items.length) rows.push({ issueId: iss.id, h: { ...h, items } });
      }
    }
    rows.sort((x, y) => Date.parse(x.h.created) - Date.parse(y.h.created) || Number(x.issueId) - Number(y.issueId));
    const page = rows.slice(offset, offset + servedPageSize(c.paging, maxResults, rows.length));
    const grouped = [];
    for (const r of page) {
      let g = grouped.find((x) => x.issueId === r.issueId);
      if (!g) grouped.push((g = { issueId: r.issueId, changeHistories: [] }));
      g.changeHistories.push(c.render.history(r.h));
    }
    const more = offset + page.length < rows.length;
    return { status: 200, body: { issueChangeLogs: grouped, ...(more ? { nextPageToken: token(offset + page.length, bind) } : {}) } };
  },

  'GET /rest/api/3/search': () => err(410, 'The requested API has been removed. Please migrate to the /rest/api/3/search/jql API. A full migration guideline is available at https://developer.atlassian.com/changelog/#CHANGE-2046'),
  'POST /rest/api/3/search': () => err(410, 'The requested API has been removed. Please migrate to the /rest/api/3/search/jql API. A full migration guideline is available at https://developer.atlassian.com/changelog/#CHANGE-2046'),

  'GET /rest/api/3/search/jql': (c) => searchJql(c, {
    jql: c.req.query.get('jql'), nextPageToken: c.req.query.get('nextPageToken'), maxResults: c.req.query.get('maxResults'),
    fields: listParam(c.req.query, 'fields'), expand: listParam(c.req.query, 'expand'),
  }),
  'POST /rest/api/3/search/jql': (c) => {
    const b = c.req.body ?? {};
    return searchJql(c, { jql: b.jql, nextPageToken: b.nextPageToken, maxResults: b.maxResults,
      fields: Array.isArray(b.fields) ? b.fields : b.fields ? [b.fields] : [], expand: b.expand ? String(b.expand).split(',') : [] });
  },

  'POST /rest/api/3/search/approximate-count': (c) => {
    const r = runSearch(c, c.req.body?.jql);
    if (r.error) return r.error;
    return { status: 200, body: { count: r.hits.length } };
  },

  'GET /rest/api/3/issue/{issueIdOrKey}/comment': (c) => {
    const iss = c.state.issueByIdOrKey(c.params.issueIdOrKey);
    if (!iss || !c.canBrowse(iss)) return err(404, NOT_FOUND_ISSUE);
    const all = c.state.st.comments.filter((x) => x.issueId === iss.id);
    const cap = c.limits.commentPage.value;
    const startAt = intParam(c.req.query.get('startAt'), 0);
    const maxResults = Math.min(intParam(c.req.query.get('maxResults'), cap), cap);
    const comments = all.slice(startAt, startAt + maxResults).map((x) => commentBody(c, x));
    return { status: 200, body: { startAt, maxResults, total: all.length, comments } };
  },

  'POST /rest/api/3/issue/{issueIdOrKey}/comment': (c) => {
    const iss = c.state.issueByIdOrKey(c.params.issueIdOrKey);
    if (!iss || !c.canBrowse(iss)) return err(404, NOT_FOUND_ISSUE);
    const body = c.req.body?.body;
    const adf = body && typeof body === 'object' && !Array.isArray(body) && body.type === 'doc' && body.version === 1 && Array.isArray(body.content);
    // Measured on Jira Cloud 2026-10-03 (a project whose scheme grants BROWSE_PROJECTS but not ADD_COMMENTS;
    // mypermissions said ADD_COMMENTS havePermission:false): an ADF comment -> 400 (NOT 403)
    // {"errorMessages":["<display name>, you do not have the permission to comment on this issue."],"errors":{}};
    // a plain-string body there -> the same message plus errors.comment. With permission, a plain-string body ->
    // 400 {"errorMessages":[],"errors":{"comment":"Comment body is not valid!"}} (measured 2026-10-02).
    const denied = !c.canComment(iss);
    if (denied || !adf) {
      const who = c.state.userById.get(c.caller.accountId)?.displayName ?? c.caller.accountId;
      return { status: 400, body: { errorMessages: denied ? [`${who}, you do not have the permission to comment on this issue.`] : [],
        errors: adf ? {} : { comment: 'Comment body is not valid!' } } };
    }
    const t = c.state.now();
    const comment = { id: String(10000 + ++c.state.st.commentSeq), issueId: iss.id, issueKey: iss.key, authorId: c.caller.accountId, body, created: new Date(t).toISOString(),
      as: c.caller.as, invocationId: c.caller.invocationId ?? null, moduleKey: c.caller.moduleKey ?? null };
    c.state.st.comments.push(comment);
    return { status: 201, body: commentBody(c, comment) };
  },

  'POST /rest/api/3/permissions/check': (c) => {
    const b = c.req.body ?? {};
    const who = b.accountId ?? c.caller.accountId;
    const out = [];
    for (const k of b.globalPermissions ?? []) if (!GLOBAL[k]) throw new NotModelledError(`global permission key ${k} in /permissions/check`);
    for (const pp of b.projectPermissions ?? []) {
      for (const perm of pp.permissions ?? []) {
        if (!PERMS[perm]) throw new NotModelledError(`permission key ${perm} in /permissions/check`);
        const issues = (pp.issues ?? []).filter((id) => {
          const iss = c.state.issueByIdOrKey(id);
          return iss && PERMS[perm](c.state, who, iss);
        });
        const projects = (pp.projects ?? []).filter((id) => c.state.pack.projects.some((p) => p.id === String(id) || p.key === String(id)));
        out.push({ permission: perm, issues, projects });
      }
    }
    return { status: 200, body: { projectPermissions: out, globalPermissions: (b.globalPermissions ?? []).filter((k) => GLOBAL[k](c.state, who)) } };
  },

  'GET /rest/api/3/mypermissions': (c) => {
    const q = c.req.query;
    const keys = listParam(q, 'permissions');
    if (!keys.length) return err(400, 'The permissions query parameter is required.');
    const issueRef = q.get('issueKey') ?? q.get('issueId');
    const iss = issueRef ? c.state.issueByIdOrKey(issueRef) : null;
    if (issueRef && !iss) return err(404, NOT_FOUND_ISSUE);
    const projectRef = q.get('projectKey') ?? q.get('projectId');
    const project = projectRef ? c.state.pack.projects.find((p) => p.key === projectRef.toUpperCase() || p.id === projectRef) : null;
    if (projectRef && !project) return err(404, `No project could be found with key '${projectRef}'.`);
    const who = c.caller.accountId;
    const permissions = {};
    for (const k of keys) {
      if (!PERMS[k] && !GLOBAL[k]) throw new NotModelledError(`permission key ${k} in /mypermissions`);
      // A project permission without an issue: in that project, or (global context) in any project.
      const have = GLOBAL[k] ? GLOBAL[k](c.state, who)
        : iss ? PERMS[k](c.state, who, iss)
          : (project ? [project] : c.state.pack.projects).some((p) => c.state.canBrowseProject(who, p.key));
      permissions[k] = { id: PERM_META[k][0], key: k, name: PERM_META[k][1], type: GLOBAL[k] ? 'GLOBAL' : 'PROJECT', description: PERM_META[k][2], havePermission: have };
    }
    return { status: 200, body: { permissions } };
  },
};

const PERMS = {
  BROWSE_PROJECTS: (s, who, iss) => s.canBrowse(who, iss),
  ADD_COMMENTS: (s, who, iss) => s.canComment(who, iss),
};
// Jira's global ADMINISTER: the site's administrators (pack.admins); the app's own account is not one (SPEC R5 checks
// the person, asUser).
const GLOBAL = {
  ADMINISTER: (s, who) => s.pack.admins.includes(who),
};
// id and description are the site's own text; apps read key and havePermission.
const PERM_META = {
  BROWSE_PROJECTS: ['10', 'Browse Projects', 'Ability to browse projects and the issues within them.'],
  ADD_COMMENTS: ['15', 'Add Comments', 'Ability to comment on issues.'],
  ADMINISTER: ['0', 'Administer Jira', 'Ability to administer Jira.'],
};

// The groups a person belongs to (pack.groups), in Jira's GroupName shape.
const groupsOf = (c, accountId) => c.state.pack.groups.filter((g) => g.members.includes(accountId))
  .map((g) => ({ name: g.name, groupId: g.groupId, self: `${c.state.pack.siteUrl}/rest/api/3/group?groupId=${g.groupId}` }));

function commentBody(c, x) {
  const self = `${c.state.pack.siteUrl}/rest/api/3/issue/${x.issueId}/comment/${x.id}`;
  const author = c.render.user(x.authorId);
  return { self, id: x.id, author, body: x.body, updateAuthor: author, created: c.render.jiraDate(x.created), updated: c.render.jiraDate(x.created), jsdPublic: true };
}

function searchJql(c, { jql, nextPageToken, maxResults, fields, expand }) {
  const r = runSearch(c, jql);
  if (r.error) return r.error;
  const sel = c.render.selectFields(fields, false);
  const idsOnly = [...sel].every((f) => f === 'id' || f === 'key');
  const cap = idsOnly ? c.limits.searchJqlIdsOnlyMax.value : c.limits.searchJqlFieldsMax.value;
  const want = intParam(maxResults, c.limits.searchJqlDefault.value);
  if (!(want >= 0)) return err(400, `maxResults: must be a non-negative number`);
  const size = servedPageSize(c.paging, Math.min(want, cap), r.hits.length);
  const bind = hash(String(jql));
  const offset = readToken(nextPageToken, bind);
  if (offset === null) return err(400, 'The provided next page token is invalid or expired.');
  const page = r.hits.slice(offset, offset + size);
  const more = offset + page.length < r.hits.length;
  return { status: 200, body: { issues: page.map((i) => c.render.issue(i, sel, { expand })), ...(more ? { nextPageToken: token(offset + page.length, bind) } : {}), isLast: !more } };
}

module.exports = { handlers, err, runSearch, listParam, intParam, NOT_FOUND_ISSUE };
