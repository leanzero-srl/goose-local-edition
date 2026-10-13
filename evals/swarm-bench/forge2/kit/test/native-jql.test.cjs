'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const http = require('node:http');
const { searchNativeJql } = require('../lib/native-jql.cjs');
const { MODEL } = require('../../site/rate.cjs');

async function withResponses(responses, run) {
  let clock = Date.UTC(2026, 9, 20, 10);
  const requests = [];
  const server = http.createServer((req, res) => {
    let raw = '';
    req.on('data', (chunk) => { raw += chunk; });
    req.on('end', () => {
      requests.push({ at: clock, headers: req.headers, body: JSON.parse(raw) });
      const next = responses.shift();
      assert.ok(next, 'transport requested an unexpected extra page');
      res.writeHead(next.status, { 'content-type': 'application/json', ...next.headers });
      res.end(JSON.stringify(next.body));
    });
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const args = { siteUrl: `http://127.0.0.1:${server.address().port}`, jql: 'issue in example("1")', accountId: 'viewer',
    scopes: ['read:jira-work'], now: () => clock, advance: async (ms) => { clock += ms; },
    latencyMs: MODEL.latencyMs.search, rateReasons: Object.values(MODEL.reasons) };
  try { await run(args, requests); } finally { await new Promise((resolve) => server.close(resolve)); }
}

test('native frontend429 retains refusal evidence and retries the same page only after Retry-After', async () => {
  for (const dateHeader of [false, true]) {
    const until = Date.UTC(2026, 9, 20, 10) + 2000;
    const retryAfter = dateHeader ? new Date(until).toUTCString() : '2';
    await withResponses([{ status: 429, headers: { 'RateLimit-Reason': 'jira-burst-based', 'Retry-After': retryAfter }, body: { errorMessages: ['Rate limit exceeded'] } },
      { status: 200, body: { issues: [{ id: '1' }], nextPageToken: 'next' } },
      { status: 200, body: { issues: [{ id: '2' }] } }], async (args, requests) => {
      const statuses = [];
      const result = await searchNativeJql({ ...args, onPage: ({ response }) => statuses.push(response.status) });
      assert.deepEqual(statuses, [429, 200, 200]);
      assert.deepEqual(result.issue_ids, ['1', '2']);
      assert.equal(result.search_ok, true);
      assert.deepEqual(result.refusals, [{ t_ms: until - 2000, reason: 'jira-burst-based', retryAfter, until_ms: until }]);
      assert.equal(requests[1].at, until);
      assert.equal(requests[2].at, until + MODEL.latencyMs.search);
      assert.deepEqual(requests[0].body, requests[1].body, 'a refused page is retried, not skipped');
      assert.equal(requests[2].body.nextPageToken, 'next');
      assert.equal(requests[0].headers['x-forge-source'], 'frontend');
      assert.equal(requests[0].headers['x-forge-account'], 'viewer');
    });
  }
});

test('native frontend429 rejects malformed or missing rate evidence without inventing a delay', async () => {
  for (const headers of [ {}, { 'Retry-After': '1', 'RateLimit-Reason': 'invented-reason' },
    { 'RateLimit-Reason': 'jira-burst-based' }, ...['', '0', '-1', 'not-a-date'].map((value) => ({ 'Retry-After': value, 'RateLimit-Reason': 'jira-burst-based' })) ]) {
    await withResponses([{ status: 429, headers, body: { errorMessages: ['Rate limit exceeded'] } }], async (args, requests) => {
      await assert.rejects(searchNativeJql(args), /Native JQL HTTP429 has missing or invalid/);
      assert.equal(requests.length, 1);
      assert.equal(args.now(), requests[0].at + MODEL.latencyMs.search);
    });
  }
});

test('native frontend400 is a recorded query refusal, with no pagination or fabricated result', async () => {
  await withResponses([{ status: 400, body: { errorMessages: ['Function requires a numeric sprint'] } }], async (args, requests) => {
    const result = await searchNativeJql(args);
    assert.deepEqual(result, { search_ok: false, issue_ids: [], error: 'Function requires a numeric sprint', refusals: [] });
    assert.equal(requests.length, 1);
  });
});
