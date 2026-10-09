'use strict';
// facts(seed) -> pack: the seeded Jira site Scope Ledger is installed into, at the Forge 2.0 scale (SPEC §2.3):
// 3 projects, 4 scrum boards (2 estimate with field A, 2 with field B) + 1 kanban, 6 active / 2 future / 6 closed
// sprints, ~1,000 issues (~300 in active sprints), a pre-history, ~200 relevant + ~800 irrelevant live updates over the
// 6 scored virtual hours after the upgrade, and the v1 rows preloaded for the first 2 days of each active sprint.
// Pure: every value comes from the seed's xorshift128+ stream; no Math.random, no wall clock.
// The ranges below are GENERATOR POLICY; the scorer reads the pack's actual values, never these.
//   node forge2/site/fixtures.cjs --seed <16 hex> [--out pack.json] [--scoring]
// facts(seed, { scoring: true }) is the SCORING site's pack: the same shapes plus `paging` (limits.cjs SCORING_PAGING)
// and the scoring-only cases (a tied pair of start dates, an empty active sprint, a long sprint name, scaled faults).
const fs = require('fs');
const { createRng, SEED_RE } = require('./rng.cjs');
const { LIMITS, SCORING_PAGING } = require('./limits.cjs');

const DAY = 86_400_000;
const HOUR = 3_600_000;
const MIN = 60_000;
const SEC = 1000;
// Generator anchor: packs are dated after the module set the task demands went GA (2026-09-22).
const ANCHOR = Date.UTC(2026, 8, 24);
const ESTIMATES = [0.5, 1, 2, 3, 5, 8, 13];
// SPEC §2.3: v1 rows exist for the first 2 days of each active sprint.
const V1_WINDOW = 2 * DAY;
// Scrum boards: [project index, active, future, closed] (SPEC §2.3 totals: 6 active, 2 future, 6 closed). The first
// project has two boards whose teams plan disjoint issues, so an issue can move to ANOTHER board's sprint (with that
// board's estimation field) without changing projects.
const BOARD_LAYOUT = [[0, 2, 1, 2], [0, 1, 0, 1], [1, 2, 1, 2], [2, 1, 0, 1]];
// Share of issues per project, and of the first project's issues homed on its first board.
const PROJECT_SHARE = [0.46, 0.30, 0.24];
const FIRST_BOARD_SHARE = 0.55;

const PROJECTS = [
  ['OPS', 'Operations'], ['PAY', 'Payments'], ['CORE', 'Core Platform'], ['WEB', 'Web Storefront'],
  ['DATA', 'Data Platform'], ['MOB', 'Mobile Apps'], ['API', 'Public API'], ['INFRA', 'Infrastructure'],
  ['SEC', 'Security Engineering'], ['GROW', 'Growth'], ['CRM', 'Customer Records'], ['BILL', 'Billing'],
  ['SHIP', 'Fulfilment'], ['SRCH', 'Search'], ['AUTH', 'Identity'], ['FIN', 'Finance Systems'],
  ['NOTIF', 'Notifications'], ['CHK', 'Checkout'],
];
const TEAMS = ['Platform', 'Hotfix', 'Reliability', 'Migration', 'Discovery', 'Integrations'];
const PEOPLE = [
  'Amara Okafor', 'Bruno Carvalho', 'Chen Wei', 'Dagny Halvorsen', 'Elif Yilmaz', 'Farah Haddad',
  'Gabriel Moreau', 'Hana Kobayashi', 'Ivan Petrov', 'Jonas Lindqvist', 'Keira Walsh', 'Luca Romano',
  'Maya Patel', 'Nikolai Sorensen', 'Olivia Brennan', 'Priya Raman', 'Quentin Dubois', 'Rosa Jimenez',
  'Sven Aaltonen', 'Tamar Cohen', 'Uchenna Eze', 'Vera Novak', 'Wiktor Zielinski', 'Yasmin Farouk',
];
const SITE_WORDS_A = ['north', 'harbour', 'granite', 'cedar', 'copper', 'aurora', 'tidal', 'summit', 'meadow', 'atlas'];
const SITE_WORDS_B = ['works', 'labs', 'systems', 'digital', 'group', 'collective', 'tech', 'industries'];
const VERBS = ['Add', 'Fix', 'Migrate', 'Refactor', 'Document', 'Instrument', 'Harden', 'Speed up', 'Replace', 'Audit',
  'Retry', 'Paginate', 'Cache', 'Validate', 'Deprecate', 'Expose', 'Split', 'Backfill', 'Throttle', 'Localise'];
const OBJECTS = ['the invoice export', 'webhook retries', 'the settlement report', 'login rate limiting', 'the audit trail',
  'card tokenisation', 'the refund queue', 'SLA timers', 'the status page', 'feature flag cleanup', 'the CSV importer',
  'search indexing', 'session expiry', 'the onboarding wizard', 'currency rounding', 'the payout scheduler',
  'alert routing', 'the customer timeline', 'dark-mode tokens', 'the permission matrix', 'bulk edit', 'the sprint report',
  'tenant isolation', 'the backup job', 'dependency upgrades', 'the PDF renderer', 'error budgets', 'the GraphQL gateway'];
const QUALIFIERS = ['for EU tenants', 'behind a flag', 'on mobile', 'in the admin console', 'for large accounts',
  'after the outage', 'before the audit', 'in staging first', 'for the partner API', 'without downtime', '', '', ''];
const LABELS = ['tech-debt', 'customer', 'security', 'perf', 'a11y', 'incident-followup', 'compliance', 'ux'];
const DECOY_FIELDS = [
  ['Rank', 'string', 'com.pyxis.greenhopper.jira:gh-lexo-rank'],
  ['Flagged', 'array', 'com.atlassian.jira.plugin.system.customfieldtypes:multicheckboxes'],
  ['Start date', 'date', 'com.atlassian.jira.plugin.system.customfieldtypes:datepicker'],
  ['Target start', 'date', 'com.atlassian.jpo:jpo-custom-field-baseline-start'],
  ['Target end', 'date', 'com.atlassian.jpo:jpo-custom-field-baseline-end'],
  ['Team', 'team', 'com.atlassian.jira.plugin.system.customfieldtypes:atlassian-team'],
  ['Development', 'any', 'com.atlassian.jira.plugins.jira-development-integration-plugin:devsummarycf'],
  ['Category', 'option', 'com.atlassian.jira.plugin.system.customfieldtypes:select'],
  ['Original story points', 'number', 'com.atlassian.jira.plugin.system.customfieldtypes:float'],
  ['Estimate (days)', 'number', 'com.atlassian.jira.plugin.system.customfieldtypes:float'],
  ['Severity', 'option', 'com.atlassian.jira.plugin.system.customfieldtypes:select'],
  ['Root cause', 'string', 'com.atlassian.jira.plugin.system.customfieldtypes:textarea'],
  ['Customer', 'string', 'com.atlassian.jira.plugin.system.customfieldtypes:textfield'],
  ['Region', 'option', 'com.atlassian.jira.plugin.system.customfieldtypes:select'],
  ['Release notes', 'string', 'com.atlassian.jira.plugin.system.customfieldtypes:textarea'],
  ['Acceptance criteria', 'string', 'com.atlassian.jira.plugin.system.customfieldtypes:textarea'],
  ['Design', 'array', 'com.atlassian.jira.plugin.system.customfieldtypes:multiurl'],
  ['Vulnerability', 'any', 'com.atlassian.jira.plugins.jira-development-integration-plugin:vulnerabilitycf'],
  ['Sentiment', 'array', 'com.atlassian.servicedesk.sentiment:sd-sentiment'],
  ['Affected services', 'array', 'com.atlassian.jira.plugins.service-entity:service-entity-field-cftype'],
  ['Locked forms', 'number', 'com.atlassian.jira.plugins.proforma-managed-fields:forms-locked-field-cftype'],
  ['Open forms', 'number', 'com.atlassian.jira.plugins.proforma-managed-fields:forms-open-field-cftype'],
  ['Submitted forms', 'number', 'com.atlassian.jira.plugins.proforma-managed-fields:forms-submitted-field-cftype'],
  ['Total forms', 'number', 'com.atlassian.jira.plugins.proforma-managed-fields:forms-total-field-cftype'],
  ['Issue color', 'string', 'com.pyxis.greenhopper.jira:jsw-issue-color'],
  ['Change type', 'option', 'com.atlassian.jira.plugin.system.customfieldtypes:select'],
  ['Change risk', 'option', 'com.atlassian.jira.plugin.system.customfieldtypes:select'],
  ['Impact', 'option', 'com.atlassian.jira.plugin.system.customfieldtypes:select'],
  ['Urgency', 'option', 'com.atlassian.jira.plugin.system.customfieldtypes:select'],
  ['Approvals', 'sd-approvals', 'com.atlassian.servicedesk.approvals-plugin:sd-approvals'],
  ['Time to resolution', 'sd-servicelevelagreement', 'com.atlassian.servicedesk:sd-sla-field'],
];
const SYSTEM_FIELDS = [
  ['summary', 'Summary', { type: 'string', system: 'summary' }, true],
  ['status', 'Status', { type: 'status', system: 'status' }, false],
  ['issuetype', 'Issue Type', { type: 'issuetype', system: 'issuetype' }, true],
  ['project', 'Project', { type: 'project', system: 'project' }, false],
  ['labels', 'Labels', { type: 'array', items: 'string', system: 'labels' }, true],
  ['assignee', 'Assignee', { type: 'user', system: 'assignee' }, true],
  ['reporter', 'Reporter', { type: 'user', system: 'reporter' }, true],
  ['creator', 'Creator', { type: 'user', system: 'creator' }, false],
  ['created', 'Created', { type: 'datetime', system: 'created' }, false],
  ['updated', 'Updated', { type: 'datetime', system: 'updated' }, false],
  ['priority', 'Priority', { type: 'priority', system: 'priority' }, true],
  ['description', 'Description', { type: 'string', system: 'description' }, true],
  ['security', 'Security Level', { type: 'securitylevel', system: 'security' }, true],
  ['parent', 'Parent', { type: 'issuelink', system: 'parent' }, true],
  ['resolution', 'Resolution', { type: 'resolution', system: 'resolution' }, true],
  ['resolutiondate', 'Resolved', { type: 'datetime', system: 'resolutiondate' }, false],
  ['duedate', 'Due date', { type: 'date', system: 'duedate' }, true],
  ['comment', 'Comment', { type: 'comments-page', system: 'comment' }, true],
  ['issuekey', 'Key', undefined, false],
];

const iso = (ms) => new Date(ms).toISOString();

// Jira Software caps a sprint name at 30 characters; the SCORING site names one active sprint as long as that allows
// (contract: "no number is clipped or truncated (long names may end in an ellipsis)"), from the longest realistic
// form that fits.
const SPRINT_NAME_MAX = 30;
const longSprintName = (key, n) => [`${key} Reliability Hardening Sprint ${n}`, `${key} Reliability Hardening ${n}`,
  `${key} Hardening Sprint ${n} - EU`, `${key} Hardening Sprint ${n}`]
  .filter((x) => x.length <= SPRINT_NAME_MAX).sort((a, b) => b.length - a.length)[0];

// v1's own reading of a Sprint changelog item (starter/src/sync.js sprintMoves): ids in `to` not in `from` were added,
// ids in `from` not in `to` were removed.
const idList = (s) => (s ? String(s).split(',').map((x) => x.trim()).filter(Boolean) : []);
function sprintMoves(item) {
  const from = new Set(idList(item.from));
  const to = new Set(idList(item.to));
  return [...[...to].filter((id) => !from.has(id)).map((sprintId) => ({ sprintId, kind: 'added' })),
    ...[...from].filter((id) => !to.has(id)).map((sprintId) => ({ sprintId, kind: 'removed' }))];
}

function facts(seed, { scoring = false } = {}) {
  if (!SEED_RE.test(seed)) throw new Error(`seed must be 16 lowercase hex chars, got ${JSON.stringify(seed)}`);
  const r = createRng(seed);
  const now = ANCHOR + r.int(0, 60) * DAY + r.int(8 * 60, 16 * 60) * MIN + r.int(0, 59_999);

  // ---- identity -------------------------------------------------------------------------------
  const cloudId = r.uuid();
  const siteUrl = `https://${r.pick(SITE_WORDS_A)}-${r.pick(SITE_WORDS_B)}-${r.hex(4)}.atlassian.net`;
  const accountPrefix = '712020';
  const people = r.sample(PEOPLE, 8);
  const users = people.map((displayName) => ({ accountId: `${accountPrefix}:${r.uuid()}`, displayName, accountType: 'atlassian' }));
  const appAccountId = `${r.int(100000, 999999)}:${r.uuid()}`;
  const [viewerIdx, peerIdx, adminIdx] = r.sample(users.map((_, i) => i), 3);
  const viewer = users[viewerIdx].accountId;
  const peer = users[peerIdx].accountId;
  // The Jira administrator (global ADMINISTER, SPEC R5): never the viewer nor the peer.
  const admin = users[adminIdx].accountId;

  // ---- statuses, issue types, projects ----------------------------------------------------------
  // Numeric id classes are DISJOINT BY CONSTRUCTION at any scale up to 50,000 issues: boards 1-400, sprints 401-1399,
  // issue types 10000-10099, statuses 10100-10199, projects 10200-11999, the security level 12000-12999, issues from
  // 13000 (span <= 4 x issues, so below 240,000), changelogs from 1,000,000 (1.0's issue band ended at 41,000 and its
  // changelogs started at 50,000: they collided above ~2,500 issues). fixtures.test.cjs asserts the disjointness.
  const statusIds = r.distinctInts(4, 10100, 10199);
  const statuses = [
    { id: String(statusIds[1]), name: 'To Do', statusCategory: { id: 2, key: 'new', colorName: 'blue-gray', name: 'To Do' } },
    { id: String(statusIds[0]), name: 'In Progress', statusCategory: { id: 4, key: 'indeterminate', colorName: 'yellow', name: 'In Progress' } },
    { id: String(statusIds[2]), name: 'In Review', statusCategory: { id: 4, key: 'indeterminate', colorName: 'yellow', name: 'In Progress' } },
    { id: String(statusIds[3]), name: 'Done', statusCategory: { id: 3, key: 'done', colorName: 'green', name: 'Done' } },
  ];
  const typeIds = r.distinctInts(3, 10000, 10099);
  const issueTypes = [
    { id: String(typeIds[0]), name: 'Story', subtask: false, hierarchyLevel: 0 },
    { id: String(typeIds[1]), name: 'Task', subtask: false, hierarchyLevel: 0 },
    { id: String(typeIds[2]), name: 'Bug', subtask: false, hierarchyLevel: 0 },
  ];
  const projectPicks = r.sample(PROJECTS, 3);
  const projectIds = r.distinctInts(3, 10200, 11999);
  const projects = projectPicks.map(([key, name], i) => ({ id: String(projectIds[i]), key, name }));

  // ---- fields -----------------------------------------------------------------------------------
  // Custom field numbers are one distinct draw, so the app's own field (`scope-status`, created when v2 is installed)
  // never shares a number with a site field.
  const customIds = r.shuffle(r.distinctInts(4 + DECOY_FIELDS.length, 10000, 19999));
  const cf = (n) => `customfield_${n}`;
  const sprintFieldId = cf(customIds[0]);
  const speFieldId = cf(customIds[1]);
  const spFieldId = cf(customIds[2]);
  const scopeStatusFieldId = cf(customIds[3]);
  const fields = [];
  for (const [id, name, schema, orderable] of SYSTEM_FIELDS) {
    fields.push({ id, key: id, name, custom: false, orderable, navigable: true, searchable: id !== 'comment',
      clauseNames: id === 'issuekey' ? ['id', 'issue', 'issuekey', 'key'] : [id], ...(schema ? { schema } : {}) });
  }
  const custom = (n, name, type, customType, extraClause = []) => ({
    id: cf(n), key: cf(n), name, untranslatedName: name, custom: true, orderable: true, navigable: true, searchable: true,
    clauseNames: [...extraClause, `cf[${n}]`, name],
    schema: { type, ...(type === 'array' ? { items: customType.endsWith('gh-sprint') ? 'json' : 'option' } : {}), custom: customType, customId: n },
  });
  fields.push(custom(customIds[0], 'Sprint', 'array', 'com.pyxis.greenhopper.jira:gh-sprint'));
  fields.push(custom(customIds[1], 'Story point estimate', 'number', 'com.pyxis.greenhopper.jira:jsw-story-points'));
  fields.push(custom(customIds[2], 'Story Points', 'number', 'com.atlassian.jira.plugin.system.customfieldtypes:float', ['Story Points[Number]']));
  DECOY_FIELDS.forEach(([name, type, ct], i) => fields.push(custom(customIds[4 + i], name, type, ct)));
  const decoyNumberFields = fields.filter((f) => f.custom && f.schema.type === 'number' && f.id !== speFieldId && f.id !== spFieldId).map((f) => f.id);
  const estimationFields = new Set([speFieldId, spFieldId]);
  const otherEstimate = (fieldId) => (fieldId === speFieldId ? spFieldId : speFieldId);

  // ---- boards -----------------------------------------------------------------------------------
  const boardIds = r.distinctInts(BOARD_LAYOUT.length + 1, 1, 400);
  const [teamBoard, teamSprint0, teamSprint2] = r.sample(TEAMS, 3);
  // Two boards per estimation field; the two boards of the first project use different fields.
  const aOnFirst = r.chance(0.5);
  const aOnThird = r.chance(0.5);
  const fieldOf = [aOnFirst ? speFieldId : spFieldId, aOnFirst ? spFieldId : speFieldId, aOnThird ? speFieldId : spFieldId, aOnThird ? spFieldId : speFieldId];
  const scrum = BOARD_LAYOUT.map(([pi], i) => ({ id: boardIds[i], name: i === 1 ? `${projects[pi].key} ${teamBoard} board` : `${projects[pi].key} board`,
    type: 'scrum', projectKey: projects[pi].key, estimationFieldId: fieldOf[i] }));
  const kanbanProject = r.pick(projects);
  const boards = [...scrum, { id: boardIds[BOARD_LAYOUT.length], name: `${kanbanProject.key} Kanban`, type: 'kanban', projectKey: kanbanProject.key, estimationFieldId: null }];
  const fieldName = (id) => fields.find((f) => f.id === id).name;

  // ---- sprints ----------------------------------------------------------------------------------
  // Start instants are distinct (DESIGN §17.1 19), except the SCORING site's stated tie (contract: "ordered by
  // startDate (ties by sprint id)"): the first board's two parallel sprints start at the same instant.
  const starts = new Set();
  const freshStart = (t) => { while (starts.has(t)) t -= 17 * MIN; starts.add(t); return t; };
  // Active sprints start 3-9 days before now, so the v1 window (their first 2 days) closed before the upgrade.
  const activeStart = () => freshStart(now - r.int(3, 9) * DAY - r.int(0, 8) * HOUR - r.int(0, 59) * MIN);
  const mk = (name, state, boardIdx, start, complete) => ({
    name, state, originBoardId: scrum[boardIdx].id,
    startDate: start === null ? null : iso(start),
    endDate: start === null ? null : iso(start + 14 * DAY),
    completeDate: complete === null ? null : iso(complete),
    createdDate: iso((start ?? now) - r.int(2, 6) * DAY),
    _start: start, _board: boardIdx, _project: scrum[boardIdx].projectKey,
  });
  const perBoard = scrum.map(() => ({ closed: [], active: [], future: [] }));
  BOARD_LAYOUT.forEach(([pi, nA], i) => {
    const key = projects[pi].key;
    const no = r.int(12, 60);
    const label = i === 1 ? `${key} ${teamBoard} Sprint` : `${key} Sprint`;
    const first = activeStart();
    perBoard[i].no = no;
    perBoard[i].label = label;
    perBoard[i].active.push(mk(scoring && i === 2 ? longSprintName(key, no) : `${label} ${no}`, 'active', i, first, null));
    if (nA > 1) {
      const team = i === 0 ? teamSprint0 : teamSprint2;
      const start = scoring && i === 0 ? first : activeStart();
      perBoard[i].active.push(mk(`${key} ${team} Sprint ${r.int(2, 9)}`, 'active', i, start, null));
    }
  });
  const actives = perBoard.flatMap((b) => b.active);
  // Every earlier sprint completes before the first active sprint starts, so the whole pre-history precedes the
  // post-start history (per-issue changelog chains stay in creation order).
  const firstActiveStart = Math.min(...actives.map((s) => s._start));
  BOARD_LAYOUT.forEach(([, , nF, nC], i) => {
    let next = firstActiveStart;
    for (let k = nC - 1; k >= 0; k--) {
      const start = freshStart(next - r.int(2, 30) * HOUR - r.int(0, 59) * MIN - 14 * DAY);
      perBoard[i].closed[k] = mk(`${perBoard[i].label} ${perBoard[i].no - nC + k}`, 'closed', i, start, start + 14 * DAY);
      next = start;
    }
    for (let k = 0; k < nF; k++) perBoard[i].future.push(mk(`${perBoard[i].label} ${perBoard[i].no + 1 + k}`, 'future', i, null, null));
  });
  // Ids grow with creation, as on Jira: closed, then active (start order, the tie by board order), then future.
  const sprintDefs = [
    ...perBoard.flatMap((b) => b.closed).sort((a, b) => a._start - b._start),
    ...actives.map((s, i) => [s, i]).sort((a, b) => a[0]._start - b[0]._start || a[1] - b[1]).map(([s]) => s),
    ...perBoard.flatMap((b) => b.future),
  ];
  const sprintIds = r.distinctInts(sprintDefs.length, 401, 1399);
  sprintDefs.forEach((s, i) => { s.id = sprintIds[i]; });
  const sprintById = new Map(sprintDefs.map((s) => [s.id, s]));
  const boardOfSprint = (id) => scrum[sprintById.get(id)._board];

  // ---- issues -----------------------------------------------------------------------------------
  const total = r.int(960, 1040);
  const issueCreated = [];
  for (let i = 0; i < total; i++) {
    const x = r.float();
    const pi = x < PROJECT_SHARE[0] ? 0 : x < PROJECT_SHARE[0] + PROJECT_SHARE[1] ? 1 : 2;
    issueCreated.push({ project: projects[pi], pi, created: now - r.int(2 * DAY, 180 * DAY) });
  }
  issueCreated.sort((x, y) => x.created - y.created);
  const issueIdBase = r.int(13000, 40000);
  const issueIds = r.distinctInts(total, issueIdBase, issueIdBase + total * 4);
  const keyNo = Object.fromEntries(projects.map((p) => [p.key, r.int(1, 400)]));
  const securityLevel = { id: String(r.int(12000, 12999)), name: r.pick(['Restricted', 'Confidential', 'Leadership only', 'Security team']) };
  const priorities = ['Highest', 'High', 'Medium', 'Low'];
  // The vocabulary holds 20 x 28 x 11 = 6,160 distinct summaries (1.0 looped forever past them): after 20 draws a
  // repeat gets a part number, so every scale terminates.
  const usedSummaries = new Set();
  const summary = () => {
    let s = '';
    for (let tries = 0; tries < 20; tries++) {
      s = `${r.pick(VERBS)} ${r.pick(OBJECTS)} ${r.pick(QUALIFIERS)}`.trim();
      if (!usedSummaries.has(s)) { usedSummaries.add(s); return s; }
    }
    for (let n = 2; ; n++) if (!usedSummaries.has(`${s} (part ${n})`)) { usedSummaries.add(`${s} (part ${n})`); return `${s} (part ${n})`; }
  };
  const homeOf = (pi) => (pi === 0 ? (r.chance(FIRST_BOARD_SHARE) ? 0 : 1) : pi + 1);
  const issues = issueCreated.map((ic, i) => {
    keyNo[ic.project.key] += r.chance(0.08) ? r.int(2, 4) : 1;
    const home = homeOf(ic.pi);
    const field = scrum[home].estimationFieldId;
    const reporter = r.pick(users).accountId;
    return {
      id: String(issueIds[i]), key: `${ic.project.key}-${keyNo[ic.project.key]}`, projectKey: ic.project.key, home,
      created: ic.created, updated: ic.created,
      summary: summary(), type: r.chance(0.6) ? issueTypes[0] : r.chance(0.6) ? issueTypes[1] : issueTypes[2],
      status: statuses[r.int(0, 2)], labels: r.chance(0.3) ? r.sample(LABELS, r.int(1, 2)).sort() : [],
      assignee: r.chance(0.8) ? r.pick(users).accountId : null, reporter, creator: reporter,
      priority: r.pick(priorities),
      // Half the issues also carry a value in the other estimation field: the estimate a move to another board reads.
      est: { [field]: r.chance(0.85) ? r.pick(ESTIMATES) : null, [otherEstimate(field)]: r.chance(0.5) ? r.pick(ESTIMATES) : null },
      decoy: { [r.pick(decoyNumberFields)]: r.chance(0.2) ? r.pick(ESTIMATES) : null },
      sprints: [], hiddenFrom: [], commentForbiddenFor: [],
    };
  });
  const byId = new Map(issues.map((s) => [s.id, s]));

  // ---- history & live: one simulation, one changelog id stream ----------------------------------
  const humans = users.map((u) => u.accountId);
  const sprintStr = (ids) => ids.join(', ');
  const sprintNames = (ids) => ids.map((id) => sprintById.get(id).name).join(', ');
  // A sprint is open until the simulation completes it (closed sprints complete in the pre-history).
  const openSprintOf = (st) => st.sprints.find((id) => !sprintById.get(id)._done);
  // The board whose estimation field counts an issue: its open sprint's board, else its home board.
  const boardOfIssue = (st) => (openSprintOf(st) ? boardOfSprint(openSprintOf(st)) : scrum[st.home]);
  const events = [];
  // Indexes the 1.0 generator recomputed by scanning every event (O(N x E), infeasible at this scale).
  const everIn = new Map(); // issueId -> sprint ids it has ever been put into
  const record = (st, at, items, phase) => {
    const e = { changelogId: `tmp-${events.length}`, issueId: st.id, created: at, authorId: r.pick(humans), items, _phase: phase, _key: st.key };
    events.push(e);
    st.updated = Math.max(st.updated, at);
    return e;
  };
  const setSprints = (st, at, next, phase) => {
    const prev = st.sprints.slice();
    st.sprints = next;
    if (!everIn.has(st.id)) everIn.set(st.id, new Set());
    for (const id of next) everIn.get(st.id).add(id);
    return record(st, at, [{ field: 'Sprint', fieldtype: 'custom', fieldId: sprintFieldId,
      from: sprintStr(prev), fromString: sprintNames(prev), to: sprintStr(next), toString: sprintNames(next) }], phase);
  };
  const addTo = (st, sprint, at, phase) => {
    const open = openSprintOf(st);
    const base = st.sprints.filter((id) => id !== open);
    return setSprints(st, at, [...base, sprint.id], phase);
  };
  const removeOpen = (st, at, phase, target = null) => {
    const open = openSprintOf(st);
    const base = st.sprints.filter((id) => id !== open);
    return setSprints(st, at, target ? [...base, target.id] : base, phase);
  };
  const setEstimate = (st, fieldId, value, at, phase) => {
    const prev = st.est[fieldId] ?? st.decoy[fieldId] ?? null;
    if (fieldId in st.est) st.est[fieldId] = value; else st.decoy[fieldId] = value;
    const s = (v) => (v === null ? '' : String(v));
    return record(st, at, [{ field: fieldName(fieldId), fieldtype: 'custom', fieldId, from: s(prev), fromString: s(prev), to: s(value), toString: s(value) }], phase);
  };
  const irrelevant = (st, at, phase) => {
    const kind = r.pick(['status', 'labels', 'summary', 'priority', 'decoy']);
    if (kind === 'status') {
      const next = r.pick(statuses.filter((x) => x.id !== st.status.id));
      const prev = st.status;
      st.status = next;
      return record(st, at, [{ field: 'status', fieldtype: 'jira', fieldId: 'status', from: prev.id, fromString: prev.name, to: next.id, toString: next.name }], phase);
    }
    if (kind === 'labels') {
      const prev = st.labels.slice();
      const next = [...new Set([...prev, r.pick(LABELS)])].sort();
      st.labels = next;
      return record(st, at, [{ field: 'labels', fieldtype: 'jira', fieldId: 'labels', from: null, fromString: prev.join(' '), to: null, toString: next.join(' ') }], phase);
    }
    if (kind === 'priority') {
      const prev = st.priority;
      st.priority = r.pick(priorities.filter((p) => p !== prev));
      return record(st, at, [{ field: 'priority', fieldtype: 'jira', fieldId: 'priority', from: String(priorities.indexOf(prev) + 1), fromString: prev, to: String(priorities.indexOf(st.priority) + 1), toString: st.priority }], phase);
    }
    if (kind === 'decoy') {
      // A number field no board estimates with: never scope, whatever its name says.
      const fieldId = Object.keys(st.decoy)[0];
      return setEstimate(st, fieldId, r.pick(ESTIMATES.filter((v) => v !== st.decoy[fieldId])), at, phase);
    }
    const prev = st.summary;
    st.summary = summary();
    return record(st, at, [{ field: 'summary', fieldtype: 'jira', fieldId: 'summary', from: null, fromString: prev, to: null, toString: st.summary }], phase);
  };

  // The pre-history, per board in time order (the boards' teams plan disjoint issues): plan the closed sprints and
  // complete each into the next (carry-over: multi-id Sprint values), then plan the active and future sprints.
  const plan = (sprint, pool, n, from, to) => {
    const chosen = r.sample(pool.filter((s) => !openSprintOf(s) && s.created < from), n);
    const span = Math.max(1, Math.floor((to - from) / MIN) - 1);
    chosen.map((st) => [st, from + r.int(1, span) * MIN]).sort((x, y) => x[1] - y[1]).forEach(([st, at]) => addTo(st, sprint, at, 'history'));
    return chosen;
  };
  const complete = (sprint, next, carryShare) => {
    const at = Date.parse(sprint.completeDate);
    const carried = [];
    for (const st of issues.filter((s) => openSprintOf(s) === sprint.id)) {
      if (r.chance(0.55)) { st.status = statuses[3]; continue; }
      if (next && r.chance(carryShare)) { setSprints(st, at, [...st.sprints, next.id], 'history'); carried.push(st); }
    }
    sprint._done = true;
    return carried;
  };
  let carriedAtStart = 0;
  perBoard.forEach((b, i) => {
    const pool = issues.filter((s) => s.home === i);
    b.closed.forEach((c, k) => {
      const from = k === 0 ? c._start - 40 * HOUR : Date.parse(b.closed[k - 1].completeDate);
      plan(c, pool, r.int(36, 46), from, c._start);
      const carried = complete(c, b.closed[k + 1] ?? b.active[0], k === b.closed.length - 1 ? 0.85 : 0.7);
      if (k === b.closed.length - 1) carriedAtStart += carried.length;
    });
    const planFrom = Date.parse(b.closed[b.closed.length - 1].completeDate);
    plan(b.active[0], pool, r.int(30, 36), planFrom, firstActiveStart);
    // The SCORING site starts the first board's parallel sprint EMPTY, so its committed is 0 and creep is the stated
    // special case (contract: "— when committed is 0"; creepPercent null). The dev site plans it.
    const parallel = r.int(30, 36);
    if (b.active[1]) plan(b.active[1], pool, scoring && i === 0 ? 0 : parallel, planFrom, firstActiveStart);
    for (const f of b.future) plan(f, pool, r.int(10, 14), planFrom, firstActiveStart);
  });

  const activeOf = (p) => actives.filter((s) => s._project === p);
  const futureOf = (p) => perBoard.flatMap((b) => b.future).find((s) => s._project === p) ?? null;
  // One scope change (or estimate / irrelevant update) at time `at`; returns the event or null.
  const step = (at, kind, phase, focus = null) => {
    const startedActives = actives.filter((s) => s._start < at);
    if (kind === 'sprint') {
      const sprint = focus?.sprint ?? r.pick(startedActives);
      const p = sprint._project;
      const inSprint = issues.filter((s) => s.projectKey === p && s.created < at && openSprintOf(s) === sprint.id);
      const outside = issues.filter((s) => s.projectKey === p && s.created < at && openSprintOf(s) !== sprint.id);
      const move = focus?.move ?? r.pick(['add', 'add', 'add', 'remove', 'remove', 'swap', 'fromFuture', 'toFuture']);
      if (move === 'add' || move === 'fromFuture' || (move === 'swap' && activeOf(p).length < 2)) {
        const future = futureOf(p);
        const pool = move === 'fromFuture' && future
          ? outside.filter((s) => openSprintOf(s) === future.id)
          : outside.filter((s) => !openSprintOf(s) || sprintById.get(openSprintOf(s)).state !== 'active');
        const st = focus?.issue ?? r.pick(pool.length ? pool : outside);
        return st ? addTo(st, sprint, at, phase) : null;
      }
      if (move === 'swap') {
        // To another started active sprint of the project: on the first project that may be ANOTHER board's.
        const other = r.pick(activeOf(p).filter((s) => s !== sprint && s._start < at));
        const st = focus?.issue ?? r.pick(inSprint);
        return st && other ? addTo(st, other, at, phase) : null;
      }
      const st = focus?.issue ?? r.pick(inSprint);
      if (!st) return null;
      return removeOpen(st, at, phase, move === 'toFuture' ? futureOf(p) : null);
    }
    if (kind === 'estimate') {
      const pool = issues.filter((s) => s.created < at && everIn.has(s.id));
      const st = focus?.issue ?? r.pick(pool);
      const field = boardOfIssue(st).estimationFieldId;
      const fieldId = r.chance(0.2) ? otherEstimate(field) : field;
      return setEstimate(st, fieldId, r.pick(ESTIMATES.filter((v) => v !== st.est[fieldId])), at, phase);
    }
    return irrelevant(focus?.issue ?? r.pick(issues.filter((s) => s.created < at)), at, phase);
  };

  // Post-start history: sprint changes after the active sprints' starts, plus estimate and irrelevant updates, with
  // the shapes the checks need guaranteed (removals to backlog, multi-id adds).
  // A post-start change: a Sprint changelog entry that puts an issue into, or takes it out of, an active sprint after
  // THAT sprint's start (the changes the ledger records).
  const touchesStarted = (e) => e.items[0].field === 'Sprint' && actives.some((a) => e.created > a._start
    && (idList(e.items[0].from).includes(String(a.id)) || idList(e.items[0].to).includes(String(a.id))));
  const MULTI_ID_ADDS = 6;
  const target = r.int(220, 260) - MULTI_ID_ADDS;
  let postStart = 0;
  let removals = 0;
  let cursorT = firstActiveStart + MIN;
  const span = now - HOUR - firstActiveStart;
  const gap = () => Math.max(MIN, Math.floor((span / (target * 1.6)) * (0.4 + r.float() * 1.2)));
  while (postStart < target && cursorT < now - HOUR) {
    cursorT += gap();
    let at = Math.min(cursorT, now - HOUR);
    // "Strictly after startDate" (DESIGN §17.1 20): no change lands exactly on a sprint start.
    while (actives.some((a) => a._start === at)) at += SEC;
    const roll = r.float();
    const kind = roll < 0.7 ? 'sprint' : roll < 0.85 ? 'estimate' : 'irrelevant';
    let e;
    if (kind === 'sprint' && removals < 10 && r.chance(0.3)) {
      const sprint = r.pick(actives.filter((s) => s._start < at));
      const inSprint = sprint && issues.filter((s) => openSprintOf(s) === sprint.id && s.created < at);
      if (inSprint?.length) { e = removeOpen(r.pick(inSprint), at, 'history'); removals++; }
    }
    if (!e) e = step(at, kind, 'history');
    if (e && touchesStarted(e)) postStart++;
  }
  // Post-start multi-id adds: an issue still carrying a closed sprint joins an active sprint of its project.
  const withClosedOnly = issues.filter((s) => s.sprints.length && !openSprintOf(s));
  let tEnd = Math.max(events[events.length - 1].created, now - 12 * HOUR);
  for (const st of r.sample(withClosedOnly, Math.min(MULTI_ID_ADDS, withClosedOnly.length))) {
    tEnd = Math.min(tEnd + r.int(5, 50) * MIN, now - MIN);
    addTo(st, r.pick(activeOf(st.projectKey)), tEnd, 'history');
  }

  // ---- visibility: hidden from the viewer / the peer, one comment-forbidden issue --------------
  const changedInActive = (phaseFilter) => [...new Set(events.filter((e) => phaseFilter(e) && touchesStarted(e)).map((e) => e.issueId))].map((id) => byId.get(id));
  const historyChanged = changedInActive((e) => e._phase === 'history');
  const hiddenViewer = r.sample(historyChanged, r.int(4, 6));
  const hiddenPool = issues.filter((s) => !hiddenViewer.includes(s) && s.sprints.length);
  hiddenViewer.push(...r.sample(hiddenPool, r.int(8, 12) - hiddenViewer.length));
  const others = humans.filter((a) => a !== viewer && a !== peer);
  for (const st of hiddenViewer) st.hiddenFrom = [viewer, ...r.sample(others, r.int(0, 2))].sort();
  const peerPool = historyChanged.filter((s) => !hiddenViewer.includes(s));
  const hiddenPeer = r.sample(peerPool, Math.min(r.int(4, 6), peerPool.length));
  for (const st of hiddenPeer) st.hiddenFrom = [peer];
  const aFirst = perBoard[0].active[0];
  const forbiddenPool = historyChanged.filter((s) => !s.hiddenFrom.length && s.projectKey === aFirst._project
    && events.some((e) => e.issueId === s.id && e.items[0].field === 'Sprint' && e.created > aFirst._start && (idList(e.items[0].to).includes(String(aFirst.id)) || idList(e.items[0].from).includes(String(aFirst.id)))));
  const forbidden = r.pick(forbiddenPool.length ? forbiddenPool : historyChanged.filter((s) => !s.hiddenFrom.length));
  forbidden.commentForbiddenFor = [viewer];

  // Snapshot install-time state before the live script mutates the simulation.
  const installState = new Map(issues.map((s) => [s.id, JSON.parse(JSON.stringify({ est: s.est, decoy: s.decoy, sprints: s.sprints, status: s.status, labels: s.labels, summary: s.summary, priority: s.priority, updated: s.updated }))]));
  const historyCount = events.length;

  // ---- live script: the 6 scored virtual hours after the upgrade --------------------------------
  // ~200 relevant changes (Sprint moves and estimation-field updates) among ~800 irrelevant updates, ~21.5 virtual
  // seconds apart on average.
  const relevantN = r.int(190, 210);
  const irrelevantN = r.int(760, 840);
  const kinds = r.shuffle([...Array(relevantN).fill('relevant'), ...Array(irrelevantN).fill('irrelevant')]);
  const liveN = kinds.length;
  const liveStart = events.length;
  let t = now;
  const tick = () => (t += r.int(6, 36) * SEC + r.int(0, 999));
  // Same-issue consecutive pairs (delivered swapped), placed early and late; the SCORING site scales the stated
  // background faults (every fault type already occurs on the dev site, DESIGN §3 rule 3): four pairs, eight
  // duplicates, five drops, one of them an estimate change only a heal can recover (2026-10-03 stringency, F7).
  const pairAt = [r.int(20, 60), r.int(Math.floor(liveN / 2), liveN - 60)];
  if (scoring) pairAt.push(r.int(120, 180), liveN - 30);
  const pairs = [];
  const visibleHistoryIssue = (p) => issues.filter((s) => s.projectKey === p && !s.hiddenFrom.length && s.created < now);
  for (let idx = 0; idx < liveN; idx++) {
    if (pairAt.includes(idx) && pairs.length < pairAt.length) {
      const sprint = pairs.length === 0 ? aFirst : r.pick(actives.filter((a) => a !== aFirst));
      const p = sprint._project;
      const st = r.pick(visibleHistoryIssue(p).filter((s) => !openSprintOf(s)));
      const first = addTo(st, sprint, tick(), 'live');
      const other = activeOf(p).find((s) => s !== sprint);
      const second = other && r.chance(0.5) ? addTo(st, other, tick(), 'live') : removeOpen(st, tick(), 'live');
      pairs.push([first.changelogId, second.changelogId]);
      continue;
    }
    if (kinds[idx] === 'irrelevant') step(tick(), 'irrelevant', 'live');
    else step(tick(), r.chance(0.75) ? 'sprint' : 'estimate', 'live');
  }
  // Every scrum board's estimation field moves live (t_reestimate_followed). The SCORING site holds each board to it:
  // the change must be on an issue now in one of that board's active sprints, so every board's numbers move.
  for (const b of perBoard) {
    const field = scrum[b.active[0]._board].estimationFieldId;
    const ids = new Set(b.active.map((a) => a.id));
    const moves = (e) => e.items[0].fieldId === field && (!scoring || ids.has(openSprintOf(byId.get(e.issueId))));
    if (!events.slice(liveStart).some(moves)) {
      const st = r.pick(issues.filter((s) => ids.has(openSprintOf(s))));
      setEstimate(st, field, r.pick(ESTIMATES.filter((v) => v !== st.est[field])), tick(), 'live');
    }
  }
  // The live-UI slot (DESIGN §5.2, §8.7 step 8): two changes held back from the delivery script (slot null, no
  // duplicates, never dropped, `delivery.liveUi: true`), created after every scripted change, which the scorer
  // delivers while the widget shows the FIRST scrum board. Both move that board's first active sprint for the viewer:
  // a visible backlog issue with points joins it after its start (added), and a visible member's estimate changes on
  // the board's estimation field.
  const uiBoard = scrum[0];
  const uiSprint = aFirst;
  const uiField = uiBoard.estimationFieldId;
  const visibleIn = (pred) => issues.filter((s) => s.projectKey === uiBoard.projectKey && !s.hiddenFrom.includes(viewer) && s.created < now && pred(s));
  const joiner = r.pick(visibleIn((s) => !openSprintOf(s) && s.est[uiField] !== null && s.est[uiField] !== undefined));
  const liveUi = [addTo(joiner, uiSprint, tick(), 'live')];
  // The widget shows every active sprint of its board, so the re-estimated member may sit in either of them.
  const uiActive = new Set(perBoard[0].active.map((a) => a.id));
  const member = r.pick(visibleIn((s) => uiActive.has(openSprintOf(s)) && s !== joiner));
  liveUi.push(setEstimate(member, uiField, r.pick(ESTIMATES.filter((v) => v !== member.est[uiField])), tick(), 'live'));
  for (const e of liveUi) e._liveUi = true;
  // "Strictly after startDate" never meets a tie (DESIGN §17.1 20): a pre-history change that landed on another
  // sprint's start instant moves one second earlier.
  const startInstants = new Set(sprintDefs.filter((s) => s._start !== null).map((s) => s._start));
  for (const e of events) while (startInstants.has(e.created)) e.created -= SEC;
  // Changelog ids are one global sequence in creation order, as on Jira.
  let changelogNo = r.int(1_000_000, 1_500_000);
  const tmpToReal = new Map();
  events.map((e, i) => [e, i]).sort((a, b) => a[0].created - b[0].created || a[1] - b[1]).forEach(([e]) => {
    changelogNo += r.int(1, 37);
    tmpToReal.set(e.changelogId, String(changelogNo));
    e.changelogId = String(changelogNo);
  });
  for (const pair of pairs) pair.forEach((x, k) => { pair[k] = tmpToReal.get(x); });
  const byCreated = (a, b) => a.created - b.created || Number(a.changelogId) - Number(b.changelogId);
  const liveEvents = events.slice(liveStart).sort(byCreated);
  const scripted = liveEvents.filter((e) => !e._liveUi);

  // ---- delivery schedule: duplicates, permuted pairs, drops, one fault target ----------------------
  const isSprint = (e) => e.items[0].field === 'Sprint';
  const isRelevant = (e) => isSprint(e) || estimationFields.has(e.items[0].fieldId);
  const paired = new Set(pairs.flat());
  const sprintLive = scripted.filter((e) => isSprint(e) && !paired.has(e.changelogId));
  const dropped = new Set(r.sample(sprintLive, scoring ? 4 : 3).map((e) => e.changelogId));
  if (scoring) {
    // A dropped estimate change on the estimation field of the board whose active sprint counts the issue: only the
    // heal re-reads it, so the numbers stay wrong unless the scheduled run heals estimates too (prompt done 5). The
    // site applies a dropped change all the same.
    const countedOn = (e) => actives.some((a) => boardOfSprint(a.id).estimationFieldId === e.items[0].fieldId
      && (byId.get(e.issueId).sprints.includes(a.id) || everIn.get(e.issueId)?.has(a.id)));
    const own = scripted.filter((e) => !isSprint(e) && estimationFields.has(e.items[0].fieldId) && !paired.has(e.changelogId));
    const pool = own.filter(countedOn).length ? own.filter(countedOn) : own;
    if (pool.length) dropped.add(r.pick(pool).changelogId);
    else dropped.add(r.pick(sprintLive.filter((e) => !dropped.has(e.changelogId))).changelogId);
  }
  const relevantLeft = scripted.filter((e) => isRelevant(e) && !paired.has(e.changelogId) && !dropped.has(e.changelogId));
  const middle = relevantLeft.filter((e, i) => i > 2 && i < relevantLeft.length - 2 && isSprint(e) && !byId.get(e.issueId).hiddenFrom.length);
  const faultChange = r.pick(middle.length ? middle : relevantLeft.filter(isSprint));
  const duplicated = new Set(r.sample(relevantLeft.filter((e) => e !== faultChange), scoring ? 8 : 4).map((e) => e.changelogId));
  let order = scripted.map((e) => e.changelogId);
  const position = new Map(order.map((id, i) => [id, i]));
  for (const [a, b] of pairs) {
    const i = position.get(a);
    order[i] = b;
    order[i + 1] = a;
  }
  order = order.filter((id) => !dropped.has(id));
  const seq = order.map((id) => ({ id, dup: false }));
  for (const id of order.filter((x) => duplicated.has(x))) {
    const at = seq.findIndex((x) => x.id === id && !x.dup);
    seq.splice(Math.min(seq.length, at + 1 + r.int(1, 3)), 0, { id, dup: true });
  }
  const delivery = Object.fromEntries(liveEvents.map((e) => [e.changelogId, { slot: null, duplicates: [], dropped: dropped.has(e.changelogId), ...(e._liveUi ? { liveUi: true } : {}) }]));
  seq.forEach((x, slot) => {
    if (x.dup) delivery[x.id].duplicates.push(slot);
    else delivery[x.id].slot = slot;
  });

  // ---- assemble the pack ------------------------------------------------------------------------
  const userById = new Map(users.map((u) => [u.accountId, u]));
  const userObj = (accountId) => (accountId ? { accountId, displayName: userById.get(accountId).displayName } : null);
  const projectByKey = new Map(projects.map((p) => [p.key, p]));
  const sprintObj = (id) => {
    const s = sprintById.get(id);
    return { id: s.id, name: s.name, state: s.state, boardId: s.originBoardId, goal: '', startDate: s.startDate ?? undefined, endDate: s.endDate ?? undefined, ...(s.completeDate ? { completeDate: s.completeDate } : {}) };
  };
  const packIssues = issues.map((st) => {
    const at = installState.get(st.id);
    const project = projectByKey.get(st.projectKey);
    const fieldsOut = {
      summary: at.summary,
      status: at.status,
      issuetype: st.type,
      project: { id: project.id, key: st.projectKey, name: project.name },
      labels: at.labels,
      assignee: userObj(st.assignee),
      reporter: userObj(st.reporter),
      creator: userObj(st.creator),
      created: iso(st.created),
      updated: iso(at.updated),
      priority: { id: String(priorities.indexOf(at.priority) + 1), name: at.priority },
      security: st.hiddenFrom.length ? securityLevel : null,
      [sprintFieldId]: at.sprints.length ? at.sprints.map(sprintObj) : null,
      [speFieldId]: at.est[speFieldId] ?? null,
      [spFieldId]: at.est[spFieldId] ?? null,
      ...at.decoy,
    };
    return { id: st.id, key: st.key, projectKey: st.projectKey, summary: at.summary, fields: fieldsOut, hiddenFrom: st.hiddenFrom, commentForbiddenFor: st.commentForbiddenFor };
  });
  const strip = (e) => ({ changelogId: e.changelogId, issueId: e.issueId, created: iso(e.created), authorId: e.authorId, items: e.items });
  const faultId = (n) => `fault-${seed.slice(0, 6)}-${n}`;

  // The v1 rows (entity `scope-change`, forge2/starter/manifest.yml; key and row exactly as starter/src/ledger.js and
  // sync.js changeRow write them): one row per (changelog entry, active sprint) the entry moved the issue into or out
  // of, strictly after that sprint's start and within its first 2 days. v1 recorded most through its trigger
  // (`source: event`) and the rest in its hourly reconcile.
  const v1Rows = {};
  for (const e of events.slice(0, historyCount)) {
    if (!isSprint(e)) continue;
    for (const { sprintId, kind } of sprintMoves(e.items[0])) {
      const s = sprintById.get(Number(sprintId));
      if (s.state !== 'active' || !(e.created > s._start) || e.created > s._start + V1_WINDOW) continue;
      v1Rows[`${e.changelogId}:${sprintId}`] = { sprintId, changeId: e.changelogId, at: e.created, created: iso(e.created),
        issueId: e.issueId, issueKey: e._key, kind, authorId: e.authorId, authorName: userById.get(e.authorId).displayName,
        source: r.chance(0.1) ? 'reconcile' : 'event' };
    }
  }
  const inActive = packIssues.filter((i) => (i.fields[sprintFieldId] ?? []).some((s) => s.state === 'active')).length;

  return {
    seed,
    now: iso(now),
    cloudId,
    siteUrl,
    appAccountId,
    viewer,
    peer,
    admins: [admin],
    users,
    fields,
    sprintFieldId,
    // The id Jira gives the app's `scope-status` custom field when v2 is installed (SPEC R7); the field itself joins
    // the site's field list at install (state.addField).
    scopeStatusFieldId,
    projects,
    boards,
    sprints: sprintDefs.map((s) => ({ id: s.id, name: s.name, state: s.state, originBoardId: s.originBoardId,
      startDate: s.startDate, endDate: s.endDate, completeDate: s.completeDate, createdDate: s.createdDate, goal: '' })),
    statuses,
    issueTypes,
    securityLevel,
    issues: packIssues,
    history: events.slice(0, historyCount).sort(byCreated).map(strip),
    live: liveEvents.map((e) => ({ ...strip(e), delivery: delivery[e.changelogId] })),
    // The KVS v1 left behind (SPEC §2.3/§2.4), in the shape of kvs.snapshot(): v1 rows only (v1 rebuilds its config and
    // memberships from Jira on its next scheduled run).
    v1Preload: { entities: { 'scope-change': v1Rows } },
    // DESIGN §5.2 faults; the comment-path 429 was dropped in §17.2 E (the per-issue write limit still applies).
    faults: [
      { id: faultId(1), match: { scope: 'consumer-of-change', changelogId: faultChange.changelogId, nth: 1 }, status: 429, retryAfter: 30, reason: 'jira-quota-tenant-based' },
      { id: faultId(2), match: { scope: 'scheduled-run', run: 1, nth: 2 }, status: 429, retryAfter: 2, reason: 'jira-burst-based' },
      // SCORING site only (2026-10-03 stringency): the same stated faults where they bite harder — a 429 on the
      // backfill's first CONTINUATION page, a 429 in the heal (F7), and one Jira 500 on a resolver's first read, armed
      // by the probe for one extra sprint-action open (F5).
      ...(scoring ? [
        { id: faultId(3), match: { scope: 'scheduled-run', run: 1, continuation: true, nth: 1 }, status: 429, retryAfter: 2, reason: 'jira-burst-based' },
        { id: faultId(4), match: { scope: 'scheduled-run', run: 2, nth: 2 }, status: 429, retryAfter: 2, reason: 'jira-burst-based' },
        { id: faultId(5), match: { scope: 'resolver-read' }, status: 500, reason: 'internal-server-error' },
      ] : []),
    ],
    limits: LIMITS,
    ...(scoring ? { paging: SCORING_PAGING } : {}),
    stats: { issues: total, inActiveSprints: inActive, history: historyCount, carryOverAtStart: carriedAtStart,
      live: liveEvents.length, liveRelevant: liveEvents.filter(isRelevant).length, liveIrrelevant: liveEvents.filter((e) => !isRelevant(e)).length,
      v1Rows: Object.keys(v1Rows).length },
  };
}

if (require.main === module) {
  const args = process.argv.slice(2);
  const get = (name) => { const i = args.indexOf(name); return i >= 0 ? args[i + 1] : undefined; };
  const seed = get('--seed');
  const json = JSON.stringify(facts(seed, { scoring: args.includes('--scoring') }), null, 1) + '\n';
  const out = get('--out');
  if (out) fs.writeFileSync(out, json); else process.stdout.write(json);
}

module.exports = { facts, sprintMoves, V1_WINDOW };
