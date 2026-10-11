'use strict';
// Jira caches custom-function expansions by clause, never by caller. The app refreshes its own
// mappings through the documented precomputation REST API; final native JQL still applies permissions.
const crypto = require('crypto');
const { compile, parse, JqlError } = require('../jql.cjs');
const { jqlModel } = require('./platform.cjs');
const TTL_MS = 7 * 24 * 60 * 60 * 1000;
const stores = new WeakMap();
const err = (status, message) => ({ status, body: { errorMessages: [message], errors: {} } });
const copy = (value) => structuredClone(value);

function store(state) {
  if (!stores.has(state.st.issues)) stores.set(state.st.issues, { byId: new Map(), byClause: new Map(), pending: new Map(), log: [] });
  const s = stores.get(state.st.issues);
  for (const [id, row] of s.byId) {
    if (state.now() - Date.parse(row.used) >= TTL_MS) {
      s.byId.delete(id);
      s.byClause.delete(row.clauseKey);
    }
  }
  return s;
}

function canonical(clause) {
  if (!clause || typeof clause !== 'object' || !Array.isArray(clause.arguments)
      || clause.arguments.some((a) => typeof a !== 'string') || typeof clause.functionName !== 'string') {
    throw new JqlError('A custom JQL clause requires a function name and string arguments.');
  }
  const field = String(clause.field ?? '').toLowerCase();
  const operator = String(clause.operator ?? '').toLowerCase();
  if (!['key', 'issue', 'issuekey', 'id'].includes(field) || !['in', 'not in'].includes(operator)) {
    throw new JqlError('This custom JQL function requires the issue field and IN or NOT IN.');
  }
  return { field: 'key', type: ['issue'], operator, functionName: clause.functionName, arguments: [...clause.arguments] };
}

function begin(state, clause, { functionKey } = {}) {
  if (typeof functionKey !== 'string' || !functionKey) throw new Error('A precomputation requires the installed function extension ARI.');
  const c = canonical(clause);
  const key = JSON.stringify([functionKey, c.field, c.operator, c.functionName, c.arguments]);
  const s = store(state);
  const id = s.byClause.get(key);
  if (id) {
    const row = s.byId.get(id);
    row.used = new Date(state.now()).toISOString();
    s.log.push({ type: 'cache-hit', id, atMs: state.now() });
    return { id, cached: true, result: row.error === undefined ? { jql: row.value } : { error: row.error } };
  }
  const next = crypto.randomUUID();
  s.pending.set(next, { clauseKey: key, functionKey, ...c });
  s.log.push({ type: 'evaluate', id: next, atMs: state.now() });
  return { id: next, cached: false, request: { precomputationId: next, clause: c } };
}

function validateFragment(state, value) {
  if (typeof value !== 'string' || !value.trim()) throw new JqlError('The custom function did not return a JQL fragment.');
  const ast = parse(value);
  if (!ast.where || ast.orderBy.length) throw new JqlError('A custom function must return a clause fragment without ORDER BY.');
  const count = (n) => n.a ? count(n.a) + count(n.b) : n.e ? count(n.e) : n.vals?.length ?? (n.val ? 1 : 0);
  if (count(ast.where) > 1000) throw new JqlError('The custom JQL fragment exceeds 1,000 right-hand-side values.');
  compile(value, jqlModel({ state, t: state.now(), caller: { accountId: state.pack.appAccountId } }));
}

function finish(state, id, result) {
  const s = store(state);
  const pending = s.pending.get(id);
  if (!pending) throw new Error(`No pending precomputation ${id}.`);
  s.pending.delete(id);
  if (!result || typeof result !== 'object' || Array.isArray(result)) throw new JqlError('The custom function returned no result.');
  const isError = typeof result.error === 'string' && result.error.length > 0;
  if (isError && result.storeErrorAsPrecomputation !== true) return { error: result.error };
  if (!isError) validateFragment(state, result.jql);
  const stamp = new Date(state.now()).toISOString();
  const row = { ...pending, id, created: stamp, updated: stamp, used: stamp,
    ...(isError ? { error: result.error } : { value: result.jql }) };
  s.byId.set(id, row);
  s.byClause.set(row.clauseKey, id);
  s.log.push({ type: 'stored', id, atMs: state.now() });
  return isError ? { error: result.error } : { jql: result.jql };
}

const publicRow = ({ clauseKey, type, ...row }) => copy(row);
const snapshot = (state) => [...store(state).byId.values()].map(publicRow);
const cached = (state, id) => { const row = store(state).byId.get(id); return row ? publicRow(row) : null; };
const log = (state) => copy(store(state).log);
const owned = (c) => c.caller.as === 'app';
function ordered(rows, query) {
  const raw = query.get('orderBy');
  if (!raw) return rows.sort((a, b) => a.id.localeCompare(b.id));
  const descending = raw.startsWith('-');
  const key = raw.replace(/^[+-]/, '');
  if (!['functionKey', 'used', 'created', 'updated'].includes(key)) throw new JqlError(`Unknown precomputation order ${raw}.`);
  return rows.sort((a, b) => String(a[key]).localeCompare(String(b[key])) * (descending ? -1 : 1) || a.id.localeCompare(b.id));
}

const handlers = {
  'GET /rest/api/3/jql/function/computation': (c) => {
    if (!owned(c)) return err(403, 'Only apps can inspect their own JQL precomputations.');
    const q = c.req.query;
    const startAt = Number(q.get('startAt') ?? 0);
    const maxResults = Number(q.get('maxResults') ?? 100);
    if (!Number.isInteger(startAt) || startAt < 0 || !Number.isInteger(maxResults) || maxResults < 1) return err(400, 'Invalid pagination.');
    const keys = q.getAll('functionKey').flatMap((x) => x.split(','));
    let rows = snapshot(c.state).filter((r) => !keys.length || keys.includes(r.functionKey));
    try { rows = ordered(rows, q); } catch (e) { return err(400, e.message); }
    const values = rows.slice(startAt, startAt + maxResults);
    return { status: 200, body: { startAt, maxResults, total: rows.length, isLast: startAt + values.length >= rows.length, values } };
  },
  'POST /rest/api/3/jql/function/computation': (c) => {
    if (!owned(c)) return err(403, 'Only apps can update their own JQL precomputations.');
    const body = c.req.body;
    if (!body || !Array.isArray(body.values) || Object.keys(body).some((k) => k !== 'values')) return err(400, 'A precomputation update requires values.');
    const s = store(c.state);
    const updates = [];
    for (const u of body.values) {
      if (!u || Array.isArray(u) || typeof u !== 'object' || typeof u.id !== 'string'
          || Object.keys(u).some((k) => !['id', 'value', 'error'].includes(k))
          || (typeof u.value === 'string') === (typeof u.error === 'string')) return err(400, 'Each update needs id and either value or error.');
      if (u.value !== undefined) {
        try { validateFragment(c.state, u.value); } catch (e) { return err(400, e.message); }
      }
      updates.push(u);
    }
    const missing = [...new Set(updates.filter((u) => !s.byId.has(u.id)).map((u) => u.id))];
    const skip = c.req.query.get('skipNotFoundPrecomputations') === 'true';
    if (missing.length && !skip) return err(404, 'One or more precomputations were not found.');
    for (const u of updates) {
      const row = s.byId.get(u.id);
      if (!row) continue;
      delete row.value;
      delete row.error;
      Object.assign(row, u.value === undefined ? { error: u.error } : { value: u.value }, { updated: new Date(c.state.now()).toISOString() });
      s.log.push({ type: 'updated', id: row.id, atMs: c.state.now(), invocationId: c.caller.invocationId });
    }
    return skip ? { status: 200, body: { notFoundPrecomputationIDs: missing } } : { status: 204, body: undefined };
  },
  'POST /rest/api/3/jql/function/computation/search': (c) => {
    if (!owned(c)) return err(403, 'Only apps can inspect their own JQL precomputations.');
    const body = c.req.body;
    if (!body || !Array.isArray(body.precomputationIDs) || body.precomputationIDs.some((id) => typeof id !== 'string')
        || Object.keys(body).some((k) => k !== 'precomputationIDs')) return err(400, 'precomputationIDs must be a list of IDs.');
    const ids = [...new Set(body.precomputationIDs)];
    let found = ids.map((id) => cached(c.state, id)).filter(Boolean);
    try { found = ordered(found, c.req.query); } catch (e) { return err(400, e.message); }
    return { status: 200, body: { precomputations: found, notFoundPrecomputationIDs: ids.filter((id) => !cached(c.state, id)) } };
  },
};
// Expand registered app functions at their clause boundary. Native predicates and query composition
// are parsed and rendered from the same AST the site's compiler consumes, including nested AND/OR/NOT.
async function expand(state, source, registeredfuncs) {
  const calls = [];
  try {
    const ast = parse(source);
    const registered = new Map(Object.entries(registeredfuncs).map(([name, entry]) => [name.toLowerCase(), { name, ...entry }]));
    const field = (f) => JSON.stringify(f.name);
    const value = (v) => v.kind === 'empty' ? 'EMPTY' : v.kind === 'fn'
      ? `${v.name}(${v.args.map((a) => JSON.stringify(String(a))).join(',')})` : JSON.stringify(String(v.v));
    const render = async (n) => {
      if (!n) return '';
      if (n.a) return `(${await render(n.a)} ${n.op.toUpperCase()} ${await render(n.b)})`;
      if (n.e) return `(NOT ${await render(n.e)})`;
      const fn = n.val?.kind === 'fn' ? n.val : n.vals?.length === 1 && n.vals[0].kind === 'fn' ? n.vals[0] : null;
      const entry = fn && registered.get(fn.name.toLowerCase());
      if (fn && !entry && ['key', 'issue', 'issuekey', 'id'].includes(n.field.name.toLowerCase())) {
        throw new JqlError(`Unable to find installed JQL function '${fn.name}()'.`);
      }
      if (entry) {
        const clause = { field: n.field.name, type: ['issue'], operator: n.op === 'notin' ? 'not in' : n.op,
          functionName: entry.name, arguments: fn.args.map(String) };
        const pre = begin(state, clause, { functionKey: entry.functionKey });
        const rec = { functionName: entry.name, clause, precomputationId: pre.id, invoked: !pre.cached };
        calls.push(rec);
        const answer = pre.cached ? pre.result : finish(state, pre.id, await entry.invoke(pre.request));
        rec.result = copy(answer);
        if (answer.error) throw new JqlError(answer.error);
        return `(${answer.jql})`;
      }
      if (['isempty', 'isnotempty'].includes(n.op)) return `${field(n.field)} IS ${n.op === 'isnotempty' ? 'NOT ' : ''}EMPTY`;
      if (n.vals) return `${field(n.field)} ${n.op === 'notin' ? 'NOT IN' : n.op.toUpperCase()} ${fn ? value(fn) : `(${n.vals.map(value).join(',')})`}`;
      return `${field(n.field)} ${n.op} ${value(n.val)}`;
    };
    const where = await render(ast.where);
    const ordering = ast.orderBy.length ? ` ORDER BY ${ast.orderBy.map((o) => `${field(o.field)}${o.dir ? ` ${o.dir.toUpperCase()}` : ''}`).join(',')}` : '';
    return { jql: where + ordering, calls };
  } catch (e) {
    if (e instanceof JqlError) return { error: e.message, calls };
    throw e;
  }
}
module.exports = { handlers, begin, finish, cached, snapshot, log, validateFragment, expand };
