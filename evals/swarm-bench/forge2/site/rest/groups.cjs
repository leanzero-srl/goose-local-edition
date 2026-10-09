'use strict';
// A Jira group's members: GET /rest/api/3/group/member (getUsersFromGroup in the pinned OpenAPI), one of the ways an
// app can check the admin panel's `Comment group` (SPEC §2.6) beside platform.cjs's GET /user/groups and GET
// /myself?expand=groups. All three read the site's groups (pack.groups).
// Measured on a Jira Cloud site 2026-10-10 and served the same way here: the group is named by `groupname`
// (case-insensitive) or `groupId`, never both; the 400/404 texts below; maxResults is clamped to 1..50
// (limits.cjs groupMemberPage) and echoed as served; `self` and `nextPage` name the group by id whatever the request
// used; `nextPage` only while more remain; a startAt past the end is an empty last page; members render like every
// other user on the site.
// HARNESS CHOICES: members come in the group's own order (Jira orders by username, which no response carries); every
// site user is active, so includeInactiveUsers changes only the echo; every caller holds the "Browse users and groups"
// global permission the operation asks for (each person through the group that holds everyone, and the app), so the
// documented 403 is never answered. The scoring site serves fewer per page than asked like its other lists
// (limits.cjs SCORING_PAGING).
const { err, intParam } = require('./platform.cjs');
const { servedPageSize } = require('../limits.cjs');

const handlers = {
  'GET /rest/api/3/group/member': (c) => {
    const q = c.req.query;
    const name = q.get('groupname');
    const id = q.get('groupId');
    if (name && id) return err(400, "The query parameters 'groupId' and 'groupname' are mutually exclusive.");
    if (!name && !id) return err(400, 'The group ID and group name can not be empty.');
    const group = c.state.pack.groups.find((g) => (id ? g.groupId === id : g.name.toLowerCase() === name.toLowerCase()));
    if (!group) return err(404, id ? `The group with group ID '${id}' does not exist` : `The group named '${name}' does not exist`);
    const cap = c.limits.groupMemberPage.value;
    const asked = intParam(q.get('maxResults'), cap);
    const startAt = Math.max(0, intParam(q.get('startAt'), 0) || 0);
    const members = group.members;
    const maxResults = servedPageSize(c.paging, Math.min(Math.max(Number.isFinite(asked) ? asked : cap, 1), cap), members.length);
    const values = members.slice(startAt, startAt + maxResults).map((accountId) => c.render.user(accountId));
    const isLast = startAt + values.length >= members.length;
    const inactive = q.get('includeInactiveUsers') === 'true';
    const url = (at) => `${c.state.pack.siteUrl}/rest/api/3/group/member?includeInactiveUsers=${inactive}&maxResults=${maxResults}&groupId=${group.groupId}&startAt=${at}`;
    return { status: 200, body: { self: url(startAt), ...(isLast ? {} : { nextPage: url(startAt + maxResults) }), maxResults, startAt, total: members.length, isLast, values } };
  },
};

module.exports = { handlers };
