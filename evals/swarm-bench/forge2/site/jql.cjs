'use strict';
// The JQL subset the site evaluates (DESIGN.md §5.3). Three outcomes for anything outside it:
//   JqlError        -> 400 with Jira's wording (a malformed or invalid query: the app's defect)
//   NotModelledError -> 501 EMULATOR_NOT_MODELLED + harness_missing (valid Jira the site lacks: a harness gap)
// Fields: project, key/issuekey/issue/id, sprint, created, updated, status, statusCategory, issuetype/type,
// labels, assignee, reporter, creator, summary, priority, cf[id], customfield_id and field names.
// Operators = != in not in > >= < <= is is not ~ !~, AND OR NOT, parentheses, ORDER BY.
// Functions openSprints() closedSprints() futureSprints() currentUser() now() startOf/endOf Day|Week|Month|Year().

class JqlError extends Error {}
class NotModelledError extends Error {}

// Valid Jira JQL the subset does not evaluate: a query using one of these is a harness gap, not the app's error.
const KNOWN_UNMODELLED_FIELDS = new Set(['affectedversion', 'approvals', 'attachments', 'category', 'comment', 'component',
  'description', 'due', 'duedate', 'environment', 'epic link', 'filter', 'fixversion', 'hierarchylevel', 'issuelink',
  'issuelinktype', 'lastviewed', 'level', 'organization', 'originalestimate', 'parent', 'remainingestimate', 'resolution',
  'resolved', 'resolutiondate', 'statuscategorychangeddate', 'text', 'timespent', 'voter', 'votes', 'watcher', 'watchers',
  'worklogauthor', 'worklogcomment', 'worklogdate', 'workratio', 'team', 'subtasks', 'request-channel-type', 'savedfilter',
  'searchrequest', 'timeestimate', 'timeoriginalestimate', 'aggregatetimeoriginalestimate', 'rank', 'security', 'parentepic',
  'development', 'attachment', 'issuefunction', 'property', 'issue.property']);
const KNOWN_UNMODELLED_FUNCTIONS = new Set(['membersof', 'issuehistory', 'updatedby', 'watchedissues', 'votedissues', 'linkedissues',
  'earliestunreleasedversion', 'latestreleasedversion', 'releasedversions', 'unreleasedversions', 'componentsleadbyuser',
  'projectsleadbyuser', 'projectswhereuserhaspermission', 'projectswhereuserhasrole', 'currentlogin', 'lastlogin',
  'standardissuetypes', 'subtaskissuetypes', 'cascadeoption', 'approved', 'approver', 'breached', 'completed', 'elapsed',
  'everbreached', 'myapproval', 'mypending', 'paused', 'pending', 'pendingby', 'remaining', 'running', 'withincalendarhours',
  'earliestunreleasedversionbyreleasedate', 'latestreleasedversionbyreleasedate', 'issuekeys', 'childissuesof',
  'portfoliochildissuesof', 'projectsofcategory', 'attachmentsof', 'startofday', 'endofday']);

const KEYWORDS = new Set(['and', 'or', 'not', 'in', 'is', 'empty', 'null', 'order', 'by', 'asc', 'desc', 'was', 'changed']);

function tokenize(src) {
  const out = [];
  let i = 0;
  while (i < src.length) {
    const c = src[i];
    if (/\s/.test(c)) { i++; continue; }
    if (c === '"' || c === "'") {
      let j = i + 1;
      let s = '';
      while (j < src.length && src[j] !== c) { if (src[j] === '\\' && j + 1 < src.length) j++; s += src[j++]; }
      if (j >= src.length) throw new JqlError(`Error in the JQL Query: The quoted string '${s}' has not been completed.`);
      out.push({ t: 'str', v: s });
      i = j + 1;
      continue;
    }
    const two = src.slice(i, i + 2);
    if (['!=', '>=', '<=', '!~'].includes(two)) { out.push({ t: 'op', v: two }); i += 2; continue; }
    if ('=<>~'.includes(c)) { out.push({ t: 'op', v: c }); i++; continue; }
    if ('(),'.includes(c)) { out.push({ t: c }); i++; continue; }
    if (c === '[') { // cf[12345]
      const j = src.indexOf(']', i);
      out.push({ t: 'bracket', v: src.slice(i + 1, j) });
      i = j + 1;
      continue;
    }
    let j = i;
    while (j < src.length && !/[\s(),=<>~!"'[\]]/.test(src[j])) j++;
    if (j === i) throw new JqlError(`Error in the JQL Query: The character '${c}' is a reserved JQL character. You must enclose it in a string or use the escape '\\u${c.charCodeAt(0).toString(16).padStart(4, '0')}' instead. (line 1, character ${i + 1})`);
    const word = src.slice(i, j);
    out.push(KEYWORDS.has(word.toLowerCase()) ? { t: 'kw', v: word.toLowerCase() } : { t: 'word', v: word });
    i = j;
  }
  return out;
}

function parse(src) {
  const toks = tokenize(src);
  let p = 0;
  const peek = () => toks[p];
  const isKw = (v) => peek()?.t === 'kw' && peek().v === v;
  const expect = (t) => { const k = toks[p++]; if (!k || k.t !== t) throw new JqlError(`Error in the JQL Query: Expecting '${t}' but got '${k ? k.v ?? k.t : 'end of query'}'.`); return k; };

  const fieldRef = () => {
    const k = toks[p++];
    if (!k || !['word', 'str'].includes(k.t)) throw new JqlError(`Error in the JQL Query: Expecting a field name but got '${k ? k.v ?? k.t : 'end of query'}'.`);
    if (k.t === 'word' && peek()?.t === 'bracket') return { name: `${k.v}[${toks[p++].v}]` };
    return { name: k.v };
  };
  const value = () => {
    const k = toks[p++];
    if (!k) throw new JqlError('Error in the JQL Query: Expecting either a value, list or function but got end of query.');
    if (k.t === 'str') return { kind: 'lit', v: k.v };
    if (k.t === 'word') {
      if (peek()?.t === '(') {
        p++;
        const args = [];
        while (peek() && peek().t !== ')') {
          const a = toks[p++];
          args.push(a.v);
          if (peek()?.t === ',') p++;
        }
        expect(')');
        return { kind: 'fn', name: k.v, args };
      }
      return { kind: 'lit', v: k.v };
    }
    if (k.t === 'kw' && (k.v === 'empty' || k.v === 'null')) return { kind: 'empty' };
    throw new JqlError(`Error in the JQL Query: Expecting either a value, list or function but got '${k.v ?? k.t}'.`);
  };
  const list = () => {
    expect('(');
    const vals = [];
    while (peek() && peek().t !== ')') {
      vals.push(value());
      if (peek()?.t === ',') p++;
    }
    expect(')');
    return vals;
  };
  const clause = () => {
    if (peek()?.t === '(') { p++; const e = orExpr(); expect(')'); return e; }
    if (isKw('not')) { p++; return { op: 'not', e: clause() }; }
    const field = fieldRef();
    const k = peek();
    if (!k) throw new JqlError(`Error in the JQL Query: Expecting operator but got end of query.`);
    if (k.t === 'kw' && (k.v === 'was' || k.v === 'changed')) throw new NotModelledError(`JQL history operator ${k.v.toUpperCase()}`);
    if (k.t === 'op') {
      p++;
      if (peek()?.t === '(' && ['=', '!='].includes(k.v)) throw new JqlError(`Error in the JQL Query: The operator '${k.v}' does not support the list value.`);
      return { op: k.v, field, val: value() };
    }
    if (k.t === 'kw' && k.v === 'in') { p++; return { op: 'in', field, vals: peek()?.t === '(' ? list() : [value()] }; }
    if (k.t === 'kw' && k.v === 'not') {
      p++;
      if (isKw('in')) { p++; return { op: 'notin', field, vals: peek()?.t === '(' ? list() : [value()] }; }
      throw new JqlError(`Error in the JQL Query: Expecting 'in' after 'not'.`);
    }
    if (k.t === 'kw' && k.v === 'is') {
      p++;
      const neg = isKw('not') ? (p++, true) : false;
      const v = value();
      if (v.kind !== 'empty') throw new JqlError(`Error in the JQL Query: The operator 'is' only supports EMPTY or NULL.`);
      return { op: neg ? 'isnotempty' : 'isempty', field };
    }
    throw new JqlError(`Error in the JQL Query: Expecting operator but got '${k.v ?? k.t}'.`);
  };
  const andExpr = () => {
    let e = clause();
    while (isKw('and')) { p++; e = { op: 'and', a: e, b: clause() }; }
    return e;
  };
  function orExpr() {
    let e = andExpr();
    while (isKw('or')) { p++; e = { op: 'or', a: e, b: andExpr() }; }
    return e;
  }
  let where = null;
  if (peek() && !isKw('order')) where = orExpr();
  const orderBy = [];
  if (isKw('order')) {
    p++;
    if (!isKw('by')) throw new JqlError(`Error in the JQL Query: Expecting 'by' after 'order'.`);
    p++;
    do {
      if (peek()?.t === ',') p++;
      const f = fieldRef();
      let dir = null;
      if (isKw('asc') || isKw('desc')) dir = toks[p++].v;
      orderBy.push({ field: f, dir });
    } while (peek()?.t === ',');
  }
  if (p < toks.length) throw new JqlError(`Error in the JQL Query: Expecting end of query but got '${toks[p].v ?? toks[p].t}'.`);
  return { where, orderBy };
}

const DAY = 86_400_000;
function parseDate(text, now, asEnd = false) {
  const s = String(text).trim();
  const rel = s.match(/^([+-]?)((?:\d+[wdhm]\s*)+)$/i);
  if (rel) {
    let ms = 0;
    for (const [, n, u] of rel[2].matchAll(/(\d+)([wdhm])/gi)) ms += Number(n) * { w: 7 * DAY, d: DAY, h: 3_600_000, m: 60_000 }[u.toLowerCase()];
    return now + (rel[1] === '-' ? -ms : ms);
  }
  const m = s.match(/^(\d{4})[-/](\d{1,2})[-/](\d{1,2})(?:\s+(\d{1,2}):(\d{2}))?$/);
  if (m) {
    const base = Date.UTC(+m[1], +m[2] - 1, +m[3], m[4] ? +m[4] : 0, m[5] ? +m[5] : 0);
    return m[4] || !asEnd ? base : base;
  }
  return null;
}

function dateFunction(name, args, now) {
  const fn = name.toLowerCase();
  if (fn === 'now') return now;
  const unit = fn.replace(/^(startof|endof)/, '');
  if (!['day', 'week', 'month', 'year'].includes(unit) || !/^(startof|endof)/.test(fn)) return undefined;
  const d = new Date(now);
  let start;
  if (unit === 'day') start = Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate());
  if (unit === 'week') start = Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate() - d.getUTCDay());
  if (unit === 'month') start = Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), 1);
  if (unit === 'year') start = Date.UTC(d.getUTCFullYear(), 0, 1);
  let offset = 0;
  if (args[0] !== undefined) {
    const o = String(args[0]).match(/^([+-]?)(\d+)([wdhmMy]?)$/);
    if (!o) throw new JqlError(`Error in the JQL Query: The function '${name}' has an invalid argument '${args[0]}'.`);
    const n = Number(o[2]) * (o[1] === '-' ? -1 : 1);
    const u = o[3] || { day: 'd', week: 'w', month: 'M', year: 'y' }[unit];
    if (u === 'M' || u === 'y') {
      const sd = new Date(start);
      start = u === 'M' ? Date.UTC(sd.getUTCFullYear(), sd.getUTCMonth() + n, sd.getUTCDate()) : Date.UTC(sd.getUTCFullYear() + n, sd.getUTCMonth(), sd.getUTCDate());
    } else offset = n * { w: 7 * DAY, d: DAY, h: 3_600_000, m: 60_000 }[u];
  }
  const span = { day: DAY, week: 7 * DAY, month: null, year: null }[unit];
  if (fn.startsWith('startof')) return start + offset;
  if (span) return start + offset + span - 1;
  const sd = new Date(start);
  const next = unit === 'month' ? Date.UTC(sd.getUTCFullYear(), sd.getUTCMonth() + 1, 1) : Date.UTC(sd.getUTCFullYear() + 1, 0, 1);
  return next - 1 + offset;
}

// model: { fields, sprints, statuses, issueTypes, projects, users, now, currentUser, appAccountId }
//   now(): the request's instant (epoch ms), what relative dates and date functions count from
// view(issue) must expose: id, key, projectKey, projectId, fields (the current field map)
// -> { bounded, matches, compare, timed }: `timed` when the query read now() (a date value or a date function), so
// a caller caching its hits keys them on the instant as well.
function compile(src, model) {
  const ast = parse(src);
  let timed = false;
  const now = () => { timed = true; return model.now(); };
  const fieldsByName = new Map();
  for (const f of model.fields) {
    for (const c of f.clauseNames ?? []) fieldsByName.set(c.toLowerCase(), f);
    fieldsByName.set(f.name.toLowerCase(), f);
    fieldsByName.set(f.id.toLowerCase(), f);
  }
  const SYSTEM = { key: 'key', issuekey: 'key', issue: 'key', id: 'key', project: 'project', sprint: 'sprint',
    created: 'created', createddate: 'created', updated: 'updated', updateddate: 'updated', status: 'status',
    statuscategory: 'statusCategory', issuetype: 'issuetype', type: 'issuetype', labels: 'labels', assignee: 'assignee',
    reporter: 'reporter', creator: 'creator', summary: 'summary', priority: 'priority' };
  const resolveField = (ref) => {
    const lower = ref.name.toLowerCase();
    if (SYSTEM[lower]) return { kind: SYSTEM[lower] };
    const cfm = lower.match(/^cf\[(\d+)\]$/);
    const f = cfm ? model.fields.find((x) => x.id === `customfield_${cfm[1]}`) : fieldsByName.get(lower);
    if (f && f.custom) {
      if (f.schema?.custom?.endsWith(':gh-sprint')) return { kind: 'sprint' };
      return { kind: 'custom', field: f };
    }
    if (KNOWN_UNMODELLED_FIELDS.has(lower) || (f && !f.custom)) throw new NotModelledError(`JQL field '${ref.name}'`);
    throw new JqlError(`Field '${ref.name}' does not exist or you do not have permission to view it.`);
  };
  const sprintSet = (state) => new Set(model.sprints.filter((s) => s.state === state).map((s) => s.id));
  const fnValue = (v, kind) => {
    const name = v.name.toLowerCase();
    // openSprints(): "assigned to a sprint that was started, but has not yet been completed" (support.atlassian.com,
    // JQL functions) — ACTIVE only. Measured on Jira Cloud 2026-10-02: nine issues whose only sprints are future
    // are returned by futureSprints() and by `sprint not in openSprints()`, never by `sprint in openSprints()`.
    if (name === 'opensprints') return [...sprintSet('active')];
    if (name === 'closedsprints') return [...sprintSet('closed')];
    if (name === 'futuresprints') return [...sprintSet('future')];
    if (name === 'currentuser') {
      if (!model.currentUser) return [];
      return [model.currentUser];
    }
    const d = dateFunction(v.name, v.args, now());
    if (d !== undefined) {
      if (!['created', 'updated'].includes(kind)) throw new JqlError(`Error in the JQL Query: The function '${v.name}' is not supported for this field.`);
      return [d];
    }
    if (KNOWN_UNMODELLED_FUNCTIONS.has(name)) throw new NotModelledError(`JQL function '${v.name}()'`);
    throw new JqlError(`Unable to find JQL function '${v.name}()'.`);
  };
  // Literal values -> canonical comparable values for the field kind.
  const canon = (kind, field, v) => {
    if (v.kind === 'fn') return fnValue(v, kind);
    if (v.kind === 'empty') return [null];
    const raw = String(v.v);
    switch (kind) {
      case 'project': {
        const p = model.projects.find((x) => x.key.toLowerCase() === raw.toLowerCase() || x.id === raw || x.name.toLowerCase() === raw.toLowerCase());
        if (!p) throw new JqlError(`The value '${raw}' does not exist for the field 'project'.`);
        return [p.key];
      }
      case 'sprint': {
        const s = model.sprints.filter((x) => String(x.id) === raw || x.name.toLowerCase() === raw.toLowerCase());
        if (!s.length) throw new JqlError(`Sprint with name or id '${raw}' does not exist or you do not have permission to view it.`);
        return s.map((x) => x.id);
      }
      case 'status': {
        const s = model.statuses.find((x) => x.id === raw || x.name.toLowerCase() === raw.toLowerCase());
        if (!s) throw new JqlError(`The value '${raw}' does not exist for the field 'status'.`);
        return [s.id];
      }
      case 'statusCategory': {
        const cats = model.statuses.map((x) => x.statusCategory);
        const c = cats.find((x) => String(x.id) === raw || x.key.toLowerCase() === raw.toLowerCase() || x.name.toLowerCase() === raw.toLowerCase());
        if (!c) throw new JqlError(`The value '${raw}' does not exist for the field 'statusCategory'.`);
        return [c.key];
      }
      case 'issuetype': {
        const t = model.issueTypes.find((x) => x.id === raw || x.name.toLowerCase() === raw.toLowerCase());
        if (!t) throw new JqlError(`The value '${raw}' does not exist for the field 'issuetype'.`);
        return [t.id];
      }
      case 'created': case 'updated': {
        const d = parseDate(raw, now());
        if (d === null) throw new JqlError(`Date value '${raw}' for field '${kind}' is invalid. Valid formats include: 'yyyy/MM/dd HH:mm', 'yyyy-MM-dd HH:mm', 'yyyy/MM/dd', 'yyyy-MM-dd', or a period format e.g. '-5d', '4w 2d'.`);
        return [d];
      }
      case 'key': {
        if (/^\d+$/.test(raw)) return [raw];
        if (!/^[A-Z][A-Z0-9_]*-\d+$/i.test(raw)) throw new JqlError(`The issue key '${raw}' for field 'key' is invalid.`);
        return [raw.toUpperCase()];
      }
      case 'custom':
        if (field.schema.type === 'number') {
          if (!/^-?\d+(\.\d+)?$/.test(raw)) throw new JqlError(`The value '${raw}' is not a valid number for the field '${field.name}'.`);
          return [Number(raw)];
        }
        return [raw];
      default:
        return [raw];
    }
  };
  const current = (kind, field, issue) => {
    const f = issue.fields;
    switch (kind) {
      case 'project': return [issue.projectKey];
      case 'key': return [issue.key, issue.id];
      case 'sprint': return (f[model.sprintFieldId] ?? []).map((s) => s.id);
      case 'created': return [Date.parse(f.created)];
      case 'updated': return [Date.parse(f.updated)];
      case 'status': return [f.status.id];
      case 'statusCategory': return [f.status.statusCategory.key];
      case 'issuetype': return [f.issuetype.id];
      case 'labels': return f.labels ?? [];
      case 'assignee': case 'reporter': case 'creator': return f[kind] ? [f[kind].accountId] : [];
      case 'summary': return [f.summary];
      case 'priority': return f.priority ? [f.priority.name] : [];
      case 'custom': {
        const v = f[field.id];
        if (v === null || v === undefined) return [];
        return Array.isArray(v) ? v.map((x) => (typeof x === 'object' ? x.value ?? x.name ?? x.id : x)) : [typeof v === 'object' ? v.value ?? v.name ?? v.id : v];
      }
      default: return [];
    }
  };
  const keyOrder = (k) => { const m = String(k).match(/^(.*)-(\d+)$/); return m ? [m[1], Number(m[2])] : [String(k), 0]; };
  const cmp = (kind, a, b) => {
    if (kind === 'key') {
      const [pa, na] = keyOrder(a);
      const [pb, nb] = keyOrder(b);
      if (pa !== pb) return null;
      return na - nb;
    }
    if (typeof a === 'number' && typeof b === 'number') return a - b;
    return null;
  };
  const eq = (kind, a, b) => (typeof a === 'string' && typeof b === 'string' ? a.toLowerCase() === b.toLowerCase() : a === b);
  // Three-valued, as Jira evaluates it: a comparison against an EMPTY field is UNKNOWN (null), NOT keeps
  // UNKNOWN, and only TRUE matches. Measured on Jira Cloud 2026-10-02: issues with no sprint are returned by
  // neither `sprint not in openSprints()` nor `NOT sprint in openSprints()`.
  const evalNode = (n, issue) => {
    switch (n.op) {
      case 'and': { const a = evalNode(n.a, issue); if (a === false) return false; const b = evalNode(n.b, issue); return b === false ? false : (a === null || b === null ? null : true); }
      case 'or': { const a = evalNode(n.a, issue); if (a === true) return true; const b = evalNode(n.b, issue); return b === true ? true : (a === null || b === null ? null : false); }
      case 'not': { const e = evalNode(n.e, issue); return e === null ? null : !e; }
      default: break;
    }
    const { kind, field } = n.r;
    const cur = current(kind, field, issue);
    if (n.op === 'isempty') return cur.length === 0;
    if (n.op === 'isnotempty') return cur.length > 0;
    if (cur.length === 0 && !((n.op === '=' || n.op === 'in') && n.vals.flat().includes(null))) return null;
    if (n.op === '~' || n.op === '!~') {
      const words = String(n.vals[0][0]).toLowerCase().split(/\s+/).filter(Boolean);
      const hit = cur.some((c) => words.every((w) => String(c).toLowerCase().includes(w.replace(/[*?]/g, ''))));
      return n.op === '~' ? hit : !hit;
    }
    const wanted = n.vals.flat();
    if (n.op === '=' || n.op === 'in') return wanted.some((w) => (w === null ? cur.length === 0 : cur.some((c) => eq(kind, c, w))));
    if (n.op === '!=' || n.op === 'notin') return cur.length > 0 && !wanted.some((w) => cur.some((c) => eq(kind, c, w)));
    const w = wanted[0];
    return cur.some((c) => {
      const d = cmp(kind, c, w);
      if (d === null) return false;
      return { '>': d > 0, '>=': d >= 0, '<': d < 0, '<=': d <= 0 }[n.op];
    });
  };
  // Resolve fields and canonicalise values once, so value errors surface before any issue is read.
  const prepare = (n) => {
    if (!n) return;
    if (n.op === 'and' || n.op === 'or') { prepare(n.a); prepare(n.b); return; }
    if (n.op === 'not') { prepare(n.e); return; }
    n.r = resolveField(n.field);
    if (n.op === 'isempty' || n.op === 'isnotempty') return;
    if (n.op === '~' || n.op === '!~') {
      if (!['summary', 'custom'].includes(n.r.kind)) throw new JqlError(`The operator '${n.op}' is not supported by the '${n.field.name}' field.`);
      n.vals = [[n.val.v]];
      return;
    }
    const vals = n.vals ?? [n.val];
    if (['>', '>=', '<', '<='].includes(n.op) && !['created', 'updated', 'key', 'custom'].includes(n.r.kind)) {
      throw new JqlError(`The operator '${n.op}' is not supported by the '${n.field.name}' field.`);
    }
    n.vals = vals.map((v) => canon(n.r.kind, n.r.field, v));
  };
  prepare(ast.where);
  if (ast.orderBy.length > 7) throw new JqlError('JQL can be ordered by at most 7 fields.');
  const ORDERABLE = new Set(['key', 'created', 'updated', 'summary', 'status', 'priority', 'project', 'custom', 'issuetype', 'assignee', 'reporter']);
  const order = ast.orderBy.map((o) => {
    const r = resolveField(o.field);
    if (!ORDERABLE.has(r.kind)) throw new NotModelledError(`JQL ORDER BY ${o.field.name}`);
    return { ...r, desc: o.dir ? o.dir === 'desc' : ['created', 'updated'].includes(r.kind) };
  });
  const sortValue = (kind, field, issue) => {
    if (kind === 'key') return keyOrder(issue.key);
    const v = current(kind, field, issue)[0];
    return v === undefined ? null : v;
  };
  const compare = (x, y) => {
    for (const o of order) {
      let a = sortValue(o.kind, o.field, x);
      let b = sortValue(o.kind, o.field, y);
      let d = 0;
      if (Array.isArray(a)) d = a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : a[1] - b[1];
      else if (a === null || b === null) d = a === b ? 0 : a === null ? 1 : -1;
      else if (typeof a === 'number' && typeof b === 'number') d = a - b;
      else { a = String(a).toLowerCase(); b = String(b).toLowerCase(); d = a < b ? -1 : a > b ? 1 : 0; }
      if (d) return o.desc ? -d : d;
    }
    // Jira's default order without ORDER BY (measured: newest issue first).
    return Number(y.id) - Number(x.id);
  };
  return {
    bounded: ast.where !== null,
    matches: (issue) => (ast.where ? evalNode(ast.where, issue) === true : true),
    compare,
    timed,
  };
}

module.exports = { compile, parse, tokenize, JqlError, NotModelledError, parseDate };
