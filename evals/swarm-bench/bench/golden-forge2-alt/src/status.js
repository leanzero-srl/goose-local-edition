import { kvs } from '@forge/kvs';
import { jiraJson, JiraError, route } from './jira';
import { STATUS_FIELD_NAME } from './config';
import { byTime, valueOf } from './ledger';
import { formatPoints, toMicro } from './numbers';

// The scope-status custom field (§15). The last value written per issue is kept under status:<issueId>, so only
// changes reach Jira.
const STATUS_PREFIX = 'status:';
const statusKey = (issueId) => `${STATUS_PREFIX}${issueId}`;

// entries: one { member, rows } per ACTIVE sprint the issue relates to (rows: its changes in that sprint).
// Returns the field's value, '' for no value, or null for a deleted issue (never compared).
export function statusFor(entries) {
  let removed = false;
  for (const { member, rows } of entries) {
    if (member?.deleted || rows.some((r) => r.deleted)) return null;
    const history = [...rows].sort(byTime);
    const inNow = member?.inSprint === true;
    const inAtStart = history.length ? history[0].kind === 'removed' : inNow;
    if (inNow) {
      if (inAtStart) return 'committed';
      const lastAdded = history.filter((r) => r.kind === 'added').pop();
      return `added +${formatPoints(toMicro(lastAdded ? valueOf(member, lastAdded.estimateField) : 0))}`;
    }
    if (inAtStart || history.some((r) => r.kind === 'added')) removed = true;
  }
  return removed ? 'removed' : '';
}

export async function keysWithPrefix(prefix) {
  const out = new Map();
  let cursor;
  do {
    let q = kvs.query().where('key', { condition: 'BEGINS_WITH', values: [prefix] }).limit(100);
    if (cursor) q = q.cursor(cursor);
    const page = await q.getMany();
    for (const r of page.results) out.set(r.key.slice(prefix.length), r.value);
    cursor = page.nextCursor;
  } while (cursor);
  return out;
}

export async function writtenStatuses() {
  const out = new Map();
  for (const [issueId, value] of await keysWithPrefix(STATUS_PREFIX)) out.set(issueId, value?.v ?? '');
  return out;
}

export const writtenStatus = async (issueId) => (await kvs.get(statusKey(issueId)))?.v ?? '';

async function postValues(cfg, chunk, policy) {
  const byValue = new Map();
  for (const [issueId, value] of chunk) {
    if (!byValue.has(value)) byValue.set(value, []);
    byValue.get(value).push(Number(issueId));
  }
  const updates = [...byValue].map(([value, issueIds]) => ({ customField: cfg.statusFieldId, issueIds, value: value === '' ? null : value }));
  await jiraJson('app', 'POST /rest/api/3/app/field/value', route`/rest/api/3/app/field/value`, policy, {
    body: { updates },
    issueId: chunk.length === 1 ? chunk[0][0] : undefined,
  });
  for (const [issueId, value] of chunk) await kvs.set(statusKey(issueId), { v: value, at: Date.now() });
  return chunk.length;
}

const refusal = (e) => e instanceof JiraError && e.status >= 400 && e.status < 500;

// Writes { issueId: value } to Jira in bulk as the app (≤ 200 updates a request), then remembers each value. One
// issue Jira refuses (deleted since) fails a whole request, so a refused request is retried issue by issue.
export async function writeStatuses(cfg, values, policy) {
  if (!cfg.statusFieldId) {
    if (values.size) console.error(`scope-status: no custom field named "${STATUS_FIELD_NAME}" on this site; ${values.size} value(s) not written`);
    return 0;
  }
  const entries = [...values].filter(([, v]) => v !== null);
  let written = 0;
  for (let i = 0; i < entries.length; i += 200) {
    const chunk = entries.slice(i, i + 200);
    try {
      written += await postValues(cfg, chunk, policy);
    } catch (e) {
      if (!refusal(e)) throw e;
      if (chunk.length === 1) {
        console.error(`scope-status: Jira refused the value of issue ${chunk[0][0]}: ${e.message}`);
        continue;
      }
      for (const one of chunk) {
        try {
          written += await postValues(cfg, [one], policy);
        } catch (e2) {
          if (!refusal(e2)) throw e2;
          console.error(`scope-status: Jira refused the value of issue ${one[0]}: ${e2.message}`);
        }
      }
    }
  }
  return written;
}
