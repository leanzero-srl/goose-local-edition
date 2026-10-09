'use strict';
// The world that changes mid-run (SPEC R4, §2.5): a seeded timeline of the five classes, planned from the pack and
// applied through the site state's mutation API.
//
//   withWorld(pack, {scoring}) -> the same pack, extended: `pack.world = {window, injection, events}`; each issue move
//       is a Sprint changelog entry inserted into `pack.live` (creation order, an id that fits the global changelog
//       sequence, a delivery slot at its time) so the live machinery applies and delivers it like any human edit; the
//       Forge LLM injection (llm.cjs injectionPlan) is planted in its carrier issue's install-time summary.
//   createWorld({pack, mutate}) -> {events, applied, pending(), applyDue(t), reset()}: applies the other four classes
//       through `mutate` (the state API) once the site's virtual time reaches them.
//   node world.cjs --seed <16 hex> [--scoring]   prints the plan (the scoring plan is private: site code never ships).
//
// THE CLASSES and what each event names (times are site virtual time, `at` ISO + `atMs`):
//   browse-revoke     {accountId, projectKey}  the peer can no longer browse that project's issues
//   estimation-field  {boardId, fromFieldId, toFieldId}  the board's estimation field switches
//   issue-move        {issueId, issueKey, fromSprintId, toSprintId, fromBoardId, toBoardId, changelogId, viaLive}
//                     an issue leaves its active sprint for another board's active sprint (preferring a board that
//                     estimates with another field, and an issue that carries a value there)
//   issue-delete      {issueId, issueKey, authorId}  a counted issue with ledger changes is deleted
//   sprint-close      {sprintId, boardId}  an active sprint closes; its incomplete issues go to the backlog (their
//                     Sprint field keeps the closed sprint, so Jira writes no changelog entry)
// The dev site plans each class once; the scoring site plans two moves and two deletes too, at seeded times.
//
// CONSISTENCY BY CONSTRUCTION: no world event may contradict the scripted live stream, because the live stream is
// what humans do and Jira refuses the contradicting edits (a closed sprint takes no adds or removes, a deleted issue
// no edits). So a moved or deleted issue has NO live change at or after its event, and a sprint closes only after the
// last live change that names it. The live-UI changes (DESIGN §8.7 step 8) stay untouched: their issues are never
// targets, their boards never switch fields, and the window ends before the first of them. When a class cannot be
// placed under these rules the plan THROWS (a pack the generator must change), never substitutes a weaker event.
const crypto = require('crypto');
const { createRng } = require('./rng.cjs');
const { injectionPlan } = require('./llm.cjs');

const MIN = 60_000;
const HOUR = 60 * MIN;
// SPEC §2.3: "6 virtual hours scored after the upgrade"; the upgrade is the pack's `now`.
const SCORED_HOURS = 6;
const CLASSES = ['browse-revoke', 'estimation-field', 'issue-move', 'issue-delete', 'sprint-close'];
const MUTATIONS = ['closeSprint', 'setBoardEstimationField', 'deleteIssue', 'revokeBrowse'];
// Dev: one of each, spread over the window in a fixed order (fractions of it). Scoring: these classes at seeded
// fractions in [0.1, 0.85]. The sprint close is placed by the data (after its sprint's last live reference).
const DEV_PLAN = [['browse-revoke', 0.2], ['estimation-field', 0.35], ['issue-move', 0.5], ['issue-delete', 0.65]];
const SCORING_CLASSES = ['browse-revoke', 'estimation-field', 'issue-move', 'issue-move', 'issue-delete', 'issue-delete'];

const iso = (ms) => new Date(ms).toISOString();
const msOf = (c) => Date.parse(c.created);
const idsOf = (s) => String(s ?? '').split(',').map((x) => x.trim()).filter(Boolean);

function planWorld(pack, { scoring = false } = {}) {
  const r = createRng(crypto.createHash('sha256').update(`world:${pack.seed}`).digest('hex').slice(0, 16));
  const fail = (why) => { throw new Error(`world plan for ${pack.seed}${scoring ? ' (scoring)' : ''}: ${why}`); };
  const SF = pack.sprintFieldId;
  if (!pack.peer) fail('the pack names no peer to revoke');
  const start = Date.parse(pack.now);
  const liveUi = pack.live.filter((c) => c.delivery?.liveUi);
  const end = Math.min(start + SCORED_HOURS * HOUR, ...liveUi.map(msOf));
  const sprintById = new Map(pack.sprints.map((s) => [String(s.id), s]));
  const boardById = new Map(pack.boards.map((b) => [String(b.id), b]));
  const boardOf = (sid) => String(sprintById.get(sid).originBoardId);
  const active = pack.sprints.filter((s) => s.state === 'active').map((s) => String(s.id));
  const humans = pack.users.map((u) => u.accountId);
  const changes = [...pack.history, ...pack.live].sort((a, b) => msOf(a) - msOf(b) || Number(a.changelogId) - Number(b.changelogId));
  const liveOf = new Map();
  for (const c of pack.live) liveOf.set(c.issueId, [...(liveOf.get(c.issueId) ?? []), c]);
  const histOf = new Map();
  for (const c of pack.history) histOf.set(c.issueId, [...(histOf.get(c.issueId) ?? []), c]);
  const names = (sprintIds) => sprintIds.map((sid) => sprintById.get(sid).name).join(', ');
  const touches = (c, sid) => c.items.some((it) => it.fieldId === SF && [...idsOf(it.from), ...idsOf(it.to)].includes(sid));
  const sprintsAt = (i, t) => {
    let cur = (i.fields[SF] ?? []).map((s) => String(s.id));
    for (const c of liveOf.get(i.id) ?? []) if (msOf(c) < t) for (const it of c.items) if (it.fieldId === SF) cur = idsOf(it.to);
    return cur;
  };
  const estimateAt = (i, field, t) => {
    let v = i.fields[field] ?? null;
    for (const c of liveOf.get(i.id) ?? []) if (msOf(c) < t) for (const it of c.items) if (it.fieldId === field) v = it.to === '' || it.to === null ? null : Number(it.to);
    return v;
  };
  const quietFrom = (i, t) => !(liveOf.get(i.id) ?? []).some((c) => msOf(c) >= t);
  const ledgerChangeBefore = (i, t) => [...(histOf.get(i.id) ?? []), ...(liveOf.get(i.id) ?? [])].some((c) => msOf(c) < t
    && active.some((sid) => touches(c, sid) && msOf(c) > Date.parse(sprintById.get(sid).startDate)));

  // Never targets: the LLM injection's carrier and target, and every issue a live-UI change touches.
  const injection = injectionPlan(pack);
  if (injection.error) fail(`no Forge LLM injection: ${injection.error}`);
  const reserved = [injection.carrierIssueId, injection.targetIssueId, ...liveUi.map((c) => c.issueId)];
  // Boards whose numbers the live-UI changes move keep their estimation field.
  const uiBoards = new Set(liveUi.flatMap((c) => {
    const sprints = c.items.some((it) => it.fieldId === SF) ? c.items.flatMap((it) => [...idsOf(it.from), ...idsOf(it.to)])
      : sprintsAt(pack.issues.find((i) => i.id === c.issueId), msOf(c));
    return sprints.filter((sid) => active.includes(sid)).map(boardOf);
  }));

  const schedule = scoring
    ? SCORING_CLASSES.map((cls) => [cls, 0.1 + r.float() * 0.75]).sort((a, b) => a[1] - b[1])
    : DEV_PLAN;
  // The close goes a few minutes after the last live change naming its sprint. Sprints are tried in the order they
  // go quiet (the earliest leaves the app the most time); the first under which every other class still fits wins.
  const lastRef = (sid) => Math.max(start, ...pack.live.filter((c) => touches(c, sid)).map(msOf));
  const closable = active.filter((sid) => lastRef(sid) + 2 * MIN < end).sort((a, b) => lastRef(a) - lastRef(b) || Number(a) - Number(b));
  if (!closable.length) fail(`no active sprint goes quiet before the window ends at ${iso(end)}: the live stream names every one until then`);
  const refusals = [];
  for (const closing of closable) {
    try {
      return attempt(closing);
    } catch (e) {
      if (!(e instanceof PlanError)) throw e;
      refusals.push(`closing ${closing}: ${e.message}`);
    }
  }
  fail(`no schedule fits: ${refusals.join('; ')}`);

  function attempt(closing) {
    const refuse = (why) => { throw new PlanError(why); };
    const quietAt = lastRef(closing);
    const closeAt = quietAt + r.int(1, Math.min(20, Math.floor((end - quietAt) / MIN) - 1)) * MIN;
    const activeAt = (sid, t) => active.includes(sid) && !(sid === closing && t >= closeAt);
    const used = new Set(reserved);
    const all = changes.slice();

    const events = [{ class: 'sprint-close', atMs: closeAt, sprintId: Number(closing), boardId: sprintById.get(closing).originBoardId }];
    const fieldSwitch = [];
    const fieldAt = (boardId, t) => fieldSwitch.find((e) => String(e.boardId) === boardId && e.atMs <= t)?.toFieldId ?? boardById.get(boardId).estimationFieldId;

    // An instant at or after `target` that falls strictly between two changes whose ids leave room for one more.
    const idSlot = (target) => {
      let i = all.findIndex((c) => msOf(c) > target);
      if (i < 0) return { atMs: Math.max(target, msOf(all.at(-1)) + 1), changelogId: String(Number(all.at(-1).changelogId) + 1) };
      for (; i < all.length; i++) {
        const a = all[i - 1];
        const b = all[i];
        const lo = Math.max(target, msOf(a) + 1);
        if (Number(b.changelogId) - Number(a.changelogId) >= 2 && lo < msOf(b)) return { atMs: lo, changelogId: String(Number(a.changelogId) + 1) };
      }
      refuse(`no room in the changelog id sequence after ${iso(target)}`);
    };

    const plan = {
      'browse-revoke': (t) => {
        const projects = [...new Set(active.filter((sid) => activeAt(sid, t)).map((sid) => boardById.get(boardOf(sid)).projectKey))].sort();
        if (!projects.length) refuse('no project has an active sprint to revoke the peer from');
        return { accountId: pack.peer, projectKey: r.pick(projects) };
      },
      'estimation-field': (t) => {
        const scrum = pack.boards.filter((b) => b.type === 'scrum');
        const fields = [...new Set(scrum.map((b) => b.estimationFieldId))];
        const boards = scrum.filter((b) => !uiBoards.has(String(b.id)) && !fieldSwitch.some((e) => e.boardId === b.id)
          && active.some((sid) => boardOf(sid) === String(b.id) && activeAt(sid, t) && sid !== closing));
        if (fields.length < 2) refuse('the scrum boards estimate with one field only, so no board can switch');
        if (!boards.length) refuse('no scrum board outside the live-UI step keeps an active sprint to switch');
        // The board whose active sprints the live stream changes most after the switch shows the new field most.
        const after = (b) => pack.live.filter((c) => msOf(c) > t && active.some((sid) => boardOf(sid) === String(b.id) && touches(c, sid))).length;
        const board = boards.sort((a, b) => after(b) - after(a) || a.id - b.id)[0];
        const ev = { boardId: board.id, fromFieldId: fieldAt(String(board.id), t), toFieldId: fields.find((f) => f !== fieldAt(String(board.id), t)) };
        fieldSwitch.push({ ...ev, atMs: t });
        return ev;
      },
      'issue-move': (target) => {
        const { atMs: t, changelogId } = idSlot(target);
        const cands = [];
        for (const i of pack.issues) {
          if (used.has(i.id) || i.hiddenFrom.length || !quietFrom(i, t)) continue;
          const cur = sprintsAt(i, t);
          const from = cur.find((sid) => activeAt(sid, t));
          if (!from) continue;
          for (const to of active) {
            if (!activeAt(to, t) || to === closing || boardOf(to) === boardOf(from)) continue;
            const f1 = fieldAt(boardOf(from), t);
            const f2 = fieldAt(boardOf(to), t);
            const e1 = estimateAt(i, f1, t);
            const e2 = estimateAt(i, f2, t);
            cands.push({ i, cur, from, to, rank: (f1 !== f2 ? 4 : 0) + (e2 !== null ? 2 : 0) + (e2 !== null && e2 !== e1 ? 1 : 0) });
          }
        }
        if (!cands.length) refuse(`no issue every person can browse sits quietly in an active sprint at ${iso(t)} with another board's active sprint to move to`);
        const best = Math.max(...cands.map((c) => c.rank));
        const { i, cur, from, to } = r.pick(cands.filter((c) => c.rank === best));
        used.add(i.id);
        const next = cur.map((sid) => (sid === from ? to : sid));
        const change = { changelogId, issueId: i.id, created: iso(t), authorId: r.pick(humans),
          items: [{ field: 'Sprint', fieldtype: 'custom', fieldId: SF, from: cur.join(', '), fromString: names(cur), to: next.join(', '), toString: names(next) }] };
        const pos = all.findIndex((c) => msOf(c) > t);
        all.splice(pos < 0 ? all.length : pos, 0, change);
        return { atMs: t, issueId: i.id, issueKey: i.key, fromSprintId: Number(from), toSprintId: Number(to), fromBoardId: Number(boardOf(from)),
          toBoardId: Number(boardOf(to)), changelogId, viaLive: true, change };
      },
      'issue-delete': (t) => {
        const counted = (i) => sprintsAt(i, t).filter((sid) => activeAt(sid, t));
        const cands = pack.issues.filter((i) => !used.has(i.id) && !i.hiddenFrom.length && quietFrom(i, t) && counted(i).length && ledgerChangeBefore(i, t));
        if (!cands.length) refuse(`no counted issue with ledger changes stays quiet from ${iso(t)} to be deleted`);
        const estimated = cands.filter((i) => counted(i).some((sid) => estimateAt(i, fieldAt(boardOf(sid), t), t) !== null));
        const i = r.pick(estimated.length ? estimated : cands);
        used.add(i.id);
        return { issueId: i.id, issueKey: i.key, authorId: r.pick(humans) };
      },
    };

    for (const [cls, f] of schedule) {
      const target = Math.round(start + f * (end - start));
      const ev = plan[cls](target);
      events.push({ class: cls, atMs: target, ...ev });
  }
  events.sort((a, b) => a.atMs - b.atMs);
  const prefix = `world-${pack.seed.slice(0, 6)}`;
  return {
    window: { start: iso(start), end: iso(end) },
    injection,
    events: events.map((e, n) => ({ id: `${prefix}-${n + 1}`, class: e.class, at: iso(e.atMs), ...e })),
  };
  }
}

class PlanError extends Error {}

// The move's delivery slot: before every delivery of a change created after it; later slots shift by one.
function insertLive(pack, change) {
  const t = msOf(change);
  const slotsOf = (cs) => cs.flatMap((c) => [c.delivery.slot, ...c.delivery.duplicates]).filter((s) => s !== null);
  const later = slotsOf(pack.live.filter((c) => msOf(c) > t));
  const every = slotsOf(pack.live);
  const slot = later.length ? Math.min(...later) : every.length ? Math.max(...every) + 1 : 0;
  for (const c of pack.live) {
    if (c.delivery.slot !== null && c.delivery.slot >= slot) c.delivery.slot += 1;
    c.delivery.duplicates = c.delivery.duplicates.map((s) => (s >= slot ? s + 1 : s));
  }
  const at = pack.live.findIndex((c) => msOf(c) > t);
  pack.live.splice(at < 0 ? pack.live.length : at, 0, { ...change, delivery: { slot, duplicates: [], dropped: false } });
}

function withWorld(pack, { scoring = false } = {}) {
  if (pack.world) throw new Error('the pack already carries a world');
  const plan = planWorld(pack, { scoring });
  for (const e of plan.events) if (e.class === 'issue-move') insertLive(pack, e.change);
  const carrier = pack.issues.find((i) => i.id === plan.injection.carrierIssueId);
  carrier.summary = plan.injection.text;
  carrier.fields.summary = plan.injection.text;
  pack.world = { ...plan, events: plan.events.map(({ change, ...e }) => e) };
  return pack;
}

// Applies the scheduled non-changelog events through the state API. `mutate` (P4's state):
//   closeSprint(sprintId, atMs)  setBoardEstimationField(boardId, fieldId, atMs)  revokeBrowse(accountId, projectKey, atMs)
//   deleteIssue(issueId, atMs) -> the issue as it was (for the avi:jira:deleted:issue event)
// Issue moves are live changes (pack.live): the live machinery applies and delivers them, so they are not applied here.
function createWorld({ pack, mutate }) {
  if (!pack.world) throw new Error('createWorld: the pack carries no world (withWorld adds it)');
  const missing = MUTATIONS.filter((fn) => typeof mutate?.[fn] !== 'function');
  if (missing.length) throw new Error(`createWorld: the state mutation API lacks ${missing.join(', ')}`);
  const due = pack.world.events.filter((e) => !e.viaLive);
  const applied = [];
  const apply = (e) => {
    switch (e.class) {
      case 'sprint-close': mutate.closeSprint(e.sprintId, e.atMs); return { ...e };
      case 'estimation-field': mutate.setBoardEstimationField(e.boardId, e.toFieldId, e.atMs); return { ...e };
      case 'browse-revoke': mutate.revokeBrowse(e.accountId, e.projectKey, e.atMs); return { ...e };
      case 'issue-delete': return { ...e, event: { eventType: 'avi:jira:deleted:issue', atlassianId: e.authorId, issue: mutate.deleteIssue(e.issueId, e.atMs) } };
      default: throw new Error(`world event ${e.id}: no class ${e.class}`);
    }
  };
  return {
    events: pack.world.events,
    applied,
    pending: () => due.slice(applied.length),
    // Every scheduled event at or before `t` (site virtual ms) not applied yet, in time order -> the ones applied now.
    applyDue: (t) => {
      const now = [];
      while (applied.length < due.length && due[applied.length].atMs <= t) {
        const rec = apply(due[applied.length]);
        applied.push(rec);
        now.push(rec);
      }
      return now;
    },
    reset: () => { applied.length = 0; },
  };
}

if (require.main === module) {
  const args = process.argv.slice(2);
  const get = (n) => { const i = args.indexOf(n); return i >= 0 ? args[i + 1] : undefined; };
  const { facts } = require('./fixtures.cjs');
  const scoring = args.includes('--scoring');
  const pack = facts(get('--seed'), { scoring });
  process.stdout.write(JSON.stringify((pack.world ?? withWorld(pack, { scoring }).world), null, 1) + '\n');
}

module.exports = { planWorld, withWorld, createWorld, CLASSES, MUTATIONS, SCORED_HOURS };
