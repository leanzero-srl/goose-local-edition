'use strict';
// https://developer.atlassian.com/cloud/jira/platform/rest/v3/api-group-issue-search/#api-rest-api-3-jql-match-post
// The issue-ID list bounds matching. Native search's unbounded-query refusal does not apply to this operation.
const { compile, JqlError } = require('../jql.cjs');
const { err, jqlModel } = require('./platform.cjs');

const handlers = {
  'POST /rest/api/3/jql/match': async (c) => {
    const body = c.req.body;
    if (!body || typeof body !== 'object' || Array.isArray(body)
        || Object.keys(body).some((key) => !['issueIds', 'jqls'].includes(key))) {
      return err(400, 'The request must contain issueIds and jqls.');
    }
    const { issueIds, jqls } = body;
    if (!Array.isArray(issueIds) || issueIds.length > c.limits.jqlMatchIssues.value || issueIds.some((id) => !Number.isSafeInteger(id))
        || new Set(issueIds).size !== issueIds.length) {
      return err(400, 'issueIds must contain up to 50 unique integer issue IDs.');
    }
    if (!Array.isArray(jqls) || jqls.length > c.limits.jqlMatchQueries.value || jqls.some((jql) => typeof jql !== 'string')) {
      return err(400, 'jqls must contain up to 10 JQL strings.');
    }
    const matches = [];
    for (const source of jqls) {
      const expanded = await c.expandJql(source);
      if (expanded.error) {
        matches.push({ matchedIssues: [], errors: [expanded.error] });
        continue;
      }
      try {
        const query = compile(expanded.jql, jqlModel(c));
        const matchedIssues = issueIds.filter((id) => {
          const issue = c.state.issueByIdOrKey(id);
          return issue && c.canBrowse(issue) && query.matches(issue);
        });
        matches.push({ matchedIssues, errors: [] });
      } catch (e) {
        if (!(e instanceof JqlError)) throw e;
        matches.push({ matchedIssues: [], errors: [e.message] });
      }
    }
    return { status: 200, body: { matches } };
  },
};
module.exports = { handlers };
