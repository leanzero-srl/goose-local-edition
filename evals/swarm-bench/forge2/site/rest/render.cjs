'use strict';
// Jira Cloud response shapes (measured on a Jira Cloud site 2026-10-02: issue, field, changelog,
// bulk changelog, Agile sprint/board). Every date in a REST body is an ISO-8601 string in Jira's
// `yyyy-MM-dd'T'HH:mm:ss.SSSZ` form, as the OpenAPI types it (`format: date-time`) and STARTER states.
// Deliberate deviation (DESIGN §17.1 0a): Jira Cloud measured 2026-10-02 returns `/changelog/bulkfetch`
// `created` as epoch milliseconds, contradicting its own OpenAPI; the site follows the documented type.

function jiraDate(isoOrMs) {
  const d = new Date(typeof isoOrMs === 'number' ? isoOrMs : Date.parse(isoOrMs));
  return d.toISOString().replace('Z', '+0000');
}

function createRenderer(state) {
  const { pack } = state;
  const site = pack.siteUrl;
  const project = (key) => pack.projects.find((p) => p.key === key);
  const user = (accountId) => {
    if (!accountId) return null;
    if (accountId === pack.appAccountId) {
      return { self: `${site}/rest/api/3/user?accountId=${encodeURIComponent(accountId)}`, accountId, accountType: 'app',
        displayName: 'Scope Ledger', active: true, timeZone: 'UTC', avatarUrls: avatars(accountId) };
    }
    const u = state.userById.get(accountId);
    if (!u) return null;
    return { self: `${site}/rest/api/3/user?accountId=${encodeURIComponent(accountId)}`, accountId, avatarUrls: avatars(accountId),
      displayName: u.displayName, active: true, timeZone: 'UTC', accountType: u.accountType ?? 'atlassian' };
  };
  const avatars = (accountId) => Object.fromEntries(['48x48', '24x24', '16x16', '32x32'].map((s) => [s, `${site}/secure/useravatar?size=${s}&ownerId=${encodeURIComponent(accountId)}`]));
  const status = (s) => ({ self: `${site}/rest/api/3/status/${s.id}`, description: '', iconUrl: `${site}/`, name: s.name, id: s.id,
    statusCategory: { self: `${site}/rest/api/3/statuscategory/${s.statusCategory.id}`, ...s.statusCategory } });
  const fieldValue = (id, v) => {
    if (v === null || v === undefined) return v ?? null;
    switch (id) {
      case 'status': return status(v);
      case 'assignee': case 'reporter': case 'creator': return user(v.accountId);
      case 'project': {
        const p = project(v.key);
        return { self: `${site}/rest/api/3/project/${p.id}`, id: p.id, key: p.key, name: p.name, projectTypeKey: 'software', simplified: false };
      }
      case 'issuetype': return { self: `${site}/rest/api/3/issuetype/${v.id}`, id: v.id, description: '', iconUrl: `${site}/rest/api/2/universal_avatar/view/type/issuetype/avatar/10315?size=medium`, name: v.name, subtask: v.subtask, hierarchyLevel: v.hierarchyLevel ?? 0 };
      case 'priority': return { self: `${site}/rest/api/3/priority/${v.id}`, iconUrl: `${site}/images/icons/priorities/${v.name.toLowerCase()}.svg`, name: v.name, id: v.id };
      case 'security': return { self: `${site}/rest/api/3/securitylevel/${v.id}`, id: v.id, description: '', name: v.name };
      case 'created': case 'updated': return jiraDate(v);
      default: return v;
    }
  };
  const ALL = () => pack.fields.filter((f) => f.id !== 'issuekey' && f.id !== 'comment').map((f) => f.id);
  // `fields` request semantics: names, `*all`, `*navigable`, `-name` exclusions; field names resolve to ids.
  const selectFields = (requested, defaultAll) => {
    let list = requested;
    if (list === undefined || list === null || (Array.isArray(list) && !list.length)) list = defaultAll ? ['*navigable'] : ['id'];
    if (typeof list === 'string') list = list.split(',');
    list = list.flatMap((x) => String(x).split(',')).map((x) => x.trim()).filter(Boolean);
    const out = new Set();
    let wildcard = false;
    for (const f of list) {
      if (f === '*all' || f === '*navigable') { wildcard = true; for (const id of ALL()) out.add(id); }
    }
    for (const f of list) {
      if (f.startsWith('-')) { out.delete(resolve(f.slice(1))); continue; }
      if (f.startsWith('*')) continue;
      out.add(resolve(f));
    }
    if (list.every((f) => f.startsWith('-')) && !wildcard) for (const id of ALL()) if (!list.includes('-' + id)) out.add(id);
    return out;
  };
  const resolve = (name) => {
    const lower = name.toLowerCase();
    const f = pack.fields.find((x) => x.id.toLowerCase() === lower || x.key.toLowerCase() === lower) ?? pack.fields.find((x) => x.name.toLowerCase() === lower);
    return f ? f.id : name;
  };
  const history = (h) => ({ id: h.changelogId, author: user(h.authorId), created: jiraDate(h.created), items: h.items });
  // One issue as GET /issue and search return it. `apiBase` is `/rest/api/3` or `/rest/agile/1.0`.
  const issue = (iss, selected, { expand = [], apiBase = '/rest/api/3', changelogCap = 100 } = {}) => {
    const onlyId = selected.size === 1 && selected.has('id');
    if (onlyId) return { id: iss.id };
    const fieldIds = [...selected].filter((f) => f !== 'id' && f !== 'key');
    if (!fieldIds.length) return { id: iss.id, self: `${site}${apiBase}/issue/${iss.id}`, key: iss.key };
    const fields = {};
    for (const id of fieldIds) {
      if (!pack.fields.some((f) => f.id === id)) continue;
      fields[id] = fieldValue(id, iss.fields[id]);
    }
    const out = { expand: 'renderedFields,names,schema,operations,editmeta,changelog,versionedRepresentations', id: iss.id, self: `${site}${apiBase}/issue/${iss.id}`, key: iss.key };
    if (expand.includes('changelog')) {
      const hs = state.st.histories.get(iss.id).slice().reverse().slice(0, changelogCap);
      const total = state.st.histories.get(iss.id).length;
      out.changelog = { startAt: 0, maxResults: hs.length, total, histories: hs.map((h) => history(h)) };
    }
    out.fields = fields;
    return out;
  };
  return { jiraDate, user, status, fieldValue, selectFields, resolveField: resolve, history, issue, project };
}

module.exports = { createRenderer, jiraDate };
