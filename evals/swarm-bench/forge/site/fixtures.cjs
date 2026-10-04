'use strict';
// facts(seed) -> pack: the seeded Jira site the Scope Ledger app is installed into (DESIGN.md §5.1/§5.2).
// Pure: every value comes from the seed's xorshift128+ stream; no Math.random, no wall clock.
// The ranges below are GENERATOR POLICY; the scorer reads the pack's actual values, never these.
//   node forge/site/fixtures.cjs --seed <16 hex> [--out pack.json] [--scoring]
// facts(seed, { scoring: true }) is the SCORING site's pack: the same data plus `paging`, the scoring page rule
// (limits.cjs SCORING_PAGING); the dev site's pack carries no `paging`.
const fs = require('fs');
const { createRng, SEED_RE } = require('./rng.cjs');
const { LIMITS, SCORING_PAGING } = require('./limits.cjs');

const DAY = 86_400_000;
const HOUR = 3_600_000;
const MIN = 60_000;
// Generator anchor: packs are dated after the module set the task demands went GA (2026-09-22).
const ANCHOR = Date.UTC(2026, 8, 24);
const ESTIMATES = [0.5, 1, 2, 3, 5, 8, 13];

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
// (contract §4: "no number is clipped or truncated (long names may end in an ellipsis)"), from the longest realistic
// form that fits.
const SPRINT_NAME_MAX = 30;
const longSprintName = (key, n) => [`${key} Reliability Hardening Sprint ${n}`, `${key} Reliability Hardening ${n}`,
  `${key} Hardening Sprint ${n} - EU`, `${key} Hardening Sprint ${n}`]
  .filter((x) => x.length <= SPRINT_NAME_MAX).sort((a, b) => b.length - a.length)[0];

function facts(seed, { scoring = false } = {}) {
  if (!SEED_RE.test(seed)) throw new Error(`seed must be 16 lowercase hex chars, got ${JSON.stringify(seed)}`);
  const r = createRng(seed);
  const now = ANCHOR + r.int(0, 60) * DAY + r.int(8 * 60, 16 * 60) * MIN + r.int(0, 59_999);

  // ---- identity -------------------------------------------------------------------------------
  const cloudId = r.uuid();
  const siteUrl = `https://${r.pick(SITE_WORDS_A)}-${r.pick(SITE_WORDS_B)}-${r.hex(4)}.atlassian.net`;
  const accountPrefix = '712020';
  const people = r.sample(PEOPLE, 6);
  const users = people.map((displayName) => ({ accountId: `${accountPrefix}:${r.uuid()}`, displayName, accountType: 'atlassian' }));
  const appAccountId = `${r.int(100000, 999999)}:${r.uuid()}`;
  const [viewerIdx, peerIdx] = r.sample([0, 1, 2, 3, 4, 5], 2);
  const viewer = users[viewerIdx].accountId;
  const peer = users[peerIdx].accountId;

  // ---- statuses, issue types, projects ----------------------------------------------------------
  // Numeric id classes are DISJOINT BY CONSTRUCTION (a changelog id never equals an issue, sprint or board id —
  // seed 5eed0123456789ab once collided): boards 1-400, sprints 401-1399, issue types 10000-10099, statuses
  // 10100-10199, projects 10200-11999, the security level 12000-12999, issues from 13000 (span <= 4 x issues,
  // so below 41000), changelogs from 50000. fixtures.test.cjs asserts the disjointness on 300 seeds.
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
  const projectPicks = r.sample(PROJECTS, 2);
  const projectIds = r.distinctInts(2, 10200, 11999);
  const projects = projectPicks.map(([key, name], i) => ({ id: String(projectIds[i]), key, name }));
  const [PA, PB] = projects; // PA carries the two parallel active sprints

  // ---- fields -----------------------------------------------------------------------------------
  const customIds = r.shuffle(r.distinctInts(3 + DECOY_FIELDS.length, 10000, 19999));
  const cf = (n) => `customfield_${n}`;
  const sprintFieldId = cf(customIds[0]);
  const speFieldId = cf(customIds[1]);
  const spFieldId = cf(customIds[2]);
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
  DECOY_FIELDS.forEach(([name, type, ct], i) => fields.push(custom(customIds[3 + i], name, type, ct)));
  const decoyNumberFields = fields.filter((f) => f.custom && f.schema.type === 'number' && f.id !== speFieldId && f.id !== spFieldId).map((f) => f.id);

  // ---- boards -----------------------------------------------------------------------------------
  const boardIds = r.distinctInts(3, 1, 400);
  const speOnA = r.chance(0.5);
  const kanbanProject = r.pick(projects);
  const boards = [
    { id: boardIds[0], name: `${PA.key} board`, type: 'scrum', projectKey: PA.key, estimationFieldId: speOnA ? speFieldId : spFieldId },
    { id: boardIds[1], name: `${PB.key} board`, type: 'scrum', projectKey: PB.key, estimationFieldId: speOnA ? spFieldId : speFieldId },
    { id: boardIds[2], name: `${kanbanProject.key} Kanban`, type: 'kanban', projectKey: kanbanProject.key, estimationFieldId: null },
  ];
  const boardOf = { [PA.key]: boards[0], [PB.key]: boards[1] };
  const fieldName = (id) => fields.find((f) => f.id === id).name;

  // ---- sprints ----------------------------------------------------------------------------------
  const sprintNoA = r.int(12, 60);
  const sprintNoB = r.int(5, 40);
  const team = r.pick(TEAMS);
  const a1Start = now - r.int(4, 9) * DAY - r.int(0, 8) * HOUR;
  const a2Offset = r.int(1, 3) * DAY + r.int(0, 6) * HOUR;
  // The dev site's start dates are all distinct (DESIGN §17.1 19). The SCORING site exercises the stated tie rule
  // instead (contract §4: "ordered by startDate (ties by sprint id)"): the first board's two parallel sprints start at
  // the same instant, so only the sprint id orders them (2026-10-03 stringency, F4).
  const a2Start = scoring ? a1Start : a1Start + a2Offset;
  let b1Start = now - r.int(2, 8) * DAY - r.int(0, 8) * HOUR;
  if (b1Start === a1Start || b1Start === a2Start) b1Start -= 17 * MIN;
  const mk = (key, name, state, board, start, complete) => ({
    key, name, state, originBoardId: board.id,
    startDate: start === null ? null : iso(start),
    endDate: start === null ? null : iso(start + 14 * DAY),
    completeDate: complete === null ? null : iso(complete),
    createdDate: iso((start ?? now) - r.int(2, 6) * DAY),
    _start: start, _project: board.projectKey,
  });
  // Every earlier sprint completes before the first active sprint starts.
  const firstActiveStart = Math.min(a1Start, a2Start, b1Start);
  const c2AComplete = firstActiveStart - r.int(2, 20) * HOUR;
  const c1AComplete = c2AComplete - 14 * DAY - r.int(1, 20) * HOUR;
  let c1BComplete = firstActiveStart - r.int(2, 30) * HOUR;
  if (c1BComplete === c2AComplete) c1BComplete -= 23 * MIN; // distinct closed-sprint starts too
  const sprintDefs = [
    mk('c1A', `${PA.key} Sprint ${sprintNoA - 2}`, 'closed', boards[0], c1AComplete - 14 * DAY, c1AComplete),
    mk('c1B', `${PB.key} Sprint ${sprintNoB - 1}`, 'closed', boards[1], c1BComplete - 14 * DAY, c1BComplete),
    mk('c2A', `${PA.key} Sprint ${sprintNoA - 1}`, 'closed', boards[0], c2AComplete - 14 * DAY, c2AComplete),
    mk('a1A', `${PA.key} Sprint ${sprintNoA}`, 'active', boards[0], a1Start, null),
    mk('a1B', scoring ? longSprintName(PB.key, sprintNoB) : `${PB.key} Sprint ${sprintNoB}`, 'active', boards[1], b1Start, null),
    mk('a2A', `${PA.key} ${team} Sprint ${r.int(2, 9)}`, 'active', boards[0], a2Start, null),
    mk('fA', `${PA.key} Sprint ${sprintNoA + 1}`, 'future', boards[0], null, null),
    mk('fB', `${PB.key} Sprint ${sprintNoB + 1}`, 'future', boards[1], null, null),
  ];
  const sprintIds = r.distinctInts(sprintDefs.length, 401, 1399);
  sprintDefs.forEach((s, i) => { s.id = sprintIds[i]; });
  const S = Object.fromEntries(sprintDefs.map((s) => [s.key, s]));
  const sprintById = new Map(sprintDefs.map((s) => [s.id, s]));

  // ---- issues -----------------------------------------------------------------------------------
  const total = r.int(229, 245);
  const nA = Math.round(total * (0.55 + r.float() * 0.07));
  const issueCreated = [];
  for (let i = 0; i < total; i++) issueCreated.push({ project: i < nA ? PA : PB, created: now - r.int(2 * DAY, 150 * DAY) });
  issueCreated.sort((x, y) => x.created - y.created);
  const issueIdBase = r.int(13000, 40000);
  const issueIds = r.distinctInts(total, issueIdBase, issueIdBase + total * 4);
  const keyNo = { [PA.key]: r.int(1, 400), [PB.key]: r.int(1, 400) };
  const securityLevel = { id: String(r.int(12000, 12999)), name: r.pick(['Restricted', 'Confidential', 'Leadership only', 'Security team']) };
  const priorities = ['Highest', 'High', 'Medium', 'Low'];
  const usedSummaries = new Set();
  const summary = () => {
    for (;;) {
      const s = `${r.pick(VERBS)} ${r.pick(OBJECTS)} ${r.pick(QUALIFIERS)}`.trim();
      if (!usedSummaries.has(s)) { usedSummaries.add(s); return s; }
    }
  };
  const issues = issueCreated.map((ic, i) => {
    keyNo[ic.project.key] += r.chance(0.08) ? r.int(2, 4) : 1;
    const board = boardOf[ic.project.key];
    const other = board.estimationFieldId === speFieldId ? spFieldId : speFieldId;
    const reporter = r.pick(users).accountId;
    const st = {
      id: String(issueIds[i]), key: `${ic.project.key}-${keyNo[ic.project.key]}`, projectKey: ic.project.key,
      created: ic.created, updated: ic.created,
      summary: summary(), type: r.chance(0.6) ? issueTypes[0] : r.chance(0.6) ? issueTypes[1] : issueTypes[2],
      status: statuses[r.int(0, 2)], labels: r.chance(0.3) ? r.sample(LABELS, r.int(1, 2)).sort() : [],
      assignee: r.chance(0.8) ? r.pick(users).accountId : null, reporter, creator: reporter,
      priority: r.pick(priorities),
      est: { [board.estimationFieldId]: r.chance(0.85) ? r.pick(ESTIMATES) : null, [other]: r.chance(0.12) ? r.pick(ESTIMATES) : null },
      decoy: { [r.pick(decoyNumberFields)]: r.chance(0.2) ? r.pick(ESTIMATES) : null },
      sprints: [], hiddenFrom: [], commentForbiddenFor: [],
    };
    return st;
  });
  const byId = new Map(issues.map((s) => [s.id, s]));

  // ---- history & live: one simulation, one changelog id stream ----------------------------------
  let changelogNo = r.int(50000, 90000);
  const nextChangelogId = () => String((changelogNo += r.int(1, 37)));
  const humans = users.map((u) => u.accountId);
  const sprintStr = (ids) => ids.join(', ');
  const sprintNames = (ids) => ids.map((id) => sprintById.get(id).name).join(', ');
  // A sprint is open until the simulation completes it (closed sprints complete in the pre-history).
  const openSprintOf = (st) => st.sprints.find((id) => !sprintById.get(id)._done);
  const events = [];
  const record = (st, at, items, phase) => {
    const e = { changelogId: `tmp-${events.length}`, issueId: st.id, created: at, authorId: r.pick(humans), items, _phase: phase, _key: st.key };
    events.push(e);
    st.updated = Math.max(st.updated, at);
    return e;
  };
  const setSprints = (st, at, next, phase) => {
    const prev = st.sprints.slice();
    st.sprints = next;
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
    const kind = r.pick(['status', 'labels', 'summary', 'priority']);
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
    const prev = st.summary;
    st.summary = summary();
    return record(st, at, [{ field: 'summary', fieldtype: 'jira', fieldId: 'summary', from: null, fromString: prev, to: null, toString: st.summary }], phase);
  };
  const projectIssues = (p, before) => issues.filter((s) => s.projectKey === p.key && s.created < before);

  // The pre-history, in time order: plan c1A/c1B, complete c1A into c2A, plan c2A, complete c1B and c2A
  // into the coming active sprints (carry-over: multi-id Sprint values), plan the open sprints.
  const plan = (sprint, pool, n, from, to) => {
    const chosen = r.sample(pool.filter((s) => !openSprintOf(s)), n);
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
  plan(S.c1A, projectIssues(PA, S.c1A._start), r.int(18, 24), S.c1A._start - 40 * HOUR, S.c1A._start);
  plan(S.c1B, projectIssues(PB, S.c1B._start), r.int(14, 20), S.c1B._start - 40 * HOUR, S.c1B._start);
  complete(S.c1A, S.c2A, 0.7);
  plan(S.c2A, projectIssues(PA, S.c2A._start), r.int(10, 14), c1AComplete, S.c2A._start);
  const carriedB = complete(S.c1B, S.a1B, 0.85);
  const carriedA = complete(S.c2A, S.a1A, 0.85);
  const planFrom = Math.max(c2AComplete, c1BComplete);
  const planAt = firstActiveStart;
  plan(S.a1A, projectIssues(PA, planFrom), r.int(12, 18), planFrom, planAt);
  // The SCORING site starts the team sprint EMPTY, so its committed is 0 and creep is the stated special case
  // (contract §1: "— when committed is 0"; §6: creepPercent null) — 2026-10-03 stringency, F4. The dev site plans it.
  const a2Planned = r.int(8, 12);
  plan(S.a2A, projectIssues(PA, planFrom), scoring ? 0 : a2Planned, planFrom, planAt);
  plan(S.a1B, projectIssues(PB, planFrom), r.int(12, 18), planFrom, planAt);
  plan(S.fA, projectIssues(PA, planFrom), r.int(5, 9), planFrom, planAt);
  plan(S.fB, projectIssues(PB, planFrom), r.int(4, 7), planFrom, planAt);

  const actives = [S.a1A, S.a2A, S.a1B];
  const futureOf = { [PA.key]: S.fA, [PB.key]: S.fB };
  const activeOf = (p) => actives.filter((s) => s._project === p);
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
        const pool = move === 'fromFuture'
          ? outside.filter((s) => openSprintOf(s) === futureOf[p].id)
          : outside.filter((s) => !openSprintOf(s) || sprintById.get(openSprintOf(s)).state !== 'active');
        const st = focus?.issue ?? r.pick(pool.length ? pool : outside);
        return st ? addTo(st, sprint, at, phase) : null;
      }
      if (move === 'swap') {
        const other = activeOf(p).find((s) => s !== sprint && s._start < at);
        const st = focus?.issue ?? r.pick(inSprint);
        return st && other ? addTo(st, other, at, phase) : null;
      }
      const st = focus?.issue ?? r.pick(inSprint);
      if (!st) return null;
      return removeOpen(st, at, phase, move === 'toFuture' ? futureOf[p] : null);
    }
    if (kind === 'estimate') {
      const pool = issues.filter((s) => s.created < at && actives.some((a) => s.sprints.includes(a.id) || events.some((e) => e.issueId === s.id && e.items[0].field === 'Sprint')));
      const st = focus?.issue ?? r.pick(pool);
      const board = boardOf[st.projectKey];
      const decoy = r.chance(0.2);
      const fieldId = decoy ? (board.estimationFieldId === speFieldId ? spFieldId : speFieldId) : board.estimationFieldId;
      const cur = st.est[fieldId];
      return setEstimate(st, fieldId, r.pick(ESTIMATES.filter((v) => v !== cur)), at, phase);
    }
    return irrelevant(focus?.issue ?? r.pick(issues.filter((s) => s.created < at)), at, phase);
  };

  // Post-start history: 50–70 sprint changes after the active sprints' starts, plus estimate and
  // irrelevant updates, with the shapes the checks need guaranteed (removals to backlog, multi-id adds).
  const firstStart = Math.min(...actives.map((s) => s._start));
  // A post-start change: a Sprint changelog entry that puts an issue into, or takes it out of, an active
  // sprint after THAT sprint's start (the changes the ledger records).
  const touchesStarted = (e) => e.items[0].field === 'Sprint' && actives.some((a) => e.created > a._start
    && (e.items[0].from.split(', ').includes(String(a.id)) || e.items[0].to.split(', ').includes(String(a.id))));
  const MULTI_ID_ADDS = 3;
  const target = r.int(50, 70) - MULTI_ID_ADDS;
  let postStart = events.filter(touchesStarted).length;
  let removals = 0;
  let cursorT = firstStart + MIN;
  const span = now - HOUR - firstStart;
  const gap = () => Math.max(MIN, Math.floor((span / (target * 2.2)) * (0.4 + r.float() * 1.2)));
  while (postStart < target && cursorT < now - HOUR) {
    cursorT += gap();
    let at = Math.min(cursorT, now - HOUR);
    // "Strictly after startDate" (DESIGN §17.1 20): no change lands exactly on a sprint start.
    while (actives.some((a) => a._start === at)) at += 1000;
    const roll = r.float();
    const kind = roll < 0.7 ? 'sprint' : roll < 0.85 ? 'estimate' : 'irrelevant';
    let e;
    if (kind === 'sprint' && removals < 5 && r.chance(0.3)) {
      const sprint = r.pick(actives.filter((s) => s._start < at));
      const inSprint = sprint && issues.filter((s) => openSprintOf(s) === sprint.id && s.created < at);
      if (inSprint?.length) { e = removeOpen(r.pick(inSprint), at, 'history'); removals++; }
    }
    if (!e) e = step(at, kind, 'history');
    if (e && touchesStarted(e)) postStart++;
  }
  // A post-start multi-id add: an issue still carrying a closed sprint joins an active sprint.
  const withClosedOnly = issues.filter((s) => s.sprints.length && !openSprintOf(s) && s.projectKey === PA.key);
  let tEnd = Math.max(events[events.length - 1].created, now - 12 * HOUR);
  for (const st of r.sample(withClosedOnly, Math.min(MULTI_ID_ADDS, withClosedOnly.length))) {
    tEnd = Math.min(tEnd + r.int(5, 50) * MIN, now - MIN);
    addTo(st, r.pick([S.a1A, S.a2A]), tEnd, 'history');
  }

  // ---- visibility: hidden from the viewer / the peer, one comment-forbidden issue --------------
  const changedInActive = (phaseFilter) => [...new Set(events.filter((e) => phaseFilter(e) && e.items[0].field === 'Sprint'
    && actives.some((a) => e.created > a._start && (e.items[0].from.split(', ').includes(String(a.id)) || e.items[0].to.split(', ').includes(String(a.id)))))
    .map((e) => e.issueId))].map((id) => byId.get(id));
  const historyChanged = changedInActive((e) => e._phase === 'history');
  const hiddenViewer = r.sample(historyChanged, r.int(2, 3));
  const hiddenPool = issues.filter((s) => !hiddenViewer.includes(s) && s.sprints.length);
  hiddenViewer.push(...r.sample(hiddenPool, r.int(4, 6) - hiddenViewer.length));
  const others = humans.filter((a) => a !== viewer && a !== peer);
  for (const st of hiddenViewer) st.hiddenFrom = [viewer, ...r.sample(others, r.int(0, 2))].sort();
  const peerPool = historyChanged.filter((s) => !hiddenViewer.includes(s));
  const hiddenPeer = r.sample(peerPool, Math.min(r.int(2, 3), peerPool.length));
  for (const st of hiddenPeer) st.hiddenFrom = [peer];
  const aFirst = S.a1A;
  const forbiddenPool = historyChanged.filter((s) => !s.hiddenFrom.length && s.projectKey === PA.key
    && events.some((e) => e.issueId === s.id && e.items[0].field === 'Sprint' && e.created > aFirst._start && (e.items[0].to.split(', ').includes(String(aFirst.id)) || e.items[0].from.split(', ').includes(String(aFirst.id)))));
  const forbidden = r.pick(forbiddenPool.length ? forbiddenPool : historyChanged.filter((s) => !s.hiddenFrom.length));
  forbidden.commentForbiddenFor = [viewer];

  // Snapshot install-time state before the live script mutates the simulation.
  const installState = new Map(issues.map((s) => [s.id, JSON.parse(JSON.stringify({ est: s.est, decoy: s.decoy, sprints: s.sprints, status: s.status, labels: s.labels, summary: s.summary, priority: s.priority, updated: s.updated }))]));
  const historyCount = events.length;

  // ---- live script -----------------------------------------------------------------------------
  const liveN = r.int(36, 44);
  const liveStart = events.length;
  let t = now;
  const tick = () => (t += r.int(2, 9) * MIN + r.int(0, 59_999));
  // Two same-issue consecutive pairs (delivered swapped), placed early and late. The SCORING site scales the stated
  // background faults (contract §3 "more than once and out of order, and some never arrive"; every fault type
  // already occurs on the dev site, DESIGN §3 rule 3): four pairs, eight duplicates, five drops — one of them an
  // estimate change only a heal can recover (2026-10-03 stringency, F7).
  const pairAt = [r.int(4, 9), r.int(Math.floor(liveN / 2), liveN - 6)];
  if (scoring) pairAt.push(r.int(12, 15), liveN - 4);
  const pairs = [];
  const visibleHistoryIssue = (p) => issues.filter((s) => s.projectKey === p && !s.hiddenFrom.length && s.created < now);
  while (events.length - liveStart < liveN - 2) {
    const idx = events.length - liveStart;
    if (pairAt.includes(idx) && pairs.length < pairAt.length) {
      const sprint = pairs.length === 0 ? S.a1A : r.pick([S.a1B, S.a2A]);
      const p = sprint._project;
      const st = r.pick(visibleHistoryIssue(p).filter((s) => !openSprintOf(s)));
      const first = addTo(st, sprint, tick(), 'live');
      const other = activeOf(p).find((s) => s !== sprint);
      const second = other && r.chance(0.5) ? addTo(st, other, tick(), 'live') : removeOpen(st, tick(), 'live');
      pairs.push([first.changelogId, second.changelogId]);
      continue;
    }
    const roll = r.float();
    const kind = roll < 0.6 ? 'sprint' : roll < 0.75 ? 'estimate' : 'irrelevant';
    step(tick(), kind, 'live');
  }
  // The last two updates make sure both estimation fields move live (t_reestimate_followed).
  for (const p of [PA, PB]) {
    const field = boardOf[p.key].estimationFieldId;
    if (!events.slice(liveStart).some((e) => e.items[0].fieldId === field)) {
      const st = r.pick(issues.filter((s) => s.projectKey === p.key && activeOf(p.key).some((a) => openSprintOf(s) === a.id)));
      setEstimate(st, field, r.pick(ESTIMATES.filter((v) => v !== st.est[field])), tick(), 'live');
    } else {
      step(tick(), r.pick(['sprint', 'irrelevant']), 'live');
    }
  }
  // The live-UI slot (DESIGN §5.2, §8.7 step 8): two changes held back from the delivery script (slot null, no
  // duplicates, never dropped, `delivery.liveUi: true`), created after every scripted change, which the scorer
  // delivers while the widget shows the FIRST scrum board (forge_probe.mjs probeUi: the first `type: scrum` board in
  // pack order, its light 380 px view). Both move that board's first active sprint for the viewer: a visible
  // backlog issue with points joins it after its start (added), and a visible member's estimate changes on the
  // board's estimation field.
  const uiBoard = boards.find((b) => b.type === 'scrum');
  const uiSprint = actives.find((s) => s.originBoardId === uiBoard.id);
  const uiField = uiBoard.estimationFieldId;
  const visibleIn = (pred) => issues.filter((s) => s.projectKey === uiBoard.projectKey && !s.hiddenFrom.includes(viewer) && s.created < now && pred(s));
  const joiner = r.pick(visibleIn((s) => !openSprintOf(s) && s.est[uiField] !== null && s.est[uiField] !== undefined));
  const liveUi = [addTo(joiner, uiSprint, tick(), 'live')];
  // The widget shows every active sprint of its board, so the re-estimated member may sit in either of them.
  const uiActive = new Set(actives.filter((a) => a.originBoardId === uiBoard.id).map((a) => a.id));
  const member = r.pick(visibleIn((s) => uiActive.has(openSprintOf(s)) && s !== joiner));
  liveUi.push(setEstimate(member, uiField, r.pick(ESTIMATES.filter((v) => v !== member.est[uiField])), tick(), 'live'));
  for (const e of liveUi) e._liveUi = true;
  // "Strictly after startDate" never meets a tie (DESIGN §17.1 20): a pre-history change that landed on
  // another sprint's start instant moves one second earlier (plan events for a sprint are already
  // strictly before its own start; post-start events are nudged later above).
  const startInstants = new Set(sprintDefs.filter((s) => s._start !== null).map((s) => s._start));
  for (const e of events) while (startInstants.has(e.created)) e.created -= 1000;
  // Changelog ids are one global sequence in creation order, as on Jira.
  const tmpToReal = new Map();
  events.map((e, i) => [e, i]).sort((a, b) => a[0].created - b[0].created || a[1] - b[1]).forEach(([e]) => {
    const id = nextChangelogId();
    tmpToReal.set(e.changelogId, id);
    e.changelogId = id;
  });
  for (const pair of pairs) pair.forEach((x, k) => { pair[k] = tmpToReal.get(x); });
  const byCreated = (a, b) => a.created - b.created || Number(a.changelogId) - Number(b.changelogId);
  const liveEvents = events.slice(liveStart).sort(byCreated);
  const scripted = liveEvents.filter((e) => !e._liveUi);

  // ---- delivery schedule: 4 duplicates, 2 permuted pairs, 3 dropped, one fault target ----------
  const isSprint = (e) => e.items[0].field === 'Sprint';
  const paired = new Set(pairs.flat());
  const sprintLive = scripted.filter((e) => isSprint(e) && !paired.has(e.changelogId));
  const isBoardEstimate = (e) => e.items[0].fieldId === boardOf[byId.get(e.issueId).projectKey].estimationFieldId;
  const dropped = new Set(r.sample(sprintLive, scoring ? 4 : 3).map((e) => e.changelogId));
  if (scoring) {
    // A dropped estimate change on a board's own estimation field, of an issue an active sprint counts: only the
    // heal re-reads it, so the numbers stay wrong unless the scheduled run heals estimates too (prompt done 5).
    const counted = (e) => actives.some((a) => byId.get(e.issueId).sprints.includes(a.id)
      || events.some((h) => h.issueId === e.issueId && h.items[0].field === 'Sprint' && h.items[0].to.split(', ').includes(String(a.id))));
    // The site applies a dropped change all the same, so the numbers (and t_reestimate_followed, read after the heal)
    // still move: only an app whose heal refreshes estimates shows them.
    const own = scripted.filter((e) => isBoardEstimate(e) && !paired.has(e.changelogId));
    const pool = own.filter(counted).length ? own.filter(counted) : own;
    // A script with no estimate change on a board's own field (1 seed in 300) drops a fifth sprint change instead.
    if (pool.length) dropped.add(r.pick(pool).changelogId);
    else dropped.add(r.pick(sprintLive.filter((e) => !dropped.has(e.changelogId))).changelogId);
  }
  const relevantLeft = scripted.filter((e) => (isSprint(e) || isBoardEstimate(e))
    && !paired.has(e.changelogId) && !dropped.has(e.changelogId));
  const middle = relevantLeft.filter((e, i) => i > 2 && i < relevantLeft.length - 2 && isSprint(e) && !byId.get(e.issueId).hiddenFrom.length);
  const faultChange = r.pick(middle.length ? middle : relevantLeft.filter(isSprint));
  const duplicated = new Set(r.sample(relevantLeft.filter((e) => e !== faultChange), scoring ? 8 : 4).map((e) => e.changelogId));
  let order = scripted.map((e) => e.changelogId);
  for (const [a, b] of pairs) {
    const i = order.indexOf(a);
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
  const userObj = (accountId) => (accountId ? { accountId, displayName: users.find((u) => u.accountId === accountId).displayName } : null);
  const sprintObj = (id) => {
    const s = sprintById.get(id);
    return { id: s.id, name: s.name, state: s.state, boardId: s.originBoardId, goal: '', startDate: s.startDate ?? undefined, endDate: s.endDate ?? undefined, ...(s.completeDate ? { completeDate: s.completeDate } : {}) };
  };
  const packIssues = issues.map((st) => {
    const at = installState.get(st.id);
    const fieldsOut = {
      summary: at.summary,
      status: at.status,
      issuetype: st.type,
      project: { id: projects.find((p) => p.key === st.projectKey).id, key: st.projectKey, name: projects.find((p) => p.key === st.projectKey).name },
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
      ...Object.fromEntries(Object.entries(at.decoy).map(([k, v]) => [k, v])),
    };
    return { id: st.id, key: st.key, projectKey: st.projectKey, summary: at.summary, fields: fieldsOut, hiddenFrom: st.hiddenFrom, commentForbiddenFor: st.commentForbiddenFor };
  });
  const strip = (e) => ({ changelogId: e.changelogId, issueId: e.issueId, created: iso(e.created), authorId: e.authorId, items: e.items });
  const faultId = (n) => `fault-${seed.slice(0, 6)}-${n}`;
  return {
    seed,
    now: iso(now),
    cloudId,
    siteUrl,
    appAccountId,
    viewer,
    peer,
    users,
    fields,
    sprintFieldId,
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
    // DESIGN §5.2 faults; the comment-path 429 was dropped in §17.2 E (the per-issue write limit still applies).
    faults: [
      { id: faultId(1), match: { scope: 'consumer-of-change', changelogId: faultChange.changelogId, nth: 1 }, status: 429, retryAfter: 30, reason: 'jira-quota-tenant-based' },
      { id: faultId(2), match: { scope: 'scheduled-run', run: 1, nth: 2 }, status: 429, retryAfter: 2, reason: 'jira-burst-based' },
      // SCORING site only (2026-10-03 stringency): the same stated faults where they bite harder — a 429 on the
      // backfill's first CONTINUATION page (the paging loop must retry that page, not restart or skip it), a 429 in
      // the heal (F7, contract §3 "Jira may answer 429 with Retry-After"), and one Jira 500 on a resolver's first read,
      // armed by the probe for one extra sprint-action open (F5, contract §2 "a failure (a Jira error, …) returns a
      // value describing it … and the surface shows it").
      ...(scoring ? [
        { id: faultId(3), match: { scope: 'scheduled-run', run: 1, continuation: true, nth: 1 }, status: 429, retryAfter: 2, reason: 'jira-burst-based' },
        { id: faultId(4), match: { scope: 'scheduled-run', run: 2, nth: 2 }, status: 429, retryAfter: 2, reason: 'jira-burst-based' },
        { id: faultId(5), match: { scope: 'resolver-read' }, status: 500, reason: 'internal-server-error' },
      ] : []),
    ],
    limits: LIMITS,
    ...(scoring ? { paging: SCORING_PAGING } : {}),
    stats: { issues: total, history: historyCount, carryOverAtStart: carriedA.length + carriedB.length, live: liveEvents.length },
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

module.exports = { facts };
