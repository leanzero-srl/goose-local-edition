// The golden's Custom UI proof on WP3's own bed: every surface in Chromium behind the default Forge
// Custom UI CSP, with @forge/bridge served by a page-side globalThis.__bridge (the spike's seam) whose
// invoke runs the real resolver in-process against the mock site. Light and dark, contrast, console,
// the dashboards edit API (updateConfig / onProductSave / host Save), the sprint-action ledger.
// Usage: node dev/test-ui.cjs [outDir]
'use strict';

const path = require('path');
const fs = require('fs');
const http = require('http');
const os = require('os');
const { chromium } = require(process.env.PLAYWRIGHT_PATH ?? path.join(os.homedir(), '.nvm/versions/node/v22.22.0/lib/node_modules/playwright'));
const { createSite, oracle, fmtCreep } = require('./site.cjs');
const { createPlatform, APP } = require('./runtime.cjs');

const CSP = "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data: blob:; font-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'self'";

// Approximate Atlassian token values (light / dark) for this bed's contrast checks.
const TOKENS = {
  '--ds-surface': ['#FFFFFF', '#1D2125'],
  '--ds-surface-sunken': ['#F7F8F9', '#161A1D'],
  '--ds-surface-raised': ['#FFFFFF', '#22272B'],
  '--ds-surface-raised-hovered': ['#F1F2F4', '#282E33'],
  '--ds-text': ['#172B4D', '#B6C2CF'],
  '--ds-text-subtle': ['#44546F', '#9FADBC'],
  '--ds-text-inverse': ['#FFFFFF', '#1D2125'],
  '--ds-text-selected': ['#0C66E4', '#579DFF'],
  '--ds-text-warning': ['#A54800', '#F5CD47'],
  '--ds-text-warning-inverse': ['#172B4D', '#1D2125'],
  '--ds-link': ['#0C66E4', '#579DFF'],
  '--ds-link-pressed': ['#0055CC', '#85B8FF'],
  '--ds-background-neutral': ['#091E420F', '#A1BDD914'],
  '--ds-background-neutral-hovered': ['#091E4224', '#A6C5E229'],
  '--ds-background-neutral-subtle-hovered': ['#091E420F', '#A1BDD914'],
  '--ds-background-neutral-bold': ['#44546F', '#9FADBC'],
  '--ds-background-selected': ['#E9F2FF', '#1C2B41'],
  '--ds-background-brand-bold': ['#0C66E4', '#579DFF'],
  '--ds-background-brand-bold-hovered': ['#0055CC', '#85B8FF'],
  '--ds-background-warning-bold': ['#F5CD47', '#F5CD47'],
  '--ds-background-danger-bold': ['#C9372C', '#F87168'],
  '--ds-border': ['#091E4224', '#A6C5E229'],
  '--ds-border-bold': ['#758195', '#738496'],
  '--ds-border-focused': ['#388BFF', '#85B8FF'],
  '--ds-border-selected': ['#0C66E4', '#579DFF'],
};
const tokensCss = ['light', 'dark']
  .map((mode, i) => `html[data-color-mode="${mode}"] {\n${Object.entries(TOKENS).map(([k, v]) => `  ${k}: ${v[i]};`).join('\n')}\n}`)
  .join('\n');

let failures = 0;
const ok = (cond, msg) => {
  if (!cond) failures += 1;
  console.log(`${cond ? 'PASS' : 'FAIL'}  ${msg}`);
};
const eq = (a, b, msg) => ok(JSON.stringify(a) === JSON.stringify(b), `${msg}${JSON.stringify(a) === JSON.stringify(b) ? '' : `\n      got      ${JSON.stringify(a)}\n      expected ${JSON.stringify(b)}`}`);
const fmtPoints = (n) => String(Math.round(n * 1e6) / 1e6);

// Each resource is served under its own path prefix, as the Forge CDN does, so an absolute asset path
// (/index.js) misses the resource and 404s.
function serve(dir, prefix) {
  const types = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css' };
  const srv = http.createServer((req, res) => {
    const u = new URL(req.url, 'http://x').pathname;
    if (u === '/__tokens.css') {
      res.writeHead(200, { 'content-type': 'text/css' });
      return res.end(tokensCss);
    }
    if (!u.startsWith(`/${prefix}/`)) {
      res.writeHead(404);
      return res.end();
    }
    const rel = u.slice(prefix.length + 1);
    const p = path.join(dir, decodeURIComponent(rel === '/' ? '/index.html' : rel));
    if (!p.startsWith(dir) || !fs.existsSync(p) || fs.statSync(p).isDirectory()) {
      res.writeHead(404);
      return res.end();
    }
    res.writeHead(200, { 'content-type': types[path.extname(p)] ?? 'application/octet-stream', 'content-security-policy': CSP });
    fs.createReadStream(p).pipe(res);
  });
  return new Promise((ok2) => srv.listen(0, '127.0.0.1', () => ok2({ url: `http://127.0.0.1:${srv.address().port}/${prefix}/`, srv })));
}

// Page-side bridge: the host owns the widget edit API (functions cannot cross exposeFunction), the
// rest goes to Node.
function pageBridge() {
  let productSave = null;
  let lastConfig;
  window.__forgeHost = {
    save: async () => {
      const result = productSave ? await productSave(lastConfig ?? {}) : lastConfig;
      return window.__forgeEmulatorCall('hostSaved', { result: result === undefined ? null : result });
    },
  };
  globalThis.__bridge = {
    callBridge: (op, payload) => {
      if (op === 'getWidgetEditApi')
        return Promise.resolve({
          onProductSave: (fn) => {
            productSave = fn;
          },
          onSave: () => {},
          onSaveError: () => {},
          updateConfig: (c) => {
            lastConfig = c;
            window.__forgeEmulatorCall('updateConfig', c);
          },
        });
      if (op === 'on') return Promise.resolve({ unsubscribe: () => {} });
      if (op === 'subscribeRealtimeChannel') {
        window.__rt = window.__rt ?? [];
        const entry = { channel: payload.channelName, onEvent: payload.onEvent, live: true };
        window.__rt.push(entry);
        window.__forgeEmulatorCall('rtSubscribed', { channel: payload.channelName, isGlobal: Boolean(payload.isGlobal), token: payload.options?.token ?? null });
        return Promise.resolve({ unsubscribe: async () => { entry.live = false; } });
      }
      if (op === 'enableTheming') {
        const link = document.createElement('link');
        link.rel = 'stylesheet';
        link.href = '/__tokens.css';
        document.head.appendChild(link);
        document.documentElement.setAttribute('data-color-mode', window.__colorMode);
        return new Promise((r) => (link.onload = () => r()));
      }
      return window.__forgeEmulatorCall(op, payload);
    },
  };
}

async function main() {
  const outDir = process.argv[2] ?? path.join(os.tmpdir(), 'golden-forge-ui');
  fs.mkdirSync(outDir, { recursive: true });
  const site = createSite({ seed: 11 });
  const platform = createPlatform({ site });
  await platform.build(path.join(outDir, 'bundle'));
  await platform.invoke('reconcile', {}, { moduleKey: 'scope-reconcile' });
  let truth = oracle(site);
  const { alice, bob } = site.users;
  const browser = await chromium.launch();
  const servers = {};
  for (const r of platform.manifest.resources) servers[r.key] = await serve(path.join(APP, r.path), `resource-${r.key}`);
  const livePages = new Set();
  platform.realtime.subscribers.push((ev) => {
    for (const pg of livePages) pg.evaluate((e) => (window.__rt ?? []).filter((x) => x.live && x.channel === e.channel).forEach((x) => x.onEvent(e.payload)), ev).catch(() => {});
  });

  async function open({ resource, moduleType, moduleKey, extension, aaid = alice.accountId, theme = 'light', width = 800, height = 700, shot }) {
    const page = await browser.newPage({ viewport: { width, height } });
    const log = { ops: [], flags: [], navigations: [], closes: 0, updates: [], saved: [], console: [], pending: 0, lastSettled: Date.now(), subscriptions: [], invokes: [] };
    page.on('console', (m) => (m.type() === 'error' || m.type() === 'warning') && log.console.push(`${m.type()}: ${m.text()}`));
    page.on('pageerror', (e) => log.console.push(`pageerror: ${e}`));
    const context = { accountId: aaid, cloudId: 'golden', siteUrl: 'https://golden.atlassian.net', localId: `${moduleKey}-1`, moduleKey, environmentType: 'DEVELOPMENT', locale: 'en-US', timezone: 'Europe/Bucharest', theme: { colorMode: theme }, extension };
    await page.exposeFunction('__forgeEmulatorCall', async (op, payload) => {
      log.ops.push(op);
      switch (op) {
        case 'getContext':
          return context;
        case 'invoke':
          log.invokes.push({ key: payload.functionKey, at: Date.now() });
          log.pending += 1;
          try {
            return await platform.resolver(moduleType, moduleKey, payload.functionKey, payload.payload, { aaid, extension });
          } finally {
            log.pending -= 1;
            log.lastSettled = Date.now();
          }
        case 'showFlag':
          log.flags.push(payload);
          return undefined;
        case 'navigate':
          log.navigations.push(payload);
          return undefined;
        case 'close':
          log.closes += 1;
          return undefined;
        case 'updateConfig':
          log.updates.push(payload);
          return undefined;
        case 'hostSaved':
          log.saved.push(payload.result);
          return undefined;
        case 'rtSubscribed':
          log.subscriptions.push(payload);
          return undefined;
        case 'emitReadyEvent':
        case 'initFeatureFlags':
        case 'emitFrontendCustomMetric':
          return undefined;
        default:
          throw new Error(`bridge op ${op} not modelled`);
      }
    });
    await page.addInitScript(`window.__colorMode = ${JSON.stringify(theme)};`);
    await page.addInitScript(pageBridge);
    await page.goto(servers[resource].url);
    await page.waitForFunction(() => document.documentElement.getAttribute('data-color-mode'));
    return { page, log, shot: async (name) => page.screenshot({ path: path.join(outDir, `${name ?? shot}.png`), fullPage: true }) };
  }

  // Inline <style>/style attributes, horizontal overflow and text contrast, measured in the page.
  async function audit(page, label) {
    const r = await page.evaluate(() => {
      const parse = (c) => {
        const m = c.match(/rgba?\(([^)]+)\)/);
        if (!m) return [0, 0, 0, 0];
        const p = m[1].split(/[ ,/]+/).filter(Boolean).map(Number);
        return [p[0], p[1], p[2], p[3] ?? 1];
      };
      const over = (top, bottom) => {
        const a = top[3];
        return [0, 1, 2].map((i) => top[i] * a + bottom[i] * (1 - a)).concat(1);
      };
      const bgOf = (el) => {
        const stack = [];
        for (let e = el; e; e = e.parentElement) stack.push(parse(getComputedStyle(e).backgroundColor));
        let c = [255, 255, 255, 1];
        for (const layer of stack.reverse()) c = over(layer, c);
        return c;
      };
      const lum = ([r, g, b]) => {
        const f = (v) => {
          const s = v / 255;
          return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
        };
        return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
      };
      const ratio = (a, b) => {
        const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p);
        return (x + 0.05) / (y + 0.05);
      };
      const low = [];
      for (const el of document.querySelectorAll('body *')) {
        const own = [...el.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim());
        if (!own || el.closest('.visually-hidden') || el.closest('svg')) continue;
        const cs = getComputedStyle(el);
        if (cs.visibility === 'hidden' || cs.display === 'none') continue;
        const fg = over(parse(cs.color), bgOf(el));
        const rr = ratio(fg, bgOf(el));
        if (rr < 4.5) low.push(`${el.tagName.toLowerCase()}.${el.className}: ${rr.toFixed(2)} "${el.textContent.trim().slice(0, 30)}"`);
      }
      const bodyBg = getComputedStyle(document.body).backgroundColor;
      return {
        low,
        styleElements: document.querySelectorAll('style').length,
        styleAttrs: [...document.querySelectorAll('[style]')].map((e) => e.tagName),
        inlineScripts: [...document.querySelectorAll('script:not([src])')].length,
        overflowX: document.scrollingElement.scrollWidth - document.scrollingElement.clientWidth,
        clipped: [...document.querySelectorAll('body *')]
          .filter((e) => !e.closest('.table-wrap') && !e.closest('.visually-hidden'))
          .filter((e) => e.getBoundingClientRect().right > window.innerWidth + 0.5 || e.scrollWidth > e.clientWidth + 1 && getComputedStyle(e).overflowX !== 'visible')
          .map((e) => `${e.tagName.toLowerCase()}.${e.className}`),
        bodyBg,
      };
    });
    ok(r.low.length === 0, `${label}: text contrast >= 4.5 everywhere${r.low.length ? `\n      ${r.low.slice(0, 6).join('\n      ')}` : ''}`);
    ok(r.styleElements === 0 && r.styleAttrs.length === 0 && r.inlineScripts === 0, `${label}: no <style>, style attributes or inline scripts (${r.styleElements}/${r.styleAttrs.join(',')}/${r.inlineScripts})`);
    ok(r.overflowX <= 0, `${label}: no horizontal scroll (${r.overflowX})`);
    ok(r.clipped.length === 0, `${label}: nothing clipped or past the right edge${r.clipped.length ? ` (${r.clipped.slice(0, 5).join(', ')})` : ''}`);
    ok(r.bodyBg !== 'rgba(0, 0, 0, 0)', `${label}: paints its own background (${r.bodyBg})`);
  }
  const clean = (log, label) => ok(log.console.length === 0, `${label}: clean console${log.console.length ? `\n      ${log.console.join('\n      ')}` : ''}`);

  // ---------- widget edit ----------
  const scrum = site.boards.filter((b) => b.type === 'scrum').map((b) => String(b.id));
  const widgetExt = (config, entryPoint) => ({ type: 'dashboards:widget', layout: { width: 760, height: 400 }, context: { dashboardId: 'd1', widgetId: 'w1' }, placement: entryPoint ? 'WIDGET_EDIT' : 'DASHBOARD_VIEW', filters: null, ...(entryPoint ? { entryPoint } : {}), config });
  for (const theme of ['light', 'dark']) {
    const e1 = await open({ resource: 'widget-edit', moduleType: 'dashboards:widget', moduleKey: 'scope-widget', extension: widgetExt({}, 'edit'), theme, shot: `edit-${theme}` });
    await e1.page.locator('[data-testid="board-option"]').first().waitFor();
    const ids = await e1.page.$$eval('[data-testid="board-option"]', (els) => els.map((e) => e.getAttribute('data-board-id')));
    eq(ids, scrum, `edit ${theme}: one option per scrum board`);
    await e1.page.click('[data-testid="board-option"][data-board-id="2"]');
    eq(await e1.page.$$eval('[data-testid="board-option"]', (els) => els.map((e) => e.getAttribute('aria-pressed'))), scrum.map((id) => String(id === '2')), `edit ${theme}: the chosen board carries aria-pressed=true`);
    eq(e1.log.updates.at(-1), { boardId: '2' }, `edit ${theme}: the choice reaches the dashboard through updateConfig`);
    await e1.page.evaluate(() => window.__forgeHost.save());
    eq(e1.log.saved.at(-1), { boardId: '2' }, `edit ${theme}: the dashboard Save stores what onProductSave returns`);
    await e1.shot();
    await audit(e1.page, `edit ${theme}`);
    clean(e1.log, `edit ${theme}`);
    await e1.page.close();
  }
  const e0 = await open({ resource: 'widget-edit', moduleType: 'dashboards:widget', moduleKey: 'scope-widget', extension: widgetExt({}, 'edit') });
  await e0.page.locator('[data-testid="board-option"]').first().waitFor();
  await e0.page.evaluate(() => window.__forgeHost.save());
  eq(e0.log.saved.at(-1), null, 'edit: Save with nothing chosen stores nothing (null)');
  await e0.page.close();
  const e2 = await open({ resource: 'widget-edit', moduleType: 'dashboards:widget', moduleKey: 'scope-widget', extension: widgetExt({ boardId: '2' }, 'edit') });
  await e2.page.locator('[data-testid="board-option"]').first().waitFor();
  eq(await e2.page.getAttribute('[data-testid="board-option"][data-board-id="2"]', 'aria-pressed'), 'true', 'edit: reopening shows the stored board selected');
  await e2.page.close();

  // ---------- widget view ----------
  const nc = await open({ resource: 'widget', moduleType: 'dashboards:widget', moduleKey: 'scope-widget', extension: widgetExt({}), shot: 'widget-needs-config' });
  await nc.page.locator('[data-testid="needs-config"]').waitFor();
  eq(await nc.page.$$eval('[data-testid="scope-widget"] [data-testid]', (els) => els.map((e) => e.getAttribute('data-testid'))), ['needs-config'], 'widget: no stored board -> needs-config and nothing else');
  await nc.shot();
  await nc.page.close();
  for (const [board, theme, width] of [['1', 'light', 380], ['1', 'dark', 1180], ['2', 'dark', 380], ['4', 'light', 760]]) {
    const label = `widget board ${board} ${theme} ${width}px`;
    const w = await open({ resource: 'widget', moduleType: 'dashboards:widget', moduleKey: 'scope-widget', extension: widgetExt({ boardId: board }), theme, width, shot: `widget-b${board}-${theme}-${width}` });
    await w.page.locator('[data-testid="chart"]').waitFor();
    const want = site.sprints.filter((s) => s.board === Number(board) && s.state === 'active').sort((a, b) => a.start - b.start);
    const got = await w.page.$$eval('[data-testid="sprint"]', (els) => els.map((e) => ({ id: e.getAttribute('data-sprint-id'), ...Object.fromEntries([...e.querySelectorAll('[data-metric]')].map((m) => [m.getAttribute('data-metric'), m.textContent])) })));
    eq(got, want.map((s) => ({ id: String(s.id), committed: fmtPoints(truth[s.id].committed), added: fmtPoints(truth[s.id].added), removed: fmtPoints(truth[s.id].removed), creep: fmtCreep(truth[s.id].creep) })), `${label}: sprints by startDate with the §1 numbers`);
    const bars = await w.page.$$eval('[data-testid="chart"] rect[data-series]', (els) => els.map((e) => ({ id: e.getAttribute('data-sprint-id'), series: e.getAttribute('data-series'), h: e.getBoundingClientRect().height })));
    ok(bars.length === want.length * 3, `${label}: one rect per sprint and series (${bars.length})`);
    const values = bars.map((b) => truth[b.id][b.series]);
    const k = Math.max(...bars.map((b) => b.h)) / Math.max(...values);
    ok(bars.every((b, i) => Math.abs(b.h - values[i] * k) <= 1), `${label}: bar heights proportional on one scale from 0 (within 1 px)`);
    await w.shot();
    await audit(w.page, label);
    clean(w.log, label);
    await w.page.close();
  }

  // ---------- live widget: ledger rows written in the backend reach an open widget without a reload ----------
  {
    const w = await open({ resource: 'widget', moduleType: 'dashboards:widget', moduleKey: 'scope-widget', extension: widgetExt({ boardId: '1' }), theme: 'light', width: 380, shot: 'widget-live' });
    livePages.add(w.page);
    await w.page.locator('[data-testid="chart"]').waitFor();
    await w.page.waitForTimeout(300);
    const sub0 = w.log.subscriptions[0];
    ok(w.log.subscriptions.length === 1 && sub0.isGlobal, `widget subscribes once to a global realtime channel (${JSON.stringify(sub0)})`);
    eq(w.log.invokes.map((i) => i.key), ['widget'], 'the widget renders and subscribes in one resolver round trip');
    const before = await w.page.textContent('[data-sprint-id="11"] [data-metric="added"]');
    const navs = await w.page.evaluate(() => performance.getEntriesByType('navigation').length);
    const pick = site.issues.find((i) => i.project === 'OPS' && !i.sprints.some((x) => site.sprintById.get(x).state === 'active') && (i.fields[i.estField] ?? 0) > 0);
    const ev = site.update(pick.key, { sprints: [...pick.sprints, 11] });
    await platform.invoke('on-issue-updated', ev, { moduleKey: 'scope-issue-updated' });
    await platform.drain();
    const t = oracle(site)['11'];
    await w.page.waitForFunction((want) => document.querySelector('[data-sprint-id="11"] [data-metric="added"]')?.textContent === want, fmtPoints(t.added), { timeout: 10000 }).catch(() => {});
    const after = await w.page.textContent('[data-sprint-id="11"] [data-metric="added"]');
    ok(after === fmtPoints(t.added) && after !== before, `live: added moved ${before} -> ${after} without a reload`);
    ok((await w.page.evaluate(() => performance.getEntriesByType('navigation').length)) === navs && (await w.page.evaluate(() => document.readyState)) === 'complete', 'live: no page reload happened');
    const n = w.log.invokes.length;
    await w.page.waitForTimeout(4000);
    ok(w.log.invokes.length === n, `live: no polling (${w.log.invokes.length - n} resolver calls in 4 s idle)`);
    await w.shot();
    clean(w.log, 'live widget');
    livePages.delete(w.page);
    await w.page.close();
  }
  truth = oracle(site);

  // ---------- sprint action ----------
  const sprintExt = (id, state = 'active') => ({ type: 'jira:sprintAction', sprint: { id: Number(id), state }, board: { id: 1, type: 'scrum' }, project: { id: '10000', key: 'OPS', type: 'software' }, location: 'https://golden.atlassian.net/jira/software/projects/OPS/boards/1/backlog' });
  const rowIds = (page) => page.$$eval('table[data-testid="ledger"] tbody tr[data-change-id]', (els) => els.map((e) => e.getAttribute('data-change-id')));
  for (const [who, user, theme] of [['alice', alice, 'light'], ['bob', bob, 'dark']]) {
    const label = `sprint action ${who} ${theme}`;
    const s = await open({ resource: 'sprint', moduleType: 'jira:sprintAction', moduleKey: 'scope-sprint-ledger', extension: sprintExt(11), aaid: user.accountId, theme, width: 900, shot: `sprint-${who}-${theme}` });
    await s.page.locator('table[data-testid="ledger"]').waitFor();
    const t = truth['11'];
    const vis = t.changes.filter((c) => user.browse(site.issueById.get(c.issueId)));
    eq(await s.page.$$eval('[data-metric]', (els) => Object.fromEntries(els.map((e) => [e.getAttribute('data-metric'), e.textContent]))), { committed: fmtPoints(t.committed), added: fmtPoints(t.added), removed: fmtPoints(t.removed), creep: fmtCreep(t.creep) }, `${label}: team totals`);
    eq(await s.page.textContent('[data-testid="hidden-count"]'), String(t.changes.length - vis.length), `${label}: hidden-count`);
    eq(await rowIds(s.page), vis.map((c) => c.changeId), `${label}: default order at asc, ties by changelog id (${vis.length} rows)`);
    const first = vis[0];
    const firstRow = await s.page.$eval(`tr[data-change-id="${first.changeId}"]`, (tr) => Object.fromEntries([...tr.querySelectorAll('td[data-col]')].map((td) => [td.getAttribute('data-col'), td.getAttribute('data-col') === 'at' ? td.querySelector('time').getAttribute('datetime') : td.textContent])));
    const issue = site.issueById.get(first.issueId);
    eq(firstRow, { issue: first.issueKey, points: fmtPoints(issue.fields[issue.estField] ?? 0), kind: first.kind, by: first.by, at: new Date(first.at).toISOString(), source: 'reconcile' }, `${label}: cells of the first row`);
    eq(await s.page.$$eval('th[data-col]', (els) => els.map((e) => e.getAttribute('data-col'))), ['issue', 'points', 'kind', 'by', 'at', 'source'], `${label}: headers`);
    await audit(s.page, label);
    await s.shot();
    if (who === 'alice') {
      await s.page.click('th[data-col="at"]');
      eq(await rowIds(s.page), [...vis].reverse().map((c) => c.changeId), 'sort: at toggles to descending');
      eq(await s.page.getAttribute('th[data-col="at"]', 'aria-sort'), 'descending', 'sort: aria-sort=descending on at');
      await s.page.click('th[data-col="at"]');
      eq(await rowIds(s.page), vis.map((c) => c.changeId), 'sort: at toggles back to ascending');
      await s.page.click('th[data-col="points"]');
      const est = (c) => { const i = site.issueById.get(c.issueId); return i.fields[i.estField] ?? 0; };
      eq(await rowIds(s.page), [...vis].sort((a, b) => est(b) - est(a) || a.at - b.at || Number(a.changeId) - Number(b.changeId)).map((c) => c.changeId), 'sort: points descending, ties by at ascending');
      eq(await s.page.getAttribute('th[data-col="points"]', 'aria-sort'), 'descending', 'sort: aria-sort on points');
      ok((await s.page.getAttribute('th[data-col="at"]', 'aria-sort')) === null, 'sort: only the active header carries aria-sort');

      await s.page.click(`tr[data-change-id="${first.changeId}"] td[data-col="issue"] a`);
      eq(s.log.navigations.at(-1), { url: `/browse/${first.issueKey}`, type: 'new-tab' }, 'the issue key opens /browse/<KEY> through the Forge router');

      const pick = vis[2];
      await s.page.click(`tr[data-change-id="${pick.changeId}"] td[data-col="by"]`);
      eq(await s.page.getAttribute(`tr[data-change-id="${pick.changeId}"]`, 'aria-selected'), 'true', 'clicking a row selects it');
      const comments = () => site.comments.filter((c) => c.issueId === pick.issueId).length;
      const flags = () => s.log.flags.filter((f) => f.type === 'success').length;
      // Settled = no resolver call in flight for 400 ms (a second, unguarded click must be counted too).
      const waitIdle = async () => {
        for (;;) {
          await s.page.waitForTimeout(100);
          const busy = await s.page.getAttribute('[data-testid="post-summary"]', 'aria-busy');
          if (busy === 'false' && s.log.pending === 0 && Date.now() - s.log.lastSettled >= 400) return;
        }
      };
      let c0 = comments();
      let f0 = flags();
      await s.page.click('[data-testid="post-summary"]');
      await s.page.waitForTimeout(150);
      await waitIdle();
      ok(comments() === c0 + 1 && flags() === f0 + 1, `post: one click -> one comment, one success flag (${comments() - c0}/${flags() - f0})`);
      const posted = site.comments.at(-1);
      ok(posted.author === alice.accountId && JSON.stringify(posted.body).includes(pick.issueKey) && JSON.stringify(posted.body).includes('OPS Sprint 10') && JSON.stringify(posted.body).includes(fmtCreep(t.creep)), 'post: ADF as the viewer naming key, sprint and creep');
      c0 = comments();
      f0 = flags();
      await s.page.dblclick('[data-testid="post-summary"]');
      await s.page.waitForTimeout(150);
      await waitIdle();
      ok(comments() === c0 + 1 && flags() === f0 + 1, `post: a double click -> one comment, one success flag (${comments() - c0}/${flags() - f0})`);
      for (const retryAfter of [1, 7]) {
        site.rateLimits.push({ match: (m, p) => m === 'POST' && p.endsWith('/comment'), times: 1, retryAfter });
        c0 = comments();
        f0 = flags();
        const t0 = Date.now();
        await s.page.click('[data-testid="post-summary"]');
        await s.page.waitForTimeout(150);
        await waitIdle();
        const reqs = site.requests.filter((r) => r.method === 'POST' && r.path.endsWith('/comment') && r.at >= t0);
        ok(comments() === c0 + 1 && flags() === f0 + 1 && reqs.length === 2 && reqs[1].at - reqs[0].at >= retryAfter * 1000, `post: 429 Retry-After ${retryAfter}s -> waited ${reqs.length === 2 ? reqs[1].at - reqs[0].at : '?'} ms, one comment, one success flag`);
      }
      site.rateLimits.push({ match: (m, p) => m === 'POST' && p.endsWith('/comment'), times: 1, status: 500 });
      c0 = comments();
      const e0n = s.log.flags.filter((f) => f.type === 'error').length;
      await s.page.click('[data-testid="post-summary"]');
      await s.page.waitForTimeout(150);
      await waitIdle();
      ok(comments() === c0 && s.log.flags.filter((f) => f.type === 'error').length === e0n + 1, 'post: another failure shows an error flag');
      await s.page.click('[data-testid="post-summary"]');
      await s.page.waitForTimeout(150);
      await waitIdle();
      ok(comments() === c0 + 1, 'post: the modal keeps working after a failure');
      // Forge LLM explanation
      const visIds = new Set(vis.map((c) => c.changeId));
      await s.page.click('[data-testid="explain"]');
      await s.page.locator('[data-testid="explanation"]').waitFor({ timeout: 10000 });
      const exIds = await s.page.$$eval('[data-testid="explanation"] [data-change-id]', (els) => els.map((e) => e.getAttribute('data-change-id')));
      const exText = await s.page.textContent('[data-testid="explanation"] p');
      ok(exIds.length > 0 && exIds.every((id) => visIds.has(id)) && !/\d/.test(exText), `explain: summary without digits + ${exIds.length} visible change elements`);
      platform.llm.script.push(() => ({ choices: [{ finish_reason: 'end_turn', message: { role: 'assistant', content: [{ type: 'text', text: 'No.' }] } }] }));
      const ef = s.log.flags.filter((f) => f.type === 'error').length;
      await s.page.click('[data-testid="explain"]');
      await waitIdle();
      ok(s.log.flags.filter((f) => f.type === 'error').length === ef + 1, 'explain: a refusal shows an error flag');
      await s.page.click(`tr[data-change-id="${vis[0].changeId}"] td[data-col="by"]`);
      ok((await s.page.getAttribute(`tr[data-change-id="${vis[0].changeId}"]`, 'aria-selected')) === 'true', 'explain: the modal keeps working after the error');
      await s.shot('sprint-alice-explained');
      await s.page.click('[data-testid="close"]');
      ok(s.log.closes === 1, 'close closes the modal');
    }
    clean(s.log, label);
    await s.page.close();
  }
  const ns = await open({ resource: 'sprint', moduleType: 'jira:sprintAction', moduleKey: 'scope-sprint-ledger', extension: sprintExt(13, 'future'), shot: 'sprint-not-started' });
  await ns.page.locator('[data-testid="not-started"]').waitFor();
  eq(await ns.page.$$eval('[data-testid="sprint-ledger"] [data-testid]', (els) => els.map((e) => e.getAttribute('data-testid'))), ['not-started'], 'sprint action: a sprint that has not started shows not-started and nothing else');
  await ns.shot();
  clean(ns.log, 'not-started');
  await ns.page.close();

  await browser.close();
  for (const s of Object.values(servers)) s.srv.close();
  console.log(`\n${failures ? `${failures} FAILED` : 'ALL PASSED'}  screenshots: ${outDir}`);
  process.exit(failures ? 1 : 0);
}

main().catch((e) => {
  console.error('CRASH', e);
  process.exit(2);
});
