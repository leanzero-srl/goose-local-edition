// WP3's own mock Jira Cloud site, written from the public REST docs and the OpenAPI only (it is NOT the
// benchmark's site — WP3 may not read that). It exists to exercise the golden offline until the dev
// kit lands: paginated search/jql with ids-only default and bounded-JQL 400, changelog bulkfetch, Jira
// Software boards/sprints, per-user browse permission, ADF-only comments, scripted 429 + Retry-After,
// and scope checks.
'use strict';

const SPRINT_FIELD = 'customfield_10020';
const EST_OPS = 'customfield_10016'; // "Story point estimate" (board 1)
const EST_WEB = 'customfield_10028'; // "Story Points" (board 2)
const SCOPE_FIELD = 'customfield_10100'; // the app's own jira:customField scope-status
const DAY = 86400000;
const HOUR = 3600000;

function rng(seed) {
  let s = seed >>> 0;
  return () => {
    s = (s + 0x6d2b79f5) >>> 0;
    let t = s;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const jiraTime = (ms) => new Date(ms).toISOString().replace('Z', '+0000');

function createSite({ seed = 7, scopes = null, clock: vclock = null } = {}) {
  const rand = rng(seed);
  const pick = (arr) => arr[Math.floor(rand() * arr.length)];
  const NOW = Date.parse('2026-10-02T12:00:00.000Z');
  // The site's "now": the shared virtual clock when the bed installs one, else its own.
  let ownClock = NOW;
  const now = () => (vclock ? vclock.now() : ownClock);
  const advance = (ms) => (vclock ? vclock.advance(ms) : (ownClock += ms));

  const users = {
    alice: { accountId: '557058:alice', displayName: 'Alice Admin', browse: () => true, admin: true },
    bob: { accountId: '557058:bob', displayName: 'Bob Builder', browse: (i) => i.project === 'OPS' && !i.restricted },
    carol: { accountId: '557058:carol', displayName: 'Carol Coach', browse: () => true },
    dave: { accountId: '557058:dave', displayName: 'Dave Dev', browse: (i) => i.project === 'OPS' },
  };
  const byAccount = Object.fromEntries(Object.values(users).map((u) => [u.accountId, u]));
  const authors = [users.carol, users.dave, users.alice];

  const boards = [
    { id: 1, name: 'OPS board', type: 'scrum', project: 'OPS', estimateField: EST_OPS },
    { id: 2, name: 'WEB board', type: 'scrum', project: 'WEB', estimateField: EST_WEB },
    { id: 3, name: 'Support kanban', type: 'kanban', project: 'OPS', estimateField: null },
    { id: 4, name: 'Platform board', type: 'scrum', project: 'OPS', estimateField: EST_OPS },
  ];
  const sprints = [
    { id: 10, name: 'OPS Sprint 9', state: 'closed', board: 1, start: NOW - 25 * DAY, complete: NOW - 11 * DAY },
    { id: 11, name: 'OPS Sprint 10', state: 'active', board: 1, start: Date.parse('2026-09-21T09:00:00.000Z') },
    { id: 12, name: 'OPS Hotfix', state: 'active', board: 1, start: Date.parse('2026-09-23T07:30:00.000Z') },
    { id: 13, name: 'OPS Sprint 11', state: 'future', board: 1, start: null },
    { id: 21, name: 'WEB Sprint 4', state: 'active', board: 2, start: Date.parse('2026-09-24T08:00:00.000Z') },
    { id: 31, name: 'Platform Sprint 1', state: 'active', board: 4, start: Date.parse('2026-09-28T08:00:00.000Z') },
  ];
  const sprintById = new Map(sprints.map((s) => [s.id, s]));

  let changelogSeq = 50000;
  const issues = [];
  const issueById = new Map();
  const issueByKey = new Map();
  const comments = [];
  const requests = [];
  const rateLimits = [];
  const fieldWrites = [];

  function newIssue(project, n) {
    const id = String((project === 'OPS' ? 10000 : 20000) + n);
    const created = NOW - (30 + Math.floor(rand() * 10)) * DAY;
    const est = project === 'OPS' ? EST_OPS : EST_WEB;
    const roll = rand();
    const estimate = roll < 0.1 ? null : roll < 0.2 ? 0.5 : pick([1, 2, 3, 5, 8, 13]);
    const issue = {
      id,
      key: `${project}-${n}`,
      project,
      restricted: project === 'OPS' && n % 17 === 0,
      created,
      updated: created,
      initialSprints: [],
      sprints: [],
      fields: { [est]: estimate, summary: `${project} work item ${n}`, [SCOPE_FIELD]: null },
      estField: est,
      changelog: [],
    };
    issues.push(issue);
    issueById.set(id, issue);
    issueByKey.set(issue.key, issue);
    return issue;
  }

  function record(issue, at, author, items) {
    changelogSeq += 1 + Math.floor(rand() * 3);
    const entry = { id: String(changelogSeq), created: Math.floor(at / 1000) * 1000, author, items };
    issue.changelog.push(entry);
    issue.updated = Math.max(issue.updated, entry.created);
    return entry;
  }

  function setSprints(issue, next, at, author) {
    const from = issue.sprints;
    if (from.join(',') === next.join(',')) return null;
    issue.sprints = [...next];
    return record(issue, at, author, [
      { field: 'Sprint', fieldtype: 'custom', fieldId: SPRINT_FIELD, from: from.length ? from.join(', ') : null, fromString: from.map((s) => sprintById.get(s).name).join(', ') || null, to: next.length ? next.join(', ') : null, toString: next.map((s) => sprintById.get(s).name).join(', ') || null },
    ]);
  }

  function setEstimate(issue, value, at, author) {
    const from = issue.fields[issue.estField];
    issue.fields[issue.estField] = value;
    return record(issue, at, author, [
      { field: issue.estField === EST_OPS ? 'Story point estimate' : 'Story Points', fieldtype: 'custom', fieldId: issue.estField, from: from === null ? null : String(from), fromString: from === null ? null : String(from), to: value === null ? null : String(value), toString: value === null ? null : String(value) },
    ]);
  }

  function setSummary(issue, text, at, author) {
    const from = issue.fields.summary;
    issue.fields.summary = text;
    return record(issue, at, author, [{ field: 'summary', fieldtype: 'jira', fieldId: 'summary', from: null, fromString: from, to: null, toString: text }]);
  }

  // ---- seed: history up to NOW ----
  for (let n = 1; n <= 150; n += 1) newIssue('OPS', n);
  for (let n = 1; n <= 100; n += 1) newIssue('WEB', n);
  const s11 = sprintById.get(11).start;
  const s12 = sprintById.get(12).start;
  const s21 = sprintById.get(21).start;
  const s31 = sprintById.get(31).start;
  for (const issue of issues) {
    const r = rand();
    let initial = [];
    if (issue.project === 'OPS') initial = r < 0.25 ? [10, 11] : r < 0.5 ? [11] : r < 0.6 ? [12] : r < 0.65 ? [31] : r < 0.7 ? [10] : [];
    else initial = r < 0.45 ? [21] : [];
    issue.initialSprints = initial;
    issue.sprints = [...initial];
    // pre-start planning churn: never a ledger row
    if (rand() < 0.15 && initial.length) {
      const t = (initial.includes(21) ? s21 : s11) - (1 + rand()) * DAY;
      const before = [...issue.sprints];
      setSprints(issue, [], t, pick(authors));
      setSprints(issue, before, t + HOUR, pick(authors));
    }
  }
  const timeline = [];
  for (const issue of issues) {
    const ops = issue.project === 'OPS';
    const start = ops ? s11 : s21;
    const r = rand();
    const t = start + (0.2 + rand() * 7) * DAY;
    if (r < 0.12) timeline.push({ t, issue, kind: 'add', sprint: ops ? (rand() < 0.7 ? 11 : 12) : 21 });
    else if (r < 0.2) timeline.push({ t, issue, kind: 'remove' });
    else if (r < 0.26 && ops) timeline.push({ t, issue, kind: 'move' });
    else if (r < 0.3) timeline.push({ t, issue, kind: 'readd', sprint: ops ? 11 : 21 });
    else if (r < 0.36) timeline.push({ t, issue, kind: 'estimate' });
    else if (r < 0.42) timeline.push({ t, issue, kind: 'summary' });
    if (ops && rand() < 0.04) timeline.push({ t: s31 + (0.5 + rand() * 2) * DAY, issue, kind: 'add', sprint: 31 });
  }
  timeline.sort((a, b) => a.t - b.t);
  // two changes at exactly the same instant: the table orders them by changelog id
  if (timeline.length > 4) timeline[3].t = timeline[2].t;
  for (const ev of timeline) {
    const { issue, t } = ev;
    const author = pick(authors);
    const active = issue.sprints.filter((s) => sprintById.get(s).state === 'active');
    if (ev.kind === 'add') setSprints(issue, [...issue.sprints.filter((s) => sprintById.get(s).state !== 'active'), ev.sprint], t, author);
    if (ev.kind === 'remove' && active.length) setSprints(issue, issue.sprints.filter((s) => !active.includes(s)), t, author);
    if (ev.kind === 'move' && active.length) setSprints(issue, [...issue.sprints.filter((s) => !active.includes(s)), active.includes(11) ? 12 : 11], t, author);
    if (ev.kind === 'readd') {
      setSprints(issue, issue.sprints.filter((s) => s !== ev.sprint), t, author);
      setSprints(issue, [...issue.sprints, ev.sprint].sort((a, b) => a - b), t + 2 * HOUR, pick(authors));
    }
    if (ev.kind === 'estimate') setEstimate(issue, pick([1, 2, 3, 5, 8, 0.5, null]), t, author);
    if (ev.kind === 'summary') setSummary(issue, `${issue.fields.summary} (edited)`, t, author);
  }
  for (const issue of issues) issue.changelog.sort((a, b) => a.created - b.created || Number(a.id) - Number(b.id));

  // ---- REST ----
  const sprintJson = (s) => ({
    id: s.id,
    self: `https://site.example/rest/agile/1.0/sprint/${s.id}`,
    state: s.state,
    name: s.name,
    ...(s.start ? { startDate: new Date(s.start).toISOString(), endDate: new Date(s.start + 14 * DAY).toISOString() } : {}),
    ...(s.complete ? { completeDate: new Date(s.complete).toISOString() } : {}),
    originBoardId: s.board,
    goal: '',
  });
  const userJson = (u) => ({ accountId: u.accountId, displayName: u.displayName, active: true, accountType: 'atlassian' });

  function fieldValue(issue, f) {
    // As Jira (and the benchmark's site) render it: a closed sprint carries its completeDate on the issue too.
    if (f === SPRINT_FIELD) return issue.sprints.length ? issue.sprints.map((s) => { const sp = sprintJson(sprintById.get(s)); return { id: sp.id, name: sp.name, state: sp.state, boardId: sp.originBoardId, startDate: sp.startDate, endDate: sp.endDate, ...(sp.completeDate ? { completeDate: sp.completeDate } : {}) }; }) : null;
    if (f === 'updated') return jiraTime(issue.updated);
    if (f === 'created') return jiraTime(issue.created);
    if (f === 'project') return { key: issue.project };
    if (f === EST_OPS || f === EST_WEB || f === 'summary' || f === SCOPE_FIELD) return issue.fields[f] ?? null;
    return undefined;
  }
  function issueJson(issue, fields) {
    const out = { id: issue.id, key: issue.key, self: `https://site.example/rest/api/3/issue/${issue.id}`, fields: {} };
    for (const f of fields) {
      if (f === 'id' || f === 'key') continue;
      const v = fieldValue(issue, f);
      if (v !== undefined) out.fields[f] = v;
    }
    return out;
  }
  const historyJson = (h, fieldIds) => ({
    id: h.id,
    author: userJson(h.author),
    created: jiraTime(h.created),
    items: fieldIds ? h.items.filter((i) => fieldIds.includes(i.fieldId)) : h.items,
  });

  function evalJql(jql) {
    const text = jql.trim();
    if (!text) return { error: 'Unbounded JQL queries are not allowed here. Please add a search restriction to your query.' };
    const orParts = text.split(/\s+OR\s+/i);
    const preds = [];
    for (const part of orParts) {
      const andParts = part.split(/\s+AND\s+/i).map((p) => p.trim().replace(/^\(|\)$/g, (m) => m));
      const conj = [];
      for (const clause of andParts) {
        let m;
        if ((m = clause.match(/^sprint\s+in\s+\(([^)]*)\)$/i))) {
          const ids = m[1].split(',').map((x) => Number(x.trim()));
          conj.push((i) => i.sprints.some((s) => ids.includes(s)));
        } else if ((m = clause.match(/^sprint\s+in\s+openSprints\(\)$/i))) {
          conj.push((i) => i.sprints.some((s) => sprintById.get(s).state === 'active'));
        } else if ((m = clause.match(/^updated\s*>=\s*-(\d+)m$/i))) {
          const t = now() - Number(m[1]) * 60000;
          conj.push((i) => i.updated >= t);
        } else if ((m = clause.match(/^updated\s*>=\s*"(\d{4}-\d{2}-\d{2})"$/i))) {
          const t = Date.parse(`${m[1]}T00:00:00.000Z`);
          conj.push((i) => i.updated >= t);
        } else if ((m = clause.match(/^project\s*=\s*"?([A-Z]+)"?$/i))) {
          conj.push((i) => i.project === m[1].toUpperCase());
        } else if ((m = clause.match(/^(id|key|issue)\s+in\s+\(([^)]*)\)$/i))) {
          const ids = m[2].split(',').map((x) => x.trim());
          conj.push((i) => ids.includes(i.id) || ids.includes(i.key));
        } else return { error: `The mock JQL parser does not understand: ${clause}` };
      }
      preds.push((i) => conj.every((c) => c(i)));
    }
    return { match: (i) => preds.some((p) => p(i)) };
  }

  const SCOPES = [
    [/^GET \/rest\/api\/3\/field$/, 'read:jira-work'],
    [/^GET \/rest\/agile\/1\.0\/board$/, 'read:board-scope:jira-software'],
    [/^GET \/rest\/agile\/1\.0\/board\/\d+\/configuration$/, 'read:board-scope.admin:jira-software'],
    [/^GET \/rest\/agile\/1\.0\/board\/\d+\/sprint$/, 'read:sprint:jira-software'],
    [/^GET \/rest\/agile\/1\.0\/sprint\/\d+$/, 'read:sprint:jira-software'],
    [/^POST \/rest\/api\/3\/search\/jql$/, 'read:jira-work'],
    [/^POST \/rest\/api\/3\/changelog\/bulkfetch$/, 'read:jira-work'],
    [/^POST \/rest\/api\/3\/issue\/[^/]+\/changelog\/list$/, 'read:jira-work'],
    [/^GET \/rest\/api\/3\/issue\/[^/]+$/, 'read:jira-work'],
    [/^POST \/rest\/api\/3\/issue\/bulkfetch$/, 'read:jira-work'],
    [/^POST \/rest\/api\/3\/issue\/[^/]+\/comment$/, 'write:jira-work'],
    [/^POST \/rest\/api\/3\/app\/field\/value$/, 'storage:app'],
    [/^GET \/rest\/api\/3\/mypermissions$/, 'read:jira-work'],
    [/^GET \/rest\/api\/3\/myself$/, 'read:jira-user'],
  ];

  // as: 'app' | 'user'; accountId for user calls.
  function handle({ as, accountId, method, path, body, kind }) {
    const url = new URL(path, 'https://site.example');
    const p = url.pathname;
    const q = url.searchParams;
    const route = `${method} ${p}`;
    const entry = { at: now(), as, accountId, method, path: p + url.search, body, kind };
    requests.push(entry);
    const send = (status, json, headers = {}) => ({ status, json, headers });
    const rl = rateLimits.find((r) => r.times > 0 && r.match(method, p, body, as));
    if (rl) {
      rl.times -= 1;
      if (rl.status && rl.status !== 429) {
        entry.status = rl.status;
        return send(rl.status, rl.json ?? { errorMessages: ['scripted failure'] });
      }
      entry.status = 429;
      return send(429, { errorMessages: ['Rate limit exceeded'] }, { 'Retry-After': String(rl.retryAfter), 'RateLimit-Reason': rl.reason ?? 'jira-burst-based' });
    }
    if (p === '/rest/api/3/search' || p === '/rest/api/2/search') return send(410, { errorMessages: ['The requested API has been removed. Please migrate to the /rest/api/3/search/jql API.'] });
    const scope = SCOPES.find(([re]) => re.test(route));
    if (!scope) return send(404, { errorMessages: [`mock site has no ${route}`] });
    if (scopes && !scopes.includes(scope[1])) return send(401, { code: 401, message: 'Unauthorized; scope does not match' });
    const user = as === 'user' ? byAccount[accountId] : null;
    if (as === 'user' && !user) return send(401, { errorMessages: ['no user'] });
    const canSee = (i) => !i.deleted && (as === 'app' || user.browse(i));
    let m;

    if (route === 'GET /rest/api/3/field') {
      return send(200, [
        { id: 'summary', key: 'summary', name: 'Summary', custom: false, schema: { type: 'string', system: 'summary' } },
        { id: SPRINT_FIELD, key: SPRINT_FIELD, name: 'Sprint', custom: true, schema: { type: 'array', items: 'json', custom: 'com.pyxis.greenhopper.jira:gh-sprint', customId: 10020 } },
        { id: EST_OPS, key: EST_OPS, name: 'Story point estimate', custom: true, schema: { type: 'number', custom: 'com.pyxis.greenhopper.jira:jsw-story-points', customId: 10016 } },
        { id: EST_WEB, key: EST_WEB, name: 'Story Points', custom: true, schema: { type: 'number', custom: 'com.atlassian.jira.plugin.system.customfieldtypes:float', customId: 10028 } },
        { id: SCOPE_FIELD, key: 'b7a3c1d2-0000-4000-8000-000000000000__DEVELOPMENT__scope-status', name: 'Scope status', custom: true, schema: { type: 'string', custom: 'ari:cloud:ecosystem::extension/b7a3c1d2-0000-4000-8000-000000000000/0f1e2d3c-0000-4000-8000-000000000000/static/scope-status', customId: 10100 } },
      ]);
    }
    if (route === 'GET /rest/api/3/myself') return send(200, userJson(user));
    if (route === 'GET /rest/api/3/mypermissions') {
      const keys = (q.get('permissions') ?? '').split(',').filter(Boolean);
      if (!keys.length) return send(400, { errorMessages: ['The permissions query parameter is required.'] });
      const permissions = {};
      for (const k of keys) permissions[k] = { key: k, havePermission: k === 'ADMINISTER' ? user?.admin === true : true };
      return send(200, { permissions });
    }
    if (route === 'POST /rest/api/3/app/field/value') {
      if (as !== 'app') return send(403, { errorMessages: ['Only the app that owns the field can update its values.'] });
      const updates = body?.updates ?? [];
      const n = updates.reduce((k, u) => k + (u.issueIds?.length ?? 0), 0);
      if (!n || n > 200) return send(400, { errorMessages: ['1 to 200 issue updates per request'] });
      for (const u of updates) {
        if (u.customField !== SCOPE_FIELD) return send(404, { errorMessages: [`field ${u.customField} not found`] });
        if (!(u.value === null || typeof u.value === 'string')) return send(400, { errorMessages: ['string field: value must be a string'] });
        for (const id of u.issueIds) if (!issueById.get(String(id)) || issueById.get(String(id)).deleted) return send(404, { errorMessages: [`issue ${id} not found`] });
      }
      for (const u of updates) for (const id of u.issueIds) {
        issueById.get(String(id)).fields[SCOPE_FIELD] = u.value;
        fieldWrites.push({ at: now(), issueId: String(id), value: u.value });
      }
      return send(204);
    }
    if (route === 'GET /rest/agile/1.0/board') {
      const list = boards.filter((b) => !q.get('type') || b.type === q.get('type'));
      const startAt = Number(q.get('startAt') ?? 0);
      const max = Math.min(Number(q.get('maxResults') ?? 50), 2);
      const values = list.slice(startAt, startAt + max).map((b) => ({ id: b.id, name: b.name, type: b.type, location: { projectKey: b.project } }));
      return send(200, { maxResults: max, startAt, total: list.length, isLast: startAt + values.length >= list.length, values });
    }
    if ((m = p.match(/^\/rest\/agile\/1\.0\/board\/(\d+)\/configuration$/))) {
      const b = boards.find((x) => x.id === Number(m[1]));
      if (!b) return send(404, { errorMessages: ['Board does not exist'] });
      return send(200, { id: b.id, name: b.name, type: b.type, estimation: b.estimateField ? { type: 'field', field: { fieldId: b.estimateField, displayName: 'Story points' } } : { type: 'none' }, filter: { id: '1' } });
    }
    if ((m = p.match(/^\/rest\/agile\/1\.0\/board\/(\d+)\/sprint$/))) {
      const b = boards.find((x) => x.id === Number(m[1]));
      if (!b) return send(404, { errorMessages: ['Board does not exist'] });
      const states = (q.get('state') ?? 'future,active,closed').split(',');
      const list = sprints.filter((s) => s.board === b.id && states.includes(s.state));
      const startAt = Number(q.get('startAt') ?? 0);
      const max = Math.min(Number(q.get('maxResults') ?? 50), 50);
      const values = list.slice(startAt, startAt + max).map(sprintJson);
      return send(200, { maxResults: max, startAt, isLast: startAt + values.length >= list.length, values });
    }
    if ((m = p.match(/^\/rest\/agile\/1\.0\/sprint\/(\d+)$/))) {
      const s = sprintById.get(Number(m[1]));
      return s ? send(200, sprintJson(s)) : send(404, { errorMessages: [`Sprint with id ${m[1]} does not exist`] });
    }
    if (route === 'POST /rest/api/3/search/jql') {
      const ev = evalJql(body?.jql ?? '');
      if (ev.error) return send(400, { errorMessages: [ev.error] });
      const fields = Array.isArray(body.fields) ? body.fields : typeof body.fields === 'string' ? body.fields.split(',') : ['id'];
      const list = issues.filter((i) => canSee(i) && ev.match(i)).sort((a, b) => Number(a.id) - Number(b.id));
      const start = body.nextPageToken ? Number(Buffer.from(body.nextPageToken, 'base64url').toString()) : 0;
      const max = Math.min(Number(body.maxResults ?? 50), 50);
      const page = list.slice(start, start + max);
      entry.returned = page.length;
      const isLast = start + page.length >= list.length;
      const issuesOut = page.map((i) => (fields.length === 1 && fields[0] === 'id' ? { id: i.id } : issueJson(i, fields)));
      return send(200, { issues: issuesOut, isLast, ...(isLast ? {} : { nextPageToken: Buffer.from(String(start + page.length)).toString('base64url') }) });
    }
    if (route === 'POST /rest/api/3/changelog/bulkfetch') {
      const ids = body?.issueIdsOrKeys ?? [];
      if (!ids.length || ids.length > 1000) return send(400, { errorMessages: ['issueIdsOrKeys must hold 1 to 1000 ids'] });
      const fieldIds = body.fieldIds?.length ? body.fieldIds : null;
      const flat = [];
      for (const ref of ids) {
        const i = issueById.get(String(ref)) ?? issueByKey.get(String(ref));
        if (!i || !canSee(i)) continue;
        for (const h of i.changelog) if (!fieldIds || h.items.some((it) => fieldIds.includes(it.fieldId))) flat.push([i, h]);
      }
      const start = body.nextPageToken ? Number(Buffer.from(body.nextPageToken, 'base64url').toString()) : 0;
      const max = Math.min(Number(body.maxResults ?? 1000), 100);
      const page = flat.slice(start, start + max);
      const grouped = new Map();
      for (const [i, h] of page) {
        if (!grouped.has(i.id)) grouped.set(i.id, []);
        grouped.get(i.id).push(historyJson(h, fieldIds));
      }
      const more = start + page.length < flat.length;
      return send(200, { issueChangeLogs: [...grouped].map(([issueId, changeHistories]) => ({ issueId, changeHistories })), ...(more ? { nextPageToken: Buffer.from(String(start + page.length)).toString('base64url') } : {}) });
    }
    if ((m = p.match(/^\/rest\/api\/3\/issue\/([^/]+)\/changelog\/list$/))) {
      const i = issueById.get(m[1]) ?? issueByKey.get(m[1]);
      if (!i || !canSee(i)) return send(404, { errorMessages: ['Issue does not exist or you do not have permission to see it.'] });
      const want = (body?.changelogIds ?? []).map(String);
      const histories = i.changelog.filter((h) => want.includes(h.id)).map((h) => historyJson(h, null));
      return send(200, { startAt: 0, maxResults: histories.length, total: histories.length, histories });
    }
    if (route === 'POST /rest/api/3/issue/bulkfetch') {
      const ids = body?.issueIdsOrKeys ?? [];
      if (ids.length > 100) return send(400, { errorMessages: ['at most 100 issues'] });
      const fields = body.fields ?? ['*navigable'];
      const out = [];
      for (const ref of ids) {
        const i = issueById.get(String(ref)) ?? issueByKey.get(String(ref));
        if (i && canSee(i)) out.push(issueJson(i, fields));
      }
      return send(200, { issues: out.sort((a, b) => Number(a.id) - Number(b.id)), issueErrors: [] });
    }
    if ((m = p.match(/^\/rest\/api\/3\/issue\/([^/]+)\/comment$/))) {
      const i = issueById.get(m[1]) ?? issueByKey.get(m[1]);
      if (!i || !canSee(i)) return send(404, { errorMessages: ['Issue does not exist or you do not have permission to see it.'] });
      const doc = body?.body;
      if (!doc || typeof doc !== 'object' || doc.type !== 'doc' || doc.version !== 1 || !Array.isArray(doc.content)) return send(400, { errorMessages: ['Operation value must be an Atlassian Document (see the Atlassian Document Format)'] });
      if (as !== 'user') return send(400, { errorMessages: ['test site: comments must be posted as the person'] });
      const c = { id: String(comments.length + 1), issueId: i.id, issueKey: i.key, author: user.accountId, body: doc, visibility: body.visibility ?? null, at: now() };
      comments.push(c);
      return send(201, { id: c.id, author: userJson(user), body: doc, created: jiraTime(now()) });
    }
    if ((m = p.match(/^\/rest\/api\/3\/issue\/([^/]+)$/)) && method === 'GET') {
      const i = issueById.get(m[1]) ?? issueByKey.get(m[1]);
      if (!i || !canSee(i)) return send(404, { errorMessages: ['Issue does not exist or you do not have permission to see it.'] });
      const fields = q.get('fields') ? q.get('fields').split(',') : ['summary', SPRINT_FIELD, EST_OPS, EST_WEB, 'updated'];
      const out = issueJson(i, fields);
      // expand=changelog embeds the most recent page of the changelog (at most 100 histories).
      if ((q.get('expand') ?? '').split(',').includes('changelog')) {
        const histories = i.changelog.slice(-100).map((h) => historyJson(h, null));
        out.changelog = { startAt: 0, maxResults: histories.length, total: i.changelog.length, histories };
      }
      return send(200, out);
    }
    return send(404, { errorMessages: [`mock site has no ${route}`] });
  }

  // ---- live updates: change the site and return the product event ----
  function update(key, change, authorName = 'carol') {
    const issue = issueByKey.get(key);
    advance(7 * 60000);
    const author = users[authorName];
    let entry;
    if (change.sprints) entry = setSprints(issue, change.sprints, now(), author);
    else if ('estimate' in change) entry = setEstimate(issue, change.estimate, now(), author);
    else entry = setSummary(issue, change.summary, now(), author);
    return {
      eventType: 'avi:jira:updated:issue',
      selfGenerated: false,
      atlassianId: author.accountId,
      issue: { id: issue.id, key: issue.key, fields: { summary: issue.fields.summary, project: { key: issue.project } } },
      changelog: { id: entry.id, items: entry.items },
    };
  }

  // ---- world changes (R4) ----
  function deleteIssue(key) {
    const issue = issueByKey.get(key);
    advance(60000);
    issue.deleted = true;
    return { eventType: 'avi:jira:deleted:issue', issue: { id: issue.id, key: issue.key } };
  }
  function closeSprint(id) {
    advance(60000);
    const s = sprintById.get(id);
    s.state = 'closed';
    s.complete = now();
  }
  function setBoardField(boardId, fieldId) {
    advance(60000);
    const board = boards.find((b) => b.id === boardId);
    // Kept so the oracle knows the field a change was made under (contract §1: a change keeps it).
    board.switches = [...(board.switches ?? []), { at: now(), from: board.estimateField, to: fieldId }];
    board.estimateField = fieldId;
  }

  return { SPRINT_FIELD, SCOPE_FIELD, users, boards, sprints, sprintById, issues, issueByKey, issueById, comments, requests, rateLimits, fieldWrites, handle, update, deleteIssue, closeSprint, setBoardField, now };
}

// Ground truth for the active sprints, computed FORWARD from each issue's initial sprints and full
// history (the app reconstructs it backwards from the ledger, so this is an independent derivation).
function oracle(site) {
  const out = {};
  const perIssue = new Map(); // issueId -> [{ sprintId, atStart, everAfter, inNow, e, addedE }]
  // Contract §1: no value counts as 0 and a deleted issue has no value; a change's points read the field its board
  // used at the time of the change.
  const valueOf = (i, field) => (i.deleted ? 0 : i.fields[field] ?? 0);
  const fieldAt = (board, t) => (board.switches ?? []).reduce((f, s) => (s.at <= t ? s.to : f), board.switches?.[0]?.from ?? board.estimateField);
  for (const s of site.sprints.filter((x) => x.state === 'active')) {
    const changes = [];
    let committed = 0;
    let added = 0;
    let removed = 0;
    for (const i of site.issues) {
      let members = new Set(i.initialSprints);
      let atStart = null;
      let everAfter = false;
      for (const h of i.changelog) {
        if (atStart === null && h.created > s.start) {
          atStart = members.has(s.id);
          everAfter = atStart;
        }
        for (const it of h.items) {
          if (it.fieldId !== site.SPRINT_FIELD) continue;
          const from = new Set((it.from ?? '').split(',').map((x) => x.trim()).filter(Boolean).map(Number));
          const to = new Set((it.to ?? '').split(',').map((x) => x.trim()).filter(Boolean).map(Number));
          if (h.created > s.start) {
            if (to.has(s.id) && !from.has(s.id)) changes.push({ changeId: h.id, issueKey: i.key, issueId: i.id, kind: 'added', at: h.created, by: h.author.displayName });
            if (from.has(s.id) && !to.has(s.id)) changes.push({ changeId: h.id, issueKey: i.key, issueId: i.id, kind: 'removed', at: h.created, by: h.author.displayName });
          }
          members = to;
        }
        if (h.created > s.start && members.has(s.id)) everAfter = true;
      }
      if (atStart === null) {
        atStart = members.has(s.id);
        everAfter = atStart;
      }
      const inNow = !i.deleted && i.sprints.includes(s.id);
      const board = site.boards.find((b) => b.id === s.board);
      const e = valueOf(i, board.estimateField);
      const lastAdd = changes.filter((c) => c.issueId === i.id && c.kind === 'added').at(-1);
      const addedE = lastAdd ? valueOf(i, fieldAt(board, lastAdd.at)) : e;
      if (!perIssue.has(i.id)) perIssue.set(i.id, []);
      perIssue.get(i.id).push({ sprintId: s.id, atStart, everAfter, inNow, e, addedE });
      if (atStart) committed += e;
      if (inNow && !atStart) added += e;
      if (everAfter && !inNow) removed += e;
    }
    changes.sort((a, b) => a.at - b.at || Number(a.changeId) - Number(b.changeId));
    const r6 = (x) => Math.round(x * 1e6) / 1e6;
    const creep = committed === 0 ? null : Math.sign(added) * Math.round(Math.abs((100 * added) / committed) * 10 + 1e-9) / 10;
    out[s.id] = { sprint: s, changes, committed: r6(committed), added: r6(added), removed: r6(removed), creep };
  }
  // R7: the scope status each (existing) issue must show.
  const status = new Map();
  for (const i of site.issues) {
    if (i.deleted) continue;
    const per = perIssue.get(i.id) ?? [];
    const now = per.find((x) => x.inNow);
    status.set(i.id, now ? (now.atStart ? 'committed' : `added +${Math.round(now.addedE * 1e6) / 1e6}`) : per.some((x) => x.everAfter) ? 'removed' : '');
  }
  Object.defineProperty(out, 'status', { value: status, enumerable: false });
  return out;
}

const fmtCreep = (c) => (c === null ? '—' : `${c.toFixed(1)}%`);

module.exports = { createSite, oracle, fmtCreep, SPRINT_FIELD, EST_OPS, EST_WEB };
