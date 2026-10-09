'use strict';
// The app's read-only custom field (SPEC R7: `jira:customField` `scope-status`, type string), written by the app
// through Jira's two app-field-value operations, as the pinned OpenAPI (kit/openapi/jira.json) defines them:
//   POST /rest/api/3/app/field/value                 updateMultipleCustomFieldValues  {updates: [{customField, issueIds, value}]}
//   PUT  /rest/api/3/app/field/{fieldIdOrKey}/value  updateCustomFieldValue           {updates: [{issueIds, value}]}
// There is no `PUT /rest/api/3/app/field/value` in Jira: the site's OpenAPI match answers it 405 like Jira would.
// Documented there and enforced here: 204 and no body on success; "Only the app that owns the custom field ... can
// update its values" (an asUser call, or a field the app does not own -> 403); "404 Returned if any field is not
// found"; "Combinations of custom field and issue should be unique within the request" (400); each update entry has
// no properties beyond its schema's (`additionalProperties: false`, 400); the value must fit the field's type
// (`string` -> a string; null clears).
// HARNESS CHOICES (Atlassian documents none of these; the contract states them):
//   * at most MAX_UPDATES field-issue combinations per request (SPEC §2.1), else 400 and nothing applied;
//   * an issue id that does not exist (never did, or was deleted) -> 400 naming it, and nothing applied;
//   * issue ids may be numbers or numeric strings (Jira's own JSON gives issue ids as strings);
//   * an empty string clears the value like null;
//   * `generateChangelog` / `generateAppEvents` are accepted and recorded, but the site writes no changelog entry
//     and sends no event for an app field write.
// The value is stored on the issue (`fields[<field id>]`), so GET /issue, search and bulkfetch show it wherever the
// renderer lists the field: the field must be in `pack.fields`, and `pack.scopeStatusFieldId` names it (P4 fixtures).
const MAX_UPDATES = 200;

const err = (status, messages) => ({ status, body: { errorMessages: [].concat(messages), errors: {} } });
const isIssueId = (x) => Number.isInteger(x) || (typeof x === 'string' && /^\d+$/.test(x));

// Every write this site took, per site lifetime: keyed by the issue map, which state.reset() replaces.
const logs = new WeakMap();
function fieldWrites(state) {
  if (!logs.has(state.st.issues)) logs.set(state.st.issues, []);
  return logs.get(state.st.issues);
}

function appField(pack) {
  if (!pack.scopeStatusFieldId) throw new Error("pack.scopeStatusFieldId is missing: the fixtures must add the app's jira:customField to pack.fields and name its id");
  const f = pack.fields.find((x) => x.id === pack.scopeStatusFieldId);
  if (!f) throw new Error(`pack.scopeStatusFieldId ${pack.scopeStatusFieldId} is not in pack.fields`);
  return f;
}

// The number of field-issue combinations a request asks for (what the rate model prices: 1 + 1 per 50 updates).
function updateCount(body) {
  const updates = body && typeof body === 'object' && Array.isArray(body.updates) ? body.updates : [];
  return updates.reduce((n, u) => n + (u && Array.isArray(u.issueIds) ? u.issueIds.length : 0), 0);
}

function fitsType(field, value) {
  if (value === null) return true;
  const type = field.schema?.type;
  if (type === 'string') return typeof value === 'string';
  if (type === 'number') return typeof value === 'number' && Number.isFinite(value);
  throw new Error(`app field ${field.id} has schema type ${type}, which the site does not model`);
}

// entries: [{fieldRef, issueIds, value}] already shape-checked -> {status, body}
function write(c, entries) {
  const { pack } = c.state;
  const own = appField(pack);
  const resolved = [];
  for (const e of entries) {
    const field = pack.fields.find((f) => f.id === e.fieldRef || f.key === e.fieldRef);
    if (!field) return err(404, `The custom field was not found: ${e.fieldRef}`);
    if (field.id !== own.id) return err(403, `Only the app that provided the field can update its values: ${e.fieldRef}`);
    resolved.push({ ...e, field });
  }
  const total = resolved.reduce((n, e) => n + e.issueIds.length, 0);
  if (total > MAX_UPDATES) return err(400, `A request can update at most ${MAX_UPDATES} field and issue combinations; this one has ${total}.`);
  const seen = new Set();
  for (const e of resolved) {
    if (!fitsType(e.field, e.value)) return err(400, `The value for ${e.field.id} must be a ${e.field.schema.type} or null.`);
    for (const id of e.issueIds) {
      const pair = `${e.field.id}:${id}`;
      if (seen.has(pair)) return err(400, `Combinations of custom field and issue should be unique within the request: ${e.field.id} on issue ${id}.`);
      seen.add(pair);
    }
  }
  const gone = (id) => { const iss = c.state.st.issues.get(id); return !iss || iss.deleted === true; };
  const missing = [...new Set(resolved.flatMap((e) => e.issueIds))].filter(gone);
  if (missing.length) return err(400, `Issue does not exist or you do not have permission to see it: ${missing.join(', ')}`);
  const at = c.state.now();
  const q = c.req.query;
  const flags = { generateChangelog: q.get('generateChangelog') ?? null, generateAppEvents: q.get('generateAppEvents') ?? null };
  const log = fieldWrites(c.state);
  for (const e of resolved) {
    const value = e.value === '' ? null : e.value;
    for (const id of e.issueIds) {
      c.state.st.issues.get(id).fields[e.field.id] = value;
      log.push({ atMs: at, t: new Date(at).toISOString(), issueId: id, fieldId: e.field.id, value, invocationId: c.caller.invocationId, ...flags });
    }
  }
  return { status: 204, body: undefined };
}

// Shape-checks `updates` against the operation's entry schema -> {entries} or {error}
function entriesOf(body, allowed, fieldRef) {
  if (!body || typeof body !== 'object' || !Array.isArray(body.updates)) return { error: err(400, 'The request must carry an updates array.') };
  const extra = Object.keys(body).filter((k) => k !== 'updates');
  if (extra.length) return { error: err(400, `Unrecognized field "${extra[0]}" in the request body.`) };
  const entries = [];
  for (const u of body.updates) {
    if (!u || typeof u !== 'object' || Array.isArray(u)) return { error: err(400, 'Each update must be an object.') };
    const unknown = Object.keys(u).find((k) => !allowed.includes(k));
    if (unknown) return { error: err(400, `Unrecognized field "${unknown}" in an update.`) };
    if (!('value' in u)) return { error: err(400, 'Each update needs a value (null clears it).') };
    if (!Array.isArray(u.issueIds) || !u.issueIds.every(isIssueId)) return { error: err(400, 'issueIds must be an array of issue ids.') };
    const ref = fieldRef ?? u.customField;
    if (typeof ref !== 'string' || !ref) return { error: err(400, 'Each update needs customField: the ID or key of the custom field.') };
    entries.push({ fieldRef: ref, issueIds: u.issueIds.map(String), value: u.value });
  }
  return { entries };
}

const onlyApp = (c) => (c.caller.as === 'app' ? null : err(403, 'Only the app that provided the field can update its values; this request is not authenticated as the app.'));

const handlers = {
  'POST /rest/api/3/app/field/value': (c) => {
    const denied = onlyApp(c);
    if (denied) return denied;
    const { entries, error } = entriesOf(c.req.body, ['customField', 'issueIds', 'value'], null);
    return error ?? write(c, entries);
  },
  'PUT /rest/api/3/app/field/{fieldIdOrKey}/value': (c) => {
    const denied = onlyApp(c);
    if (denied) return denied;
    const { entries, error } = entriesOf(c.req.body, ['issueIds', 'value'], c.params.fieldIdOrKey);
    return error ?? write(c, entries);
  },
};

// The app field's current value per issue that has one (the oracle's read of R7).
function fieldValues(state) {
  const id = appField(state.pack).id;
  return Object.fromEntries([...state.st.issues.values()].filter((i) => i.fields[id] !== null && i.fields[id] !== undefined).map((i) => [i.id, i.fields[id]]));
}

module.exports = { handlers, updateCount, fieldWrites, fieldValues, MAX_UPDATES };
