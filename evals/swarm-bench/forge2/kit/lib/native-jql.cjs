'use strict';
// Native frontend searches bypass app quota but consume endpoint request tokens. Charge the same
// disclosed virtual search latency as runtime requests; cached pages must not occur at one instant.
async function searchNativeJql({ siteUrl, jql, accountId, scopes, now, advance, latencyMs, rateReasons, onPage = () => {} }) {
  if (!(Number.isFinite(latencyMs) && latencyMs > 0)) throw new Error('Native JQL search requires a positive virtual request latency.');
  const issueIds = [];
  const seenTokens = new Set();
  const refusals = [];
  let nextPageToken;
  for (;;) {
    const requestedAt = now();
    const response = await fetch(new URL('/rest/api/3/search/jql', siteUrl), { method: 'POST', headers: {
      'content-type': 'application/json', 'x-forge-as': 'user', 'x-forge-account': accountId,
      'x-forge-scopes': JSON.stringify(scopes), 'x-forge-source': 'frontend', 'x-forge-module-type': 'jira:sprintAction',
    }, body: JSON.stringify({ jql, maxResults: 100, fields: ['summary'], ...(nextPageToken ? { nextPageToken } : {}) }) });
    const body = await response.json();
    const page = { response, body, t_ms: requestedAt, nextPageToken };
    await onPage(page);
    await advance(latencyMs);
    if (response.status === 429) {
      const reason = response.headers.get('ratelimit-reason');
      const header = response.headers.get('retry-after');
      if (!rateReasons.includes(reason)) throw new Error(`Native JQL HTTP429 has missing or invalid RateLimit-Reason: ${reason}`);
      const seconds = /^\d+$/.test(header ?? '') ? Number(header) : null;
      const until = seconds === null ? Date.parse(header) : requestedAt + seconds * 1000;
      if (!(Number.isFinite(until) && until > requestedAt)) throw new Error(`Native JQL HTTP429 has missing or invalid Retry-After: ${header}`);
      refusals.push({ t_ms: requestedAt, reason, retryAfter: header, until_ms: until });
      const waitMs = until - now();
      if (waitMs > 0) await advance(waitMs);
      continue;
    }
    if (!response.ok) return { search_ok: false, issue_ids: issueIds,
      error: (body.errorMessages ?? []).join('; ') || body.message || `Native search returned HTTP ${response.status}`, refusals };
    if (!Array.isArray(body.issues)) throw new Error('Native JQL search response omitted issues');
    issueIds.push(...body.issues.map((issue) => String(issue.id)));
    nextPageToken = body.nextPageToken;
    if (!nextPageToken) return { search_ok: true, issue_ids: issueIds, error: null, refusals };
    if (seenTokens.has(nextPageToken)) throw new Error('Native JQL pagination token repeated');
    seenTokens.add(nextPageToken);
  }
}
module.exports = { searchNativeJql };
