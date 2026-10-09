'use strict';
// The mutable Jira site built from a pack: issues, change histories, comments, the virtual clock and
// the live-update cursor. Live changes are applied in CREATION order whatever the delivery order, so
// the site state is always the truth the delivery schedule (duplicates, permuted pairs, drops) hides.

const clone = (x) => JSON.parse(JSON.stringify(x));

function deliveryPlan(pack) {
  const seq = [];
  for (const c of pack.live) {
    if (c.delivery.slot !== null) seq.push({ slot: c.delivery.slot, changelogId: c.changelogId, duplicate: false });
    for (const s of c.delivery.duplicates) seq.push({ slot: s, changelogId: c.changelogId, duplicate: true });
  }
  return seq.sort((a, b) => a.slot - b.slot);
}

function createState(pack, { realNow = () => Date.now() } = {}) {
  const base = Date.parse(pack.now);
  const sprintById = new Map(pack.sprints.map((s) => [s.id, s]));
  const statusById = new Map(pack.statuses.map((s) => [s.id, s]));
  const userById = new Map(pack.users.map((u) => [u.accountId, u]));
  const liveOrder = pack.live.map((c) => c.changelogId);
  const liveById = new Map(pack.live.map((c) => [c.changelogId, c]));
  const plan = deliveryPlan(pack);
  const estimateFields = new Set(pack.fields.filter((f) => f.custom && f.schema?.type === 'number').map((f) => f.id));

  const st = {};
  const reset = () => {
    st.realStart = realNow();
    st.skipped = 0;
    st.issues = new Map(pack.issues.map((i) => [i.id, { id: i.id, key: i.key, projectKey: i.projectKey, fields: clone(i.fields), hiddenFrom: i.hiddenFrom, commentForbiddenFor: i.commentForbiddenFor }]));
    st.byKey = new Map([...st.issues.values()].map((i) => [i.key, i]));
    st.histories = new Map([...st.issues.keys()].map((id) => [id, []]));
    for (const h of pack.history) st.histories.get(h.issueId).push(clone(h));
    st.comments = [];
    st.commentSeq = 0;
    st.applied = new Set();
    st.cursor = 0;
  };
  reset();

  const now = () => base + (realNow() - st.realStart) + st.skipped;
  const advance = (ms) => { if (ms > 0) st.skipped += ms; return now(); };
  const advanceTo = (t) => { const n = now(); if (t > n) st.skipped += t - n; return now(); };

  const sprintObj = (id) => {
    const s = sprintById.get(Number(id));
    return { id: s.id, name: s.name, state: s.state, boardId: s.originBoardId, goal: s.goal ?? '',
      ...(s.startDate ? { startDate: s.startDate, endDate: s.endDate } : {}), ...(s.completeDate ? { completeDate: s.completeDate } : {}) };
  };
  const applyItem = (issue, item) => {
    const f = issue.fields;
    if (item.fieldId === pack.sprintFieldId) {
      const ids = item.to ? item.to.split(',').map((x) => x.trim()).filter(Boolean) : [];
      f[pack.sprintFieldId] = ids.length ? ids.map(sprintObj) : null;
    } else if (estimateFields.has(item.fieldId)) {
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
  // Apply every not-yet-applied live change created up to and including `changelogId`.
  const applyThrough = (changelogId) => {
    const upto = liveOrder.indexOf(changelogId);
    if (upto < 0) throw new Error(`unknown live change ${changelogId}`);
    const appliedNow = [];
    for (let i = 0; i <= upto; i++) {
      const id = liveOrder[i];
      if (st.applied.has(id)) continue;
      const c = liveById.get(id);
      const issue = st.issues.get(c.issueId);
      advanceTo(Date.parse(c.created));
      for (const item of c.items) applyItem(issue, item);
      issue.fields.updated = c.created;
      st.histories.get(c.issueId).push({ changelogId: c.changelogId, issueId: c.issueId, created: c.created, authorId: c.authorId, items: clone(c.items) });
      st.applied.add(id);
      appliedNow.push(id);
    }
    return appliedNow;
  };
  // Every scripted live change (dropped ones included) — never the live-UI changes, which stay held back until the
  // scorer delivers them with the widget open (DESIGN §8.7 step 8); they are created last, so nothing scripted
  // waits behind them.
  const scriptedOrder = pack.live.filter((c) => !c.delivery.liveUi).map((c) => c.changelogId);
  const flush = () => (scriptedOrder.length ? applyThrough(scriptedOrder[scriptedOrder.length - 1]) : []);
  const nextDelivery = () => {
    if (st.cursor >= plan.length) return null;
    const d = plan[st.cursor++];
    const applied = applyThrough(d.changelogId);
    return { ...d, applied, change: liveById.get(d.changelogId), issue: st.issues.get(liveById.get(d.changelogId).issueId), remaining: plan.length - st.cursor };
  };

  return {
    pack, st, plan, sprintById, statusById, userById,
    now, advance, advanceTo, reset, applyThrough, flush, nextDelivery,
    issueByIdOrKey: (k) => st.issues.get(String(k)) ?? st.byKey.get(String(k).toUpperCase()),
    allIssues: () => [...st.issues.values()],
    canBrowse: (accountId, issue) => accountId === pack.appAccountId || !issue.hiddenFrom.includes(accountId),
    canComment: (accountId, issue) => accountId === pack.appAccountId || (!issue.hiddenFrom.includes(accountId) && !issue.commentForbiddenFor.includes(accountId)),
  };
}

module.exports = { createState, deliveryPlan };
