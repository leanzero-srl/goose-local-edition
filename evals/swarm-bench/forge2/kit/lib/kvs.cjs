'use strict';
// Forge KVS + Custom Entity Store, modelled for every endpoint @forge/kvs 2.0.7 calls
// (out/storage-api.js): /api/v1/{get,set,delete,query}, /api/v1/secret/{get,set,delete},
// /api/v1/entity/{get,set,delete,query}, /api/v1/batch/{get,set,delete}, /api/v1/transaction.
// Limits and error codes are the documented ones (developer.atlassian.com, fetched 2026-10-02):
//   limits-kvs-ce: key 500, value 240 KiB (raw bytes), object depth 31, 25 operations per transaction;
//   kvs-batch / entities-batch: "Each batch operation can contain a maximum of 25 keys.";
//   kvs-api-query / entities-api-query: "up to 10 values by default ... a maximum of 100";
//   entities-errorhandling: EMPTY_KEY INVALID_KEY KEY_TOO_LONG NOT_FOUND MAX_SIZE MAX_DEPTH
//   INVALID_ENTITY_TYPE INVALID_ENTITY_VALUE INVALID_ENTITY_INDEX COMPLEX_QUERY_PAGE_LIMIT_NOT_IN_RANGE
//   EMPTY_FILTER_OPERATOR INVALID_FILTER_OPERATORS_COMBINATION INSUFFICIENT_FILTER_VALUES.
// MEASURED on real Forge (wolfaenpak, 2026-10-09, @forge/kvs 2.0.7; forge2/research/understand/
// real-forge-fidelity.md §3.1, the 24-case probe) — status, code and message as the platform answered:
//   FAIL_IF_EXISTS on an existing key        409 KEY_CONFLICT "Provided key already exists and cannot be overwritten"
//   transaction condition false / key absent 400 CONDITIONAL_CHECK_FAILED "Request failed due to conditional check
//                                                specified or optimistic locking" (atomic: nothing lands)
//   transaction of 26 operations             422 UNPROCESSABLE_ENTITY "Request cannot be processed due to one or more
//                                                semantic errors"
//   the same key twice in one transaction    400 KEY_DUPLICATION_ERROR "Duplicate key found in request"
//   batchSet of 26 items                     400 TOO_MANY_BATCH_ENTITIES "Number of entities to set was 26, but you can
//                                                only set a maximum of 25 entities at a time."
//   batchGet of a missing key                failedKeys[].error {code: KEY_NOT_FOUND, message: "Provided key does not exist"}
//   an undeclared entity                     404 SCHEMA_NOT_FOUND "The schema provided does not exist"
//   ttl {value: 0}                           400 INVALID_TTL "TTL value must be a positive integer, received: 0"
//   integer attribute 2^31 or "5"            400 INCORRECT_PROPERTY_TYPE 'Data type for property "n" is defined as "integer"'
// UNMEASURED (harness names for documented refusals): batchGet/batchDelete over 25 (same code as batchSet, its verb),
//   INVALID_REQUEST, INVALID_CONDITION, INCORRECT_PROPERTY_TYPE for non-integer attribute types, and a plain get/
//   delete of a missing key answering 404 KEY_NOT_FOUND (the SDK turns it into undefined / a no-op, as measured).
//
// CONSISTENCY (contract S2). kvs and storage-api-custom-entities: "The kvs.query method used by the Key-Value Store is
// eventually consistent. This means that the method returns data that may be slightly out of date." / "The kvs.get
// method, on the other hand, is strictly consistent. It will always return current data." (the same for entity().query
// and entity().get; forge2/research/robustness.md §10.1). So kvs.query and entity queries see a write only once it is
// QUERY_LAG_MS old in virtual time (until then the value before it, or no row), dated by its request's virtual send
// time (`handle(op, body, {t})`); get, batchGet, transaction conditions and FAIL_IF_EXISTS read the current state.
// A write the harness makes outside any invocation (no `t`: the v1 preload, test setup) is settled at once.

const KEY_RE = /^(?!\s+$)[a-zA-Z0-9:._\s\-#]+$/;
const LIMITS = { keyLength: 500, valueBytes: 240 * 1024, depth: 31, transactionOps: 25, batchKeys: 25, pageDefault: 10, pageMax: 100 };
const TTL_UNIT = { SECONDS: 1000, MINUTES: 60_000, HOURS: 3_600_000, DAYS: 86_400_000 };
// The docs give no size for "slightly out of date" (robustness.md §17 q10): 5 virtual seconds is the harness's policy,
// stated in the contract.
const QUERY_LAG_MS = 5_000;

class KvsError extends Error {
  constructor(status, code, message, extra = {}) { super(message); this.status = status; this.code = code; this.extra = extra; }
}

function depth(v) {
  if (v === null || typeof v !== 'object') return 0;
  let d = 0;
  for (const x of Object.values(v)) d = Math.max(d, depth(x));
  return d + 1;
}

// `floor()`: the earliest virtual time any later request can carry (the oldest running invocation's start, else `now`);
// a version older than the newest one a query at the floor sees is unreachable and dropped.
function createKvs({ entities: declared = [], now = () => Date.now(), floor = now } = {}) {
  let kv = new Map();
  let secrets = new Map();
  let ents = new Map(); // `${entity}\u0000${key}` -> record
  // What the query index has not caught up with, per kv / entity record id: {base: the record before the first write
  // still in flight, versions: [{at, rec}] in write order (rec undefined = deleted)}. Absent = the index is current.
  let kvHist = new Map();
  let entHist = new Map();
  let at = null; // the virtual send time of the request being handled; null = a harness write, settled at once
  const entityDefs = new Map(declared.map((e) => [e.name, e]));
  const histOf = (map) => (map === kv ? kvHist : map === ents ? entHist : null);
  // Drop the versions no query at or after the floor can see: everything older than the newest version visible there.
  const settle = (hist, id, h) => {
    const f = floor();
    for (let i = h.versions.length - 1; i >= 0; i--) {
      if (h.versions[i].at + QUERY_LAG_MS > f) continue;
      if (i === h.versions.length - 1) hist.delete(id);
      else { h.base = h.versions[i].rec; h.versions = h.versions.slice(i + 1); }
      return;
    }
  };
  const track = (map, id, before, after) => {
    const hist = histOf(map);
    if (!hist) return;
    if (at === null) { hist.delete(id); return; }
    let h = hist.get(id);
    if (!h) hist.set(id, (h = { base: before, versions: [] }));
    h.versions.push({ at, rec: after });
    settle(hist, id, h);
  };
  const put = (map, id, rec) => { track(map, id, map.get(id), rec); map.set(id, rec); };
  const drop = (map, id) => { if (!map.has(id)) return; track(map, id, map.get(id), undefined); map.delete(id); };
  // The record a query sent at `tq` sees: the newest write at least QUERY_LAG_MS old, else what preceded them all.
  const indexed = (map, id, tq) => {
    const hist = histOf(map);
    const h = hist.get(id);
    if (!h) return map.get(id);
    if (h.versions[h.versions.length - 1].at + QUERY_LAG_MS <= floor()) { hist.delete(id); return map.get(id); }
    for (let i = h.versions.length - 1; i >= 0; i--) if (h.versions[i].at + QUERY_LAG_MS <= tq) return h.versions[i].rec;
    return h.base;
  };
  const indexedIds = (map) => new Set([...map.keys(), ...histOf(map).keys()]);

  const checkKey = (key) => {
    if (key === undefined || key === null || key === '') throw new KvsError(400, 'EMPTY_KEY', 'Key cannot be empty.');
    if (typeof key !== 'string' || !KEY_RE.test(key)) throw new KvsError(400, 'INVALID_KEY', `The provided key does not match the regex: ${KEY_RE}`);
    if (key.length > LIMITS.keyLength) throw new KvsError(400, 'KEY_TOO_LONG', `The provided key has exceeded the maximum ${LIMITS.keyLength} characters.`, { limit: 'key-length' });
  };
  const checkValue = (value) => {
    const bytes = Buffer.byteLength(JSON.stringify(value ?? null));
    if (bytes > LIMITS.valueBytes) throw new KvsError(400, 'MAX_SIZE', 'The provided value has exceeded the maximum size limit.', { limit: 'value-size', bytes });
    if (depth(value) > LIMITS.depth) throw new KvsError(400, 'MAX_DEPTH', `The provided value has exceeded the maximum object depth (${LIMITS.depth}) limit.`, { limit: 'depth' });
  };
  const entityDef = (name) => {
    const def = entityDefs.get(name);
    if (!def) throw new KvsError(404, 'SCHEMA_NOT_FOUND', 'The schema provided does not exist');
    return def;
  };
  const TYPE_OK = {
    string: (v) => typeof v === 'string',
    integer: (v) => Number.isInteger(v) && v >= -2147483648 && v <= 2147483647,
    float: (v) => typeof v === 'number' && Number.isFinite(v),
    boolean: (v) => typeof v === 'boolean',
    any: () => true,
  };
  const checkEntityValue = (def, value) => {
    if (value === null || typeof value !== 'object' || Array.isArray(value)) throw new KvsError(400, 'INVALID_ENTITY_VALUE', 'Entity values must be objects.');
    for (const [attr, spec] of Object.entries(def.attributes ?? {})) {
      if (value[attr] === undefined || value[attr] === null) continue;
      const ok = TYPE_OK[spec.type];
      // Measured for `integer` (2^31 and "5" both answer this); the documented range is a 32-bit signed integer.
      if (ok && !ok(value[attr])) throw new KvsError(400, 'INCORRECT_PROPERTY_TYPE', `Data type for property "${attr}" is defined as "${spec.type}"`);
    }
  };
  const alive = (rec) => rec && !(rec.expireAt && rec.expireAt <= now());
  const meta = (key, rec, extra = {}) => ({ key, value: rec.value, createdAt: rec.createdAt, updatedAt: rec.updatedAt, ...(rec.expireAt ? { expireTime: new Date(rec.expireAt).toISOString() } : {}), ...extra });
  const write = (map, key, value, options = {}) => {
    const t = now();
    const prev = map.get(key);
    const live = alive(prev) ? prev : undefined;
    if (options.keyPolicy === 'FAIL_IF_EXISTS' && live) throw new KvsError(409, 'KEY_CONFLICT', 'Provided key already exists and cannot be overwritten');
    const ttl = options.ttl ? options.ttl.value * (TTL_UNIT[options.ttl.unit] ?? NaN) : null;
    if (ttl !== null && !(ttl > 0)) throw new KvsError(400, 'INVALID_TTL', `TTL value must be a positive integer, received: ${options.ttl?.value}`);
    const rec = { value, createdAt: live?.createdAt ?? t, updatedAt: t, expireAt: ttl ? t + ttl : null };
    put(map, key, rec);
    if (options.returnValue === 'PREVIOUS') return live ? meta(key, live) : undefined;
    if (options.returnValue === 'LATEST') return meta(key, rec);
    return undefined;
  };
  const read = (map, key) => {
    const rec = map.get(key);
    if (!alive(rec)) throw new KvsError(404, 'KEY_NOT_FOUND', `Key '${key}' not found.`);
    return rec;
  };
  const ekey = (name, key) => `${name}\u0000${key}`;
  const encodeCursor = (k) => Buffer.from(k).toString('base64url');
  const decodeCursor = (c) => (c ? Buffer.from(String(c), 'base64url').toString() : null);
  const pageSize = (limit) => {
    if (limit === undefined || limit === null) return LIMITS.pageDefault;
    if (!(Number.isInteger(limit) && limit >= 1 && limit <= LIMITS.pageMax)) throw new KvsError(400, 'COMPLEX_QUERY_PAGE_LIMIT_NOT_IN_RANGE', `The page limit must be set between 1 and ${LIMITS.pageMax}.`);
    return limit;
  };
  const cond = (c, v) => {
    const [a, b] = c.values ?? [];
    switch (c.condition) {
      case 'BEGINS_WITH': return typeof v === 'string' && v.startsWith(a);
      case 'BETWEEN': return v >= a && v <= b;
      case 'EQUAL_TO': return v === a;
      case 'NOT_EQUAL_TO': return v !== a;
      case 'GREATER_THAN': return v > a;
      case 'GREATER_THAN_EQUAL_TO': return v >= a;
      case 'LESS_THAN': return v < a;
      case 'LESS_THAN_EQUAL_TO': return v <= a;
      case 'CONTAINS': return typeof v === 'string' && v.includes(a);
      case 'NOT_CONTAINS': return typeof v === 'string' && !v.includes(a);
      case 'EXISTS': return v !== undefined && v !== null;
      case 'NOT_EXISTS': return v === undefined || v === null;
      default: throw new KvsError(400, 'INVALID_CONDITION', `Unknown condition ${c.condition}.`);
    }
  };
  const filtersMatch = (filters, value) => {
    if (!filters) return true;
    const ops = Object.keys(filters);
    if (ops.length > 1) throw new KvsError(400, 'INVALID_FILTER_OPERATORS_COMBINATION', 'Filter operators "and" and "or" cannot be present at the same level.');
    const op = ops[0];
    const list = filters[op] ?? [];
    if (!list.length) throw new KvsError(400, 'EMPTY_FILTER_OPERATOR', 'Filter operators "and" and "or" cannot be empty.');
    const hits = list.map((c) => {
      if (c.condition === 'BETWEEN' && (c.values ?? []).length < 2) throw new KvsError(400, 'INSUFFICIENT_FILTER_VALUES', 'The specified condition needs at least two values.');
      return cond(c, value?.[c.property]);
    });
    return op === 'and' ? hits.every(Boolean) : hits.some(Boolean);
  };
  const indexOf = (def, name) => {
    for (const ix of def.indexes ?? []) {
      if (typeof ix === 'string' && ix === name) return { name: ix, partition: [], range: [ix] };
      if (ix && typeof ix === 'object' && ix.name === name) return { name: ix.name, partition: ix.partition ?? [], range: ix.range ?? [] };
    }
    throw new KvsError(400, 'INVALID_ENTITY_INDEX', `Index '${name}' is not declared for entity '${def.name}'.`);
  };

  const ops = {
    '/api/v1/get': (b) => { checkKey(b.key); return meta(b.key, read(kv, b.key)); },
    '/api/v1/set': (b) => { checkKey(b.key); checkValue(b.value); return write(kv, b.key, b.value, b.options); },
    '/api/v1/delete': (b) => { checkKey(b.key); read(kv, b.key); drop(kv, b.key); return undefined; },
    '/api/v1/secret/get': (b) => { checkKey(b.key); return meta(b.key, read(secrets, b.key)); },
    '/api/v1/secret/set': (b) => { checkKey(b.key); checkValue(b.value); return write(secrets, b.key, b.value, b.options); },
    '/api/v1/secret/delete': (b) => { checkKey(b.key); read(secrets, b.key); drop(secrets, b.key); return undefined; },
    '/api/v1/query': (b) => {
      const limit = pageSize(b.limit);
      const tq = at ?? now();
      const view = new Map();
      for (const k of indexedIds(kv)) { const rec = indexed(kv, k, tq); if (alive(rec)) view.set(k, rec); }
      let keys = [...view.keys()].sort();
      const w = b.where?.[0];
      if (b.where && b.where.length > 1) throw new KvsError(400, 'INVALID_CONDITION', 'There may only be a single where condition for a query.');
      if (w) {
        if (w.property !== 'key' || w.condition !== 'BEGINS_WITH') throw new KvsError(400, 'INVALID_CONDITION', 'The only condition supported by the Key-value store is beginsWith on key.');
        keys = keys.filter((k) => k.startsWith(w.values[0]));
      }
      const after = decodeCursor(b.after);
      if (after !== null) keys = keys.filter((k) => k > after);
      const page = keys.slice(0, limit);
      const more = keys.length > page.length;
      return { data: page.map((k) => ({ key: k, value: view.get(k).value })), ...(more ? { cursor: encodeCursor(page[page.length - 1]) } : {}) };
    },
    '/api/v1/entity/get': (b) => { entityDef(b.entityName); checkKey(b.key); return meta(b.key, read(ents, ekey(b.entityName, b.key))); },
    '/api/v1/entity/set': (b) => {
      const def = entityDef(b.entityName);
      checkKey(b.key); checkValue(b.value); checkEntityValue(def, b.value);
      return write(ents, ekey(b.entityName, b.key), b.value, b.options);
    },
    '/api/v1/entity/delete': (b) => { entityDef(b.entityName); checkKey(b.key); read(ents, ekey(b.entityName, b.key)); drop(ents, ekey(b.entityName, b.key)); return undefined; },
    '/api/v1/entity/query': (b) => {
      const def = entityDef(b.entityName);
      const ix = indexOf(def, b.indexName);
      const limit = pageSize(b.limit);
      const part = b.partition ?? [];
      if (ix.partition.length && part.length !== ix.partition.length) throw new KvsError(400, 'INVALID_PARTITION', `Index '${ix.name}' needs a partition of ${ix.partition.length} value(s).`);
      const tq = at ?? now();
      const rows = [];
      for (const k of indexedIds(ents)) {
        const rec = indexed(ents, k, tq);
        const [name, key] = k.split('\u0000');
        if (name !== b.entityName || !alive(rec)) continue;
        const v = rec.value;
        if (ix.partition.some((a, i) => v[a] !== part[i])) continue;
        if (ix.range.some((a) => v[a] === undefined || v[a] === null)) continue;
        if (b.range && !cond(b.range, v[ix.range[0]])) continue;
        if (!filtersMatch(b.filters, v)) continue;
        rows.push({ key, rec, sortKey: ix.range.map((a) => v[a]) });
      }
      const dir = b.sort === 'DESC' ? -1 : 1;
      rows.sort((x, y) => {
        for (let i = 0; i < x.sortKey.length; i++) {
          if (x.sortKey[i] < y.sortKey[i]) return -dir;
          if (x.sortKey[i] > y.sortKey[i]) return dir;
        }
        return x.key < y.key ? -dir : x.key > y.key ? dir : 0;
      });
      const after = decodeCursor(b.cursor);
      const start = after === null ? 0 : rows.findIndex((r) => r.key === after) + 1;
      const page = rows.slice(start, start + limit);
      const more = start + page.length < rows.length;
      return { data: page.map((r) => ({ key: r.key, value: r.rec.value })), ...(more ? { cursor: encodeCursor(page[page.length - 1].key) } : {}) };
    },
    '/api/v1/batch/set': (items) => batch(items, 'set', (it) => {
      if (it.entityName) { const def = entityDef(it.entityName); checkKey(it.key); checkValue(it.value); checkEntityValue(def, it.value); write(ents, ekey(it.entityName, it.key), it.value, it.options); }
      else { checkKey(it.key); checkValue(it.value); write(kv, it.key, it.value, it.options); }
      return { key: it.key, ...(it.entityName ? { entityName: it.entityName } : {}) };
    }),
    '/api/v1/batch/delete': (items) => batch(items, 'delete', (it) => {
      checkKey(it.key);
      if (it.entityName) { entityDef(it.entityName); drop(ents, ekey(it.entityName, it.key)); } else drop(kv, it.key);
      return { key: it.key, ...(it.entityName ? { entityName: it.entityName } : {}) };
    }),
    '/api/v1/batch/get': (items) => batch(items, 'get', (it) => {
      checkKey(it.key);
      const rec = it.entityName ? (entityDef(it.entityName), ents.get(ekey(it.entityName, it.key))) : kv.get(it.key);
      if (!alive(rec)) throw new KvsError(404, 'KEY_NOT_FOUND', 'Provided key does not exist');
      return { key: it.key, ...(it.entityName ? { entityName: it.entityName } : {}), value: rec.value, createdAt: rec.createdAt, updatedAt: rec.updatedAt };
    }),
    '/api/v1/transaction': (b) => {
      const sets = b.set ?? [];
      const dels = b.delete ?? [];
      const checks = b.check ?? [];
      const n = sets.length + dels.length + checks.length;
      if (n > LIMITS.transactionOps) throw new KvsError(422, 'UNPROCESSABLE_ENTITY', 'Request cannot be processed due to one or more semantic errors', { limit: 'transaction-ops' });
      const seen = new Set();
      for (const op of [...sets, ...dels, ...checks]) {
        const id = `${op.entityName ?? ''}\u0000${op.key}`;
        if (seen.has(id)) throw new KvsError(400, 'KEY_DUPLICATION_ERROR', 'Duplicate key found in request');
        seen.add(id);
        checkKey(op.key);
      }
      const conditionHolds = (op) => {
        if (!op.conditions) return true;
        const rec = ents.get(ekey(op.entityName, op.key));
        return alive(rec) && filtersMatch(op.conditions, rec.value);
      };
      for (const op of [...sets, ...dels, ...checks]) {
        if (op.entityName) entityDef(op.entityName);
        if (!conditionHolds(op)) throw new KvsError(400, 'CONDITIONAL_CHECK_FAILED', 'Request failed due to conditional check specified or optimistic locking');
      }
      for (const op of sets) {
        checkValue(op.value);
        if (op.entityName) { checkEntityValue(entityDef(op.entityName), op.value); write(ents, ekey(op.entityName, op.key), op.value, op.options); }
        else write(kv, op.key, op.value, op.options);
      }
      for (const op of dels) drop(op.entityName ? ents : kv, op.entityName ? ekey(op.entityName, op.key) : op.key);
      return undefined;
    },
  };
  function batch(items, verb, fn) {
    if (!Array.isArray(items)) throw new KvsError(400, 'INVALID_REQUEST', 'Batch requests take an array of items.');
    if (items.length > LIMITS.batchKeys) {
      throw new KvsError(400, 'TOO_MANY_BATCH_ENTITIES', `Number of entities to ${verb} was ${items.length}, but you can only ${verb} a maximum of ${LIMITS.batchKeys} entities at a time.`, { limit: 'batch-keys' });
    }
    const successfulKeys = [];
    const failedKeys = [];
    for (const it of items) {
      try { successfulKeys.push(fn(it)); } catch (e) {
        if (!(e instanceof KvsError)) throw e;
        failedKeys.push({ key: it.key, ...(it.entityName ? { entityName: it.entityName } : {}), error: { code: e.code, message: e.message } });
      }
    }
    return { successfulKeys, failedKeys };
  }

  // -> {status, body, error?}. `t`: the request's virtual send time (an invocation's; absent for the harness's own writes).
  const handle = (op, body, { t = null } = {}) => {
    const fn = ops[op];
    if (!fn) return { status: 501, body: { code: 'EMULATOR_NOT_MODELLED', message: `KVS ${op} is not modelled` }, notModelled: true };
    at = Number.isFinite(t) ? t : null;
    try {
      const out = fn(body ?? {});
      return out === undefined ? { status: 204, body: undefined } : { status: 200, body: out };
    } catch (e) {
      if (e instanceof KvsError) return { status: e.status, body: { code: e.code, message: e.message }, error: { code: e.code, ...e.extra } };
      throw e;
    } finally { at = null; }
  };
  const snapshot = () => ({
    kvs: Object.fromEntries([...kv].filter(([, r]) => alive(r)).map(([k, r]) => [k, r.value])),
    secrets: [...secrets.keys()].filter((k) => alive(secrets.get(k))).sort(),
    entities: [...ents].filter(([, r]) => alive(r)).reduce((acc, [k, r]) => {
      const [name, key] = k.split('\u0000');
      (acc[name] ??= {})[key] = r.value;
      return acc;
    }, {}),
  });
  // `index`: the writes the query index has not caught up with (forge-dev carries them from one process to the next). A
  // dump without it was written outside any invocation (the v1 preload): settled, nothing in flight.
  const dump = () => ({ kv: [...kv], secrets: [...secrets], ents: [...ents], index: { kv: [...kvHist], ents: [...entHist] } });
  const load = (d) => {
    kv = new Map(d.kv); secrets = new Map(d.secrets); ents = new Map(d.ents);
    kvHist = new Map(d.index?.kv ?? []); entHist = new Map(d.index?.ents ?? []);
  };
  const clear = () => { kv = new Map(); secrets = new Map(); ents = new Map(); kvHist = new Map(); entHist = new Map(); };
  return { handle, snapshot, dump, load, clear, LIMITS, ops: Object.keys(ops) };
}

module.exports = { createKvs, LIMITS, QUERY_LAG_MS };
