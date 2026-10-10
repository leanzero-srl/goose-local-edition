'use strict';
// GET /rest/api/3/group/member (site/rest/groups.cjs) through a live site: routing and scopes, the documented page
// shape, the texts and clamps measured on Jira Cloud 2026-10-10, and the scoring site's half pages.
// Run: node --test evals/swarm-bench/forge2/site/rest/groups.test.cjs
const test = require('node:test');
const assert = require('node:assert');
const fs = require('node:fs');
const path = require('node:path');
const { createSite } = require('../site.cjs');

const GRANULAR = ['read:group:jira', 'read:user:jira', 'read:avatar:jira'];
const spec = JSON.parse(fs.readFileSync(path.join(__dirname, '..', '..', 'kit', 'openapi', 'jira.json'), 'utf8'));
const PAGE_KEYS = Object.keys(spec.components.schemas.PageBeanUserDetails.properties);
const USER_KEYS = Object.keys(spec.components.schemas.UserDetails.properties);

async function withSite(fn, opts = {}) {
  const site = await createSite({ seed: '0123456789abcdef', port: 0, ...opts });
  let vt = site.state.now();
  // as: 'app' | <accountId>; each call 10 s after the previous one, so the burst bucket never refuses.
  const call = async (query, { as = 'app', scopes = GRANULAR } = {}) => {
    vt += 10_000;
    const headers = { 'x-forge-as': as === 'app' ? 'app' : 'user', 'x-forge-scopes': JSON.stringify(scopes), 'x-forge-vtime': String(vt),
      ...(as === 'app' ? {} : { 'x-forge-account': as, 'x-forge-source': 'resolver', 'x-forge-module-type': 'jira:adminPage' }) };
    const res = await fetch(`${site.url}/rest/api/3/group/member${query}`, { headers });
    return { status: res.status, body: await res.json() };
  };
  try { await fn(site, call); } finally { await site.stop(); }
}

test('members of a group by name (any case) or id, in the documented page shape, asApp or as a person', () => withSite(async (site, call) => {
  const [everyone, team] = site.pack.groups;
  const r = await call(`?groupname=${encodeURIComponent(team.name.toUpperCase())}`);
  assert.strictEqual(r.status, 200, JSON.stringify(r.body));
  assert.deepStrictEqual(r.body.values.map((u) => u.accountId), team.members);
  assert.deepStrictEqual((await call(`?groupId=${team.groupId}`)).body.values, r.body.values, 'by id: the same members');
  assert.deepStrictEqual([r.body.startAt, r.body.maxResults, r.body.total, r.body.isLast, 'nextPage' in r.body], [0, 50, team.members.length, true, false]);
  assert.strictEqual(r.body.self, `${site.pack.siteUrl}/rest/api/3/group/member?includeInactiveUsers=false&maxResults=50&groupId=${team.groupId}&startAt=0`);
  assert.deepStrictEqual(Object.keys(r.body).filter((k) => !PAGE_KEYS.includes(k)), [], 'only PageBeanUserDetails keys');
  for (const u of r.body.values) {
    assert.deepStrictEqual(Object.keys(u).filter((k) => !USER_KEYS.includes(k)), [], 'only UserDetails keys');
    assert.strictEqual(u.displayName, site.pack.users.find((x) => x.accountId === u.accountId).displayName);
  }
  assert.strictEqual((await call(`?groupname=${everyone.name}`, { scopes: ['manage:jira-configuration'] })).body.total, everyone.members.length, 'the classic scope');
  assert.strictEqual((await call(`?groupname=${everyone.name}`, { as: site.pack.viewer })).status, 200, 'a person holds Browse users and groups');
  assert.strictEqual((await call(`?groupname=${everyone.name}`, { scopes: ['read:jira-user'] })).status, 401, 'no group scope');
  assert.deepStrictEqual(site.harnessMissing, [], 'modelled: never a 501');
}));

test('paging: nextPage while more remain, maxResults clamped to 1..50 and echoed, a start past the end is an empty last page', () => withSite(async (site, call) => {
  const everyone = site.pack.groups[0];
  const seen = [];
  let startAt = 0;
  for (;;) {
    const r = await call(`?groupId=${everyone.groupId}&maxResults=3&startAt=${startAt}`);
    seen.push(...r.body.values.map((u) => u.accountId));
    if (r.body.isLast) { assert.ok(!('nextPage' in r.body)); break; }
    startAt += 3;
    assert.strictEqual(r.body.nextPage, `${site.pack.siteUrl}/rest/api/3/group/member?includeInactiveUsers=false&maxResults=3&groupId=${everyone.groupId}&startAt=${startAt}`);
  }
  assert.deepStrictEqual(seen, everyone.members);
  for (const [asked, served] of [[0, 1], [-1, 1], [100, 50]]) assert.strictEqual((await call(`?groupId=${everyone.groupId}&maxResults=${asked}`)).body.maxResults, served, `maxResults=${asked}`);
  const past = await call(`?groupId=${everyone.groupId}&startAt=999&includeInactiveUsers=true`);
  assert.deepStrictEqual([past.status, past.body.values, past.body.isLast, past.body.total], [200, [], true, everyone.members.length]);
  assert.match(past.body.self, /includeInactiveUsers=true/);
}));

test('the measured refusals: no group, both parameters, an unknown name or id', () => withSite(async (site, call) => {
  const g = site.pack.groups[1];
  const cases = [
    ['', 400, 'The group ID and group name can not be empty.'],
    [`?groupname=${g.name}&groupId=${g.groupId}`, 400, "The query parameters 'groupId' and 'groupname' are mutually exclusive."],
    ['?groupname=no-such-group', 404, "The group named 'no-such-group' does not exist"],
    ['?groupId=00000000-0000-0000-0000-000000000000', 404, "The group with group ID '00000000-0000-0000-0000-000000000000' does not exist"],
  ];
  for (const [query, status, text] of cases) {
    const r = await call(query);
    assert.deepStrictEqual([r.status, r.body], [status, { errorMessages: [text], errors: {} }], query);
  }
}));

test('the scoring site serves at most half the group per page, like its other lists', () => withSite(async (site, call) => {
  const everyone = site.pack.groups[0];
  const first = await call(`?groupId=${everyone.groupId}`);
  assert.deepStrictEqual([first.body.maxResults, first.body.values.length, first.body.isLast], [Math.ceil(everyone.members.length / 2), Math.ceil(everyone.members.length / 2), false]);
  const second = await call(`?groupId=${everyone.groupId}&startAt=${first.body.values.length}`);
  assert.deepStrictEqual([...first.body.values, ...second.body.values].map((u) => u.accountId), everyone.members);
  assert.strictEqual(second.body.isLast, true);
}, { scoring: true }));
