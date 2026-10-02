// Pure ledger arithmetic: no Jira, no KVS. Everything here is a function of the issue as Jira
// returns it (current fields + full changelog) and the sprint's startDate.

export function sprintIdList(raw) {
  if (raw == null || raw === '') return [];
  return String(raw)
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean);
}

const byTimeThenId = (a, b) => a.atMs - b.atMs || Number(a.changeId) - Number(b.changeId);

export function sprintEntries(issue, sprintFieldId) {
  const out = [];
  for (const h of (issue.changelog && issue.changelog.histories) || []) {
    const atMs = Date.parse(h.created);
    for (const item of h.items || []) {
      if (item.fieldId !== sprintFieldId && !(item.fieldId == null && item.field === 'Sprint')) continue;
      out.push({
        changeId: String(h.id),
        atMs,
        from: sprintIdList(item.from),
        to: sprintIdList(item.to),
        authorId: h.author ? h.author.accountId : null,
        authorName: h.author ? h.author.displayName : 'Unknown',
      });
    }
  }
  return out.sort(byTimeThenId);
}

export function currentSprintIds(issue, sprintFieldId) {
  const v = issue.fields && issue.fields[sprintFieldId];
  return (Array.isArray(v) ? v : []).map((s) => String(s && typeof s === 'object' ? s.id : s));
}

export function estimateOf(issue, fieldId) {
  const v = fieldId && issue.fields ? issue.fields[fieldId] : null;
  const n = typeof v === 'number' ? v : Number(v);
  return v == null || v === '' || !Number.isFinite(n) ? 0 : n;
}

// For one (issue, sprint): where it stood at startDate, where it is now, and the Sprint-field
// changes strictly after startDate that put it into or took it out of the sprint.
export function assess(issue, sprint, sprintFieldId) {
  const S = String(sprint.id);
  const startMs = Date.parse(sprint.startDate);
  const entries = sprintEntries(issue, sprintFieldId);
  const after = entries.filter((e) => e.atMs > startMs);
  const inNow = currentSprintIds(issue, sprintFieldId).includes(S);
  const createdMs = Date.parse(issue.fields && issue.fields.created);
  let atStart;
  if (Number.isFinite(createdMs) && createdMs > startMs) atStart = false;
  else if (after.length) atStart = after[0].from.includes(S);
  else atStart = inNow;

  const seen = new Set();
  const changes = [];
  for (const e of after) {
    const wasIn = e.from.includes(S);
    const isIn = e.to.includes(S);
    if (wasIn === isIn || seen.has(e.changeId)) continue;
    seen.add(e.changeId);
    changes.push({
      changeId: e.changeId,
      atMs: e.atMs,
      kind: isIn ? 'added' : 'removed',
      authorId: e.authorId,
      authorName: e.authorName,
    });
  }
  const everIn = atStart || inNow || changes.some((c) => c.kind === 'added');
  return { atStart, inNow, everIn, changes };
}

const SCALE = 1000000;
const scaled = (x) => BigInt(Math.round(x * SCALE));

export function totals(members) {
  let committed = 0n;
  let added = 0n;
  let removed = 0n;
  for (const m of members) {
    const p = scaled(m.points || 0);
    if (m.atStart) committed += p;
    if (m.inNow && !m.atStart) added += p;
    if (!m.inNow && m.everIn) removed += p;
  }
  return { committed, added, removed };
}

const asNumber = (big) => Number(big) / SCALE;

// creep in tenths of a percent, half away from zero (all inputs are >= 0).
export function creepTenths(t) {
  if (t.committed === 0n) return null;
  const num = 1000n * t.added;
  return Number((2n * num + t.committed) / (2n * t.committed));
}

export function fmtPoints(x) {
  const n = Number(Number(x).toFixed(6));
  return Object.is(n, -0) ? '0' : String(n);
}

export function summarize(members) {
  const t = totals(members);
  const tenths = creepTenths(t);
  return {
    committed: asNumber(t.committed),
    added: asNumber(t.added),
    removed: asNumber(t.removed),
    creepPercent: tenths == null ? null : tenths / 10,
    creepText: tenths == null ? '—' : `${Math.floor(tenths / 10)}.${tenths % 10}%`,
  };
}

export const tableOrder = (a, b) => a.atMs - b.atMs || Number(a.changeId) - Number(b.changeId);
