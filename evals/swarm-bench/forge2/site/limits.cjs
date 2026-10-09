'use strict';
// Every page cap and limit the site enforces, with its receipt. `openapi` receipts are verified
// against the pinned kit OpenAPI files by forge2/kit/test/site.test.cjs (the quote must occur in the
// named operation's text); `measured` receipts were read on a Jira Cloud site on 2026-10-02.
// The rate limits (points quota, burst bucket, per-issue writes) are the benchmark's own model: rate.cjs MODEL.
const LIMITS = {
  searchJqlDefault: { value: 50, receipt: { openapi: 'jira.json GET /rest/api/3/search/jql maxResults', quote: '"default":50' } },
  searchJqlIdsOnlyMax: { value: 5000, receipt: { openapi: 'jira.json GET /rest/api/3/search/jql maxResults', quote: 'The greatest number of items returned per page is achieved when requesting `id` or `key` only. It returns max 5000 issues.' } },
  searchJqlFieldsMax: { value: 100, receipt: { openapi: 'jira.json GET /rest/api/3/search/jql maxResults', quote: 'API may return fewer items per page where a large number of fields or properties are requested', measured: 'maxResults=5000&fields=summary returned 100 issues and a nextPageToken; fields=id or key returned 5000' } },
  searchJqlOrderByFields: { value: 7, receipt: { openapi: 'jira.json GET /rest/api/3/search/jql jql', quote: '`orderBy` clause can contain a maximum of 7 fields', measured: '8 ORDER BY fields -> 400 "JQL can be ordered by at most 7 fields."' } },
  changelogBulkIssues: { value: 1000, receipt: { openapi: 'jira.json POST /rest/api/3/changelog/bulkfetch', quote: 'You can request the changelogs of up to 1000 issues and can filter them by up to 10 field IDs.' } },
  changelogBulkFields: { value: 10, receipt: { openapi: 'jira.json POST /rest/api/3/changelog/bulkfetch', quote: 'You can request the changelogs of up to 1000 issues and can filter them by up to 10 field IDs.' } },
  changelogBulkPageDefault: { value: 1000, receipt: { openapi: 'jira.json components.schemas.BulkChangelogRequestBean', quote: '"default":1000,"description":"The maximum number of items to return per page"' } },
  changelogBulkPageMax: { value: 10000, receipt: { openapi: 'jira.json components.schemas.BulkChangelogRequestBean', quote: '"maximum":10000' } },
  issueBulkDefault: { value: 100, receipt: { openapi: 'jira.json POST /rest/api/3/issue/bulkfetch', quote: 'By default you can request up to 100 issues in a single call.' } },
  issueBulkNamedFields: { value: 1000, receipt: { openapi: 'jira.json POST /rest/api/3/issue/bulkfetch', quote: 'You can request up to 1000 issues in a single call when the request is shaped so that it can be served efficiently' } },
  issueChangelogPage: { value: 100, receipt: { openapi: 'jira.json GET /rest/api/3/issue/{issueIdOrKey}/changelog maxResults', quote: '"default":100', measured: 'maxResults=1000 echoed maxResults 100' } },
  commentPage: { value: 100, receipt: { measured: 'GET /rest/api/3/issue/{key}/comment?maxResults=1000 echoed maxResults 100' } },
  fieldSearchPage: { value: 50, receipt: { measured: 'GET /rest/api/3/field/search?maxResults=1000 echoed maxResults 50' } },
  agileBoardPage: { value: 50, receipt: { measured: 'GET /rest/agile/1.0/board?maxResults=1000 echoed maxResults 50' } },
  agileSprintPage: { value: 50, receipt: { measured: 'GET /rest/agile/1.0/board/{id}/sprint?maxResults=1000 echoed maxResults 50' } },
  agileIssuePage: { value: 5000, receipt: { measured: 'Jira Cloud 2026-10-02: GET /rest/agile/1.0/board/{id}/issue?maxResults=10000 echoed maxResults 5000 and returned all 2976 issues; maxResults=1000 on sprint/{id}/issue echoed 1000' } },
  softwareIssueDefault: { value: 50, receipt: { measured: 'Jira Cloud 2026-10-02: GET /rest/software/1.0/board/{id}/issue and /backlog with no maxResults returned 50 issues, isLast false, a nextPageToken' } },
  softwareIssuePage: { value: 5000, receipt: { openapi: 'jsw.json GET /rest/software/1.0/sprint/{sprintId}/issue maxResults', quote: 'It returns max 5000 issues.' } },
  agileIssueDefault: { value: 50, receipt: { openapi: 'jsw.json GET /rest/agile/1.0/board/{boardId}/backlog (also epic issue lists; board/sprint issue lists state no default)', quote: 'The maximum number of issues to return per page. Default: 50.' } },
};

// The SCORING site's page rule (DESIGN §17.6; the dev site never carries it). Jira serves at most `maxResults` per
// page and may serve fewer: "To manage page size, API may return fewer items per page" (jira.json GET
// /rest/api/3/search/jql maxResults; jsw.json /rest/software/1.0 issue lists), "Each operation can have a different
// limit for the number of items returned, and these limits may change without notice"
// (developer.atlassian.com/cloud/jira/platform/rest/v3/intro/#pagination). At the pack's volume no backfill read
// crosses a documented cap, so an app's paging loop never ran (Sol 2026-10-03: 79 of 83 walks were one page). The
// scoring site uses that latitude so every list of two or more items spans at least two pages — a ratio of the
// list itself, never a typed size.
const SCORING_PAGING = {
  rule: 'half',
  receipt: { doc: 'https://developer.atlassian.com/cloud/jira/platform/rest/v3/intro/#pagination',
    quote: 'Each operation can have a different limit for the number of items returned, and these limits may change without notice.',
    openapi: 'jira.json GET /rest/api/3/search/jql maxResults', openapiQuote: 'To manage page size, API may return fewer items per page' },
};

// Items the site puts on one page: `size` is the request clamped to the documented cap; under the scoring rule a
// list of `total` >= 2 items is served in pages of at most ceil(total / 2).
const servedPageSize = (paging, size, total) => (paging?.rule === 'half' && total >= 2 ? Math.min(size, Math.ceil(total / 2)) : size);

module.exports = { LIMITS, SCORING_PAGING, servedPageSize };
