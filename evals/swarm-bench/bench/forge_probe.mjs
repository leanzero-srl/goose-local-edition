// forge-1.0 probe: drive WP1's emulator (interface I2) through the scoring sequence of forge/DESIGN.md §8.7 and
// write the I5 evidence `forge-observations.json` that bench/score_forge.py grades. It probes ONLY the hooks
// FORGE-CONTRACT.md names (I3) and calls nothing of the emulator but I2.
//
//   node forge_probe.mjs --app <clone> --kit <kit dir> --seed <16 hex> --out obs.json --shots <dir>
//                        --runtime wrapper|shim --repo <evals/swarm-bench>
//   node forge_probe.mjs --preflight
//
// Harness failures (the site or emulator cannot start, Chromium cannot launch, an I2 call this probe needs is
// absent) land in `sectionErrors[section]` — the scorer makes those rows UNAVAILABLE, never app zeros. App
// failures (a missing hook, a thrown resolver, a console error) are evidence and are recorded as such.
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
const log = (...a) => console.error('[forge-probe]', ...a);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

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

const appDir = resolve(opt('app'));
const kitDir = resolve(opt('kit'));
const repo = resolve(opt('repo'));
const seed = opt('seed');
const outPath = resolve(opt('out'));
const shotsDir = resolve(opt('shots'));
const runtime = opt('runtime', 'wrapper');
mkdirSync(shotsDir, { recursive: true });

const obs = {
  schema: 'forge-observations/1', runtime, seed, manifest: null, manifestError: null, kit: {}, lint: { runs: [] },
  build: { functions: [] }, phases: {}, rovo: { calls: [] }, comments: [], ui: { surfaces: [], calls: [] },
  harnessMissing: [], sectionErrors: {}, shots: [], i2Gaps: [],
};
const save = () => writeFileSync(outPath, JSON.stringify(obs, null, 1));
// An I2 surface this probe needs and the emulator lacks: a harness gap, so the sections it feeds are unavailable.
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
  'dashboards:widget': 'resolver', 'jira:sprintAction': 'resolver' };

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
    scopes: alt ? { chosen: alt.scopes, state: alt.state } : (e.scopes ?? undefined),
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

let logCursor = 0;
const takeCalls = (emu) => {
  const all = emu.log || [];
  const fresh = all.slice(logCursor).map(normCall);
  logCursor = all.length;
  return fresh;
};

// ── main sequence ─────────────────────────────────────────────────────────────────────────

let site = null;
let emu = null;
let browser = null;

async function section(name, fn) {
  try {
    await fn();
  } catch (e) {
    obs.sectionErrors[name] = String(e?.stack || e).split('\n').slice(0, 3).join(' | ').slice(0, 400);
    log(`section ${name} failed:`, e?.message || e);
  }
  save();
}

async function main() {
  obs.kit = { pins: kitPins(), dir: kitDir };
  const lintRuns = [];
  await section('lint', async () => {
    lintRuns.push(runLint(), runLint());
    obs.lint.runs = lintRuns;
  });

  const { createSite } = require(join(repo, 'forge', 'site', 'site.cjs'));
  const { createEmulator } = require(join(kitDir, 'lib', 'emulator.cjs'));
  site = await createSite({ seed, port: 0, trace: null });
  const pack = site.pack;
  // A refusal here (no sandbox, wrapper sha mismatch) is the harness's: main()'s catch marks every section.
  emu = await createEmulator({ appDir, kitDir, site, runtime });
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
  takeCalls(emu);

  const runSchedules = async (phase) => {
    const ph = obs.phases[phase] = { calls: [], invocations: [], deliveries: [] };
    for (const m of modules('scheduledTrigger')) {
      const r = await emu.runScheduled(m.key);
      ph.invocations.push(normInvocation(r?.invocation ?? r, 'scheduled', m.key));
      for (const d of r?.deliveries || []) {
        ph.deliveries.push(normDelivery(d));
        ph.invocations.push(normInvocation({ ...d, ok: d.ok ?? !d.error }, 'consumer', d.moduleKey));
      }
    }
    ph.calls = takeCalls(emu);
    ph.kvsAfter = snapshot(phase);
  };

  await section('backfill', () => runSchedules('backfill'));

  await section('live', async () => {
    const ph = obs.phases.live = { calls: [], invocations: [], deliveries: [], events: [] };
    const record = async (r, fallback) => {
      const trig = (r?.invocations || (Array.isArray(r) ? r : [])).map((x) => normInvocation(x, 'trigger', x?.moduleKey));
      ph.invocations.push(...trig);
      ph.events.push({ changelogId: r?.changelogId ?? fallback.changelogId, slot: r?.slot ?? fallback.slot,
        duplicate: Boolean(r?.duplicate ?? fallback.duplicate), triggerInvocations: trig.map((x) => x.inv).filter(Boolean) });
      for (const d of (await emu.drainQueues()) || []) {
        ph.deliveries.push(normDelivery(d));
        ph.invocations.push(normInvocation({ ...d, ok: d.ok ?? !d.error }, 'consumer', d.moduleKey));
      }
    };
    if (typeof emu.deliverNext === 'function') {
      // The site owns the delivery plan (slot order, duplicate slots, drops): exactly what production would do.
      for (let r = await emu.deliverNext(); r; r = await emu.deliverNext()) await record(r, {});
      if (typeof site.flushLive === 'function') site.flushLive();
      else gap('site.flushLive (dropped changes after the last delivery)', 'live', 'heal');
    } else {
      gap('emu.deliverNext (the site-owned delivery plan)', 'live', 'heal');
    }
    ph.calls = takeCalls(emu);
    ph.kvsAfter = snapshot('live');
  });

  await section('heal', () => runSchedules('heal'));
  await section('rerun', () => runSchedules('rerun'));

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
    await probeUi(pack);
  });
  obs.comments = commentAttempts(pack);
  obs.harnessMissing = [...new Set([...(emu.harnessMissing || []).map((x) => (typeof x === 'string' ? x : JSON.stringify(x)))])];
}

function commentAttempts(pack) {
  const all = [...Object.values(obs.phases).flatMap((p) => p.calls || []), ...obs.ui.calls,
    ...obs.rovo.calls.flatMap((c) => c.calls || [])];
  return all.filter((c) => c.service === 'jira' && c.method === 'POST' && /\/rest\/api\/[23]\/issue\/[^/]+\/comment$/.test(String(c.path).split('?')[0]))
    .map((c) => ({ t: c.t, issueKey: String(c.path).split('/issue/')[1].split('/')[0], provider: c.provider,
      accountId: c.provider === 'user' ? pack.viewer : pack.appAccountId, status: c.status,
      body: c.body && typeof c.body === 'object' ? c.body.body : c.body, fault: c.fault }));
}

// ── UI (§8.7 step 8) ──────────────────────────────────────────────────────────────────────

const PAGE_HELPERS = () => {
  window.__forgeProbe = { csp: [] };
  document.addEventListener('securitypolicyviolation', (e) =>
    window.__forgeProbe.csp.push(`${e.violatedDirective} ${e.blockedURI}`));
};

async function openSurface(spec) {
  const page = await browser.newPage({ viewport: { width: spec.width, height: spec.height } });
  const ev = { consoleErrors: [], pageErrors: [], failedRequests: [], popups: 0 };
  page.on('console', (m) => { if (m.type() === 'error') ev.consoleErrors.push(m.text().slice(0, 300)); });
  page.on('pageerror', (e) => ev.pageErrors.push(String(e).slice(0, 300)));
  page.on('requestfailed', (r) => ev.failedRequests.push(r.url()));
  page.on('response', (r) => { if (r.status() >= 400 && r.request().resourceType() !== 'fetch') ev.failedRequests.push(`${r.status()} ${r.url()}`); });
  page.on('popup', () => { ev.popups += 1; });
  await page.addInitScript(PAGE_HELPERS);
  const bridgeStart = (emu.bridgeLog || []).length;
  const cspStart = typeof emu.cspReports === 'function' ? emu.cspReports().length : 0;
  const t0 = Date.now();
  const opened = await emu.openSurface(page, { moduleKey: spec.moduleKey, entry: spec.entry, theme: spec.theme,
    layout: { width: spec.width, height: spec.height }, asUser: spec.asUser, extension: spec.extension });
  return { page, ev, bridgeStart, cspStart, surfaceId: opened?.surfaceId ?? null, t0, startUrl: page.url() };
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
    const bgOf = (el) => {
      for (let e = el; e; e = e.parentElement) {
        const c = getComputedStyle(e).backgroundColor;
        const v = rgb(c);
        if (v.length >= 3 && (v.length < 4 || v[3] > 0)) return v.slice(0, 3);
      }
      return [255, 255, 255];
    };
    // Every visible element that owns a text node: links may use --ds-link*, disabled controls are exempt (§7).
    const styles = [];
    for (const el of document.body ? document.body.querySelectorAll('*') : []) {
      const own = [...el.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim());
      const r = el.getBoundingClientRect();
      if (!own || r.width === 0 || r.height === 0 || getComputedStyle(el).visibility === 'hidden') continue;
      const role = el.closest('[disabled], [aria-disabled="true"]') ? 'disabled'
        : el.closest('a, [role="link"]') ? 'link' : el.closest('[data-metric]') ? 'metric' : 'text';
      styles.push({ role, color: rgb(getComputedStyle(el).color).slice(0, 3), background: bgOf(el),
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
  const surface = { ...meta, ...dom, tokens, shot, enableTheming: ops.some((b) => opName(b) === 'enableTheming'),
    bridgeOps: ops.map((b) => ({ op: opName(b) })), consoleErrors: s.ev.consoleErrors, pageErrors: s.ev.pageErrors,
    cspViolations: [...new Set([...dom.csp, ...cspReportsOf(s), ...s.ev.consoleErrors.filter((m) => /Content Security Policy/i.test(m))])],
    failedRequests: s.ev.failedRequests, nominal: meta.nominal !== false,
    invokesBeforePaint: paintOps !== null ? before.filter((b) => ['invoke', 'fetchProduct'].includes(opName(b))).length : null };
  delete surface.csp;
  obs.ui.surfaces.push(surface);
  for (const b of ops.filter((x) => opName(x) === 'invoke')) {
    obs.ui.invokeResponses = obs.ui.invokeResponses || [];
    obs.ui.invokeResponses.push({ surface: meta.id, functionKey: b.payload?.functionKey ?? b.functionKey ?? null,
      response: b.result ?? b.response ?? null, threw: Boolean(b.error), undefined: b.ok !== false && b.result === undefined && 'result' in b,
      error: b.error ? String(b.error) : null });
  }
  return surface;
}

const widgetMetrics = (page) => page.evaluate(() => [...document.querySelectorAll('[data-testid="sprint"][data-sprint-id]')].map((el) => {
  const m = (n) => { const x = el.querySelector(`[data-metric="${n}"]`); return x ? x.textContent.trim() : null; };
  const r = el.getBoundingClientRect();
  return { id: el.getAttribute('data-sprint-id'), metrics: { committed: m('committed'), added: m('added'), removed: m('removed'), creep: m('creep') },
    visible: r.width > 0 && r.height > 0 && r.left >= -1 && r.right <= window.innerWidth + 1 };
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
        for (const width of [380, 1180]) {
          const v = await openSurface({ moduleKey: widget.key, entry: 'view', theme, width, height: 480, asUser: viewer, extension: ctx('w1', stored) });
          await finishSurface(v, { id: `widget-view-${board}-${theme}-${width}x480`, kind: 'widget-view', theme, width }, '[data-testid="sprint"], [data-testid="needs-config"]');
          const sprints = await widgetMetrics(v.page);
          const surf = obs.ui.surfaces[obs.ui.surfaces.length - 1];
          obs.ui.widget.views.push({ board, theme, width, sprints: sprints.map(({ id, metrics }) => ({ id, metrics })),
            chart: await chartOf(v.page), overflow: surf.overflow, sprintsVisible: sprints.length > 0 && sprints.every((x) => x.visible) });
          if (theme === 'light' && width === 380) pick.viewSprints = sprints.map((x) => x.id);
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

function flagCounts(ops) {
  const flags = ops.filter((b) => opName(b) === 'showFlag').map((b) => String(b.payload?.type ?? b.options?.type ?? b.type ?? ''));
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
  if (!out.rows.length) return out;
  out.sortAt = [];
  for (let i = 0; i < 2; i++) {
    await page.locator('th[data-col="at"]').first().click().catch(() => {});
    await sleep(200);
    out.sortAt.push({ rows: (await tableRows(page)).map((r) => r.changeId), ariaSort: await ariaSort(page) });
  }
  await page.locator('th[data-col="points"]').first().click().catch(() => {});
  await sleep(200);
  out.sortPoints = { rows: (await tableRows(page)).map((r) => r.changeId), ariaSort: await ariaSort(page) };
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
  const target = rows.find((r) => !forbidden.has(r.cells.issue));
  if (target) {
    const row = post.page.locator(`tr[data-change-id="${target.changeId}"]`);
    await row.locator('td[data-col="kind"]').click().catch(() => row.click().catch(() => {}));
    await sleep(200);
    out.select = { changeId: target.changeId, ariaSelected: (await tableRows(post.page)).find((r) => r.changeId === target.changeId)?.selected ?? false };
    // Gap #24: the double click is two clicks on the SAME element handle (a moving label cannot dodge the second);
    // force skips actionability waits but a disabled button still swallows the click, as a browser does.
    const twice = async (l) => { const h = await l.elementHandle(); await h.click(); await h.click({ force: true }); };
    for (const [name, act] of [['post', (l) => l.click()], ['doubleClick', twice]]) {
      const c0 = commentsNow();
      const o0 = bridgeOps(post).length;
      await act(post.page.locator('[data-testid="post-summary"]').first()).catch(() => {});
      await settle(post);
      out[name] = { commentsAdded: commentsNow() - c0, ...flagCounts(bridgeOps(post).slice(o0)) };
    }
    const forb = rows.find((r) => forbidden.has(r.cells.issue));
    if (forb) {
      const fr = post.page.locator(`tr[data-change-id="${forb.changeId}"]`);
      await fr.locator('td[data-col="kind"]').click().catch(() => fr.click().catch(() => {}));
      const c0 = commentsNow();
      const o0 = bridgeOps(post).length;
      await post.page.locator('[data-testid="post-summary"]').first().click().catch(() => {});
      await settle(post);
      const orderBefore = (await tableRows(post.page)).map((r) => r.changeId);
      await post.page.locator('th[data-col="at"]').first().click().catch(() => {});
      await sleep(200);
      const orderAfter = (await tableRows(post.page)).map((r) => r.changeId);
      out.forbidden = { issueKey: forb.cells.issue, commentsAdded: commentsNow() - c0, ...flagCounts(bridgeOps(post).slice(o0)),
        sortWorksAfter: orderAfter.length === orderBefore.length && orderAfter.join() !== orderBefore.join() };
    }
  }
  const o1 = bridgeOps(post).length;
  await post.page.locator('[data-testid="close"]').first().click().catch(() => {});
  await sleep(200);
  out.close = { closeCalled: bridgeOps(post).slice(o1).some((b) => opName(b) === 'close') };
  await post.page.close();
  return out;
}

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

const cleanup = async () => {
  try { if (browser) await browser.close(); } catch {}
  try { if (emu?.stop) await emu.stop(); } catch {}
  try { if (site?.stop) await site.stop(); } catch {}
};
for (const sig of ['SIGINT', 'SIGTERM']) process.on(sig, async () => { await cleanup(); process.exit(130); });

try {
  await main();
} catch (e) {
  obs.sectionErrors.harness = String(e?.stack || e).split('\n').slice(0, 3).join(' | ').slice(0, 400);
  for (const s of ['build', 'backfill', 'live', 'heal', 'rerun', 'rovo', 'ui']) if (!obs.phases[s] && !['build', 'rovo', 'ui'].includes(s)) obs.sectionErrors[s] = obs.sectionErrors[s] || 'harness did not start';
  for (const s of ['build', 'rovo', 'ui', 'lint']) if (!obs.sectionErrors[s] && !(s === 'lint' && obs.lint.runs.length === 2)) obs.sectionErrors[s] = obs.sectionErrors[s] || 'harness did not start';
  log('harness failure:', e?.message || e);
} finally {
  save();
  await cleanup();
}
process.exit(0);
