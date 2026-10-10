import { jiraJson, route, postJson } from './jira';

const MAX_UPDATES = 200; // issue updates per bulk request (RATE-MODEL.json)

// Writes `scope-status` values (issueId -> value, '' = empty) through the bulk app field-value API, as
// the app: one update entry per distinct value, at most 200 issue updates per request. The app's own
// writes generate no changelog and no app events, so they never come back to the trigger as work.
export async function writeStatuses(fieldId, values, work) {
  const byValue = new Map();
  for (const [issueId, value] of values) {
    if (!byValue.has(value)) byValue.set(value, []);
    byValue.get(value).push(Number(issueId));
  }
  let updates = [];
  let count = 0;
  const send = async () => {
    if (!updates.length) return;
    await jiraJson('app', route`/rest/api/3/app/field/value?generateChangelog=false&generateAppEvents=false`, postJson({ updates }), work);
    updates = [];
    count = 0;
  };
  for (const [value, ids] of byValue) {
    for (let i = 0; i < ids.length; ) {
      const take = Math.min(MAX_UPDATES - count, ids.length - i);
      updates.push({ customField: fieldId, issueIds: ids.slice(i, i + take), value: value === '' ? null : value });
      count += take;
      i += take;
      if (count === MAX_UPDATES) await send();
    }
  }
  await send();
  return values.size;
}

export const currentStatus = (fields, fieldId) => {
  const v = fields?.[fieldId];
  return v === null || v === undefined ? '' : String(v);
};
