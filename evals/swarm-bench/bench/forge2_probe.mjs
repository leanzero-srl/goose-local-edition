// forge-2.0 probe (forge2/SPEC.md §3 P9): Forge 1.0's scoring drive (forge/DESIGN.md §8.7) on the forge2 site,
// extended into the 2.0 upgrade drive. The site and emulator start with Scope Ledger v1's KVS content (the preload),
// the entrant's v2 app takes over (the upgrade), and six virtual hours run as one ordered agenda: the site's delivery
// plan, its world schedule, the hourly scheduled triggers, the CI web-trigger sequence and the admin's UI Kit panel;
// then 1.0's heal/rerun/Rovo/UI lanes, the Custom UI boot counts and the Forge LLM cases. It writes the evidence
// bench/score_forge2.py (P7) and bench/forge2_checks.py (P8) grade: 1.0's keys plus the 2.0 contract keys (rate,
// invocations, migration, world, webtrigger, admin, field, llm_v2, boot) and `clock` (the upgrade and checkpoint times).
//
//   node forge2_probe.mjs --app <clone> --kit <kit dir> --seed <16 hex> --out obs.json --shots <dir>
//                         --runtime wrapper|shim --repo <evals/swarm-bench> [--media <dir>]
//   node forge2_probe.mjs --preflight
//   node forge2_probe.mjs --selftest      the 2.0 drive's agenda and evidence helpers against fakes (no site, no browser)
//
// Harness failures (the site or emulator cannot start, Chromium cannot launch, an interface this probe needs is
// absent) land in `sectionErrors[section]` — the scorer makes those rows UNAVAILABLE, never app zeros. App
// failures (a missing hook, a thrown resolver, a console error, a missing v2 module) are evidence and are recorded.
//
// The other packages' interfaces this probe calls (SPEC §3). Each absence is a named `i2Gaps` entry plus the
// sectionErrors of the contract keys it feeds:
//   P4 site.pack.v1Preload             {entities: {<name>: [{key, value}]}, keys: [{key, value}]}: v1's KVS content
//   P4 site.pack.admin                 the accountId holding Jira's global ADMINISTER (pack.viewer is a non-admin)
//   P4 site.state.plan / .st.cursor    the delivery plan (1.0's): the next delivery's change and its `created` time
//   P4 site.control.ratelog({since})   {entries: [{t_ms, invocation, kind, method, path_tpl, cost, status, reason,
//                                       retry_after_s}], next}: every /rest request the rate model priced; site.log
//                                       holds the same requests (with their paths) and rate.cjs writtenRefs what each writes
//   P5 site.control.world({until})     {applied: [{t_ms, class, detail}]}: applies the world schedule through `until`
//   P5 site.control.fieldvalues()      {values: {<issueKey>: string}}: the app's scope-status values on the site now
//   P5 emu.llm.phase(<name>)           restarts the scripted model on a named script (LLM_V2_CASES)
//   P1 emu.drainQueues({until}), emu.runScheduled(key, {until})   only deliveries due by `until` (virtual ms)
//   P1 emu.invokeResolver(moduleKey, functionKey, payload, context, asUser)   req.context built from `context`
//   P1 emu.webtriggerUrl(moduleKey)    the URL of the ingress P3 mounts at POST /x/webtrigger/<moduleKey>
//   P2 <kit>/lib/uikit-host/index.cjs  render({appDir, moduleKey, context, invoke}) -> {tree(), text(),
//                                       findByLabel(l), setValue(l, v), click(l), waitIdle(), invokes}
//   P3 <repo>/forge2/site/ci.cjs       scoringSequence({secret, issueKeys, nowSeconds}) -> [{case, headers, body}]
import { createRequire } from 'module';
import { execSync, spawnSync } from 'child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'fs';
import { dirname, join, relative, resolve } from 'path';

const require = createRequire(import.meta.url);
const args = process.argv.slice(2);
const opt = (name, fallback = null) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 && i + 1 < args.length ? args[i + 1] : fallback;
};
const log = (...a) => console.error('[forge2-probe]', ...a);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const SELFTEST = args.includes('--selftest');

function loadPlaywright() {
  const attempts = [];
  const tries = [
    () => (process.env.BENCH_BROWSER_MODULE ? require(process.env.BENCH_BROWSER_MODULE) : null),
    () => require('playwright'),
    () => createRequire(join(execSync('npm root -g', { encoding: 'utf8' }).trim(), '__probe__.js'))('playwright'),
    () => createRequire(join(dirname(process.execPath), '..', 'lib', 'node_modules', '__probe__.js'))('playwright'),
  ];
  for (const t of tries) {
    try {
      const pw = t();
      if (pw) return pw;
    } catch (e) {
      attempts.push(e.message.split('\n')[0]);
    }
  }
  throw new Error('cannot resolve playwright: ' + attempts.join(' | '));
}

async function launch() {
  const pw = loadPlaywright();
  const executablePath = process.env.BENCH_BROWSER_EXECUTABLE || undefined;
  return pw.chromium.launch({ headless: true, executablePath });
}

if (args.includes('--preflight')) {
  try {
    const b = await launch();
    await b.close();
    console.log(JSON.stringify({ ok: true, node: process.version, execPath: process.execPath }));
    process.exit(0);
  } catch (e) {
    console.log(JSON.stringify({ ok: false, node: process.version, error: String(e?.message || e) }));
    process.exit(3);
  }
}

const appDir = SELFTEST ? null : resolve(opt('app'));
const kitDir = SELFTEST ? null : resolve(opt('kit'));
const repo = SELFTEST ? null : resolve(opt('repo'));
const seed = opt('seed');
const outPath = SELFTEST ? null : resolve(opt('out'));
const shotsDir = SELFTEST ? null : resolve(opt('shots'));
const runtime = opt('runtime', 'wrapper');
// The graded browser recording (owner, 2026-10-03: Forge results carry video like SB7.2): every graded surface page
// is recorded and the pages are joined, in the order they were graded, into one clip under <tree>/bench-media.
const mediaDir = opt('media') ? resolve(opt('media')) : null;
const RECORD_SIZE = { width: 1280, height: 800 };   // SB7.1's recording frame (product_probe_sb71.mjs)
let recording = null;                               // { context, pages: [{page, label, openedAt}] }
if (!SELFTEST) mkdirSync(shotsDir, { recursive: true });

const obs = {
  schema: 'forge2-observations/1', runtime, seed, manifest: null, manifestError: null, kit: {}, lint: { runs: [] },
  build: { functions: [] }, phases: {}, rovo: { calls: [] }, comments: [], ui: { surfaces: [], calls: [] },
  harnessMissing: [], sectionErrors: {}, shots: [], i2Gaps: [],
  // ── the 2.0 contract (P9 ↔ P8); checkpoints are 'h1'..'h6' (virtual-hour marks after the upgrade) and 'final' ──
  clock: { upgrade_t_ms: null, checkpoints: {} },
  rate: { requests: [], hours: [] },
  invocations: [],
  migration: { v1_rows: [], v2_by_checkpoint: {}, v1_final: [], panel_by_checkpoint: {}, preload: null },
  world: [],
  webtrigger: [],
  admin: { actions: [], tree_text: '', secret_leaks: [] },
  field: { writes: [], values_by_checkpoint: {} },
  llm_v2: [],
  boot: {},
  checkpoints: {},
  upgrade: null,
  person_reads: [],
};
const save = () => writeFileSync(outPath, JSON.stringify(obs, null, 1));
// An interface this probe needs and the harness lacks: a harness gap, so the sections it feeds are unavailable.
const gap = (what, ...sections) => {
  if (!obs.i2Gaps.includes(what)) obs.i2Gaps.push(what);
  for (const s of sections) obs.sectionErrors[s] = obs.sectionErrors[s] || `I2 gap: ${what}`;
};

// ── kit identity and lint ×2 (DESIGN §7: the fixed point) ──────────────────────────────────

function kitPins() {
  const pins = {};
  const kitJson = join(kitDir, 'KIT.json');
  if (existsSync(kitJson)) Object.assign(pins, JSON.parse(readFileSync(kitJson, 'utf8')).app_modules || {});
  const forge = join(kitDir, 'app-modules', 'node_modules', '@forge');
  for (const name of existsSync(forge) ? readdirSync(forge) : []) {
    const p = join(forge, name, 'package.json');
    if (existsSync(p) && !(`@forge/${name}` in pins)) pins[`@forge/${name}`] = JSON.parse(readFileSync(p, 'utf8')).version;
  }
  return pins;
}

function runLint() {
  const bin = join(kitDir, 'bin', 'lint.cjs');
  if (!existsSync(bin)) throw new Error(`${bin} missing`);
  const r = spawnSync(process.execPath, [bin, '--json'], { cwd: appDir, encoding: 'utf8', maxBuffer: 64 << 20 });
  const line = (r.stdout || '').split('\n').map((l) => l.replace(/^LINT_JSON /, '')).reverse()
    .find((l) => l.trim().startsWith('{'));
  if (!line) return { crashed: true, exit: r.status, stderr: (r.stderr || '').slice(-400) };
  const j = JSON.parse(line);
  return {
    counts: j.counts ?? { errors: (j.problems || []).filter((p) => p.sev === 'error').length,
      warnings: (j.problems || []).filter((p) => p.sev === 'warning').length },
    problems: (j.problems || []).map(({ sev, file, line: ln, column, message, linter }) =>
      ({ sev, file: file ? relative(appDir, resolve(appDir, file)) : file, line: ln, column, message, linter })),
    stageReached: j.stageReached ?? j.stage ?? null, stagesTotal: j.stagesTotal ?? j.stages ?? null,
  };
}

// ── static import walk: which @forge packages a handler bundle pulls in ───────────────────

const IMPORT_RE = /(?:import\s[^'"]*?from\s*|import\s*\(\s*|require\s*\(\s*|export\s[^'"]*?from\s*)['"]([^'"]+)['"]/g;
function importClosure(entry) {
  const seen = new Set();
  const bare = new Set();
  const walk = (file) => {
    if (seen.has(file) || !existsSync(file)) return;
    seen.add(file);
    const text = readFileSync(file, 'utf8');
    for (const m of text.matchAll(IMPORT_RE)) {
      const spec = m[1];
      if (spec.startsWith('.')) {
        const base = resolve(dirname(file), spec);
        const hit = [base, ...['.js', '.jsx', '.ts', '.tsx', '.mjs', '.cjs'].map((x) => base + x),
          ...['index.js', 'index.ts'].map((x) => join(base, x))].find((p) => existsSync(p) && statSync(p).isFile());
        if (hit) walk(hit);
      } else if (!spec.startsWith('node:')) {
        bare.add(spec.startsWith('@') ? spec.split('/').slice(0, 2).join('/') : spec.split('/')[0]);
      }
    }
  };
  walk(entry);
  return [...bare];
}

function handlerEntry(handler) {
  const file = String(handler || '').split('.').slice(0, -1).join('.');
  return ['.ts', '.tsx', '.js', '.jsx', '.mjs'].map((x) => join(appDir, 'src', file + x)).find(existsSync) || null;
}

const NODE_BUILTINS = new Set(require('module').builtinModules);

// ── I2 normalisation (WP1's log -> the I5 call/invocation shapes score_forge.py reads) ────

const KIND = { trigger: 'trigger', consumer: 'consumer', scheduledTrigger: 'scheduled', action: 'action',
  'dashboards:widget': 'resolver', 'jira:sprintAction': 'resolver', 'jira:adminPage': 'resolver', webtrigger: 'webtrigger' };

// Scopes an operation whose CHOSEN OAuth2 alternative is empty (e.g. POST /rest/api/3/permissions/check: Current
// [] while its description names Classic read:jira-work, Granular read:permission:jira) still tolerates: every
// scope its other alternatives or its description name. l_scopes charges neither reading.
const toleratedCache = new Map();
let openapiSpecs = null;
function toleratedFor(op) {
  if (!op) return [];
  if (toleratedCache.has(op)) return toleratedCache.get(op);
  const [method, template] = op.split(' ');
  openapiSpecs ??= ['jira.json', 'jsw.json'].map((f) => join(kitDir, 'openapi', f)).filter(existsSync)
    .map((f) => JSON.parse(readFileSync(f, 'utf8')));
  const found = openapiSpecs.map((sp) => sp.paths?.[template]?.[method.toLowerCase()]).find(Boolean);
  const out = new Set();
  for (const a of found?.['x-atlassian-oauth2-scopes'] || []) for (const sc of a.scopes || []) out.add(sc);
  for (const m of String(found?.description || '').matchAll(/\*\*(?:Classic|Granular)\*\*:\s*([^\n]+)/g)) {
    for (const sc of m[1].matchAll(/`([a-z]+:[a-z0-9:.\-]+)`/g)) out.add(sc[1]);
  }
  const list = [...out];
  toleratedCache.set(op, list);
  return list;
}

// Virtual time as epoch seconds (WP1 logs ISO strings).
const vt = (t) => (typeof t === 'string' ? Date.parse(t) / 1000 : t ?? null);

function normCall(e) {
  const path = e.path ?? e.target ?? '';
  const service = e.service === 'kvs' || e.service === 'queue' || e.service === 'egress' ? e.service
    : e.service === 'jira' || e.service === undefined ? 'jira' : e.service;
  // WP1's site reports the OAuth2 alternative a call was satisfied by ({state, scopes}); a scope mismatch is a 401
  // with no alternative.
  const alt = e.scopes && !Array.isArray(e.scopes) && Array.isArray(e.scopes.scopes) ? e.scopes : null;
  return {
    t: vt(e.t_virtual ?? e.t), inv: e.invocationId ?? e.inv ?? null,
    kind: KIND[e.moduleType] ?? e.kind ?? (e.invocationId ? null : 'ui'), moduleKey: e.moduleKey ?? null,
    provider: e.provider ?? null, service, method: e.method, path, op: e.op ?? null, body: e.body ?? null, status: e.status,
    response: e.response ?? e.responseBody ?? undefined,
    scopes: alt ? { chosen: alt.scopes, state: alt.state, ...(alt.scopes.length ? {} : { tolerated: toleratedFor(e.op) }) }
      : (e.scopes ?? undefined),
    scopeMismatch: service === 'jira' && e.status === 401 && !alt && !e.needsAuthentication ? (e.op ?? path) : undefined,
    needsAuthentication: Boolean(e.needsAuthentication), missingScope: e.missingScope ?? null,
    fault: e.fault ?? null, earlyRetry: e.earlyRetry === true ? e.fault : (e.earlyRetry || null), limitError: e.limitError ?? null,
  };
}

function normInvocation(r, kind, moduleKey) {
  return {
    inv: r?.invocationId ?? r?.inv ?? null, kind, moduleKey: moduleKey ?? r?.moduleKey ?? null, functionKey: r?.functionKey ?? null,
    t0: vt(r?.t0), t1: vt(r?.t1), ok: Boolean(r?.ok), threw: r?.ok === false && !r?.timedOut,
    error: r?.error ? String(r.error.message ?? r.error) : null, errorName: r?.error?.name ?? r?.errorName ?? null,
    timedOut: Boolean(r?.timedOut), retryAfter: r?.result?._retry ? (r.result.retryOptions?.retryAfter ?? null)
      : (r?.retryAfter ?? null),
  };
}

// Each call an invocation made carries that invocation's lineage, which the kit's queue push hands to the consumer it
// starts: the product event (`originChange`) or the scheduled run (`scheduledRun`) its chain began with. 2.0 drains
// several chains in one phase (a backfill continuation beside an event's consumer), so the economy rows attribute calls
// by it. A kit without the invocation records stamps nothing, and the rows say they fell back.
const lineageOf = (emu, inv) => {
  const rec = inv && emu.invocations instanceof Map ? emu.invocations.get(inv) : null;
  return rec ? { originChange: rec.originChange ?? null, scheduledRun: rec.scheduledRun ?? null } : {};
};
let logCursor = 0;
const takeCalls = (emu) => {
  const all = emu.log || [];
  const fresh = all.slice(logCursor).map((e) => {
    const call = normCall(e);
    return call.inv ? { ...call, ...lineageOf(emu, call.inv) } : call;
  });
  logCursor = all.length;
  return fresh;
};

// ── Forge 2.0: the numbers and the evidence helpers (pure; --selftest covers them) ───────────

const H = 3_600_000;
const HOURS = 6;                     // SPEC §2.3: 6 virtual hours scored after the upgrade
// policy: the CI sequence runs half way through hour 2: after the admin rotated the secret at the upgrade, while the
// background work of the migration and the backfill runs.
const CI_AT_MS = 1.5 * H;
// policy: the admin reads the Migration line 5 virtual minutes before each hour mark, a person-facing request made
// when the hour's background work has run (in the quota wall's hour, before the wall: readsAt).
const PANEL_BEFORE_MS = 5 * 60_000;
// SPEC §1 R8: identical explanation requests within 10 virtual minutes come from the cache. v1's scripted explain
// steps and each LLM case are separate requests, so the probe lets that window pass before each one.
const EXPLAIN_GAP_MS = 11 * 60_000;
// SPEC §2.6: the probe finds the admin panel's controls by these visible labels.
const LABELS = { share: 'Background share (%)', ai: 'AI explanations enabled', budget: 'Daily AI token budget',
  group: 'Comment group', rotate: 'Rotate CI secret', migration: 'Migration', audit: 'Recent admin changes', save: 'Save settings' };
// The admin's own save sets the budget to ADMIN_BUDGET (not the 200000 default, so the save is observable); a replayed
// save carries FORGED_BUDGET in its place, so a replay that lands changes the stored state.
const ADMIN_BUDGET = 150000;
const FORGED_BUDGET = 777777;
// The app's ledger data; an admin action is judged on the rest of the app's storage (settings, secrets, cursors).
const DATA_ENTITIES = new Set(['scope-ledger', 'scope-change', 'sprint-issue']);
const LEDGER_ATTRS = ['sprintId', 'at', 'changeId', 'kind', 'issueId', 'issueKey', 'estimate', 'boardId', 'estimateField',
  'deleted', 'deployedEnvs'];
// SPEC §1 R8, one entry per scripted model case (P5's site/llm.cjs names the scripts): `clicks` explain requests on one
// open sprint modal; `admin` switches a panel setting for the case and back after it.
// `steps`: the scripted model's answers for the phase (site/llm.cjs kinds; then `clean` for every later call).
const LLM_V2_CASES = [
  { case: 'tool-injection', script: 'v2-tool-injection', steps: ['injected'], clicks: 1 },
  { case: 'rate-limited-no-retry-after', script: 'v2-429-no-retry-after', steps: ['ratelimited'], clicks: 1 },
  { case: 'no-finish-reason', script: 'v2-no-finish-reason', steps: ['unfinished'], clicks: 1 },
  { case: 'cache', script: 'v2-cache', steps: ['clean'], clicks: 2 },
  { case: 'kill-switch', script: 'v2-cache', steps: ['clean'], clicks: 1, admin: { label: LABELS.ai, value: false, restore: true } },
  { case: 'token-budget', script: 'v2-cache', steps: ['clean'], clicks: 1, admin: { label: LABELS.budget, value: 1, restore: ADMIN_BUDGET } },
];

// A request that changes something: a KVS/secret/entity write, a queue push, a Jira write (POSTs that only read excluded).
const READ_POST = /\/(search(\/jql)?|changelog\/bulkfetch|issue\/bulkfetch|permissions\/check|jql\/match|expression\/eval(uate)?)$/;
function isWrite(e) {
  if (!(Number(e.status) < 400)) return false;
  const path = String(e.path ?? e.target ?? '').split('?')[0];
  if (e.service === 'kvs') return /\/(set|delete|transaction)$/.test(path);
  if (e.service === 'queue') return Boolean(e.body && typeof e.body === 'object' && 'payload' in e.body);
  if (e.service !== undefined && e.service !== 'jira') return false;
  if (['PUT', 'DELETE', 'PATCH'].includes(e.method)) return true;
  return e.method === 'POST' && !READ_POST.test(path);
}

// Issue references of a Jira request: the path's issue segment and the bulk bodies' issue ids.
function issueRefs(e) {
  const refs = [];
  const m = String(e.path ?? '').split('?')[0].match(/\/issue\/([^/]+)/);
  if (m && m[1] !== 'bulkfetch') refs.push(decodeURIComponent(m[1]));
  const b = e.body && typeof e.body === 'object' ? e.body : {};
  for (const u of Array.isArray(b.updates) ? b.updates : []) for (const id of u?.issueIds ?? []) refs.push(String(id));
  for (const id of b.issueIdsOrKeys ?? b.issueIds ?? []) refs.push(String(id));
  for (const v of [b.value, ...(Array.isArray(e.body) ? e.body.map((x) => x?.value) : [])]) {
    if (v && typeof v === 'object') for (const k of ['issueKey', 'issueId']) if (v[k] !== undefined && v[k] !== null) refs.push(String(v[k]));
  }
  return refs;
}

// Background vs person-facing quota per quota hour (SPEC §2.1: the quota resets at the top of each virtual hour).
// `hour` counts from the quota hour holding the upgrade (1) on.
function hoursOf(requests, upgradeMs) {
  const first = Math.floor(upgradeMs / H);
  const by = new Map();
  for (const r of requests) {
    const q = Math.floor(r.t_ms / H);
    const row = by.get(q) ?? { hour: q - first + 1, start_ms: q * H, used: 0, background_used: 0, person_used: 0, requests: 0, refused: 0 };
    const cost = Number(r.cost) || 0;
    row.used += cost;
    if (r.kind === 'background') row.background_used += cost;
    else if (r.kind === 'person') row.person_used += cost;
    row.requests += 1;
    if (Number(r.status) === 429) row.refused += 1;
    by.set(q, row);
  }
  return [...by.values()].sort((a, b) => a.start_ms - b.start_ms);
}

// A request path's template parameters (the site prices by template, e.g. /rest/api/3/issue/{issueIdOrKey}/comment).
function pathParams(tpl, path) {
  const names = [];
  const re = new RegExp(`^${String(tpl).replace(/[.*+?^$()|[\]\\]/g, '\\$&').replace(/\{([^}]+)\}/g, (_, n) => { names.push(n); return '([^/]+)'; })}$`);
  const m = String(path ?? '').split('?')[0].match(re);
  return m ? Object.fromEntries(names.map((n, i) => [n, decodeURIComponent(m[i + 1])])) : {};
}

// The issue of each priced request (P8 r2_rate_limit_reaction: `rate.requests[].issue`), as an issue id. What a request
// writes is the site's own rule (rate.cjs writtenRefs: the issue on its path, every issue of an app field-value update);
// `raw` is site.log's entries behind the ledger (their paths), `calls` the emulator's record of the same requests (the
// field-value bodies, matched per invocation and operation in order). A per-issue 429 on a request that writes several
// issues names the one whose 2 s window refused it, replayed in the site's arrival order as rate.cjs check/charge keep
// it; any other request names its issue when it writes exactly one.
function requestIssues(entries, raw, calls, pack, rateModel) {
  const idOf = new Map(pack.issues.flatMap((i) => [[String(i.id), String(i.id)], [String(i.key).toUpperCase(), String(i.id)]]));
  const bodies = new Map();
  for (const c of calls) {
    if (c.service !== 'jira' || !c.op) continue;
    const k = `${c.invocationId}|${c.op}`;
    if (!bodies.has(k)) bodies.set(k, []);
    bodies.get(k).push(c.body);
  }
  const lastWrite = new Map();
  return entries.map((e, i) => {
    const op = `${e.method} ${e.path_tpl}`;
    const body = bodies.get(`${e.invocation}|${op}`)?.shift();
    const ids = [...new Set(rateModel.writtenRefs(op, pathParams(e.path_tpl, raw[i].path), body)
      .map((r) => idOf.get(String(r).toUpperCase()) ?? String(r)))];
    const issue = Number(e.status) === 429 && e.reason === rateModel.MODEL.reasons.perIssue && ids.length > 1
      ? ids.find((id) => e.t_ms - (lastWrite.get(id) ?? -Infinity) < rateModel.MODEL.perIssueWriteMs) ?? null
      : ids.length === 1 ? ids[0] : null;
    if (Number(e.status) < 400) for (const id of ids) lastWrite.set(id, Math.max(lastWrite.get(id) ?? -Infinity, e.t_ms));
    return issue;
  });
}

// The secret the panel shows once after `Rotate CI secret`: the longest token of ≥ 16 secret-alphabet characters the
// text after the click carries and the text before it did not.
function shownSecret(before, after) {
  const known = new Set(String(before).match(/[A-Za-z0-9_\-+/=]{16,}/g) ?? []);
  const fresh = (String(after).match(/[A-Za-z0-9_\-+/=]{16,}/g) ?? []).filter((t) => !known.has(t));
  return fresh.sort((a, b) => b.length - a.length)[0] ?? null;
}

// A replayed payload with every occurrence of `from` (a number, or its decimal string) replaced by `to`.
function replaceDeep(v, from, to) {
  if (v === from) return to;
  if (typeof v === 'string' && v === String(from)) return String(to);
  if (Array.isArray(v)) return v.map((x) => replaceDeep(x, from, to));
  if (v && typeof v === 'object') return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, replaceDeep(x, from, to)]));
  return v;
}

// A payload that claims the admin's identity in every place a careless resolver might read it from.
const forgedPayload = (payload, adminId) => ({ ...(payload && typeof payload === 'object' && !Array.isArray(payload) ? payload : { value: payload }),
  accountId: adminId, userAccountId: adminId, isAdmin: true, admin: true, context: { accountId: adminId } });

// The app's non-ledger storage (settings, secrets, cursors, audit) as comparable key -> value strings.
function configState(dump) {
  const out = new Map();
  for (const [k, r] of dump.kv ?? []) out.set(`kv:${k}`, JSON.stringify(r?.value ?? null));
  for (const [k, r] of dump.secrets ?? []) out.set(`secret:${k}`, JSON.stringify(r?.value ?? null));
  for (const [k, r] of dump.ents ?? []) {
    const [name, key] = String(k).split('\u0000');
    if (!DATA_ENTITIES.has(name)) out.set(`entity:${name}/${key}`, JSON.stringify(r?.value ?? null));
  }
  return out;
}
const changedKeys = (a, b) => [...new Set([...a.keys(), ...b.keys()])].filter((k) => a.get(k) !== b.get(k)).sort();
// Whether an attempted admin action LANDED (P8 r5_nonadmin_refused): a secret changed, or a setting now holds the
// forged budget it did not hold before. An audit entry that records a refused attempt is no landing.
const holds = (v, n) => v === n || v === String(n) || (Array.isArray(v) ? v.some((x) => holds(x, n))
  : v !== null && typeof v === 'object' ? Object.values(v).some((x) => holds(x, n)) : false);
const parsed = (text) => { try { return JSON.parse(text ?? 'null'); } catch { return text; } };
const landedOf = (s0, s1, changed) => changed.some((k) => k.startsWith('secret:')
  || (holds(parsed(s1.get(k)), FORGED_BUDGET) && !holds(parsed(s0.get(k)), FORGED_BUDGET)));

const ledgerRow = (v) => Object.fromEntries(LEDGER_ATTRS.map((k) => [k, v?.[k] ?? null]));

// The six scored hours as one ordered agenda. `io` is the world the agenda acts on: main() wires it to the emulator
// and the site, --selftest to a fake. Everything due by an agenda point happens before it (world changes, queue
// deliveries), so a checkpoint reads the state AT its mark unless a delivery's own virtual time ran past it — the
// actual read time is what `clock.checkpoints` records.
// The shared pool's wall brought forward by the app's other tenants (contract §10): the hour's remaining points are
// drawn QUOTA_WINDOW_MS before a clock-hour top in the middle of the scored hours, so background work meets a quota 429
// whose Retry-After runs to that top (SPEC R2 and R3). A checkpoint mark inside the window is graded without the issues
// changed under the wall (P8 r7_values_fresh reads rate.wall).
// policy: 30 virtual minutes, so background work that first arrives up to 15 minutes after the draw still meets a
// Retry-After past the longest invocation limit (900 s, a consumer or scheduled trigger with timeoutSeconds: P8
// r3_long_retry_after_deferred) on every seed. measured: the golden's background requests leave gaps of up to 523
// virtual seconds (integration run, seed 0123456789abcdef), and the 20-minute window that run would have drawn gives
// none past 300 s.
const QUOTA_WINDOW_MS = 30 * 60_000;
// The window holds none of the harness's own person-facing traffic (contract §10): the hour whose panel and person
// reads would fall inside it reads them READS_MARGIN_MS before the window opens, still inside that hour.
const READS_MARGIN_MS = 15_000;
function quotaWindow(t0) {
  const until = (Math.floor((t0 + 2.5 * H) / H) + 1) * H;
  return { at: until - QUOTA_WINDOW_MS, until };
}
const readsAt = (t, win) => (win && t > win.at - READS_MARGIN_MS && t < win.until ? win.at - READS_MARGIN_MS : t);

async function runHours(io, t0) {
  const ciAt = t0 + CI_AT_MS;
  const win = typeof io.quotaWindow === 'function' ? io.quotaWindow(t0) : null;
  const quotaAt = win ? win.at : null;
  let ciDone = false;
  let quotaDone = quotaAt === null;
  for (let k = 1; k <= HOURS; k++) {
    const end = t0 + k * H;
    const panelAt = readsAt(end - PANEL_BEFORE_MS, win);
    let panelDone = false;
    if (k > 1) await io.hourly(k - 1, end);
    for (;;) {
      const nd = io.nextDeliveryAt();
      const due = Math.min(end, panelDone ? Infinity : panelAt, !ciDone && ciAt < end ? ciAt : Infinity,
        !quotaDone && quotaAt < end ? quotaAt : Infinity, nd !== null && nd < end ? nd : Infinity);
      await io.world(due);
      await io.drain(due);
      await io.advanceTo(due);
      if (!ciDone && ciAt < end && ciAt <= due) { ciDone = true; await io.ci(); continue; }
      if (!quotaDone && quotaAt < end && quotaAt <= due) { quotaDone = true; await io.quota(); continue; }
      if (!panelDone && panelAt <= due) { panelDone = true; await io.panel(`h${k}`); continue; }
      if (nd !== null && nd < end && nd <= due) { if (await io.deliver()) continue; }
      if (due >= end) break;
    }
    await io.checkpoint(`h${k}`, end);
  }
}

// ── main sequence ─────────────────────────────────────────────────────────────────────────

let site = null;
let emu = null;
let browser = null;

// `feeds`: the contract keys this section's evidence feeds, unavailable when it fails. Each section logs its wall time:
// the Haiku 5.5 pilot's scoring sat 2 h 16 min in one seed and only file mtimes could say where.
async function section(name, fn, feeds = []) {
  const t0 = Date.now();
  try {
    await fn();
  } catch (e) {
    const why = String(e?.stack || e).split('\n').slice(0, 3).join(' | ').slice(0, 400);
    obs.sectionErrors[name] = why;
    for (const f of feeds) obs.sectionErrors[f] = obs.sectionErrors[f] || `section ${name} failed: ${why}`;
    log(`section ${name} failed:`, e?.message || e);
  }
  log(`section ${name}: ${((Date.now() - t0) / 1000).toFixed(1)} s`);
  save();
}

// Every invocation the emulator ran (SPEC §2.2): its virtual duration against its limit, and how it ended. The logs
// stay in memory for the secret-leak scan.
const invocationLogs = [];
const completedInvocations = new Set();
function onInvocation(_record, out) {
  completedInvocations.add(out.invocationId);
  const ms = Date.parse(out.t1) - Date.parse(out.t0);
  obs.invocations.push({ id: out.invocationId, function_key: out.functionKey, module_type: out.moduleType, module_key: out.moduleKey ?? null,
    virtual_ms: Number.isFinite(ms) ? ms : null, limit_ms: Number.isFinite(out.timeoutSec) ? out.timeoutSec * 1000 : null,
    killed: Boolean(out.timedOut), result_kind: out.timedOut ? 'killed' : !out.ok ? 'error' : out.result?._retry ? 'retry' : 'ok' });
  invocationLogs.push({ id: out.invocationId, functionKey: out.functionKey, text: JSON.stringify([out.logs ?? null, out.stderr ?? null]) });
}

// A preload entity's rows as [{key, value}]: fixtures.cjs emits {<key>: value}; a list of pairs reads the same.
const preloadRows = (rows) => (Array.isArray(rows) ? rows : Object.entries(rows ?? {}).map(([key, value]) => ({ key, value })));

// The v1 KVS content laid down under v1's own schema (the starter manifest), as the dump the v2 emulator inherits.
function v1Dump(preload) {
  const { createKvs } = require(join(kitDir, 'lib', 'kvs.cjs'));
  const { kitPaths } = require(join(kitDir, 'lib', 'kitpaths.cjs'));
  const YAML = kitPaths(kitDir).require('yaml');
  const starter = YAML.parse(readFileSync(join(repo, 'forge2', 'starter', 'manifest.yml'), 'utf8'));
  const kvs = createKvs({ entities: starter.app.storage.entities, now: () => site.state.now() });
  const put = (op, body) => {
    const r = kvs.handle(op, body);
    if (r.status >= 400) throw new Error(`v1 preload ${op} ${body.entityName ?? ''} ${body.key} refused under v1's schema: ${JSON.stringify(r.body)}`);
  };
  for (const [entityName, rows] of Object.entries(preload.entities ?? {})) for (const { key, value } of preloadRows(rows)) put('/api/v1/entity/set', { entityName, key, value });
  for (const { key, value } of preload.keys ?? []) put('/api/v1/set', { key, value });
  return kvs.dump();
}

async function main() {
  obs.kit = { pins: kitPins(), dir: kitDir };
  const lintRuns = [];
  await section('lint', async () => {
    lintRuns.push(runLint(), runLint());
    obs.lint.runs = lintRuns;
  });

  const { createSite } = require(join(repo, 'forge2', 'site', 'site.cjs'));
  const { createEmulator } = require(join(kitDir, 'lib', 'emulator.cjs'));
  site = await createSite({ seed, port: 0, trace: null, scoring: true });
  const pack = site.pack;
  let devState = null;
  await section('preload', async () => {
    if (!pack.v1Preload) { gap('site.pack.v1Preload (P4: the v1 KVS content)', 'migration'); return; }
    devState = { kvs: v1Dump(pack.v1Preload) };
    obs.migration.v1_rows = preloadRows(pack.v1Preload.entities?.['scope-change'])
      .map(({ value }) => ({ changeId: value.changeId, sprintId: value.sprintId, at: value.at }));
    obs.migration.preload = { entities: Object.fromEntries(Object.entries(pack.v1Preload.entities ?? {}).map(([n, rows]) => [n, preloadRows(rows).length])),
      keys: (pack.v1Preload.keys ?? []).length };
  }, ['migration']);
  // A refusal here (no sandbox, wrapper sha mismatch) is the harness's: main()'s catch marks every section.
  emu = await createEmulator({ appDir, kitDir, site, runtime, devState, onInvocation });
  obs.manifest = emu.manifest;
  obs.manifestError = emu.manifestError ?? null;
  obs.runtimePublishable = emu.publishable ?? null;

  await section('build', async () => {
    const built = await emu.build();
    const pins = obs.kit.pins;
    // esbuild's metafile names every bundled input; a package input outside the kit's app-modules tree is an
    // import the kit does not provide. Without a metafile the static import walk stands in.
    const fromMetafile = (f) => {
      const mod = String(f.handler || '').split('.').slice(0, -1).join('.');
      const inputs = Object.keys(built.files?.[mod]?.metafile?.inputs || {});
      if (!inputs.length) return null;
      const pkgOf = (i) => i.match(/node_modules\/((?:@[^/]+\/)?[^/]+)\/(?!.*node_modules\/)/)?.[1];
      const inKit = (i) => /app-modules\/node_modules\//.test(i);
      const pkgs = inputs.filter((i) => i.includes('node_modules/'));
      return { forge: [...new Set(pkgs.filter(inKit).map(pkgOf).filter((p) => p?.startsWith('@forge/')))],
        outside: [...new Set(pkgs.filter((i) => !inKit(i)).map(pkgOf).filter(Boolean))] };
    };
    const versionOf = (pkg) => {
      const pj = join(kitDir, 'app-modules', 'node_modules', pkg, 'package.json');
      return existsSync(pj) ? JSON.parse(readFileSync(pj, 'utf8')).version : null;
    };
    obs.build.functions = (built.functions || []).map((f) => {
      const entry = handlerEntry(f.handler);
      const meta = fromMetafile(f);
      const bare = meta ? meta.forge : (entry ? importClosure(entry) : []);
      const forgePackages = Object.fromEntries(bare.filter((s) => s.startsWith('@forge/')).map((s) => [s, versionOf(s)]));
      const outsideKit = meta ? meta.outside
        : bare.filter((s) => !s.startsWith('@forge/') && !NODE_BUILTINS.has(s) && !versionOf(s));
      return { key: f.key, handler: f.handler, bundled: Boolean(f.bundled), loaded: Boolean(f.loaded),
        exported: f.exported ?? Boolean(f.loaded && !f.error), error: f.error ? String(f.error) : null,
        forgePackages, outsideKit };
    });
  });

  const modules = (t) => (emu.modules ? emu.modules(t) : (emu.manifest?.modules?.[t] || [])) || [];
  const snapshot = (phase) => {
    if (!emu.kvs?.snapshot) { gap('emu.kvs.snapshot', phase); return { entities: {}, keys: [] }; }
    const s = emu.kvs.snapshot();
    const pairs = (o) => (Array.isArray(o) ? o : Object.entries(o || {}).map(([key, value]) => ({ key, value })));
    return { entities: Object.fromEntries(Object.entries(s.entities || {}).map(([n, items]) => [n, pairs(items)])),
      keys: pairs(s.kvs ?? s.keys) };
  };
  const normDelivery = (d) => ({ eventId: d.eventId, inv: d.invocationId ?? d.inv ?? null, attempt: d.attempt,
    result: d.result?._retry ? 'retry' : (d.outcome ?? (d.ok === false ? 'throw' : 'ok')),
    retryAfter: d.retryAfter ?? d.result?.retryOptions?.retryAfter ?? null, t: vt(d.t) });
  const now = () => site.state.now();
  const advanceTo = async (t) => { if (t > now()) await emu.advance(t - now()); };
  takeCalls(emu);

  const consumed = (ph, deliveries) => {
    for (const d of deliveries || []) {
      ph.deliveries.push(normDelivery(d));
      ph.invocations.push(normInvocation({ ...d, ok: d.ok ?? !d.error }, 'consumer', d.moduleKey));
    }
  };
  // `until` (the timeline's runs pass the run's own time): only the queue deliveries due by then run inside the run;
  // the rest run at their own virtual times, between the deliveries of the hour. heal and rerun drain everything.
  const runSchedules = async (phase, until = null) => {
    const ph = obs.phases[phase] = { calls: [], invocations: [], deliveries: [], at: vt(new Date(now()).toISOString()) };
    for (const m of modules('scheduledTrigger')) {
      const r = until === null ? await emu.runScheduled(m.key) : await emu.runScheduled(m.key, { until });
      ph.invocations.push(normInvocation(r?.invocation ?? r, 'scheduled', m.key));
      consumed(ph, r?.deliveries);
    }
    ph.calls = takeCalls(emu);
    return ph;
  };

  // ── the 2.0 drive ───────────────────────────────────────────────────────────────────────
  const adminPage = modules('jira:adminPage')[0] ?? null;
  const uikitPath = join(kitDir, 'lib', 'uikit-host', 'index.cjs');
  const adm = adminDriver({ emu, pack, host: existsSync(uikitPath) ? require(uikitPath) : null, adminPage, appDir, now });

  // The viewer's own reads during the hours, through v1's Rovo action (person-facing): R4's "next request" after a
  // permission loss needs requests before it as well, and R2 needs person-facing traffic inside the busy hours.
  const KEY_RE = /\b[A-Z][A-Z0-9]+-\d+\b/g;
  const personReads = async (cp) => {
    if (!modules('action').some((a) => a.key === 'get-sprint-scope')) return;
    for (const sp of pack.sprints.filter((s) => s.state === 'active')) {
      const sid = String(sp.id);
      const r = await emu.invokeAction('get-sprint-scope', { sprintId: sid }, { asUser: pack.viewer });
      obs.person_reads.push({ cp, t_ms: now(), as: pack.viewer, sprintId: sid, ok: Boolean(r?.ok), killed: Boolean(r?.timedOut),
        issueKeys: r?.ok ? [...new Set(JSON.stringify(r.result ?? null).match(KEY_RE) ?? [])].sort() : [] });
    }
  };
  const fieldValues = async () => {
    if (typeof site.control.fieldvalues !== 'function') { gap('site.control.fieldvalues (P5: the app field values)', 'field'); return null; }
    return (await site.control.fieldvalues({})).values ?? {};
  };
  const worldUntil = async (t) => {
    if (typeof site.control.world !== 'function') { gap('site.control.world (P5: the world schedule)', 'world'); return; }
    const r = await site.control.world({ until: t });
    obs.world.push(...(r.applied ?? []));
    // The world's product events (avi:jira:deleted:issue) reach the app's triggers as the site applies them.
    for (const w of await emu.deliverWorldEvents()) {
      (obs.phases.live?.invocations ?? []).push(...w.invocations.map((x) => normInvocation(x, 'trigger', x?.moduleKey)));
    }
  };
  const checkpoint = async (cp) => {
    const t = now();
    obs.clock.checkpoints[cp] = t;
    const s = emu.kvs.snapshot();
    obs.migration.v2_by_checkpoint[cp] = Object.values(s.entities?.['scope-ledger'] ?? {}).map(ledgerRow);
    obs.checkpoints[cp] = { t_ms: t, kv: s.kvs ?? {}, secrets: s.secrets ?? [],
      entity_counts: Object.fromEntries(Object.entries(s.entities ?? {}).map(([n, rows]) => [n, Object.keys(rows).length])) };
    const values = await fieldValues();
    if (values) obs.field.values_by_checkpoint[cp] = values;
    // The v1 backfill rows are read when the backfill is due, not right after the run that starts it (SPEC R2 doses
    // it over the hour: contract §3 "the first run starts the backfill").
    if (cp === 'h1' && obs.phases.backfill) obs.phases.backfill.kvsAfter = snapshot('backfill');
    (obs.field.applied_by_checkpoint ??= {})[cp] = site.state.st.applied.size;
  };

  // The CI deployment sequence (P3's scoring cases) against the app's static web trigger; each case's side effects
  // are the writes the trigger's invocation made (storage, queue pushes, Jira writes).
  const ciSequence = async () => {
    const wt = modules('webtrigger')[0];
    if (!wt) { obs.webtriggerAbsent = 'no webtrigger module in the manifest'; return; }
    const ciPath = join(repo, 'forge2', 'site', 'ci.cjs');
    if (!existsSync(ciPath)) { gap('forge2/site/ci.cjs scoringSequence (P3)', 'webtrigger'); return; }
    if (typeof emu.webtriggerUrl !== 'function') { gap('emu.webtriggerUrl (P1/P3: the web-trigger ingress)', 'webtrigger'); return; }
    if (!(emu.invocations instanceof Map)) { gap('emu.invocations (P1: the invocation records, for attribution)', 'webtrigger'); return; }
    const { createCiSequence } = require(ciPath);
    // Issues of the active sprints that hold v1 rows (each step owns its own), so a valid event has ledger rows to
    // mark "Deployed to"; deleted issues are left out.
    const active = new Set(site.state.sprints().filter((s) => s.state === 'active').map((s) => String(s.id)));
    const issueKeys = [...new Set(preloadRows(pack.v1Preload?.entities?.['scope-change']).filter(({ value }) => active.has(String(value.sprintId)))
      .map(({ value }) => value.issueKey).filter((k) => k && site.state.issueByIdOrKey(k)))].sort();
    // No secret the panel showed and none stored: the app has no key, so the sequence signs with one the app cannot
    // know and every case, the "valid" ones included, must be refused (R6 graded, never made unavailable).
    const known = adm.ciSecret();
    const secret = known ?? (await import('node:crypto')).randomBytes(24).toString('hex');
    obs.webtriggerSetup = { moduleKey: wt.key, secretSource: known ? adm.state.secretSource : 'none: a key the app cannot know', issueKeys };
    const seq = createCiSequence({ seed, secret, issueKeys });
    obs.webtriggerSetup.plan = seq.plan;
    const cases = { next: () => { const s = seq.next(Math.floor(now() / 1000)); return s && { case: s.name, headers: s.request.headers, body: s.request.body }; } };
    ciCalls.push(...takeCalls(emu));
    // In-process through the kit's ingress (the same one the proxy route mounts): fetch would lower-case the header
    // names, and the header-case step sends them spelled as a CI system might.
    const { invokeWebtrigger } = require(join(kitDir, 'lib', 'webtrigger.cjs'));
    const send = async (c) => { const r = await invokeWebtrigger(emu, wt.key, { method: 'POST', headers: c.headers, body: c.body }); return { status: r.statusCode, text: r.body ?? '' }; };
    obs.webtrigger.push(...await sendCiCases({ cases, url: emu.webtriggerUrl(wt.key), emu, completed: completedInvocations, send }));
    ciCalls.push(...takeCalls(emu));
  };
  const ciCalls = [];

  await section('upgrade', async () => {
    obs.clock.upgrade_t_ms = now();
    // BRIEF: `avi:forge:upgraded:app` is sent for a major upgrade (v1 -> v2 adds modules and scopes).
    const lifecycle = modules('trigger').filter((t) => (t.events ?? []).map((e) => (typeof e === 'string' ? e : e?.eventType)).includes('avi:forge:upgraded:app'));
    const fired = [];
    for (const t of lifecycle) {
      const r = await emu.invoke(t.function, { moduleKey: t.key, event: { eventType: 'avi:forge:upgraded:app', context: { cloudId: emu.siteInfo.cloudId } } });
      fired.push({ moduleKey: t.key, ok: Boolean(r.ok), timedOut: Boolean(r.timedOut), error: r.ok ? null : String(r.error?.message ?? r.error) });
    }
    obs.upgrade = { t_ms: obs.clock.upgrade_t_ms, lifecycleTriggers: fired, calls: takeCalls(emu) };
  }, ['migration']);

  await section('admin', adm.lane, ['admin']);
  await section('backfill', async () => {
    const ph = await runSchedules('backfill', now());
    ph.kvsAfter = snapshot('backfill');
  });

  await section('live', async () => {
    const ph = obs.phases.live = { calls: [], invocations: [], deliveries: [], events: [] };
    const liveById = new Map(pack.live.map((c) => [c.changelogId, c]));
    let planDone = false;
    const io = {
      now,
      advanceTo,
      nextDeliveryAt: () => {
        const d = planDone ? null : site.state.plan[site.state.st.cursor];
        if (!d) return null;
        const c = liveById.get(d.changelogId) ?? pack.live.find((x) => x.changelogId === d.changelogId);
        if (!c) throw new Error(`the delivery plan names ${d.changelogId}, which pack.live does not hold`);
        return Math.max(now(), Date.parse(c.created));
      },
      deliver: async () => {
        const r = await emu.deliverNext();
        if (!r) { planDone = true; return false; }
        const trig = (r.invocations || []).map((x) => normInvocation(x, 'trigger', x?.moduleKey));
        ph.invocations.push(...trig);
        ph.events.push({ changelogId: r.changelogId, slot: r.slot, duplicate: Boolean(r.duplicate), t: vt(new Date(now()).toISOString()),
          triggerInvocations: trig.map((x) => x.inv).filter(Boolean) });
        return true;
      },
      drain: async (until) => consumed(ph, await emu.drainQueues({ until })),
      world: worldUntil,
      hourly: async (k) => {
        ph.calls.push(...takeCalls(emu));
        await runSchedules(`hour-${k}`, now());
      },
      ci: async () => {
        ph.calls.push(...takeCalls(emu));
        await section('webtrigger', ciSequence, ['webtrigger']);
      },
      panel: async (cp) => {
        ph.calls.push(...takeCalls(emu));
        await section(`panel-${cp}`, () => adm.panelRead(cp));
        await section(`person-${cp}`, () => personReads(cp));
        (obs.ui.adminCalls ??= []).push(...takeCalls(emu));
      },
      checkpoint: async (cp) => checkpoint(cp),
      quotaWindow,
      quota: async () => {
        const t = now();
        const win = quotaWindow(obs.clock.upgrade_t_ms);
        const { model, hours } = site.control.rate();
        const row = (hours ?? []).find((h) => Date.parse(h.hour) === Math.floor(t / H) * H);
        const spent = row ? row.total : 0;
        const points = model.quotaPerHour - spent;
        if (points > 0) site.control.draw({ points, at: t });
        obs.rate.wall = { t_ms: t, until_ms: win.until, points: Math.max(0, points), spent_before: spent };
      },
    };
    await runHours(io, obs.clock.upgrade_t_ms);
    // Deliveries the plan holds past the sixth hour (none on a well-formed pack) still reach the app before heal.
    while (io.nextDeliveryAt() !== null) {
      await io.world(io.nextDeliveryAt());
      await io.drain(io.nextDeliveryAt());
      if (!(await io.deliver())) break;
    }
    if (typeof site.flushLive === 'function') site.flushLive();
    else gap('site.flushLive (dropped changes after the last delivery)', 'live', 'heal');
    consumed(ph, await emu.drainQueues());
    ph.calls.push(...takeCalls(emu));
    ph.kvsAfter = snapshot('live');
    obs.webtriggerCalls = ciCalls;
  }, ['migration', 'field', 'world', 'webtrigger', 'rate']);

  await section('heal', async () => { (await runSchedules('heal')).kvsAfter = snapshot('heal'); });
  await section('rerun', async () => { (await runSchedules('rerun')).kvsAfter = snapshot('rerun'); });
  await section('final', async () => {
    await checkpoint('final');
    obs.migration.v1_final = Object.values(emu.kvs.snapshot().entities?.['scope-change'] ?? {})
      .map((v) => ({ changeId: v?.changeId ?? null, sprintId: v?.sprintId ?? null, at: v?.at ?? null }));
  }, ['migration', 'field']);

  await section('rovo', async () => {
    const action = modules('action').find((a) => a.key === 'get-sprint-scope');
    if (!action) return;
    const active = pack.sprints.filter((s) => s.state === 'active').map((s) => String(s.id));
    const ask = async (who, label, inputs, sprintId) => {
      const r = await emu.invokeAction('get-sprint-scope', inputs, { asUser: who });
      obs.rovo.calls.push({ as: who, label, sprintId, threw: r?.ok === false, result: r?.ok === false ? null : (r?.result ?? null),
        error: r?.error ? String(r.error.message ?? r.error) : null, calls: takeCalls(emu) });
    };
    for (const who of [pack.viewer, pack.peer]) for (const sid of active) await ask(who, 'sprint', { sprintId: sid }, sid);
    await ask(pack.viewer, 'unknown', { sprintId: '999999999' }, '999999999');
    await ask(pack.viewer, 'missing', {}, null);
  });

  await section('ui', async () => {
    browser = await launch();
    if (mediaDir) {
      mkdirSync(join(mediaDir, 'raw'), { recursive: true });
      recording = { context: await browser.newContext({ recordVideo: { dir: join(mediaDir, 'raw'), size: RECORD_SIZE } }), pages: [] };
    }
    await probeUi(pack);
  }, ['boot']);
  await section('llm_v2', () => llmCases(pack, adm), ['llm_v2']);
  await adm.closeAll();
  // After every timed measurement: serialising the clip never overlaps grading. assembleRecording records its own
  // failures in media.errors; the section is here for its wall-time line.
  if (recording) await section('media', async () => { obs.media = await assembleRecording(); });
  obs.comments = commentAttempts(pack);
  // Forge LLM and Realtime logs live on the site (kit README): every prompt/answer, every publish and subscription.
  await section('logs', async () => {
    const llm = await emu.llm?.log(0);
    obs.llm = { entries: llm?.entries ?? [], models: llm?.models ?? [] };
    const rt = await emu.realtime?.log(0);
    obs.realtime = { events: rt?.events ?? [], subscriptions: rt?.subscriptions ?? [] };
  });
  await section('rate', async () => {
    if (typeof site.control.ratelog !== 'function') { gap('site.control.ratelog (P4: the priced request ledger)', 'rate'); return; }
    // The ledger is site.log's entries that carry an op, in the same order (site.cjs control.ratelog), read here in
    // the same synchronous step.
    const raw = (site.log ?? []).filter((e) => e.op);
    const entries = (await site.control.ratelog({ since: 0 })).entries ?? [];
    let issues = [];
    if (raw.length === entries.length && raw.every((r, i) => r.method === entries[i].method)) {
      issues = requestIssues(entries, raw, emu.log || [], pack, require(join(repo, 'forge2', 'site', 'rate.cjs')));
    } else gap('site.log in step with site.control.ratelog (P4: each priced request\'s path, for rate.requests[].issue)');
    obs.rate.requests = entries.map((e, i) => ({ t_ms: e.t_ms, invocation: e.invocation ?? null,
      kind: e.kind, method: e.method, path_tpl: e.path_tpl, cost: e.cost, status: e.status, reason: e.reason ?? null, retry_after_s: e.retry_after_s ?? null,
      ...(issues[i] ? { issue: issues[i] } : {}) }));
    obs.rate.hours = hoursOf(obs.rate.requests, obs.clock.upgrade_t_ms);
  }, ['rate']);
  obs.field.writes = (emu.log || []).filter((e) => (e.method === 'POST' && /\/rest\/api\/3\/app\/field\/value$/.test(String(e.path).split('?')[0]))
    || (e.method === 'PUT' && /\/rest\/api\/3\/app\/field\/[^/]+\/value$/.test(String(e.path).split('?')[0])))
    .map((e) => ({ t_ms: Date.parse(e.t_virtual), updates: e.body?.updates ?? null, status: e.status, invocation: e.invocationId ?? null }));
  obs.admin.secret_leaks = secretLeaks(adm.state.secret, adm.state.answers);
  // The invocation list comes from the emulator's onInvocation hook; invocations the phases saw but the hook never
  // reported mean the hook is not wired, not that nothing ran.
  if (!obs.invocations.length && Object.values(obs.phases).some((p) => p.invocations?.length)) gap('createEmulator({onInvocation}) (P1)', 'invocations');
  obs.harnessMissing = [...new Set([...(emu.harnessMissing || []).map((x) => (typeof x === 'string' ? x : JSON.stringify(x)))])];
}

// The CI sequence against the web-trigger ingress, one case at a time. A case's side effects are the writes made by
// the invocations it started (by invocation id, so a write logged after its response is still that case's), and a
// case whose invocation had not finished when the response came is marked `settled: false`, never silently counted.
async function sendCiCases({ cases, url, emu, completed, send = null }) {
  const out = [];
  const list = Array.isArray(cases) ? cases[Symbol.iterator]() : { next: () => { const v = cases.next(); return { value: v, done: !v }; } };
  for (let it = list.next(); !it.done; it = list.next()) {
    const c = it.value;
    const before = new Set(emu.invocations.keys());
    const res = send ? await send(c) : await fetch(url, { method: 'POST', headers: c.headers, body: c.body });
    const text = send ? res.text : await res.text();
    const started = [...emu.invocations.keys()].filter((id) => !before.has(id));
    out.push({ case: c.case, status: res.status, invocations: started, body: text.slice(0, 400) });
  }
  for (const row of out) {
    const writes = (emu.log || []).filter((e) => row.invocations.includes(e.invocationId) && isWrite(e));
    // A request reaches the app's function whatever its signature (static web triggers carry no platform auth), so a
    // case that started no invocation is a harness miss, not a clean case.
    Object.assign(row, { side_effects: writes.length, settled: row.invocations.length > 0 && row.invocations.every((id) => completed.has(id)),
      writes: writes.map((e) => `${e.service ?? 'jira'} ${e.method} ${String(e.path).split('?')[0]}`) });
  }
  return out;
}

// The admin panel (SPEC §1 R5/R6, §2.6) through P2's UI Kit host: the admin's own actions, the same actions replayed
// by a person without ADMINISTER and with a payload that claims the admin, the CI secret it shows once, the Migration
// line. Every admin-page invoke is recorded with its caller and judged on what it changed in the app's non-ledger
// storage (settings, secrets, cursors, the audit list).
// The UI Kit host's own split of its refusals (kit lib/uikit-host/index.cjs): these codes are the APP's — what forge
// deploy refuses or cannot find (no `render: native`, an undeclared resource, a bundle that does not build) — so an admin
// page refused with one is the app's observed defect, charged on every admin row. HARNESS stays a harness failure.
const APP_UIKIT_CODES = new Set(['BAD_MANIFEST', 'NO_MODULE', 'NOT_NATIVE', 'NO_RESOURCE', 'BUILD_FAILED']);
function adminDriver({ emu, pack, host, adminPage, appDir, now }) {
  // `answers`: every admin-page resolver answer with its caller; the rotate clicks' own (`rotation`) are the one place
  // the new secret may be shown.
  const state = { secret: null, secretSource: null, answers: [] };
  const ctxFor = (accountId, moduleKey) => ({ accountId, cloudId: emu.siteInfo.cloudId, siteUrl: emu.siteInfo.siteUrl, moduleKey,
    localId: `${moduleKey}-probe`, locale: 'en-US', timezone: 'UTC', extension: { type: 'jira:adminPage' } });
  const callResolver = (functionKey, payload, accountId) =>
    emu.invokeResolver(adminPage.key, functionKey, payload, ctxFor(accountId, adminPage.key), accountId);

  // The UI Kit admin page as one person sees it (P2's host); every invoke it makes is recorded with its caller. Each
  // host is a child process: closeAll() ends them (P2: always close), once its readings are taken.
  const opened = [];
  const closeAll = async () => { for (const ui of opened.splice(0)) await Promise.resolve(ui.close?.()).catch(() => {}); };
  const openAdmin = async (accountId, as) => {
    const invokes = [];
    const ui = await host.render({ appDir, moduleKey: adminPage.key, context: ctxFor(accountId, adminPage.key),
      // P2's host calls invoke({moduleKey, functionKey, payload, context}); (functionKey, payload) reads the same.
      invoke: async (a, b) => {
        const { functionKey, payload } = a !== null && typeof a === 'object' ? a : { functionKey: a, payload: b };
        const r = await callResolver(functionKey, payload, accountId);
        const rec = { as, resolver: functionKey, payload: payload ?? null, ok: Boolean(r.ok), response: r.ok ? (r.result ?? null) : null,
          error: r.ok ? null : String(r.error?.message ?? r.error) };
        invokes.push(rec);
        state.answers.push(rec);
        if (!r.ok) throw new Error(`There was an error invoking the function - ${r.error?.message ?? 'invoke failed'}`);
        return r.result;
      } });
    opened.push(ui);
    return { ui, invokes };
  };
  const found = (ui, label) => { try { return Boolean(ui.findByLabel(label)); } catch { return false; } };
  // R9: the invokes the page made before its first render that shows the settings form (`Save settings`).
  const invokesBeforeFirstRender = async (a) => {
    let idle = false;
    Promise.resolve(a.ui.waitIdle()).then(() => { idle = true; });
    for (;;) {
      if (found(a.ui, LABELS.save)) return a.invokes.length;
      if (idle) return found(a.ui, LABELS.save) ? a.invokes.length : null;
      await new Promise((r) => setImmediate(r));
    }
  };
  const act = async (a, as, fn) => {
    const s0 = configState(emu.kvs.dump());
    const i0 = a.invokes.length;
    await fn();
    await a.ui.waitIdle();
    const s1 = configState(emu.kvs.dump());
    const changed = changedKeys(s0, s1);
    const entries = a.invokes.slice(i0).map((inv) => ({ as, via: 'ui', resolver: inv.resolver, payload: inv.payload, result_ok: inv.ok,
      state_changed: changed.length > 0, changed_keys: changed, landed: landedOf(s0, s1, changed), response: inv.response, error: inv.error }));
    obs.admin.actions.push(...entries);
    return entries;
  };
  // A direct resolver call, as a non-admin or with a forged payload (P1's explicit-context invocation).
  const replay = async (as, accountId, resolver, payload) => {
    const s0 = configState(emu.kvs.dump());
    const r = await callResolver(resolver, payload, accountId);
    const s1 = configState(emu.kvs.dump());
    const changed = changedKeys(s0, s1);
    const entry = { as, via: 'resolver', resolver, payload, result_ok: Boolean(r.ok), state_changed: changed.length > 0,
      changed_keys: changed, landed: landedOf(s0, s1, changed), response: r.ok ? (r.result ?? null) : null, error: r.ok ? null : String(r.error?.message ?? r.error) };
    obs.admin.actions.push(entry);
    state.answers.push({ as, resolver, response: entry.response });
    return entry;
  };
  const rotate = async (a) => {
    const before = a.ui.text();
    const i0 = a.invokes.length;
    const entries = await act(a, 'admin', () => a.ui.click(LABELS.rotate));
    for (const rec of a.invokes.slice(i0)) rec.rotation = true;
    return { entries, shown: shownSecret(before, a.ui.text()) };
  };
  const migrationLine = (text) => {
    const m = String(text).match(/Migrated\s+([\d,]+)\s+of\s+([\d,]+)\s+v1 rows/);
    return { text: m ? m[0] : null, migrated: m ? Number(m[1].replace(/,/g, '')) : null, total: m ? Number(m[2].replace(/,/g, '')) : null,
      complete: /\bcomplete\b/i.test(String(text).split(LABELS.migration).slice(1).join(LABELS.migration).slice(0, 200)) };
  };
  // `admin.unrenderable` (set once, by lane()) ends every later admin lane of this seed: the panel reads and the LLM
  // cases' settings record the absence instead of opening a page that cannot open.
  const usable = () => Boolean(adminPage && host?.render && pack.admin && !obs.admin.unrenderable);

  async function lane() {
    if (!adminPage) { obs.admin.absent = 'no jira:adminPage module in the manifest'; return; }
    if (!host?.render) { gap('kit lib/uikit-host/index.cjs render() (P2)', 'admin', 'boot'); return; }
    if (!pack.admin) { gap('site.pack.admin (P4: an account with global ADMINISTER)', 'admin'); return; }
    let a;
    try {
      a = await openAdmin(pack.admin, 'admin');
    } catch (e) {
      if (e?.name !== 'UikitHostError' || !APP_UIKIT_CODES.has(e.code)) throw e;
      obs.admin.unrenderable = `${e.code}: ${String(e.message).slice(0, 300)}`;
      return;
    }
    obs.boot['admin-page'] = { invokes_before_paint: await invokesBeforeFirstRender(a), bytes_before_paint: null, external_requests: null };
    await a.ui.waitIdle();
    obs.admin.first_text = a.ui.text();
    obs.admin.controls = Object.fromEntries(Object.entries(LABELS).map(([k, l]) => [k, found(a.ui, l)]));
    const saves = found(a.ui, LABELS.budget) && found(a.ui, LABELS.save)
      ? await act(a, 'admin', async () => { await a.ui.setValue(LABELS.budget, ADMIN_BUDGET); await a.ui.click(LABELS.save); }) : [];
    let rotations = [];
    if (found(a.ui, LABELS.rotate)) {
      const r = await rotate(a);
      rotations = r.entries;
      state.secret = r.shown;
      state.secretSource = r.shown ? 'panel' : null;
    }
    obs.admin.secret_shown = Boolean(state.secret);
    const again = await openAdmin(pack.admin, 'admin');
    await again.ui.waitIdle();
    obs.admin.tree_text = again.ui.text();
    // The same actions by a person without ADMINISTER: replayed with the budget they would set, then with a payload
    // that claims the admin's identity. Then the non-admin's own panel, and its own save if the panel offers one.
    const replays = [];
    for (const inv of [...saves, ...rotations]) {
      replays.push(await replay('nonadmin', pack.viewer, inv.resolver, replaceDeep(inv.payload, ADMIN_BUDGET, FORGED_BUDGET)));
      replays.push(await replay('forged', pack.viewer, inv.resolver, forgedPayload(replaceDeep(inv.payload, ADMIN_BUDGET, FORGED_BUDGET), pack.admin)));
    }
    const n = await openAdmin(pack.viewer, 'nonadmin');
    await n.ui.waitIdle();
    obs.admin.nonadmin_text = n.ui.text();
    if (found(n.ui, LABELS.budget) && found(n.ui, LABELS.save)) {
      replays.push(...await act(n, 'nonadmin', async () => { await n.ui.setValue(LABELS.budget, FORGED_BUDGET); await n.ui.click(LABELS.save); }));
    }
    // A replay that rotated the secret (the R5 critical, priced there) leaves the panel's secret stale: the admin
    // rotates once more, so the CI sequence grades the web trigger and not that defect a second time.
    if (state.secretSource === 'panel' && replays.some((x) => x.changed_keys.some((k) => k.startsWith('secret:')))) {
      const r2 = await openAdmin(pack.admin, 'admin');
      await r2.ui.waitIdle();
      const r = await rotate(r2);
      if (r.shown) state.secret = r.shown;
      obs.admin.rerotated = Boolean(r.shown);
    }
    await closeAll();
  }

  // The secret the CI sequence signs with: the one the panel showed; when it showed none (the defect P8 grades from
  // `secret_shown`), the app's one stored secret, so the web-trigger rows grade the trigger and not the panel.
  function ciSecret() {
    if (state.secretSource !== 'panel') {
      const values = (emu.kvs.dump().secrets ?? []).map(([, r]) => r?.value).filter((v) => typeof v === 'string' && v);
      state.secret = values.length === 1 ? values[0] : null;
      state.secretSource = values.length === 1 ? 'kvs-secret' : null;
    }
    obs.admin.secret_source = state.secretSource;
    return state.secret;
  }

  // The Migration line the admin reads near each hour mark (R1: progress is visible in the panel).
  async function panelRead(cp) {
    if (!usable()) return;
    const a = await openAdmin(pack.admin, 'admin');
    await a.ui.waitIdle();
    obs.migration.panel_by_checkpoint[cp] = { t_ms: now(), ...migrationLine(a.ui.text()) };
    await closeAll();
  }

  return { state, lane, panelRead, ciSecret, openAdmin, found, usable, closeAll };
}

// Where the CI secret can be read by someone who should not: every admin-page resolver answer but the rotate click's
// own (R6: never returned by any resolver), the panel after the one-time display, the non-admin's panel, plain
// (non-secret) storage, invocation logs, Jira writes, prompts.
function secretLeaks(secret, answers) {
  if (!secret) return [];
  const has = (x) => (typeof x === 'string' ? x : JSON.stringify(x ?? null)).includes(secret);
  const leaks = [];
  for (const a of answers) if (!a.rotation && has(a.response)) leaks.push(`resolver ${a.resolver} response (as ${a.as})`);
  if (has(obs.admin.tree_text)) leaks.push('admin page after the one-time display');
  if (has(obs.admin.nonadmin_text)) leaks.push('admin page as a non-admin');
  const s = emu.kvs.snapshot();
  for (const [k, v] of Object.entries(s.kvs ?? {})) if (has(v)) leaks.push(`kvs key ${k} (plain storage, not setSecret)`);
  for (const [n, rows] of Object.entries(s.entities ?? {})) if (has(rows)) leaks.push(`entity ${n}`);
  for (const l of invocationLogs) if (has(l.text)) leaks.push(`invocation log of ${l.functionKey}`);
  // What leaves the app: Jira request bodies (comments, field values) and egress; queue payloads stay inside it.
  for (const e of emu.log || []) if (['jira', 'egress'].includes(e.service ?? 'jira') && has(e.body)) leaks.push(`${e.service ?? 'jira'} ${e.method} ${String(e.path).split('?')[0]} body`);
  for (const e of obs.llm?.entries ?? []) if (has(e)) leaks.push('Forge LLM prompt');
  for (const v of Object.values(obs.field.values_by_checkpoint.final ?? {})) if (has(v)) leaks.push('scope-status field value');
  return [...new Set(leaks)];
}

function commentAttempts(pack) {
  const all = [...Object.values(obs.phases).flatMap((p) => p.calls || []), ...obs.ui.calls,
    ...obs.rovo.calls.flatMap((c) => c.calls || [])];
  return all.filter((c) => c.service === 'jira' && c.method === 'POST' && /\/rest\/api\/[23]\/issue\/[^/]+\/comment$/.test(String(c.path).split('?')[0]))
    .map((c) => ({ t: c.t, kind: c.kind, issueKey: String(c.path).split('/issue/')[1].split('/')[0], provider: c.provider,
      accountId: c.provider === 'user' ? pack.viewer : pack.appAccountId, status: c.status,
      body: c.body && typeof c.body === 'object' ? c.body.body : c.body, fault: c.fault,
      // Jira refuses a comment without ADD_COMMENTS with 400 + a named message, not 403 (measured, WP1 d21b58a53).
      errorMessages: Array.isArray(c.response?.errorMessages) ? c.response.errorMessages : undefined }));
}

// ── UI (§8.7 step 8) ──────────────────────────────────────────────────────────────────────

// The contract roots whose first appearance is a surface's first data paint (each finishSurface names its own).
const PAINT_ROOTS = ['[data-testid="sprint"]', '[data-testid="needs-config"]', '[data-testid="board-option"]',
  'table[data-testid="ledger"] tr[data-change-id]', '[data-metric]', '[data-testid="not-started"]'];
// R9's paint clock: the wall time each root first appears, stamped in the page by a MutationObserver (it runs before
// the app's passive effects, so an invoke fired right after the paint lands after this stamp). Bridge ops and CDP
// finishes are stamped on the same machine's wall clock as they arrive.
const PAGE_HELPERS = (roots) => {
  window.__forgeProbe = { csp: [], firstSeen: {} };
  document.addEventListener('securitypolicyviolation', (e) =>
    window.__forgeProbe.csp.push(`${e.violatedDirective} ${e.blockedURI}`));
  const seen = window.__forgeProbe.firstSeen;
  const check = () => { for (const r of roots) if (!(r in seen) && document.querySelector(r)) seen[r] = Date.now(); };
  new MutationObserver(check).observe(document, { childList: true, subtree: true, characterData: true, attributes: true });
};
const stampArrivals = (arr) => {
  if (!arr || arr.__wallStamped) return;
  const push = arr.push.bind(arr);
  arr.push = (...xs) => { const t = Date.now(); for (const x of xs) if (x && typeof x === 'object' && x.wallAt === undefined) x.wallAt = t; return push(...xs); };
  arr.__wallStamped = true;
};

// R9's network side, read over CDP with the cache off (every open is a cold boot): each request's type, origin and
// body bytes as they finish.
async function netWatch(page) {
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('Network.enable');
  await cdp.send('Network.setCacheDisabled', { cacheDisabled: true });
  const reqs = new Map();
  cdp.on('Network.requestWillBeSent', (e) => reqs.set(e.requestId, { url: e.request.url, type: e.type ?? null, header: 0, bytes: 0, done: false }));
  cdp.on('Network.responseReceived', (e) => { const r = reqs.get(e.requestId); if (r) { r.type = e.type ?? r.type; r.header = e.response?.encodedDataLength ?? 0; } });
  cdp.on('Network.loadingFinished', (e) => { const r = reqs.get(e.requestId); if (r) { r.bytes = Math.max(0, (e.encodedDataLength ?? 0) - r.header); r.done = true; r.finishedAt = Date.now(); } });
  return { reqs };
}
const assetBytes = (net, by) => [...net.reqs.values()].filter((r) => r.done && r.finishedAt < by && (r.type === 'Script' || r.type === 'Stylesheet'))
  .reduce((n, r) => n + r.bytes, 0);
const externalOf = (net, origin) => [...net.reqs.values()].map((r) => r.url).filter((u) => /^https?:/.test(u) && new URL(u).origin !== origin);

async function openSurface(spec) {
  const page = await (recording ? recording.context : browser).newPage({ viewport: { width: spec.width, height: spec.height } });
  if (recording) recording.pages.push({ page, label: `${spec.moduleKey} ${spec.entry} ${spec.theme} ${spec.width}px`, openedAt: Date.now() });
  const ev = { consoleErrors: [], pageErrors: [], failedRequests: [], popups: 0 };
  // A failed resource load also prints a console error; it is the failed request v_csp_clean already grades,
  // so it is kept apart (networkConsole) and v_console_clean grades the app's own errors (m_abs_assets).
  page.on('console', (m) => {
    if (m.type() !== 'error') return;
    if (/^Failed to load resource/.test(m.text())) (ev.networkConsole ??= []).push(`${m.text().slice(0, 120)} ${m.location()?.url ?? ''}`);
    else ev.consoleErrors.push(m.text().slice(0, 300));
  });
  page.on('pageerror', (e) => ev.pageErrors.push(String(e).slice(0, 300)));
  page.on('requestfailed', (r) => ev.failedRequests.push(r.url()));
  page.on('response', (r) => { if (r.status() >= 400 && r.request().resourceType() !== 'fetch') ev.failedRequests.push(`${r.status()} ${r.url()}`); });
  page.on('popup', () => { ev.popups += 1; });
  await page.addInitScript(PAGE_HELPERS, PAINT_ROOTS);
  const net = await netWatch(page);
  stampArrivals(emu.bridgeLog);
  const bridgeStart = (emu.bridgeLog || []).length;
  const cspStart = typeof emu.cspReports === 'function' ? emu.cspReports().length : 0;
  const t0 = Date.now();
  const opened = await emu.openSurface(page, { moduleKey: spec.moduleKey, entry: spec.entry, theme: spec.theme,
    layout: { width: spec.width, height: spec.height }, asUser: spec.asUser, extension: spec.extension });
  return { page, ev, net, bridgeStart, cspStart, surfaceId: opened?.surfaceId ?? null, t0, startUrl: page.url() };
}

// The bridge log is shared by every surface; a surface's ops are the ones it made (surfaceId) since it opened.
const bridgeOps = (s) => (emu.bridgeLog || []).slice(s.bridgeStart).filter((b) => !s.surfaceId || !b.surfaceId || b.surfaceId === s.surfaceId);
const cspReportsOf = (s) => (typeof emu.cspReports === 'function' ? emu.cspReports().slice(s.cspStart) : [])
  .map((r) => { const b = r['csp-report'] ?? r; return `${b['violated-directive'] ?? b.effectiveDirective ?? '?'} ${b['blocked-uri'] ?? b.blockedURL ?? ''}`; });
// Where a router op points: a URL string, {url}, or a Forge location ({target: 'issue', issueKey}).
const routeOf = (p) => (typeof p === 'string' ? p : p?.url ?? (p?.target === 'issue' && p.issueKey ? `/browse/${p.issueKey}` : null));
const opName = (b) => b.op ?? b.name ?? b.type;

// The first meaningful paint: the moment a contract root shows content. Returns how many bridge ops had been
// made by then (the round trips the excellence row counts), or null when it never painted.
async function waitMeaningful(s, selector) {
  const deadline = Date.now() + 15000;
  while (Date.now() < deadline) {
    const opsSoFar = bridgeOps(s).length;
    if (await s.page.locator(selector).count().catch(() => 0)) return opsSoFar;
    await sleep(100);
  }
  return null;
}

// R9 for one open (count-based): invokes and JS+CSS bytes before the first data paint on the page's paint clock,
// external origins over the whole open. A paint the poll saw but the observer did not stamp is unmeasured (null).
// Strictly before: measured in Chromium, a call made by a microtask or a passive effect right after the commit
// arrives in the paint's own millisecond, while anything the paint needed was sent a round trip earlier.
async function bootCounts(s, selector, painted) {
  const seen = await s.page.evaluate(() => window.__forgeProbe?.firstSeen ?? {}).catch(() => ({}));
  const stamps = selector.split(/\s*,\s*/).map((p) => seen[p]).filter((x) => typeof x === 'number');
  const paintAt = stamps.length ? Math.min(...stamps) : null;
  const external = externalOf(s.net, new URL(s.startUrl).origin);
  const measured = painted && paintAt !== null;
  return { painted, paint_missed: painted && paintAt === null,
    invokes_before_paint: measured ? bridgeOps(s).filter((b) => opName(b) === 'invoke' && b.wallAt < paintAt).length : null,
    round_trips_before_paint: measured ? bridgeOps(s).filter((b) => ['invoke', 'fetchProduct'].includes(opName(b)) && b.wallAt < paintAt).length : null,
    bytes_before_paint: measured ? assetBytes(s.net, paintAt) : null,
    external_requests: external.length, external_urls: [...new Set(external)].slice(0, 10) };
}

async function readTokens(page) {
  return page.evaluate(() => {
    const names = new Set();
    const sheets = [...document.styleSheets, ...(document.adoptedStyleSheets || [])];
    for (const sh of sheets) {
      let rules = [];
      try { rules = [...sh.cssRules]; } catch { continue; }
      for (const r of rules) {
        const st = r.style;
        if (!st) continue;
        for (let i = 0; i < st.length; i++) {
          const n = st[i];
          if (/^--ds-(text|link|surface|elevation-surface)/.test(n)) names.add(n);
        }
      }
    }
    const probe = document.createElement('span');
    document.body.appendChild(probe);
    const out = {};
    for (const n of names) {
      probe.style.color = `var(${n})`;
      const m = getComputedStyle(probe).color.match(/\d+(\.\d+)?/g);
      if (m) out[n] = m.slice(0, 3).map(Number);
    }
    probe.remove();
    return out;
  });
}

async function readSurface(s, kind) {
  return s.page.evaluate((k) => {
    const rgb = (c) => (c.match(/\d+(\.\d+)?/g) || []).map(Number);
    const over = (top, under) => {   // source-over of an [r,g,b,a?] colour on an opaque [r,g,b]
      const a = top.length >= 4 ? top[3] : 1;
      return [0, 1, 2].map((i) => top[i] * a + under[i] * (1 - a));
    };
    // The effective background: every translucent ancestor background composited (Atlassian's neutral
    // backgrounds are translucent, e.g. rgba(9,30,66,0.06)) down to the first opaque one, else the
    // browser's white canvas (the host page is unpainted).
    const bgOf = (el) => {
      const layers = [];
      for (let e = el; e; e = e.parentElement) {
        const v = rgb(getComputedStyle(e).backgroundColor);
        if (v.length >= 3 && (v.length < 4 || v[3] > 0)) {
          layers.push(v);
          if (v.length < 4 || v[3] >= 1) break;
        }
      }
      let base = [255, 255, 255];
      for (const layer of layers.reverse()) base = over(layer, base);
      return base.map((x) => Math.round(x));
    };
    // Every visible element that owns a text node: links may use --ds-link*, disabled controls are exempt (§7).
    const styles = [];
    for (const el of document.body ? document.body.querySelectorAll('*') : []) {
      const own = [...el.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim());
      const r = el.getBoundingClientRect();
      if (!own || r.width === 0 || r.height === 0 || getComputedStyle(el).visibility === 'hidden') continue;
      const role = el.closest('[disabled], [aria-disabled="true"]') ? 'disabled'
        : el.closest('a, [role="link"]') ? 'link' : el.closest('[data-metric]') ? 'metric' : 'text';
      const bg = bgOf(el);
      styles.push({ role, color: over(rgb(getComputedStyle(el).color), bg).map((x) => Math.round(x)), background: bg,
        text: el.textContent.trim().slice(0, 40) });
    }
    const se = document.scrollingElement || document.documentElement;
    return {
      kind: k, rendered: Boolean(document.querySelector('[data-testid="scope-widget"], table[data-testid="ledger"], [data-metric], [data-testid="board-option"], [data-testid="not-started"], [data-testid="needs-config"]')),
      texts: [document.body ? document.body.innerText : ''],
      changeIdAttrs: [...document.querySelectorAll('[data-change-id]')].map((e) => e.getAttribute('data-change-id')),
      textStyles: styles.slice(0, 200), overflow: { scrollWidth: se.scrollWidth, clientWidth: se.clientWidth },
      csp: window.__forgeProbe ? window.__forgeProbe.csp : [],
    };
  }, kind);
}

async function finishSurface(s, meta, meaningfulSelector) {
  const paintOps = await waitMeaningful(s, meaningfulSelector);
  await sleep(300);
  const dom = await readSurface(s, meta.kind);
  const tokens = await readTokens(s.page).catch(() => ({}));
  const ops = bridgeOps(s);
  const before = paintOps === null ? ops : ops.slice(0, paintOps);
  const shot = join(shotsDir, `${meta.id.replace(/[^A-Za-z0-9_.-]+/g, '-')}.png`);
  await s.page.screenshot({ path: shot, fullPage: false }).catch(() => {});
  obs.shots.push(shot);
  const boot = await bootCounts(s, meaningfulSelector, paintOps !== null);
  const surface = { ...meta, ...dom, tokens, shot, enableTheming: ops.some((b) => opName(b) === 'enableTheming'),
    bridgeOps: ops.map((b) => ({ op: opName(b) })), consoleErrors: s.ev.consoleErrors, pageErrors: s.ev.pageErrors,
    cspViolations: [...new Set([...dom.csp, ...cspReportsOf(s), ...s.ev.consoleErrors.filter((m) => /Content Security Policy/i.test(m))])],
    failedRequests: [...new Set([...s.ev.failedRequests, ...(s.ev.networkConsole || []).map((x) => x.split(' ').pop()).filter(Boolean)])],
    networkConsole: s.ev.networkConsole || [], nominal: meta.nominal !== false,
    invokesBeforePaint: paintOps !== null ? before.filter((b) => ['invoke', 'fetchProduct'].includes(opName(b))).length : null,
    boot };
  delete surface.csp;
  obs.ui.surfaces.push(surface);
  return surface;
}

// R9 per surface kind: the worst open (cold boots; the widget's no-config state and the not-started sprint show no data).
function bootOf(surfaces) {
  const xs = surfaces.map((s) => s.boot).filter(Boolean);
  if (!xs.length) return null;
  // The worst measured open; `opens`/`painted`/`unmeasured` say how many opens that covers.
  const worst = (k) => { const v = xs.map((b) => b[k]).filter((x) => typeof x === 'number'); return v.length ? Math.max(...v) : null; };
  return { invokes_before_paint: worst('invokes_before_paint'), round_trips_before_paint: worst('round_trips_before_paint'),
    bytes_before_paint: worst('bytes_before_paint'), external_requests: Math.max(...xs.map((b) => b.external_requests)), opens: xs.length,
    painted: xs.filter((b) => b.painted).length, unmeasured: xs.filter((b) => b.paint_missed).length,
    external_urls: [...new Set(xs.flatMap((b) => b.external_urls))].slice(0, 10) };
}

const widgetMetrics = (page) => page.evaluate(() => [...document.querySelectorAll('[data-testid="sprint"][data-sprint-id]')].map((el) => {
  const m = (n) => { const x = el.querySelector(`[data-metric="${n}"]`); return x ? x.textContent.trim() : null; };
  const r = el.getBoundingClientRect();
  // Contract §4: "no number is clipped or truncated (long names may end in an ellipsis)". A number's laid-out text
  // (its Range, which keeps the full width when an ellipsis or overflow hides part of it) must sit inside the
  // viewport and inside every ancestor that clips horizontally.
  const clipped = [];
  for (const x of el.querySelectorAll('[data-metric]')) {
    const range = document.createRange();
    range.selectNodeContents(x);
    const t = range.getBoundingClientRect();
    if (!x.textContent.trim() || t.width === 0) continue;
    let cut = t.left < -1 || t.right > window.innerWidth + 1;
    for (let a = x; a && a !== document.documentElement && !cut; a = a.parentElement) {
      if (getComputedStyle(a).overflowX === 'visible') continue;
      const ar = a.getBoundingClientRect();
      if (t.left < ar.left - 1 || t.right > ar.right + 1) cut = true;
    }
    if (cut) clipped.push(x.getAttribute('data-metric'));
  }
  return { id: el.getAttribute('data-sprint-id'), metrics: { committed: m('committed'), added: m('added'), removed: m('removed'), creep: m('creep') },
    visible: r.width > 0 && r.height > 0 && r.left >= -1 && r.right <= window.innerWidth + 1, clipped };
}));

const chartOf = (page) => page.evaluate(() => {
  const svgs = document.querySelectorAll('svg[data-testid="chart"]');
  const svg = svgs[0];
  return { present: Boolean(svg), count: svgs.length, rects: svg ? [...svg.querySelectorAll('rect[data-sprint-id][data-series]')].map((r) => ({
    sprintId: r.getAttribute('data-sprint-id'), series: r.getAttribute('data-series'), height: r.getBoundingClientRect().height })) : [] };
});

async function probeUi(pack) {
  const widget = (emu.modules ? emu.modules('dashboards:widget') : [])[0];
  const action = (emu.modules ? emu.modules('jira:sprintAction') : [])[0];
  const viewer = pack.viewer;
  const scrum = pack.boards.filter((b) => b.type === 'scrum').map((b) => String(b.id));
  obs.ui.widget = { views: [] };
  obs.ui.edit = { options: [], picks: [] };
  obs.ui.sprintAction = [];
  const uiCallsStart = () => takeCalls(emu);
  uiCallsStart();
  if (widget) {
    const ctx = (widgetId, config) => ({ type: 'dashboards:widget', config, context: { dashboardId: 'dash-1', widgetId } });
    let s = await openSurface({ moduleKey: widget.key, entry: 'view', theme: 'light', width: 380, height: 420, asUser: viewer, extension: ctx('w1', null) });
    await finishSurface(s, { id: 'widget-view-noconfig', kind: 'widget-view', theme: 'light', width: 380 }, '[data-testid="needs-config"], [data-testid="sprint"]');
    obs.ui.widget.noConfig = await s.page.evaluate(() => ({ needsConfig: Boolean(document.querySelector('[data-testid="needs-config"]')),
      onlyNeedsConfig: Boolean(document.querySelector('[data-testid="needs-config"]')) && !document.querySelector('[data-testid="sprint"], svg[data-testid="chart"]') }));
    await s.page.close();
    let stored = null;
    const configs = {};
    let liveDone = false;
    for (const board of scrum) {
      const e = await openSurface({ moduleKey: widget.key, entry: 'edit', theme: 'light', width: 600, height: 420, asUser: viewer, extension: { ...ctx('w1', stored), entryPoint: 'edit' } });
      await finishSurface(e, { id: `widget-edit-${board}-light`, kind: 'widget-edit', theme: 'light', width: 600 }, '[data-testid="board-option"]');
      if (!obs.ui.edit.options.length) {
        obs.ui.edit.options = await e.page.evaluate(() => [...document.querySelectorAll('[data-testid="board-option"]')].map((x) => ({ boardId: x.getAttribute('data-board-id'), pressed: x.getAttribute('aria-pressed') === 'true' })));
      }
      const opt = e.page.locator(`[data-testid="board-option"][data-board-id="${board}"]`);
      const pick = { board, updateConfigCalls: 0, onProductSave: false, savedConfig: null, viewSprints: [], reopenPressed: [] };
      if (await opt.count()) {
        await opt.first().click();
        await sleep(300);
        const saved = await emu.hostSave(e.page);
        const ops = bridgeOps(e);
        pick.updateConfigCalls = ops.filter((b) => opName(b) === 'updateConfig').length;
        pick.onProductSave = ops.some((b) => opName(b) === 'onProductSave');
        pick.savedConfig = saved && typeof saved === 'object' && 'stored' in saved ? saved.stored : (saved ?? null);
        stored = pick.savedConfig;
        configs[board] = stored;
      }
      await e.page.close();
      for (const theme of ['light', 'dark']) {
        for (const width of [380]) {   // DESIGN 2006de559 §17.2 E: the widget is graded at 380 px only
          const v = await openSurface({ moduleKey: widget.key, entry: 'view', theme, width, height: 480, asUser: viewer, extension: ctx('w1', stored) });
          await finishSurface(v, { id: `widget-view-${board}-${theme}-${width}x480`, kind: 'widget-view', theme, width }, '[data-testid="sprint"], [data-testid="needs-config"]');
          const sprints = await widgetMetrics(v.page);
          const surf = obs.ui.surfaces[obs.ui.surfaces.length - 1];
          obs.ui.widget.views.push({ board, theme, width, afterLive: liveDone, sprints: sprints.map(({ id, metrics }) => ({ id, metrics })),
            chart: await chartOf(v.page), overflow: surf.overflow, sprintsVisible: sprints.length > 0 && sprints.every((x) => x.visible),
            metricsClipped: sprints.flatMap((x) => x.clipped.map((mm) => `${x.id}:${mm}`)) });
          if (theme === 'light' && width === 380) pick.viewSprints = sprints.map((x) => x.id);
          if (theme === 'light' && !liveDone) {
            obs.ui.live = await liveStep(v, board, pack);   // the live-UI slot, with this view open (§8.7 step 8)
            liveDone = true;
          }
          await v.page.close();
        }
      }
      const re = await openSurface({ moduleKey: widget.key, entry: 'edit', theme: 'dark', width: 600, height: 420, asUser: viewer, extension: { ...ctx('w1', stored), entryPoint: 'edit' } });
      await finishSurface(re, { id: `widget-edit-${board}-dark`, kind: 'widget-edit', theme: 'dark', width: 600 }, '[data-testid="board-option"]');
      pick.reopenPressed = await re.page.evaluate(() => [...document.querySelectorAll('[data-testid="board-option"][aria-pressed="true"]')].map((x) => x.getAttribute('data-board-id')));
      await re.page.close();
      obs.ui.edit.picks.push(pick);
    }
    // G3: a second instance whose host-injected config names the FIRST board, after the second board was saved.
    if (scrum.length > 1 && configs[scrum[0]] !== undefined) {
      const v = await openSurface({ moduleKey: widget.key, entry: 'view', theme: 'light', width: 1180, height: 480, asUser: viewer, extension: ctx('w2', configs[scrum[0]]) });
      await finishSurface(v, { id: 'widget-view-second-instance', kind: 'widget-view', theme: 'light', width: 1180 }, '[data-testid="sprint"], [data-testid="needs-config"]');
      obs.ui.widget.secondInstance = { configBoard: scrum[0], sprints: (await widgetMetrics(v.page)).map((x) => x.id) };
      await v.page.close();
    }
  }
  if (action) {
    const active = pack.sprints.filter((x) => x.state === 'active');
    const forbidden = new Set(pack.issues.filter((i) => (i.commentForbiddenFor || []).includes(viewer)).map((i) => i.key));
    const ext = (sp) => ({ type: 'jira:sprintAction', sprint: { id: String(sp.id), state: sp.state }, board: { id: String(sp.originBoardId), type: 'scrum' } });
    for (const sp of active) {
      const sid = String(sp.id);
      for (const theme of ['light', 'dark']) {
        const s = await openSurface({ moduleKey: action.key, entry: 'view', theme, width: 800, height: 600, asUser: viewer, extension: ext(sp) });
        await finishSurface(s, { id: `sprint-action-${sid}-${theme}-800x600`, kind: 'sprint-action', theme, width: 800 }, 'table[data-testid="ledger"] tr[data-change-id], [data-metric]');
        if (theme === 'light') obs.ui.sprintAction.push(await exerciseModal(s, sid, forbidden, () => openSurface({ moduleKey: action.key, entry: 'view', theme, width: 800, height: 600, asUser: viewer, extension: ext(sp) })));
        await s.page.close();
      }
    }
    // Contract §2: "Every resolver returns a value and never throws: a failure (a Jira error, …) returns a value
    // describing it … and the surface shows it." One extra open of the first active sprint with the scoring site's
    // resolver-read fault armed: the resolver's first Jira read answers 500 (DESIGN §5.2; the scoring pack only).
    const rf = (pack.faults || []).find((f) => f.match?.scope === 'resolver-read');
    if (rf && active.length) obs.ui.resolverFault = await resolverFaultStep(action, active[0], ext(active[0]), rf, viewer);
    const future = pack.sprints.find((x) => x.state === 'future');
    if (future) {
      const s = await openSurface({ moduleKey: action.key, entry: 'view', theme: 'light', width: 800, height: 600, asUser: viewer, extension: ext(future) });
      await finishSurface(s, { id: `not-started-${future.id}`, kind: 'sprint-action', theme: 'light', width: 800 }, '[data-testid="not-started"], table[data-testid="ledger"]');
      obs.ui.notStarted = await s.page.evaluate(() => ({ onlyNotStarted: Boolean(document.querySelector('[data-testid="not-started"]'))
        && !document.querySelector('table[data-testid="ledger"], [data-metric], [data-testid="hidden-count"]') }));
      obs.ui.notStarted.sprintId = String(future.id);
      await s.page.close();
    }
  }
  obs.ui.calls = takeCalls(emu);
  // Every resolver invoke of the whole UI phase, including those after first paint (explain, post, sort): an invoke
  // that throws is the defect b_invoke_contract grades (m_llm_no_refusal_path's refusal throws on a click).
  obs.ui.invokeResponses = (emu.bridgeLog || []).filter((b) => opName(b) === 'invoke').map((b) => ({
    surface: b.surfaceId ?? null, functionKey: b.payload?.functionKey ?? b.functionKey ?? null, invocationId: b.invocationId ?? null,
    response: b.result ?? b.response ?? null, threw: Boolean(b.error) || b.ok === false, error: b.error ? String(b.error) : null }));
  // R9: the data views (the widget with a board, the modal of a started sprint).
  const views = obs.ui.surfaces.filter((x) => x.kind === 'widget-view' && x.id !== 'widget-view-noconfig');
  const modals = obs.ui.surfaces.filter((x) => x.kind === 'sprint-action' && !String(x.id).startsWith('not-started'));
  if (widget) obs.boot['widget-view'] = bootOf(views);
  if (action) obs.boot['sprint-modal'] = bootOf(modals);
  await contactSheet();
}

const tableRows = (page) => page.evaluate(() => [...document.querySelectorAll('table[data-testid="ledger"] tr[data-change-id]')].map((tr) => {
  const cell = (c) => tr.querySelector(`td[data-col="${c}"]`);
  const text = (c) => (cell(c) ? cell(c).textContent.trim() : null);
  const time = cell('at') ? cell('at').querySelector('time[datetime]') : null;
  return { changeId: tr.getAttribute('data-change-id'), selected: tr.getAttribute('aria-selected') === 'true',
    cells: { issue: text('issue'), points: text('points'), kind: text('kind'), by: text('by'), at: time ? time.getAttribute('datetime') : null, source: text('source') } };
}));
const ariaSort = (page) => page.evaluate(() => Object.fromEntries([...document.querySelectorAll('th[data-col][aria-sort]')]
  .filter((th) => th.getAttribute('aria-sort') !== 'none').map((th) => [th.getAttribute('data-col'), th.getAttribute('aria-sort')])));

// @forge/bridge FlagOptions: "If `appearance` is given, `type` is overriden to equal `appearance`" — and showFlag
// itself forwards `type: options.type ?? 'info'`, so an appearance-only error flag arrives typed 'info'. The kit's
// host page renders `appearance ?? type` (bridge-page.cjs); the probe reads the flag the same way.
const flagType = (b) => String(b.payload?.appearance ?? b.options?.appearance ?? b.appearance
  ?? b.payload?.type ?? b.options?.type ?? b.type ?? '');
function flagCounts(ops) {
  const flags = ops.filter((b) => opName(b) === 'showFlag').map(flagType);
  return { successFlags: flags.filter((t) => t === 'success').length, errorFlags: flags.filter((t) => ['error', 'warning'].includes(t)).length };
}

const commentsNow = () => (emu.log || []).filter((e) => e.method === 'POST' && /\/comment$/.test(String(e.path ?? e.target ?? '').split('?')[0]) && [200, 201].includes(e.status)).length;

async function settle(s) {
  let last = -1;
  for (let i = 0; i < 40; i++) {
    const n = bridgeOps(s).length + commentsNow();
    if (n === last && i > 4) return;
    last = n;
    await sleep(250);
  }
}

// §17.8 A: a gesture's click the probe could not deliver (an overlay intercepted it, it never settled) is harness
// evidence, never the app's: it is recorded as `clickFailed` (score_forge2 _undeliverable). The click waits as a person
// would (Playwright's actionability wait); a control that is still absent, hidden or disabled when the wait ends is the
// app's own state, so the gesture's counts stand as the app's.
const firstLine = (e) => String(e?.message ?? e).split('\n')[0].slice(0, 200);
async function deliver(control, act) {
  try {
    await act(control);
    return null;
  } catch (e) {
    const appState = await (async () => !(await control.count()) || !(await control.isVisible()) || await control.isDisabled())()
      .catch(() => false);
    return appState ? null : firstLine(e);
  }
}
// Clicking a row selects it (contract §5): its `kind` cell, or the row itself when it has none.
function selectRow(page, changeId) {
  const row = page.locator(`tr[data-change-id="${changeId}"]`).first();
  return deliver(row, async () => {
    const cell = row.locator('td[data-col="kind"]').first();
    await ((await cell.count()) ? cell : row).click();
  });
}
// One post gesture on the selected row: the comments the site saw and the flags the page raised. A selection the probe
// could not deliver makes the gesture the harness's too (the post would land on whichever row was selected before).
async function postGesture(s, selectFailed, act) {
  const c0 = commentsNow();
  const o0 = bridgeOps(s).length;
  const failed = selectFailed ? `select: ${selectFailed}` : await deliver(s.page.locator('[data-testid="post-summary"]').first(), act);
  await settle(s);
  return { commentsAdded: commentsNow() - c0, ...flagCounts(bridgeOps(s).slice(o0)), ...(failed ? { clickFailed: failed } : {}) };
}
// The issue of a ledger row the viewer may see now: it exists and they can browse it (the world deletes issues and
// revokes browse permission while v2 runs). A leaked hidden row is b_no_permission_leak's evidence; posting on it would
// grade its 404 as the comment flow's.
function viewerIssue(cell) {
  const key = String(cell ?? '').match(/\b[A-Z][A-Z0-9]+-\d+\b/)?.[0];
  const issue = key ? site.state.issueByIdOrKey(key) : null;
  return issue && site.state.canBrowse(site.pack.viewer, issue) ? issue : null;
}

async function exerciseModal(s, sid, forbidden, reopen) {
  const page = s.page;
  const out = { sprintId: sid };
  Object.assign(out, await page.evaluate(() => {
    const m = (n) => { const x = document.querySelector(`[data-metric="${n}"]`); return x ? x.textContent.trim() : null; };
    const h = document.querySelector('[data-testid="hidden-count"]');
    return { metrics: { committed: m('committed'), added: m('added'), removed: m('removed'), creep: m('creep') },
      hiddenCount: h ? h.textContent.trim() : null,
      headers: [...document.querySelectorAll('table[data-testid="ledger"] th[data-col]')].map((th) => th.getAttribute('data-col')) };
  }));
  out.rows = (await tableRows(page)).map(({ changeId, cells }) => ({ changeId, cells }));
  if (!out.rows.length) {
    // An empty table still owes a working close (u_modal_close is graded on every rendered modal).
    const o1 = bridgeOps(s).length;
    await page.locator('[data-testid="close"]').first().click({ timeout: 3000 }).catch(() => {});
    await sleep(200);
    out.close = { closeCalled: bridgeOps(s).slice(o1).some((b) => opName(b) === 'close') };
    return out;
  }
  out.sortAt = [];
  for (let i = 0; i < 2; i++) {
    await page.locator('th[data-col="at"]').first().click().catch(() => {});
    await sleep(200);
    out.sortAt.push({ rows: (await tableRows(page)).map((r) => r.changeId), ariaSort: await ariaSort(page) });
  }
  // Contract §5: the `at` toggle starts "with ascending when another sort was active". Only an app whose other
  // headers sort can be held to it: the first header whose click moves aria-sort off `at` is that other sort.
  out.sortAfterOther = { col: null, tried: [] };
  for (const col of out.headers.filter((c) => c !== 'at')) {
    await page.locator(`th[data-col="${col}"]`).first().click().catch(() => {});
    await sleep(200);
    const active = await ariaSort(page);
    out.sortAfterOther.tried.push(col);
    if (Object.keys(active).some((c) => c !== 'at')) {
      out.sortAfterOther.col = col;
      out.sortAfterOther.otherAriaSort = active;
      await page.locator('th[data-col="at"]').first().click().catch(() => {});
      await sleep(200);
      out.sortAfterOther.rows = (await tableRows(page)).map((r) => r.changeId);
      out.sortAfterOther.ariaSort = await ariaSort(page);
      break;
    }
  }

  await sleep(200);

  // router: the first row's issue key
  const first = out.rows[0];
  const before = bridgeOps(s).length;
  const url0 = page.url();
  await page.locator(`tr[data-change-id="${first.changeId}"] td[data-col="issue"] a, tr[data-change-id="${first.changeId}"] td[data-col="issue"]`).first().click().catch(() => {});
  await sleep(400);
  out.router = [{ issueKey: first.cells.issue, ops: bridgeOps(s).slice(before).filter((b) => ['open', 'navigate'].includes(opName(b)))
    .map((b) => ({ op: opName(b), url: routeOf(b.payload) })),
    popup: s.ev.popups > 0, topNavigation: page.url() !== url0 }];
  // post flows on a fresh surface so sorting and routing leave no state behind
  const post = await reopen();
  await waitMeaningful(post, 'table[data-testid="ledger"] tr[data-change-id]');
  const rows = await tableRows(post.page);
  // §17.8 B: every post goes to a row the viewer may see now (viewerIssue).
  const seen = rows.map((r) => ({ ...r, issue: viewerIssue(r.cells.issue) })).filter((r) => r.issue);
  const postable = seen.filter((r) => !forbidden.has(r.issue.key));
  const target = postable[0];
  if (target) {
    const selectFailed = await selectRow(post.page, target.changeId);
    await sleep(200);
    out.select = { changeId: target.changeId, ariaSelected: (await tableRows(post.page)).find((r) => r.changeId === target.changeId)?.selected ?? false,
      ...(selectFailed ? { clickFailed: selectFailed } : {}) };
    out.post = await postGesture(post, selectFailed, (l) => l.click());
    // §17.8 B: the double click goes to a fresh row on another issue, so an app that will not post the same summary
    // twice is not read as one that posts nothing. Gap #24: two clicks on the SAME element handle (a moving label
    // cannot dodge the second); force skips actionability waits but a disabled button still swallows the click, as a
    // browser does.
    const fresh = postable.find((r) => r.issue.key !== target.issue.key);
    if (fresh) {
      const failed = await selectRow(post.page, fresh.changeId);
      await sleep(200);
      let second = null;
      const g = await postGesture(post, failed, async (l) => {
        const h = await l.elementHandle();
        await h.click();
        // The second click misses only when the first changed the page (the app removed, hid or replaced the button),
        // which a person's second click meets as well: recorded, never `clickFailed`.
        await h.click({ force: true }).catch((e) => { second = firstLine(e); });
      });
      out.doubleClick = { changeId: fresh.changeId, ...g, ...(second ? { secondClick: second } : {}) };
    } else out.doubleClickSkipped = 'no viewer-visible row on another issue';
    const forb = seen.find((r) => forbidden.has(r.issue.key));
    if (forb) {
      const g = await postGesture(post, await selectRow(post.page, forb.changeId), (l) => l.click());
      const orderBefore = (await tableRows(post.page)).map((r) => r.changeId);
      await post.page.locator('th[data-col="at"]').first().click().catch(() => {});
      await sleep(200);
      const orderAfter = (await tableRows(post.page)).map((r) => r.changeId);
      out.forbidden = { issueKey: forb.cells.issue, ...g,
        sortWorksAfter: orderAfter.length === orderBefore.length && orderAfter.join() !== orderBefore.join() };
    }
  }
  if (!obs.ui.explain && rows.length) obs.ui.explain = await explainSteps(post, sid);
  const o1 = bridgeOps(post).length;
  await post.page.locator('[data-testid="close"]').first().click().catch(() => {});
  await sleep(200);
  out.close = { closeCalled: bridgeOps(post).slice(o1).some((b) => opName(b) === 'close') };
  await post.page.close();
  return out;
}

// One clip from the per-page recordings: Playwright writes one VP8 WebM per page at RECORD_SIZE, so the concat
// demuxer joins them without re-encoding; SB's encoder (media_sb71.mjs encodeFullRecording: VP9, 4 MiB limit,
// ffmpeg/ffprobe from BENCH_FFMPEG/BENCH_FFPROBE or PATH) makes the publishable copy. Any failure is recorded in
// media.errors and the graded evidence is untouched.
async function assembleRecording() {
  const root = dirname(mediaDir);
  const media = { schemaVersion: 1, scorerVersion: 'forge-2.0', recording: 'graded-browser', videos: [], errors: [] };
  try {
    const segments = [];
    for (const { page, label } of recording.pages) {
      if (!page.isClosed()) await page.close();
      const raw = await page.video()?.path();
      if (raw && existsSync(raw)) segments.push({ raw, label });
    }
    await recording.context.close();
    if (!segments.length) throw new Error('no graded surface was recorded');
    const { encodeFullRecording, videoDuration } = await import('./media_sb71.mjs');
    const list = join(mediaDir, 'raw', 'segments.txt');
    writeFileSync(list, segments.map((x) => `file '${x.raw.replace(/'/g, "'\\''")}'`).join('\n') + '\n');
    const session = join(mediaDir, 'raw', 'forge-session.webm');
    execSync(`${JSON.stringify(process.env.BENCH_FFMPEG || 'ffmpeg')} -hide_banner -loglevel error -f concat -safe 0 -i ${JSON.stringify(list)} -c copy -y ${JSON.stringify(session)}`);
    let at = 0;
    const timeline = segments.map(({ raw, label }) => {
      const d = videoDuration(raw);
      const seg = { surface: label, startSeconds: Math.round(at * 100) / 100, endSeconds: Math.round((at + d) * 100) / 100 };
      at += d;
      return seg;
    });
    const sourceFile = relative(root, session);
    const video = { file: sourceFile, caption: 'Original graded browser recording; publication encoding pending', mimeType: 'video/webm',
      scenario: 'ui', sourceFile, segments: timeline, selection: 'Every graded Custom UI surface, in grading order, at original speed' };
    const output = join(mediaDir, 'forge-ui.webm');
    try {
      const sourceInterval = encodeFullRecording(session, output);
      Object.assign(video, { file: relative(root, output), sourceInterval,
        caption: 'Full graded browser recording: dashboard widget (no config, edit + Save, light and dark, 380 and 1180 px, a second '
          + 'instance), sprint action modal (sort, router, select, comment post through the 429 retry, double click, forbidden post, close), '
          + 'the not-started sprint and the Forge LLM cases' });
    } catch (error) {
      media.errors.push('Publication encoding failed; original retained: ' + String(error.message).slice(0, 180));
    }
    const bytes = readFileSync(join(root, video.file));
    video.sha256 = (await import('node:crypto')).createHash('sha256').update(bytes).digest('hex');
    video.bytes = bytes.length;
    video.publishable = bytes.length <= 4 * 1024 * 1024 && video.file !== sourceFile;
    media.videos.push(video);
  } catch (error) {
    media.errors.push('Recording unavailable: ' + String(error?.message || error).slice(0, 240));
  }
  const manifest = join(mediaDir, 'media-manifest.json');
  writeFileSync(manifest, JSON.stringify(media, null, 2) + '\n');
  return { manifest: relative(dirname(mediaDir), manifest), ...media };
}


// The live-UI slot (DESIGN §5.2/§8.7 step 8): with the widget open, watch it idle (polling would invoke), then deliver
// the two held-back live changes and drain, then read whether the open widget shows the new numbers without a reload.
// policy: an idle window longer than the "every few seconds" polling the m_rt_poll_instead mutant does (DESIGN §13.5).
const LIVE_IDLE_MS = 8000;
// policy: how long the open widget has to show the new numbers after the drain (one realtime round trip).
const LIVE_SETTLE_MS = 15000;
async function liveStep(s, board, pack) {
  const changes = pack.live.filter((e) => e.delivery?.liveUi);
  if (!changes.length) return { absent: 'the pack carries no live-UI changes (delivery.liveUi, DESIGN §5.2)' };
  let navigations = 0;
  s.page.on('framenavigated', (f) => { if (f === s.page.mainFrame()) navigations += 1; });
  const ops0 = bridgeOps(s).length;
  await sleep(LIVE_IDLE_MS);
  const idle = bridgeOps(s).slice(ops0);
  const before = await widgetMetrics(s.page);
  for (const change of changes) {
    await emu.deliverProductEvent(change);
    await emu.drainQueues();
  }
  const want = JSON.stringify(before.map((x) => x.metrics));
  const deadline = Date.now() + LIVE_SETTLE_MS;
  let after = before;
  while (Date.now() < deadline) {
    after = await widgetMetrics(s.page);
    if (JSON.stringify(after.map((x) => x.metrics)) !== want) break;
    await sleep(250);
  }
  await sleep(500);
  after = await widgetMetrics(s.page);
  const ops = bridgeOps(s);
  return { board, subscribed: ops.some((b) => opName(b) === 'subscribeRealtimeChannel'),
    idleInvokes: idle.filter((b) => ['invoke', 'fetchProduct'].includes(opName(b))).length, idleMs: LIVE_IDLE_MS,
    reloaded: navigations > 0, delivered: changes.map((c) => c.changelogId),
    sprintsBefore: before.map(({ id, metrics }) => ({ id, metrics })), sprintsAfter: after.map(({ id, metrics }) => ({ id, metrics })) };
}

// The resolver-read fault (DESIGN §5.2): armed for one open, it answers the first Jira read of the surface's resolver
// with Jira's 500. Graded: the invoke that met it returned a value (no throw through the bridge) and the surface shows
// something (not a blank page). Not a nominal scenario: console output here is not graded by v_console_clean.
async function resolverFaultStep(action, sp, extension, fault, viewer) {
  const fired0 = site.faultLog.length;
  site.control.arm({ id: fault.id });
  const s = await openSurface({ moduleKey: action.key, entry: 'view', theme: 'light', width: 800, height: 600, asUser: viewer, extension });
  await settle(s);
  await sleep(500);
  const rec = site.faultLog.slice(fired0).find((x) => x.fault === fault.id && x.kind === 'fired');
  site.control.disarm({ id: fault.id });
  const invokes = bridgeOps(s).filter((b) => opName(b) === 'invoke');
  const hit = rec ? invokes.find((b) => b.invocationId && b.invocationId === rec.invocationId) : null;
  const text = await s.page.evaluate(() => (document.body ? document.body.innerText.replace(/\s+/g, ' ').trim() : ''));
  const shot = join(shotsDir, `sprint-action-resolver-500-${sp.id}.png`);
  await s.page.screenshot({ path: shot, fullPage: false }).catch(() => {});
  obs.shots.push(shot);
  await s.page.close();
  return { sprintId: String(sp.id), faultId: fault.id, fired: Boolean(rec), invocationId: rec?.invocationId ?? null,
    invoke: hit ? { functionKey: hit.payload?.functionKey ?? null, threw: Boolean(hit.error) || hit.ok === false,
      error: hit.error ? String(hit.error) : null, response: hit.result ?? null } : null,
    pageErrors: s.ev.pageErrors, text: text.slice(0, 400), blank: !text };
}

// The explanation box's text as a person reads it (contract §5's [data-testid="explanation"]).
const explanationOf = (page) => page.evaluate(() => {
  const box = document.querySelector('[data-testid="explanation"]');
  // The explanation's own words: the box without its per-change elements (contract §5: one [data-change-id]
  // element per returned id), whose keys and ids are not the sentence's numbers.
  let own = '';
  if (box) {
    const copy = box.cloneNode(true);
    copy.querySelectorAll('[data-change-id]').forEach((e) => e.remove());
    own = copy.textContent.replace(/\s+/g, ' ').trim();
  }
  return { text: box ? box.textContent.trim() : '', own,
    ids: box ? [...box.querySelectorAll('[data-change-id]')].map((e) => e.getAttribute('data-change-id')) : [] };
});

// The five scripted explain answers (site/llm.cjs SCRIPT: clean, digits, refusal, malformed, error), in order. Each is
// a separate request: the virtual clock moves past R8's 10-minute cache window before each one.
async function explainSteps(s, sid) {
  const button = s.page.locator('[data-testid="explain"]').first();
  if (!(await button.count())) return { sprintId: sid, steps: [], absent: 'no [data-testid="explain"] control' };
  await emu.llm.phase(`explain-${sid}`);
  const steps = [];
  for (let i = 0; i < 5; i += 1) {
    await emu.advance(EXPLAIN_GAP_MS);
    const since = (await emu.llm.log(0)).next;
    const o0 = bridgeOps(s).length;
    await button.click().catch(() => {});
    await settle(s);
    const fresh = ((await emu.llm.log(since)).entries || []).filter((e) => e.op === 'chat' || e.op === 'stream');
    const shown = await explanationOf(s.page);
    const orderBefore = (await tableRows(s.page)).map((r) => r.changeId);
    await s.page.locator('th[data-col="at"]').first().click().catch(() => {});
    await sleep(200);
    const orderAfter = (await tableRows(s.page)).map((r) => r.changeId);
    await s.page.locator('th[data-col="at"]').first().click().catch(() => {});   // back to the default order
    await sleep(200);
    steps.push({ step: fresh[0]?.step ?? null, llmCalls: fresh.length, llm: fresh[0] ?? null, explanation: shown.text,
      explanationOwn: shown.own, idsShown: shown.ids, ...flagCounts(bridgeOps(s).slice(o0)),
      sortWorksAfter: orderAfter.length === orderBefore.length && orderAfter.length > 1 ? orderAfter.join() !== orderBefore.join() : orderAfter.length > 0 });
  }
  return { sprintId: sid, steps };
}

// R8's cases (LLM_V2_CASES) on the first started sprint's modal: how many model calls each request made, what the
// person was shown, and the writes the explain flow made outside the viewer's sprint scope (a manipulated tool call).
async function llmCases(pack, v) {
  const action = modules0('jira:sprintAction')[0];
  const sp = pack.sprints.find((x) => x.state === 'active');
  if (!action || !sp) { obs.llm_v2Absent = !action ? 'no jira:sprintAction module in the manifest' : 'no active sprint on the site'; return; }
  if (!browser) throw new Error('the browser did not start (section ui)');
  const sid = String(sp.id);
  const viewer = pack.viewer;
  const ext = { type: 'jira:sprintAction', sprint: { id: sid, state: sp.state }, board: { id: String(sp.originBoardId), type: 'scrum' } };
  const inScope = () => new Set(site.state.allIssues().filter((i) => (i.fields[pack.sprintFieldId] || []).some((x) => String(x.id) === sid)
    && site.state.canBrowse(viewer, i)).flatMap((i) => [String(i.id), i.key]));
  const panelSet = async (label, value) => {
    const a = await v.openAdmin(pack.admin, 'admin');
    await a.ui.waitIdle();
    if (!v.found(a.ui, label) || !v.found(a.ui, LABELS.save)) { await v.closeAll(); return false; }
    await a.ui.setValue(label, value);
    await a.ui.click(LABELS.save);
    await a.ui.waitIdle();
    await v.closeAll();
    return true;
  };
  for (const c of LLM_V2_CASES) {
    await emu.advance(EXPLAIN_GAP_MS);
    const row = { case: c.case, llm_calls: null, shown_text: '', writes_out_of_scope: null, per_click: [] };
    if (c.admin) {
      if (!v.usable()) { row.absent = 'no admin panel to switch the setting'; obs.llm_v2.push(row); continue; }
      row.setting_applied = await panelSet(c.admin.label, c.admin.value);
    }
    await emu.llm.phase(c.script, c.steps);
    takeCalls(emu);
    const s = await openSurface({ moduleKey: action.key, entry: 'view', theme: 'light', width: 800, height: 600, asUser: viewer, extension: ext });
    await waitMeaningful(s, 'table[data-testid="ledger"] tr[data-change-id], [data-metric]');
    const button = s.page.locator('[data-testid="explain"]').first();
    if (!(await button.count())) row.absent = 'no [data-testid="explain"] control';
    // Contract §16: the budget binds "once the virtual day's tokens reach" it. The day is a UTC day of the virtual
    // clock and turns over between cases on a late upgrade: a click on a day whose tokens are still under the budget
    // owes no refusal, so the case records the day's tokens before its click.
    if (c.admin?.label === LABELS.budget) {
      const day = new Date(site.state.now()).toISOString().slice(0, 10);
      row.day_tokens = (await emu.llm.log(0)).state?.tokensByDay?.[day]?.total_tokens ?? 0;
      row.budget_reached = row.day_tokens >= c.admin.value;
    }
    const l0 = emu.log.length;
    for (let i = 0; i < c.clicks && !row.absent; i += 1) {
      const since = (await emu.llm.log(0)).next;
      await button.click().catch(() => {});
      await settle(s);
      row.per_click.push(((await emu.llm.log(since)).entries || []).filter((e) => e.op === 'chat' || e.op === 'stream').length);
    }
    // A repeated request is graded on its own calls (the cache); every other case on all of its calls.
    row.llm_calls = row.absent ? null : c.clicks > 1 ? row.per_click[row.per_click.length - 1] : row.per_click.reduce((n, x) => n + x, 0);
    row.shown_text = (await explanationOf(s.page)).text;
    const scope = inScope();
    const writes = emu.log.slice(l0).filter((e) => isWrite(e) && ['jira', 'kvs'].includes(e.service ?? 'jira'));
    row.writes_out_of_scope = writes.filter((e) => issueRefs(e).some((r) => !scope.has(r))).length;
    row.writes = writes.map((e) => `${e.method} ${String(e.path).split('?')[0]}`);
    await s.page.close();
    (obs.ui.llmCalls ??= []).push(...takeCalls(emu));
    if (c.admin?.restore !== undefined && row.setting_applied) row.setting_restored = await panelSet(c.admin.label, c.admin.restore === true ? !c.admin.value : c.admin.restore);
    obs.llm_v2.push(row);
  }
}
const modules0 = (t) => (emu.modules ? emu.modules(t) : (emu.manifest?.modules?.[t] || [])) || [];

async function contactSheet() {
  const shots = obs.shots.filter(existsSync);
  if (!shots.length) return;
  const page = await browser.newPage({ viewport: { width: 1600, height: 1200 } });
  const tiles = shots.map((p) => `<figure><img src="data:image/png;base64,${readFileSync(p).toString('base64')}"><figcaption>${relative(shotsDir, p)}</figcaption></figure>`).join('');
  await page.setContent(`<style>body{margin:0;font:11px sans-serif;display:flex;flex-wrap:wrap;gap:6px;background:#222;color:#eee}figure{margin:0;width:390px}img{width:390px}</style>${tiles}`);
  const sheet = join(shotsDir, 'contact-sheet.png');
  await page.screenshot({ path: sheet, fullPage: true });
  obs.shots.push(sheet);
  await page.close();
}

// ── --selftest: the agenda and the evidence helpers against fakes ───────────────────────────

async function selftest() {
  const assert = (await import('node:assert/strict')).default;
  const results = [];
  const test = async (name, fn) => { await fn(); results.push(name); };

  await test('runHours: one ordered agenda, every hour checkpointed at its mark', async () => {
    const t0 = 1_000 * H;
    let clock = t0;
    const plan = [t0 + 10 * 60_000, t0 + 70 * 60_000, t0 + 2 * H, t0 + 2 * H + 1, t0 + 5.99 * H, t0 + 7 * H];
    let cursor = 0;
    const seen = [];
    const io = {
      now: () => clock,
      advanceTo: async (t) => { if (t > clock) clock = t; },
      nextDeliveryAt: () => (cursor < plan.length ? Math.max(clock, plan[cursor]) : null),
      deliver: async () => { seen.push(['deliver', clock, plan[cursor]]); cursor += 1; return true; },
      drain: async (t) => seen.push(['drain', t]),
      world: async (t) => seen.push(['world', t]),
      hourly: async (k, end) => seen.push(['hourly', clock, k, end]),
      ci: async () => seen.push(['ci', clock]),
      panel: async (cp) => seen.push(['panel', clock, cp]),
      checkpoint: async (cp, mark) => seen.push(['checkpoint', clock, cp, mark]),
    };
    await runHours(io, t0);
    const cps = seen.filter((x) => x[0] === 'checkpoint');
    assert.deepEqual(cps.map((x) => x[2]), ['h1', 'h2', 'h3', 'h4', 'h5', 'h6']);
    for (const [, at, , mark] of cps) assert.equal(at, mark);
    assert.deepEqual(seen.filter((x) => x[0] === 'ci').map((x) => x[1]), [t0 + CI_AT_MS]);
    assert.deepEqual(seen.filter((x) => x[0] === 'panel').map((x) => [x[1], x[2]]), [1, 2, 3, 4, 5, 6].map((k) => [t0 + k * H - PANEL_BEFORE_MS, `h${k}`]));
    assert.deepEqual(seen.filter((x) => x[0] === 'hourly').map((x) => [x[1], x[2]]), [1, 2, 3, 4, 5].map((k) => [t0 + k * H, k]));
    // every delivery inside the six hours happens at its own time, in its own hour; the one past them waits
    assert.deepEqual(seen.filter((x) => x[0] === 'deliver').map((x) => x[1]), plan.slice(0, 5));
    assert.equal(cursor, 5);
    const order = seen.filter((x) => x[0] === 'deliver' || x[0] === 'checkpoint');
    for (const [i, x] of order.entries()) {
      if (x[0] !== 'deliver') continue;
      const hour = Math.floor((x[2] - t0) / H) + 1;
      assert.equal(order.slice(0, i).filter((y) => y[0] === 'checkpoint').length, hour - 1, `delivery at ${x[2] - t0} ms lands in hour ${hour}`);
    }
    // the world and the queues are brought to each agenda point before it acts, never backwards
    const worlds = seen.filter((x) => x[0] === 'world').map((x) => x[1]);
    assert.deepEqual(worlds, [...worlds].sort((a, b) => a - b));
    for (const [i, x] of seen.entries()) if (x[0] === 'deliver') assert.equal(seen[i - 1][0], 'drain');
  });

  await test('quotaWindow: the full window on every upgrade offset; the reads it would hold move before it, in their hour', async () => {
    // 58.906 min: seed 0123456789abcdef's upgrade offset, where the old reads-avoiding window shrank to 300 s.
    for (const offMin of [0, 3, 17, 25, 30, 44.9, 58, 58.906, 59.99]) {
      const t0 = 1_000 * H + offMin * 60_000;
      const w = quotaWindow(t0);
      assert.equal(w.until % H, 0);
      assert.equal(w.until - w.at, QUOTA_WINDOW_MS);
      assert.ok(QUOTA_WINDOW_MS - 900_000 >= 15 * 60_000, 'background arriving 15 virtual minutes into the wall still meets a Retry-After past 900 s');
      assert.ok(w.at > t0 + CI_AT_MS && w.until < t0 + HOURS * H, `offset ${offMin}: the wall sits after the CI sequence, inside the scored hours`);
      let clock = t0;
      const reads = [];
      const draws = [];
      await runHours({ now: () => clock, advanceTo: async (t) => { if (t > clock) clock = t; }, nextDeliveryAt: () => null, deliver: async () => false,
        drain: async () => {}, world: async () => {}, hourly: async () => {}, ci: async () => {}, panel: async (cp) => reads.push([cp, clock]),
        checkpoint: async () => {}, quotaWindow, quota: async () => draws.push(clock) }, t0);
      assert.deepEqual(draws, [w.at], `offset ${offMin}: one draw, at the window's start`);
      assert.deepEqual(reads.map(([cp]) => cp), ['h1', 'h2', 'h3', 'h4', 'h5', 'h6']);
      for (const [cp, p] of reads) {
        const k = Number(cp.slice(1));
        assert.ok(p > t0 + (k - 1) * H && p < t0 + k * H, `offset ${offMin}: ${cp} reads inside its own hour`);
        assert.ok(!(p > w.at - READS_MARGIN_MS && p < w.until), `offset ${offMin}: ${cp} reads outside the wall`);
        assert.equal(p, readsAt(t0 + k * H - PANEL_BEFORE_MS, w));
      }
      if (offMin === 58.906) assert.equal(reads[2][1], w.at - READS_MARGIN_MS, 'the h3 read moved before the wall');
    }
  });

  await test('runHours: a drain that runs past a mark makes the checkpoint late, never early', async () => {
    const t0 = 0;
    let clock = t0;
    const marks = [];
    await runHours({
      now: () => clock, advanceTo: async (t) => { if (t > clock) clock = t; }, nextDeliveryAt: () => null, deliver: async () => false,
      drain: async (t) => { if (t === 2 * H) clock = 2 * H + 90_000; }, world: async () => {}, hourly: async () => {},
      ci: async () => {}, panel: async () => {}, checkpoint: async (cp) => marks.push([cp, clock]),
    }, t0);
    assert.deepEqual(marks.slice(0, 3), [['h1', H], ['h2', 2 * H + 90_000], ['h3', 3 * H]]);
  });

  await test('isWrite: storage writes, queue pushes and Jira writes count; reads and refusals do not', async () => {
    assert.equal(isWrite({ service: 'kvs', method: 'POST', path: '/api/v1/entity/set', status: 200 }), true);
    assert.equal(isWrite({ service: 'kvs', method: 'POST', path: '/api/v1/secret/set', status: 204 }), true);
    assert.equal(isWrite({ service: 'kvs', method: 'POST', path: '/api/v1/transaction', status: 200 }), true);
    assert.equal(isWrite({ service: 'kvs', method: 'POST', path: '/api/v1/entity/query', status: 200 }), false);
    assert.equal(isWrite({ service: 'kvs', method: 'POST', path: '/api/v1/set', status: 409 }), false);
    assert.equal(isWrite({ service: 'queue', method: 'POST', path: '/webhook/queue/publish', body: { payload: [] }, status: 201 }), true);
    assert.equal(isWrite({ service: 'queue', method: 'POST', path: '/webhook/queue/stats', body: { jobId: 'j' }, status: 200 }), false);
    assert.equal(isWrite({ service: 'jira', method: 'POST', path: '/rest/api/3/search/jql', status: 200 }), false);
    assert.equal(isWrite({ service: 'jira', method: 'POST', path: '/rest/api/3/changelog/bulkfetch', status: 200 }), false);
    assert.equal(isWrite({ service: 'jira', method: 'POST', path: '/rest/api/3/issue/OPS-1/comment', status: 201 }), true);
    assert.equal(isWrite({ service: 'jira', method: 'PUT', path: '/rest/api/3/app/field/value', status: 204 }), true);
    assert.equal(isWrite({ service: 'jira', method: 'GET', path: '/rest/api/3/issue/OPS-1', status: 200 }), false);
  });

  await test('issueRefs: path keys and bulk field-value ids', async () => {
    assert.deepEqual(issueRefs({ path: '/rest/api/3/issue/OPS-7/comment' }), ['OPS-7']);
    assert.deepEqual(issueRefs({ path: '/rest/api/3/app/field/value', body: { updates: [{ customField: 'x', issueIds: [101, 102], value: 'removed' }] } }), ['101', '102']);
    assert.deepEqual(issueRefs({ service: 'kvs', path: '/api/v1/entity/set', body: { entityName: 'scope-ledger', key: 'k', value: { issueKey: 'PAY-9', issueId: '77' } } }), ['PAY-9', '77']);
    assert.deepEqual(issueRefs({ service: 'kvs', path: '/api/v1/set', body: { key: 'explain:41', value: { text: 'why' } } }), []);
  });

  await test('hoursOf: per quota hour, background and person apart, counted from the upgrade hour', async () => {
    const up = 10 * H + 5;
    const rows = hoursOf([
      { t_ms: 10 * H + 10, kind: 'background', cost: 2, status: 200 },
      { t_ms: 10 * H + 20, kind: 'person', cost: 1, status: 200 },
      { t_ms: 11 * H, kind: 'background', cost: 0, status: 429 },
      { t_ms: 12 * H + 1, kind: 'background', cost: 3, status: 200 },
    ], up);
    assert.deepEqual(rows.map((r) => [r.hour, r.used, r.background_used, r.person_used, r.refused]), [[1, 3, 2, 1, 0], [2, 0, 0, 0, 1], [3, 3, 3, 0, 0]]);
  });

  await test('requestIssues: a per-issue 429 names the issue its write window refused; reads name none', async () => {
    const rateModel = require('../forge2/site/rate.cjs');
    const pack = { issues: [{ id: 101, key: 'PAY-1' }, { id: 102, key: 'PAY-2' }, { id: 103, key: 'PAY-3' }] };
    const comment = '/rest/api/3/issue/{issueIdOrKey}/comment';
    const fv = '/rest/api/3/app/field/value';
    const perIssue = rateModel.MODEL.reasons.perIssue;
    const entries = [
      { t_ms: 0, invocation: 'a', method: 'POST', path_tpl: comment, status: 201 },
      { t_ms: 500, invocation: 'b', method: 'POST', path_tpl: fv, status: 204 },
      { t_ms: 1000, invocation: 'c', method: 'POST', path_tpl: comment, status: 429, reason: perIssue },
      { t_ms: 1200, invocation: 'd', method: 'POST', path_tpl: fv, status: 429, reason: perIssue },
      { t_ms: 1300, invocation: 'e', method: 'GET', path_tpl: '/rest/api/3/issue/{issueIdOrKey}', status: 200 },
      { t_ms: 1400, invocation: 'f', method: 'POST', path_tpl: fv, status: 204 },
    ];
    const raw = [{ path: '/rest/api/3/issue/PAY-1/comment' }, { path: fv }, { path: '/rest/api/3/issue/101/comment?expand=x' }, { path: fv },
      { path: '/rest/api/3/issue/PAY-3' }, { path: fv }];
    const calls = [
      { service: 'jira', invocationId: 'b', op: `POST ${fv}`, body: { updates: [{ customField: 'f', issueIds: [102], value: 'committed' }] } },
      { service: 'jira', invocationId: 'd', op: `POST ${fv}`, body: { updates: [{ customField: 'f', issueIds: [103, 102], value: 'committed' }] } },
      { service: 'jira', invocationId: 'f', op: `POST ${fv}`, body: { updates: [{ customField: 'f', issueIds: [101, 103], value: 'removed' }] } },
    ];
    // the comment by key and the 429 by id are one issue; the bulk 429 names PAY-2 (written 700 ms before), not PAY-3
    assert.deepEqual(requestIssues(entries, raw, calls, pack, rateModel), ['101', '102', '101', '102', null, null]);
    // a refused write opens no window: PAY-2's own 429 does not shadow PAY-3, written 100 ms before the bulk 429
    assert.deepEqual(requestIssues([
      { t_ms: 3000, invocation: 'g', method: 'POST', path_tpl: comment, status: 429, reason: perIssue },
      { t_ms: 3100, invocation: 'h', method: 'POST', path_tpl: comment, status: 201 },
      { t_ms: 3200, invocation: 'i', method: 'POST', path_tpl: fv, status: 429, reason: perIssue },
    ], [{ path: '/rest/api/3/issue/PAY-2/comment' }, { path: '/rest/api/3/issue/PAY-3/comment' }, { path: fv }],
    [{ service: 'jira', invocationId: 'i', op: `POST ${fv}`, body: { updates: [{ customField: 'f', issueIds: [102, 103], value: 'removed' }] } }],
    pack, rateModel), ['102', '103', '103']);
    assert.deepEqual(pathParams('/rest/agile/1.0/board/{boardId}/sprint', '/rest/agile/1.0/board/7/sprint?state=active'), { boardId: '7' });
  });

  await test('deliver: a click that fails on a live control is named; one on an absent, hidden or disabled control is the app\'s', async () => {
    // Playwright's click waits for the control to be attached, visible, enabled and to receive the click, then throws.
    const control = ({ count = 1, visible = true, disabled = false, intercepted = false }) => {
      const c = { clicks: 0, count: async () => count, isVisible: async () => visible, isDisabled: async () => disabled,
        click: async () => {
          if (!count || !visible || disabled || intercepted) throw new Error('locator.click: Timeout 30000ms exceeded.\nCall log: …');
          c.clicks += 1;
        } };
      return c;
    };
    for (const [label, spec, want, clicks] of [['absent', { count: 0 }, null, 0], ['hidden', { visible: false }, null, 0],
      ['disabled', { disabled: true }, null, 0], ['delivered', {}, null, 1],
      ['intercepted', { intercepted: true }, 'locator.click: Timeout 30000ms exceeded.', 0]]) {
      const c = control(spec);
      assert.equal(await deliver(c, (l) => l.click()), want, label);
      assert.equal(c.clicks, clicks, `${label}: clicks`);
    }
  });

  await test('viewerIssue: the row\'s issue only while the viewer may browse it', async () => {
    const issues = new Map([['PAY-1', { key: 'PAY-1', hidden: false }], ['PAY-2', { key: 'PAY-2', hidden: true }]]);
    site = { pack: { viewer: 'acct-viewer' }, state: { issueByIdOrKey: (k) => issues.get(k), canBrowse: (who, i) => who === 'acct-viewer' && !i.hidden } };
    try {
      assert.equal(viewerIssue('PAY-1')?.key, 'PAY-1');
      assert.equal(viewerIssue('PAY-1 ↗')?.key, 'PAY-1');
      assert.equal(viewerIssue('PAY-2'), null, 'a leaked hidden row');
      assert.equal(viewerIssue('PAY-3'), null, 'a deleted issue');
      assert.equal(viewerIssue(null), null);
    } finally {
      site = null;
    }
  });

  await test('shownSecret: the new long token only', async () => {
    assert.equal(shownSecret('Rotate CI secret ••••a1b2', 'Rotate CI secret New secret: Zk3p_9QwErTyUiOp1234 (copy it now)'), 'Zk3p_9QwErTyUiOp1234');
    assert.equal(shownSecret('Migrated 10 of 20 v1 rows', 'Migrated 12 of 20 v1 rows'), null);
  });

  await test('replaceDeep and forgedPayload: the replay changes the value and claims the admin', async () => {
    assert.deepEqual(replaceDeep({ settings: { budget: 150000, share: 70, label: '150000' } }, 150000, 777777),
      { settings: { budget: 777777, share: 70, label: '777777' } });
    const f = forgedPayload({ budget: 1 }, 'admin-1');
    assert.equal(f.budget, 1);
    assert.equal(f.accountId, 'admin-1');
    assert.equal(f.context.accountId, 'admin-1');
  });

  await test('configState: ledger entities ignored, settings and secrets compared by value', async () => {
    const a = configState({ kv: [['settings', { value: { budget: 1 } }]], secrets: [['ci', { value: 's1' }]],
      ents: [['scope-ledger\u0000r1', { value: { changeId: '1' } }], ['admin-log\u0000a1', { value: { what: 'x' } }]] });
    const b = configState({ kv: [['settings', { value: { budget: 1 } }]], secrets: [['ci', { value: 's2' }]],
      ents: [['scope-ledger\u0000r2', { value: { changeId: '2' } }], ['admin-log\u0000a1', { value: { what: 'x' } }]] });
    assert.deepEqual(changedKeys(a, b), ['secret:ci']);
  });

  await test('ledgerRow: the §2.4 attributes, absent ones null', async () => {
    assert.deepEqual(ledgerRow({ changeId: 'c', sprintId: '7', at: 1.5, deleted: false, junk: 1 }),
      { sprintId: '7', at: 1.5, changeId: 'c', kind: null, issueId: null, issueKey: null, estimate: null, boardId: null, estimateField: null, deleted: false, deployedEnvs: null });
  });

  await test("takeCalls: a call carries its invocation's lineage; a page's own call carries none", async () => {
    const fake = { log: [{ invocationId: 'i-ev', moduleType: 'consumer', method: 'GET', path: '/rest/api/3/issue/1', status: 200 },
      { invocationId: 'i-run', moduleType: 'consumer', method: 'GET', path: '/rest/api/3/search/jql', status: 200 },
      { method: 'GET', path: '/rest/api/3/myself', status: 200 }],
    invocations: new Map([['i-ev', { originChange: '9201', scheduledRun: null }], ['i-run', { originChange: null, scheduledRun: 1 }]]) };
    const saved = logCursor;
    logCursor = 0;
    const [ev, run, page] = takeCalls(fake);
    logCursor = saved;
    assert.deepEqual([ev.originChange, ev.scheduledRun, run.originChange, run.scheduledRun], ['9201', null, null, 1]);
    assert.ok(!('originChange' in page) && !('scheduledRun' in page));
  });

  // The admin lane against a fake UI Kit host and three fake apps: one that authorizes from req.context, one that
  // trusts the payload's identity claim, one that hands its secret back on every read.
  const ADMIN = 'acct-admin';
  const fakeApp = (kind) => {
    const kv = new Map([['settings', { value: { budget: 200000 } }]]);
    const secrets = new Map();
    const allowed = (ctx, payload) => ctx.accountId === ADMIN || (kind === 'trusting' && payload?.isAdmin === true);
    const resolvers = {
      getSettings: () => ({ budget: kv.get('settings').value.budget, mask: secrets.has('ci') ? `••••${secrets.get('ci').value.slice(-4)}` : null,
        ...(kind === 'leaky' && secrets.has('ci') ? { secret: secrets.get('ci').value } : {}) }),
      saveSettings: (p, ctx) => {
        if (!allowed(ctx, p)) return { error: 'Only Jira administrators can change these settings.' };
        kv.set('settings', { value: { budget: p.budget } });
        return { saved: true };
      },
      rotateSecret: (p, ctx) => {
        if (!allowed(ctx, p)) return { error: 'Only Jira administrators can rotate the secret.' };
        const secret = `S${Math.random().toString(36).slice(2).padEnd(12, 'x')}Q${Date.now().toString(36)}`;
        secrets.set('ci', { value: secret });
        return { secret };
      },
    };
    const fake = {
      siteInfo: { cloudId: 'cloud-1', siteUrl: 'https://site.example' }, log: [],
      kvs: { dump: () => ({ kv: [...kv], secrets: [...secrets], ents: [] }),
        snapshot: () => ({ kvs: Object.fromEntries([...kv].map(([k, r]) => [k, r.value])), secrets: [...secrets.keys()], entities: {} }) },
      invokeResolver: async (_m, fn, payload, ctx) => ({ ok: true, result: resolvers[fn](payload, ctx) }),
      stored: () => secrets.get('ci')?.value ?? null,
    };
    return fake;
  };
  const fakeHost = {
    render: async ({ invoke }) => {
      let settings = await invoke('getSettings', {});
      let shown = null;
      const values = {};
      const labels = [LABELS.budget, LABELS.save, LABELS.rotate, LABELS.migration];
      return {
        text: () => [LABELS.budget, settings.budget, LABELS.rotate, shown ? `New secret ${shown} (shown once)` : (settings.mask ?? 'not set'),
          settings.secret ?? '', LABELS.migration, 'Migrated 3 of 10 v1 rows', LABELS.save].join(' '),
        findByLabel: (l) => (labels.includes(l) ? { label: l } : null),
        setValue: (l, val) => { values[l] = val; },
        click: async (l) => {
          if (l === LABELS.save) { await invoke('saveSettings', { budget: values[LABELS.budget] }); settings = await invoke('getSettings', {}); }
          if (l === LABELS.rotate) shown = (await invoke('rotateSecret', {})).secret ?? null;
        },
        waitIdle: async () => {},
      };
    },
  };
  const runLane = async (kind) => {
    obs.admin = { actions: [], tree_text: '', secret_leaks: [] };
    obs.boot = {};
    const fake = fakeApp(kind);
    emu = fake;
    const d = adminDriver({ emu: fake, pack: { admin: ADMIN, viewer: 'acct-viewer' }, host: fakeHost, adminPage: { key: 'admin-page' }, appDir: null, now: () => 0 });
    await d.lane();
    const secret = d.ciSecret();
    return { d, fake, secret, leaks: secretLeaks(d.state.secret, d.state.answers),
      by: (as, resolver) => obs.admin.actions.filter((x) => x.as === as && x.resolver === resolver) };
  };

  await test('admin lane, an app that authorizes from req.context: no replay changes anything, nothing leaks', async () => {
    const r = await runLane('correct');
    assert.equal(obs.boot['admin-page'].invokes_before_paint, 1);
    assert.deepEqual(obs.ui.surfaces, [], "the UI Kit admin page is graded by boot['admin-page'], never as a Custom UI surface");
    assert.equal(obs.admin.secret_shown, true);
    assert.equal(r.secret, r.fake.stored());
    assert.equal(r.by('admin', 'saveSettings')[0].state_changed, true);
    assert.equal(r.by('admin', 'rotateSecret')[0].state_changed, true);
    assert.equal(r.by('admin', 'rotateSecret')[0].changed_keys.join(), 'secret:ci');
    for (const as of ['nonadmin', 'forged']) for (const fn of ['saveSettings', 'rotateSecret']) {
      assert.equal(r.by(as, fn).length >= 1, true, `${as} ${fn} replayed`);
      assert.equal(r.by(as, fn).every((x) => !x.state_changed), true, `${as} ${fn} changed nothing`);
    }
    assert.equal(r.by('nonadmin', 'saveSettings').some((x) => x.via === 'ui'), true, "the non-admin's own panel save");
    assert.deepEqual(r.by('forged', 'saveSettings')[0].payload.budget, FORGED_BUDGET);
    assert.match(obs.admin.tree_text, /••••/);
    assert.deepEqual(r.leaks, []);
  });

  await test('admin lane, an app that trusts the payload: the forged replays land, the CI secret follows the store', async () => {
    const r = await runLane('trusting');
    assert.equal(r.by('nonadmin', 'saveSettings').every((x) => !x.state_changed), true);
    assert.equal(r.by('forged', 'saveSettings')[0].state_changed, true);
    assert.equal(r.by('forged', 'rotateSecret')[0].state_changed, true);
    assert.equal(obs.admin.rerotated, true);
    assert.equal(r.secret, r.fake.stored());
    assert.equal(r.leaks.includes('resolver rotateSecret response (as forged)'), false, 'the forged rotation showed a secret since replaced');
  });

  await test('admin lane, an app that returns its secret on every read: the leaks are named', async () => {
    const r = await runLane('leaky');
    assert.equal(r.leaks.includes('resolver getSettings response (as admin)'), true, 'the re-rendered panel read');
    assert.equal(r.leaks.includes('resolver getSettings response (as nonadmin)'), true, "the non-admin's read");
    assert.equal(r.leaks.includes('admin page after the one-time display'), true);
    assert.equal(r.leaks.includes('resolver rotateSecret response (as admin)'), false, 'the rotation shows it once, legitimately');
  });

  await test('admin lane, a page the host refuses for the app (not UI Kit): the cause is recorded, no admin lane retries it', async () => {
    const refusing = (code) => ({ render: async () => {
      throw Object.assign(new Error("module 'admin-page' (jira:adminPage) is not UI Kit: it has no `render: native`"), { name: 'UikitHostError', code });
    } });
    const driver = (host) => adminDriver({ emu: fakeApp('correct'), pack: { admin: ADMIN, viewer: 'acct-viewer' }, host, adminPage: { key: 'admin-page' }, appDir: null, now: () => 0 });
    obs.admin = { actions: [], tree_text: '', secret_leaks: [] };
    obs.boot = {};
    obs.migration.panel_by_checkpoint = {};
    const d = driver(refusing('NOT_NATIVE'));
    await d.lane();
    assert.match(obs.admin.unrenderable, /^NOT_NATIVE: module 'admin-page' \(jira:adminPage\) is not UI Kit/);
    assert.equal(d.usable(), false, 'the panel reads and the LLM cases\' settings record the absence');
    await d.panelRead('h1');
    assert.deepEqual(obs.migration.panel_by_checkpoint, {});
    assert.equal('admin-page' in obs.boot, false);
    obs.admin = { actions: [], tree_text: '', secret_leaks: [] };
    await assert.rejects(driver(refusing('HARNESS')).lane(), /not UI Kit/, "the host's own failure stays a harness failure");
    assert.equal(obs.admin.unrenderable, undefined);
  });

  await test('sendCiCases: side effects follow the invocation, even when its writes land after its response', async () => {
    const http = await import('node:http');
    const fake = { invocations: new Map(), log: [] };
    const completed = new Set();
    let seq = 0;
    let pending = null;   // the 'late' case's write, logged only once the NEXT case's request has arrived
    const write = (inv) => fake.log.push({ invocationId: inv, service: 'kvs', method: 'POST', path: '/api/v1/entity/set', status: 200 });
    const server = http.createServer((req, res) => {
      if (pending) { pending(); pending = null; }
      const kind = req.headers['x-case'];
      const inv = `inv-${++seq}`;
      fake.invocations.set(inv, { moduleType: 'webtrigger' });
      if (kind === 'valid') { write(inv); completed.add(inv); res.writeHead(202); return res.end('{}'); }
      if (kind === 'late') { res.writeHead(202); res.end('{}'); pending = () => { write(inv); completed.add(inv); }; return undefined; }
      if (kind === 'writes-then-401') { write(inv); completed.add(inv); res.writeHead(401); return res.end(''); }
      if (kind === 'unrecorded') { fake.invocations.delete(inv); res.writeHead(401); return res.end(''); }
      completed.add(inv);
      res.writeHead(401);
      res.end('');
    });
    await new Promise((ok) => server.listen(0, '127.0.0.1', ok));
    const url = `http://127.0.0.1:${server.address().port}/x/webtrigger/ci`;
    const cases = ['valid', 'late', 'bad-signature', 'writes-then-401', 'unrecorded'].map((k) => ({ case: k, headers: { 'x-case': k }, body: '{}' }));
    const rows = await sendCiCases({ cases, url, emu: fake, completed });
    server.close();
    assert.deepEqual(rows.map((r) => [r.case, r.status, r.side_effects, r.settled]),
      [['valid', 202, 1, true], ['late', 202, 1, true], ['bad-signature', 401, 0, true], ['writes-then-401', 401, 1, true], ['unrecorded', 401, 0, false]]);
  });

  for (const r of results) console.log(`ok - ${r}`);
  console.log(`selftest: ${results.length} passed`);
}

const cleanup = async () => {
  try { if (browser) await browser.close(); } catch {}
  try { if (emu?.stop) await emu.stop(); } catch {}
  try { if (site?.stop) await site.stop(); } catch {}
};

if (SELFTEST) {
  try {
    await selftest();
    process.exit(0);
  } catch (e) {
    console.log(`not ok - ${e?.stack || e}`);
    process.exit(1);
  }
}

for (const sig of ['SIGINT', 'SIGTERM']) process.on(sig, async () => { await cleanup(); process.exit(130); });

try {
  await main();
} catch (e) {
  obs.sectionErrors.harness = String(e?.stack || e).split('\n').slice(0, 3).join(' | ').slice(0, 400);
  for (const s of ['backfill', 'live', 'heal', 'rerun']) if (!obs.phases[s]) obs.sectionErrors[s] = obs.sectionErrors[s] || 'harness did not start';
  for (const s of ['build', 'rovo', 'ui', 'lint', 'migration', 'rate', 'invocations', 'world', 'webtrigger', 'admin', 'field', 'llm_v2', 'boot']) {
    if (!obs.sectionErrors[s] && !(s === 'lint' && obs.lint.runs.length === 2)) obs.sectionErrors[s] = obs.sectionErrors[s] || 'harness did not start';
  }
  log('harness failure:', e?.message || e);
} finally {
  save();
  await cleanup();
}
process.exit(0);
