import { kvs, Filter } from '@forge/kvs';
import { hourIndex, msToNextHour, remainingMs, sleep } from './time';
import { loadSettings } from './settings';

// The app's own count of the hour's Jira points (§10: no response shows the spend until fewer than 480 remain),
// and the Retry-After holds every invocation must respect ("whichever invocation sends it"). One row per virtual
// clock hour; every change is a conditional transaction on `version`, so concurrent invocations never lose an update.
export const BUDGET = 'rate-budget';
export const QUOTA_POINTS = 2400;

export class Deferred extends Error {
  constructor(waitMs, reason) {
    super(`Jira work deferred ${Math.ceil(waitMs / 1000)} s (${reason})`);
    this.waitMs = waitMs;
    this.reason = reason;
  }
}

export const isKvsCode = (e, ...codes) => codes.includes(e?.code);

const keyOf = (hour) => `h${hour}`;

function parse(row) {
  let holds = {};
  try {
    holds = row?.holds ? JSON.parse(row.holds) : {};
  } catch {
    console.error(`rate-budget: unreadable holds ${JSON.stringify(row?.holds)}; treated as none`);
  }
  return { spent: row?.spent ?? 0, blocked: row?.blocked === true, holds, version: row?.version ?? 0 };
}

async function readHour(hour = hourIndex()) {
  const row = await kvs.entity(BUDGET).get(keyOf(hour));
  return { hour, row, state: parse(row) };
}

// Applies `mutate(state)` (returns the next state, or null for "no change") to the hour's row atomically.
async function updateHour(mutate, hour = hourIndex()) {
  for (;;) {
    const { row, state } = await readHour(hour);
    const next = mutate(state);
    if (!next) return state;
    const now = Date.now();
    const holds = Object.fromEntries(Object.entries(next.holds).filter(([, until]) => until > now));
    const value = { hour, spent: next.spent, blocked: next.blocked, holds: JSON.stringify(holds), version: state.version + 1 };
    try {
      if (!row) {
        await kvs.entity(BUDGET).set(keyOf(hour), value, { keyPolicy: 'FAIL_IF_EXISTS' });
      } else {
        await kvs
          .transact()
          .set(keyOf(hour), value, { entityName: BUDGET, conditions: new Filter().and('version', { condition: 'EQUAL_TO', values: [state.version] }) })
          .execute();
      }
      return { ...next, version: value.version };
    } catch (e) {
      if (!isKvsCode(e, 'KEY_CONFLICT', 'CONDITIONAL_CHECK_FAILED')) throw e;
    }
  }
}

// policy: { background, limit, maxWaitMs, marginMs }. Background policies carry the hour's point limit.
export async function backgroundPolicy({ maxWaitMs, marginMs }) {
  const settings = await loadSettings();
  return { background: true, limit: Math.floor((QUOTA_POINTS * settings.backgroundShare) / 100), share: settings.backgroundShare, maxWaitMs, marginMs };
}

export const personPolicy = (maxWaitMs) => ({ background: false, maxWaitMs, marginMs: 3000 });

function waitOrDefer(policy, waitMs, reason) {
  const room = remainingMs() - (policy.marginMs ?? 0);
  if (waitMs <= policy.maxWaitMs && waitMs < room) return sleep(waitMs + 50);
  throw new Deferred(waitMs, reason);
}

// Before every Jira request: honour every Retry-After hold on this endpoint (and this issue, for writes), and for
// background work stop at a quota block or at the share, else take the request's points from the hour.
export async function admit(policy, endpoint, points, issueId) {
  for (;;) {
    const now = Date.now();
    const { hour, state } = await readHour();
    const holdUntil = Math.max(state.holds[`e:${endpoint}`] ?? 0, issueId ? state.holds[`i:${issueId}`] ?? 0 : 0);
    if (policy.background && state.blocked) throw new Deferred(msToNextHour(now), 'quota');
    if (holdUntil > now) {
      await waitOrDefer(policy, holdUntil - now, 'retry-after');
      continue;
    }
    if (!policy.background) return;
    if (state.spent + points > policy.limit) throw new Deferred(msToNextHour(now), 'background share');
    let refused = false;
    await updateHour((s) => {
      if (s.blocked || s.spent + points > policy.limit) {
        refused = true;
        return null;
      }
      return { ...s, spent: s.spent + points };
    }, hour);
    if (!refused) return;
  }
}

export async function refund(points) {
  if (points <= 0) return;
  await updateHour((s) => ({ ...s, spent: Math.max(0, s.spent - points) }));
}

export async function recordHold(holdKey, untilMs) {
  await updateHour((s) => (s.holds[holdKey] >= untilMs ? null : { ...s, holds: { ...s.holds, [holdKey]: untilMs } }));
}

// jira-quota-tenant-based, or the site warning that the points person-facing work needs are running out: no
// background request until the next virtual hour.
export async function recordQuotaBlock() {
  await updateHour((s) => (s.blocked ? null : { ...s, blocked: true }));
}

export async function hourSpend() {
  return (await readHour()).state;
}
