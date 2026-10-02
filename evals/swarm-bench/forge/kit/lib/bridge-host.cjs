'use strict';
// The Custom UI host (DESIGN.md §6.4). Serves `resources[].path` under a nested random prefix (an absolute
// `/assets/...` 404s as in production) with the Forge Custom UI CSP (csp.cjs, Atlassian's @forge/csp), and
// answers @forge/bridge 7.1.0 ops for each surface: context, resolver invoke (user-led, through the
// runtime), requestJira (as the viewer, through the proxy), theming (tokens.cjs), router, flags, modal
// close, the event bus, and the dashboards widget host (view API, edit API, the dashboard's Save, layout).
// Any other bridge op is recorded and reported as harness_missing, never silently answered.
//
//   await emu.openSurface(page, { moduleKey, entry: 'view'|'edit', theme, layout, asUser, extension })
//   await emu.hostSave(page)            -> { config, via }   (the dashboard's Save on an open edit surface)
//   await emu.resizeSurface(page, layout)
//   startDevServer(emu, ...)            -> the same host for forge-dev `serve`, transport over HTTP
const fs = require('fs');
const path = require('path');
const http = require('http');
const crypto = require('crypto');
const { pageBridge } = require('./bridge-page.cjs');
const { themeFor } = require('./tokens.cjs');
const { cspFor } = require('./csp.cjs');

const MIME = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8', '.svg': 'image/svg+xml', '.png': 'image/png', '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg',
  '.gif': 'image/gif', '.webp': 'image/webp', '.ico': 'image/x-icon', '.json': 'application/json', '.map': 'application/json',
  '.woff': 'font/woff', '.woff2': 'font/woff2', '.ttf': 'font/ttf', '.txt': 'text/plain; charset=utf-8' };
const INERT = new Set(['close', 'submit', 'refresh', 'changeWindowTitle', 'emitReadyEvent', 'emitFrontendCustomMetric', 'initFeatureFlags',
  'onClose', 'showFlag', 'closeFlag', 'open', 'navigate', 'reload', 'on', 'onPublic', 'emit', 'emitPublic', 'getWidgetApi',
  'getWidgetEditApi', 'setPreviewConfig', 'createHistory', 'historyChange', 'onSave', 'onProductSave', 'onSaveError']);
const pages = new WeakMap();

function hostState(emu) {
  if (!emu._host) emu._host = { server: null, url: null, prefix: `${crypto.randomBytes(6).toString('hex')}/${crypto.randomBytes(6).toString('hex')}`, surfaces: new Map(), widgetConfigs: new Map(), cspReports: [], seq: 0 };
  return emu._host;
}

function resourceFor(emu, moduleKey, entry) {
  const found = emu.moduleByKey(moduleKey);
  if (!found) throw new Error(`no module '${moduleKey}' in the manifest`);
  const key = entry === 'edit' ? found.module.edit?.resource : found.module.resource;
  if (!key) throw new Error(`module '${moduleKey}' declares no ${entry === 'edit' ? 'edit.resource' : 'resource'}`);
  const res = (emu.manifest.resources ?? []).find((r) => r.key === key);
  if (!res) throw new Error(`resource '${key}' is not declared under resources`);
  return { type: found.type, module: found.module, resourceKey: key, dir: path.resolve(emu.appDir, res.path) };
}

function ensureServer(emu) {
  const h = hostState(emu);
  if (h.server) return Promise.resolve(h.url);
  h.server = http.createServer((req, res) => serve(emu, req, res));
  return new Promise((ok) => h.server.listen(0, '127.0.0.1', () => { h.url = `http://127.0.0.1:${h.server.address().port}`; ok(h.url); }));
}

function staticFile(emu, req, res, resourceKey, rel, inject) {
  let res0;
  try { res0 = (emu.manifest.resources ?? []).find((r) => r.key === resourceKey); } catch { res0 = null; }
  if (!res0) { res.writeHead(404); return res.end(); }
  const root = path.resolve(emu.appDir, res0.path);
  const file = path.resolve(root, rel || 'index.html');
  const target = fs.existsSync(file) && fs.statSync(file).isDirectory() ? path.join(file, 'index.html') : file;
  if (!target.startsWith(root + path.sep) && target !== root) { res.writeHead(404); return res.end(); }
  if (!fs.existsSync(target) || !fs.statSync(target).isFile()) { res.writeHead(404); return res.end(); }
  let body = fs.readFileSync(target);
  const headers = { 'content-type': MIME[path.extname(target).toLowerCase()] ?? 'application/octet-stream', 'cache-control': 'no-store' };
  if (path.extname(target).toLowerCase() === '.html') {
    const h = hostState(emu);
    headers['content-security-policy'] = cspFor(emu.paths, { indexHtml: body.toString('utf8'), permissions: emu.manifest.permissions,
      siteUrl: emu.siteInfo.siteUrl, reportUri: `${h.url}/__forge/csp-report` });
    if (inject) body = Buffer.from(injectScript(body.toString('utf8'), inject));
  }
  res.writeHead(200, headers);
  res.end(body);
}

// Dev serving only: the host's bridge client is loaded as a same-origin external script, first in <head>.
function injectScript(html, src) {
  const tag = `<script src="${src}"></script>`;
  if (/<head[^>]*>/i.test(html)) return html.replace(/<head[^>]*>/i, (m) => m + tag);
  return tag + html;
}

function serve(emu, req, res) {
  const h = hostState(emu);
  const url = new URL(req.url, 'http://host');
  const p = url.pathname;
  let m;
  if (p === '/__forge/csp-report' && req.method === 'POST') {
    let raw = '';
    req.on('data', (c) => (raw += c));
    req.on('end', () => { try { h.cspReports.push(JSON.parse(raw)); } catch { h.cspReports.push({ raw }); } res.writeHead(204); res.end(); });
    return;
  }
  if (p.startsWith(`/${h.prefix}/`)) {
    const rest = p.slice(h.prefix.length + 2).split('/');
    return staticFile(emu, req, res, decodeURIComponent(rest[0]), rest.slice(1).map(decodeURIComponent).join('/'), null);
  }
  if ((m = p.match(/^\/dev\/([a-z0-9-]+)\/([^/]+)\/(.*)$/))) {
    const s = h.surfaces.get(m[1]);
    if (!s) { res.writeHead(404); return res.end(); }
    return staticFile(emu, req, res, decodeURIComponent(m[2]), m[3].split('/').map(decodeURIComponent).join('/'), `/__forge/client/${s.id}.js`);
  }
  if ((m = p.match(/^\/__forge\/client\/([a-z0-9-]+)\.js$/))) {
    const s = h.surfaces.get(m[1]);
    if (!s) { res.writeHead(404); return res.end(); }
    res.writeHead(200, { 'content-type': 'text/javascript; charset=utf-8', 'cache-control': 'no-store' });
    return res.end(`(${pageBridge.toString()})(${JSON.stringify(s.cfg)});`);
  }
  if ((m = p.match(/^\/__forge\/events\/([a-z0-9-]+)$/))) {
    const s = h.surfaces.get(m[1]);
    if (!s) { res.writeHead(404); return res.end(); }
    res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-store', connection: 'keep-alive' });
    res.write(': forge host events\n\n');
    s.sse.add(res);
    req.on('close', () => s.sse.delete(res));
    return;
  }
  if (p === '/__forge/op' && req.method === 'POST') {
    let raw = '';
    req.on('data', (c) => (raw += c));
    req.on('end', async () => {
      const msg = JSON.parse(raw || '{}');
      const s = h.surfaces.get(msg.surfaceId);
      const out = s ? await answer(emu, s, msg.op, msg.payload) : { ok: false, error: 'unknown surface' };
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify(out));
    });
    return;
  }
  // A surface opened as a top-level page (the dev URL, the scorer's browser) makes the browser ask for the
  // origin's /favicon.ico; a 404 there is a console error that is not the app's. In production the surface is
  // an iframe and asks for none. 204: no icon, no error.
  if (p === '/favicon.ico') { res.writeHead(204, { 'cache-control': 'max-age=86400' }); return res.end(); }
  if (p === '/' || p === '') {
    res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
    return res.end(`<!doctype html><title>forge-dev host</title><ul>${[...h.surfaces.values()].filter((s) => s.dev).map((s) => `<li><a href="${s.url}">${s.moduleKey} ${s.entry}</a></li>`).join('')}</ul>`);
  }
  res.writeHead(404);
  res.end();
}

function contextFor(emu, s) {
  const info = emu.siteInfo;
  const ext = { ...(s.extension ?? {}) };
  if (s.type === 'dashboards:widget') {
    ext.type = 'dashboards:widget';
    ext.config = s.config ?? null;
    ext.context = ext.context ?? { dashboardId: 'dashboard-1', widgetId: s.widgetId };
    ext.layout = s.layout ? { ...ext.layout, width: s.layout.width, height: s.layout.height } : ext.layout;
    ext.placement = ext.placement ?? (s.entry === 'edit' ? 'WIDGET_EDIT' : 'DASHBOARD_VIEW');
    if (!('filters' in ext)) ext.filters = null;
    if (s.entry === 'edit') ext.entryPoint = 'edit'; else delete ext.entryPoint;
  } else if (!ext.type) ext.type = s.type;
  const appId = String(emu.manifest?.app?.id ?? '').split('/').pop();
  return {
    accountId: s.asUser, cloudId: info.cloudId, siteUrl: info.siteUrl, localId: `ari:cloud:ecosystem::extension/${appId}/emulator-env/static/${s.moduleKey}`,
    moduleKey: s.moduleKey, environmentId: 'emulator-env', environmentType: 'DEVELOPMENT', locale: 'en-US', timezone: 'UTC',
    theme: { colorMode: s.theme }, extension: ext,
  };
}

function urlFor(emu, location) {
  const site = emu.siteInfo.siteUrl;
  if (location?.target === 'issue' && location.issueKey) return `${site}/browse/${location.issueKey}`;
  if (location?.target === 'projectSettingsDetails' && location.projectKey) return `${site}/jira/software/projects/${location.projectKey}/settings/details`;
  if (location?.target === 'dashboard' && location.dashboardId) return `${site}/jira/dashboards/${location.dashboardId}`;
  if (location?.url) return new URL(location.url, site).toString();
  return null;
}

async function answer(emu, s, op, payload) {
  const h = hostState(emu);
  const entry = { surfaceId: s.id, moduleKey: s.moduleKey, entry: s.entry, op, payload, t: new Date(emu.clock.now()).toISOString(), initiator: s.dev ? 'dev' : 'page' };
  if (op === 'navigate' || op === 'open') {
    const loc = payload?.location ?? payload?.url ?? payload;
    entry.url = typeof loc === 'string' ? new URL(loc, emu.siteInfo.siteUrl).toString() : urlFor(emu, loc);
  }
  emu.bridgeLog.push(entry);
  try {
    let value;
    switch (op) {
      case 'getContext': value = contextFor(emu, s); break;
      case 'invoke': {
        const r = await emu.invokeResolver(s.moduleKey, payload?.functionKey, payload?.payload, contextFor(emu, s), s.asUser);
        entry.invocationId = r.invocationId ?? null;
        entry.ok = r.ok;
        if (!r.ok) {
          entry.error = r.error?.message ?? 'invoke failed';
          throw new Error(`There was an error invoking the function - ${entry.error}`);
        }
        value = r.result === undefined || r.result === null ? {} : r.result;
        entry.result = value;
        break;
      }
      case 'fetchProduct': {
        const init = payload?.fetchRequestInit ?? {};
        const headers = Object.fromEntries((init.headers ?? []).map(([k, v]) => [String(k).toLowerCase(), v]));
        const r = await emu.proxy.productFetch({ inv: { id: `ui:${s.id}`, moduleType: s.type, moduleKey: s.moduleKey, source: 'frontend', aaid: s.asUser },
          provider: 'user', product: payload?.product, method: (init.method ?? 'GET').toUpperCase(), path: payload?.restPath, headers, body: init.body ?? undefined });
        entry.status = r.status;
        value = { body: r.body, headers: r.headers, status: r.status, statusText: http.STATUS_CODES[r.status] ?? '', isAttachment: false };
        break;
      }
      case 'enableTheming': {
        const t = themeFor(emu.paths, s.theme);
        s.themed = true;
        value = { attrs: t.attrs, css: t.css };
        break;
      }
      case 'getUrl': value = urlFor(emu, payload); break;
      case 'currentConfig': value = s.config ?? null; break;
      case 'updateConfig':
        s.config = payload;
        s.lastUpdate = payload;
        value = null;
        break;
      case 'save': {
        if (payload?.initiator !== 'host' && payload?.initiator !== 'dev') entry.saveNotFromHost = true;
        const stored = payload?.config ?? null;
        if (stored !== null) { h.widgetConfigs.set(s.widgetId, stored); s.config = stored; }
        entry.stored = stored;
        entry.via = payload?.via;
        for (const o of h.surfaces.values()) if (o !== s && o.widgetId === s.widgetId && o.entry === 'view' && stored !== null) { o.config = stored; await emit(o, 'FORGE_DASHBOARDS_WIDGET_CONFIG_CHANGED', { widgetId: s.widgetId }); }
        value = { config: stored, stored, via: payload?.via };
        break;
      }
      default:
        if (!INERT.has(op)) {
          emu.harnessMissing.push({ what: `bridge op ${op}`, at: entry.t, moduleKey: s.moduleKey });
          entry.notModelled = true;
          throw new Error(`emulator: bridge op '${op}' is not modelled`);
        }
        value = op === 'close' || op === 'onClose' ? true : undefined;
    }
    return { ok: true, value: value === undefined ? null : value };
  } catch (e) {
    entry.error = entry.error ?? String(e.message);
    return { ok: false, error: String(e.message) };
  }
}

async function emit(s, event, payload) {
  if (s.page) {
    await s.page.evaluate(([k, ev, p]) => window[k]?.emit(ev, p), [s.cfg.hostKey, event, payload]).catch(() => {});
  }
  for (const res of s.sse) res.write(`data: ${JSON.stringify({ event, payload })}\n\n`);
}

function newSurface(emu, { moduleKey, entry = 'view', theme = 'light', layout = null, asUser, extension = null, widgetId = null }) {
  const h = hostState(emu);
  const r = resourceFor(emu, moduleKey, entry);
  const id = `s${++h.seq}-${crypto.randomBytes(3).toString('hex')}`;
  const wid = widgetId ?? extension?.context?.widgetId ?? 'widget-1';
  const s = { id, moduleKey, type: r.type, entry, resourceKey: r.resourceKey, theme, layout, asUser: asUser ?? emu.siteInfo.viewer,
    extension, widgetId: wid, config: extension && 'config' in extension ? extension.config : (h.widgetConfigs.get(wid) ?? null),
    sse: new Set(), page: null, dev: false };
  h.surfaces.set(id, s);
  return s;
}

async function openSurface(emu, page, opts) {
  if (pages.has(page)) throw new Error('one surface per page: open each surface in a fresh page');
  await ensureServer(emu);
  const h = hostState(emu);
  const s = newSurface(emu, opts);
  s.page = page;
  const binding = `__forgeHostCall_${crypto.randomBytes(4).toString('hex')}`;
  s.cfg = { transport: 'binding', binding, surfaceId: s.id, widgetId: s.widgetId, hostKey: `__forgeHost_${crypto.randomBytes(4).toString('hex')}` };
  pages.set(page, s);
  await page.exposeFunction(binding, async (surfaceId, op, json) => JSON.stringify(await answer(emu, h.surfaces.get(surfaceId), op, JSON.parse(json))));
  await page.addInitScript({ content: `(${pageBridge.toString()})(${JSON.stringify(s.cfg)});` });
  if (opts.layout?.width && opts.layout?.height) await page.setViewportSize({ width: opts.layout.width, height: opts.layout.height });
  s.url = `${h.url}/${h.prefix}/${encodeURIComponent(s.resourceKey)}/index.html`;
  await page.goto(s.url, { waitUntil: 'load' });
  return { surfaceId: s.id, url: s.url };
}

async function hostSave(emu, page) {
  const s = pages.get(page);
  if (!s) throw new Error('hostSave: no surface open on this page');
  if (s.entry !== 'edit') throw new Error('hostSave: the open surface is not an edit surface');
  return page.evaluate((k) => window[k].save(), s.cfg.hostKey);
}

async function resize(emu, page, layout) {
  const s = pages.get(page);
  if (!s) throw new Error('resize: no surface open on this page');
  s.layout = layout;
  await page.setViewportSize({ width: layout.width, height: layout.height });
  await emit(s, s.entry === 'edit' ? 'FORGE_DASHBOARDS_WIDGET_EDIT_LAYOUT_CHANGED' : 'FORGE_DASHBOARDS_WIDGET_LAYOUT_CHANGED', { widgetId: s.widgetId });
}

// forge-dev serve: the same host over HTTP; the surface is a URL the entrant opens in any browser.
async function serveDev(emu, opts) {
  const url = await ensureServer(emu);
  const s = newSurface(emu, opts);
  s.dev = true;
  s.cfg = { transport: 'http', surfaceId: s.id, widgetId: s.widgetId, opUrl: `${url}/__forge/op`, eventsUrl: `${url}/__forge/events/${s.id}`,
    hostKey: `__forgeHost_${crypto.randomBytes(4).toString('hex')}` };
  s.url = `${url}/dev/${s.id}/${encodeURIComponent(s.resourceKey)}/index.html`;
  return { surfaceId: s.id, url: s.url, surface: s };
}

function cspReports(emu) { return hostState(emu).cspReports; }
function widgetConfigs(emu) { return Object.fromEntries(hostState(emu).widgetConfigs); }
async function closeHost(emu) { const h = emu._host; if (h?.server) await new Promise((ok) => h.server.close(() => ok())); }

module.exports = { openSurface, hostSave, resize, serveDev, cspReports, widgetConfigs, closeHost, contextFor };
