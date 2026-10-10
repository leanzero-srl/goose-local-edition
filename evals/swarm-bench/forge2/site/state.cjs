'use strict';
// The mutable Jira site built from a pack: issues, change histories, comments, sprints, boards, fields, browse
// revocations, the virtual clock and the live-update cursor. Live changes are applied in CREATION order whatever the
// delivery order, so the site state is always the truth the delivery schedule (duplicates, permuted pairs, drops)
// hides. The world changes mid-run (SPEC §2.5) through the mutation API at the bottom (site-world drives it); every
// mutation is appended to `st.world` and bumps `st.version`, which keys the search cache.
// The clock is VIRTUAL: it moves only through advance/advanceTo, applied live changes and world mutations.

const clone = (x) => JSON.parse(JSON.stringify(x));
const iso = (ms) => new Date(ms).toISOString();

function deliveryPlan(pack) {
  const seq = [];
  for (const c of pack.live) {
    if (c.delivery.slot !== null) seq.push({ slot: c.delivery.slot, changelogId: c.changelogId, duplicate: false });
    for (const s of c.delivery.duplicates) seq.push({ slot: s, changelogId: c.changelogId, duplicate: true });
  }
  return seq.sort((a, b) => a.slot - b.slot);
}

// The deliveries the drive leaves queued together (SPEC §2.8 S1: deliveries of one event, or of one issue, run
// concurrently only while both are pending): a duplicate with every delivery since its original, and the two halves of
// a permuted same-issue pair (the later change delivered first). Per plan index, the index of its batch's last delivery
// (null outside every batch). Derived from the plan alone, with no draw, so every pack stays as it was.
function deliveryBatches(plan, liveById) {
  const spans = [];
  const original = new Map();
  let previous = null;
  plan.forEach((d, i) => {
    if (d.duplicate) {
      if (original.has(d.changelogId)) spans.push([original.get(d.changelogId), i]);
      return;
    }
    original.set(d.changelogId, i);
    const a = previous === null ? null : liveById.get(plan[previous].changelogId);
    const b = liveById.get(d.changelogId);
    if (a && a.issueId === b.issueId && Date.parse(a.created) > Date.parse(b.created)) spans.push([previous, i]);
    previous = i;
  });
  const end = plan.map(() => null);
  let open = null;
  const close = () => { if (open) for (let k = open[0]; k <= open[1]; k++) end[k] = open[1]; };
  for (const s of spans.sort((x, y) => x[0] - y[0] || x[1] - y[1])) {
    if (open && s[0] <= open[1]) open[1] = Math.max(open[1], s[1]);
    else { close(); open = [...s]; }
  }
  close();
  return end;
}

function createState(pack) {
  const base = Date.parse(pack.now);
  const statusById = new Map(pack.statuses.map((s) => [s.id, s]));
  const userById = new Map(pack.users.map((u) => [u.accountId, u]));
  const liveOrder = pack.live.map((c) => c.changelogId);
  const liveIndex = new Map(liveOrder.map((id, i) => [id, i]));
  const liveById = new Map(pack.live.map((c) => [c.changelogId, c]));
  const plan = deliveryPlan(pack);
  const batchEnd = deliveryBatches(plan, liveById);
  // Changelog entries the world writes take ids above every pack id: unique, and increasing among themselves.
  const worldIdBase = (Math.floor(Math.max(0, ...[...pack.history, ...pack.live].map((h) => Number(h.changelogId))) / 1_000_000) + 1) * 1_000_000;

  const st = {};
  let fieldById;
  const reset = () => {
    st.skipped = 0;
    st.issues = new Map(pack.issues.map((i) => [i.id, { id: i.id, key: i.key, projectKey: i.projectKey, fields: clone(i.fields), hiddenFrom: i.hiddenFrom, commentForbiddenFor: i.commentForbiddenFor }]));
    st.byKey = new Map([...st.issues.values()].map((i) => [i.key, i]));
    st.histories = new Map([...st.issues.keys()].map((id) => [id, []]));
    for (const h of pack.history) st.histories.get(h.issueId).push(clone(h));
    st.comments = [];
    st.commentSeq = 0;
    st.applied = new Set();
    st.cursor = 0;
    st.sprints = new Map(pack.sprints.map((s) => [s.id, clone(s)]));
    st.boards = new Map(pack.boards.map((b) => [b.id, clone(b)]));
    st.fields = clone(pack.fields);
    fieldById = new Map(st.fields.map((f) => [f.id, f]));
    st.revoked = new Map(); // accountId -> Set of project keys the person may no longer browse
    st.deleted = new Map(); // issueId -> { issue, at }
    st.world = [];
    st.worldSeq = 0;
    st.version = 0;
  };
  reset();

  const now = () => base + st.skipped;
  const advance = (ms) => { if (ms > 0) { st.skipped += ms; st.version++; } return now(); };
  const advanceTo = (t) => { const n = now(); if (t > n) { st.skipped += t - n; st.version++; } return now(); };

  // Results computed from the state (search hits, board lists) are reused until the state or the clock changes, so a
  // page walk does not re-filter and re-sort every issue per page.
  let cache = { version: -1, map: new Map() };
  const cached = (key, compute) => {
    if (cache.version !== st.version) cache = { version: st.version, map: new Map() };
    if (!cache.map.has(key)) cache.map.set(key, compute());
    return cache.map.get(key);
  };

  const sprintObj = (id) => {
    const s = st.sprints.get(Number(id));
    if (!s) throw new Error(`no sprint ${id}`);
    return { id: s.id, name: s.name, state: s.state, boardId: s.originBoardId, goal: s.goal ?? '',
      ...(s.startDate ? { startDate: s.startDate, endDate: s.endDate } : {}), ...(s.completeDate ? { completeDate: s.completeDate } : {}) };
  };
  const isNumberField = (id) => fieldById.get(id)?.custom === true && fieldById.get(id).schema?.type === 'number';
  const applyItem = (issue, item) => {
    const f = issue.fields;
    if (item.fieldId === pack.sprintFieldId) {
      const ids = item.to ? item.to.split(',').map((x) => x.trim()).filter(Boolean) : [];
      f[pack.sprintFieldId] = ids.length ? ids.map(sprintObj) : null;
    } else if (isNumberField(item.fieldId)) {
      f[item.fieldId] = item.to === '' || item.to === null ? null : Number(item.to);
    } else if (item.fieldId === 'status') {
      f.status = clone(statusById.get(item.to));
    } else if (item.fieldId === 'labels') {
      f.labels = item.toString ? item.toString.split(' ').filter(Boolean) : [];
    } else if (item.fieldId === 'summary') {
      f.summary = item.toString;
    } else if (item.fieldId === 'priority') {
      f.priority = { id: item.to, name: item.toString };
    } else {
      throw new Error(`site cannot apply changelog item for field ${item.fieldId}`);
    }
  };
  // The world's scheduled events (world.cjs createWorld.applyDue), set by site.cjs: called with a change's creation
  // instant before the change applies, so world events and human edits interleave in creation order.
  let worldDue = null;
  const setWorldHook = (fn) => { worldDue = fn; };
  // Apply every not-yet-applied live change created up to and including `changelogId`.
  const applyThrough = (changelogId) => {
    const upto = liveIndex.get(changelogId);
    if (upto === undefined) throw new Error(`unknown live change ${changelogId}`);
    const appliedNow = [];
    for (let i = 0; i <= upto; i++) {
      const id = liveOrder[i];
      if (st.applied.has(id)) continue;
      const c = liveById.get(id);
      worldDue?.(Date.parse(c.created));
      const issue = st.issues.get(c.issueId);
      advanceTo(Date.parse(c.created));
      for (const item of c.items) applyItem(issue, item);
      issue.fields.updated = c.created;
      st.histories.get(c.issueId).push({ changelogId: c.changelogId, issueId: c.issueId, created: c.created, authorId: c.authorId, items: clone(c.items) });
      st.applied.add(id);
      st.version++;
      appliedNow.push(id);
    }
    return appliedNow;
  };
  // Every scripted live change (dropped ones included) — never the live-UI changes, which stay held back until the
  // scorer delivers them with the widget open (DESIGN §8.7 step 8); they are created last, so nothing scripted
  // waits behind them.
  const scriptedOrder = pack.live.filter((c) => !c.delivery.liveUi).map((c) => c.changelogId);
  const flush = () => {
    const applied = scriptedOrder.length ? applyThrough(scriptedOrder[scriptedOrder.length - 1]) : [];
    if (pack.world) worldDue?.(Date.parse(pack.world.window.end));
    return applied;
  };
  // The site as Jira stands at `t`: every scripted live change created by then (in creation order) and the world
  // events due by then. A change applies whatever its delivery: the delivery schedule only hides it from the app.
  const applyUntil = (t) => {
    let last = null;
    for (const id of scriptedOrder) if (Date.parse(liveById.get(id).created) <= t) last = id;
    const applied = last ? applyThrough(last) : [];
    worldDue?.(t);
    return applied;
  };
  const nextDelivery = () => {
    if (st.cursor >= plan.length) return null;
    const i = st.cursor++;
    const d = plan[i];
    const applied = applyThrough(d.changelogId);
    return { ...d, batchEnd: batchEnd[i], applied, change: liveById.get(d.changelogId), issue: st.issues.get(liveById.get(d.changelogId).issueId), remaining: plan.length - st.cursor };
  };

  const canBrowse = (accountId, issue) => accountId === pack.appAccountId
    || (!issue.hiddenFrom.includes(accountId) && !st.revoked.get(accountId)?.has(issue.projectKey));

  // ---- the world mutation API (SPEC §2.5). Each takes the virtual instant `at` (default: now) and returns what
  // changed; each refuses loudly what Jira could not do. Changelog entries are written as Jira writes them. ----------
  const mustIssue = (ref) => {
    const issue = st.issues.get(String(ref)) ?? st.byKey.get(String(ref).toUpperCase());
    if (!issue) throw new Error(`no issue ${ref}`);
    return issue;
  };
  const mustSprint = (id) => {
    const s = st.sprints.get(Number(id));
    if (!s) throw new Error(`no sprint ${id}`);
    return s;
  };
  const mustAuthor = (authorId) => {
    if (!userById.has(authorId)) throw new Error(`a changelog entry needs an author among the site's users, got ${JSON.stringify(authorId)}`);
    return authorId;
  };
  const sprintIds = (issue) => (issue.fields[pack.sprintFieldId] ?? []).map((s) => s.id);
  const sprintItem = (prev, next) => ({ field: 'Sprint', fieldtype: 'custom', fieldId: pack.sprintFieldId,
    from: prev.join(', '), fromString: prev.map((id) => mustSprint(id).name).join(', '),
    to: next.join(', '), toString: next.map((id) => mustSprint(id).name).join(', ') });
  const numberItem = (fieldId, prev, value) => {
    const s = (v) => (v === null || v === undefined ? '' : String(v));
    return { field: fieldById.get(fieldId).name, fieldtype: 'custom', fieldId, from: s(prev), fromString: s(prev), to: s(value), toString: s(value) };
  };
  const writeChange = (issue, t, authorId, items) => {
    const h = { changelogId: String(worldIdBase + ++st.worldSeq), issueId: issue.id, created: iso(t), authorId: mustAuthor(authorId), items };
    for (const item of items) applyItem(issue, item);
    issue.fields.updated = h.created;
    st.histories.get(issue.id).push(h);
    return clone(h);
  };
  const record = (type, t, details) => {
    const entry = { type, at: iso(t), ...details };
    st.world.push(entry);
    st.version++;
    return entry;
  };
  // Live changes of an issue the site has not applied yet (an issue with any left cannot be deleted).
  const liveRemainingFor = (issueId) => pack.live.filter((c) => c.issueId === String(issueId) && !st.applied.has(c.changelogId)).length;

  // The issue leaves its open sprint for `toSprintId` (Jira keeps closed sprints in the value). `estimate`, when given,
  // also sets the destination board's estimation field in the same changelog entry.
  const moveIssue = (ref, toSprintId, { at = now(), authorId, estimate } = {}) => {
    const issue = mustIssue(ref);
    const to = mustSprint(toSprintId);
    if (to.state === 'closed') throw new Error(`sprint ${to.id} is closed`);
    const prev = sprintIds(issue);
    const open = prev.find((id) => mustSprint(id).state !== 'closed') ?? null;
    if (open === to.id) throw new Error(`issue ${issue.key} is already in sprint ${to.id}`);
    const t = advanceTo(at);
    const items = [sprintItem(prev, [...prev.filter((id) => id !== open), to.id])];
    if (estimate !== undefined) {
      const fieldId = st.boards.get(to.originBoardId).estimationFieldId;
      items.push(numberItem(fieldId, issue.fields[fieldId], estimate));
    }
    const change = writeChange(issue, t, authorId, items);
    return record('moveIssue', t, { issueId: issue.id, issueKey: issue.key, fromSprintId: open, toSprintId: to.id, change });
  };
  // Jira answers 404 for a deleted issue everywhere and leaves it out of every search and list.
  const deleteIssue = (ref, { at = now() } = {}) => {
    const issue = mustIssue(ref);
    const pending = liveRemainingFor(issue.id);
    if (pending) throw new Error(`issue ${issue.key} still has ${pending} live change(s) to apply; delete an issue whose live script is done`);
    const t = advanceTo(at);
    st.issues.delete(issue.id);
    st.byKey.delete(issue.key);
    st.deleted.set(issue.id, { issue, at: iso(t) });
    return record('deleteIssue', t, { issueId: issue.id, issueKey: issue.key, projectKey: issue.projectKey, issue: clone(issue) });
  };
  // The sprint completes: state closed, completeDate set. With `carryTo`, each incomplete issue gets the Sprint change
  // Jira writes (the closed sprint stays in the value: "S" -> "S, T"); without it they stay (the backlog).
  const closeSprint = (sprintId, { at = now(), carryTo = null, authorId } = {}) => {
    const s = mustSprint(sprintId);
    if (s.state !== 'active') throw new Error(`sprint ${s.id} is ${s.state}, not active`);
    const target = carryTo === null ? null : mustSprint(carryTo);
    if (target && target.state === 'closed') throw new Error(`cannot carry issues into closed sprint ${target.id}`);
    if (target) mustAuthor(authorId);
    const t = advanceTo(at);
    s.state = 'closed';
    s.completeDate = iso(t);
    const carried = [];
    for (const issue of st.issues.values()) {
      const ids = sprintIds(issue);
      if (!ids.includes(s.id)) continue;
      if (target && issue.fields.status.statusCategory.key !== 'done' && !ids.includes(target.id)) carried.push(writeChange(issue, t, authorId, [sprintItem(ids, [...ids, target.id])]));
      else issue.fields[pack.sprintFieldId] = ids.map(sprintObj);
    }
    return record('closeSprint', t, { sprintId: s.id, carryTo: target ? target.id : null, carried });
  };
  // Changes after the switch are estimated with the new field; nothing about earlier changes moves.
  const setBoardEstimationField = (boardId, fieldId, { at = now() } = {}) => {
    const b = st.boards.get(Number(boardId));
    if (!b || b.type !== 'scrum') throw new Error(`no scrum board ${boardId}`);
    if (!isNumberField(fieldId)) throw new Error(`${fieldId} is not a number custom field of this site`);
    if (b.estimationFieldId === fieldId) throw new Error(`board ${b.id} already estimates with ${fieldId}`);
    const t = advanceTo(at);
    const from = b.estimationFieldId;
    b.estimationFieldId = fieldId;
    return record('setBoardEstimationField', t, { boardId: b.id, from, to: fieldId });
  };
  // From `at` on, the person can browse no issue of the project (404 / omitted, like an issue hidden from them).
  const revokeBrowse = (accountId, projectKey, { at = now() } = {}) => {
    if (!userById.has(accountId)) throw new Error(`no user ${accountId}`);
    if (!pack.projects.some((p) => p.key === projectKey)) throw new Error(`no project ${projectKey}`);
    const t = advanceTo(at);
    if (!st.revoked.has(accountId)) st.revoked.set(accountId, new Set());
    st.revoked.get(accountId).add(projectKey);
    return record('revokeBrowse', t, { accountId, projectKey });
  };
  // The app's own custom field joins the field list when the app is installed (SPEC R7: id pack.scopeStatusFieldId).
  const addField = (def) => {
    if (!def?.id || !def.name) throw new Error('a field needs an id and a name');
    if (fieldById.has(def.id)) throw new Error(`field ${def.id} exists`);
    const f = clone(def);
    st.fields.push(f);
    fieldById.set(f.id, f);
    return record('addField', now(), { fieldId: f.id });
  };
  const setFieldValue = (ref, fieldId, value) => {
    const issue = mustIssue(ref);
    if (!fieldById.has(fieldId)) throw new Error(`no field ${fieldId}`);
    issue.fields[fieldId] = value;
    st.version++;
    return { issueId: issue.id, fieldId, value };
  };

  return {
    pack, st, plan, batchEnd, statusById, userById,
    now, advance, advanceTo, reset, applyThrough, applyUntil, flush, nextDelivery, cached, setWorldHook,
    issueByIdOrKey: (k) => st.issues.get(String(k)) ?? st.byKey.get(String(k).toUpperCase()),
    allIssues: () => [...st.issues.values()],
    sprints: () => [...st.sprints.values()],
    sprint: (id) => st.sprints.get(Number(id)),
    boards: () => [...st.boards.values()],
    board: (id) => st.boards.get(Number(id)),
    fields: () => st.fields,
    field: (id) => fieldById.get(id),
    canBrowse,
    canBrowseProject: (accountId, projectKey) => accountId === pack.appAccountId || !st.revoked.get(accountId)?.has(projectKey),
    canComment: (accountId, issue) => accountId === pack.appAccountId || (canBrowse(accountId, issue) && !issue.commentForbiddenFor.includes(accountId)),
    liveRemainingFor, moveIssue, deleteIssue, closeSprint, setBoardEstimationField, revokeBrowse, addField, setFieldValue,
  };
}

module.exports = { createState, deliveryPlan, deliveryBatches };
