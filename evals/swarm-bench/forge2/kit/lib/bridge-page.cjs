'use strict';
// The page side of the Custom UI host. It runs in the surface's page BEFORE the app bundle
// (@forge/bridge 7.1.0 captures globalThis.__bridge.callBridge at module load) and is serialised with
// Function.prototype.toString, so it must not close over anything outside its own body.
// Ops whose arguments or results are functions (`on`, `getWidgetApi`, `getWidgetEditApi`,
// `createHistory`, `onClose`, `openModal`) are handled here; every other op is forwarded to the host
// process (Playwright binding when scoring, same-origin HTTP when the dev kit serves the surface).

function pageBridge(cfg) {
  const subs = new Map();
  const rtSubs = new Map();
  const editHandlers = { onSave: [], onProductSave: null, onSaveError: [] };
  let lastUpdate = null;
  let hasUpdate = false;
  const strip = (v) => (v === undefined ? null : JSON.parse(JSON.stringify(v, (k, x) => (typeof x === 'function' ? '[function]' : x))));
  const send = cfg.transport === 'binding'
    ? (op, payload) => window[cfg.binding](cfg.surfaceId, op, JSON.stringify(strip(payload))).then((r) => JSON.parse(r))
    : (op, payload) => fetch(cfg.opUrl, { method: 'POST', headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ surfaceId: cfg.surfaceId, op, payload: strip(payload) }) }).then((r) => r.json());
  const call = async (op, payload) => {
    const r = await send(op, payload);
    if (!r.ok) throw new Error(r.error);
    return r.value;
  };
  const emitLocal = (event, payload) => {
    if (event === '__forge_realtime_event__') {
      const cb = rtSubs.get(payload.subscriptionId);
      if (cb) { try { cb(payload.payload); } catch (e) { console.error(e); } }
      return;
    }
    for (const cb of subs.get(event) ?? []) {
      try { cb(payload); } catch (e) { console.error(e); }
    }
  };
  const flags = (() => {
    let root = null;
    const ensure = () => {
      if (root) return root;
      const host = document.createElement('forge-host-flags');
      host.setAttribute('data-forge-host', 'flags');
      document.documentElement.appendChild(host);
      root = host.attachShadow({ mode: 'closed' });
      const sheet = new CSSStyleSheet();
      sheet.replaceSync(':host{position:fixed;left:16px;bottom:16px;z-index:2147483647;display:flex;flex-direction:column;gap:8px;font:14px/1.4 -apple-system,Segoe UI,sans-serif}'
        + '.f{min-width:260px;max-width:360px;padding:12px 16px;border-radius:4px;color:#fff;background:#0C66E4;box-shadow:0 4px 12px rgba(0,0,0,.3)}'
        + '.f[data-appearance=success]{background:#1F845A}.f[data-appearance=error]{background:#C9372C}.f[data-appearance=warning]{background:#B65C02}'
        + '.t{font-weight:600}');
      root.adoptedStyleSheets = [sheet];
      return root;
    };
    return {
      show(o) {
        const r = ensure();
        const el = document.createElement('div');
        el.className = 'f';
        el.dataset.id = String(o.id);
        el.dataset.appearance = o.appearance ?? o.type ?? 'info';
        const t = document.createElement('div');
        t.className = 't';
        t.textContent = String(o.title ?? '');
        el.appendChild(t);
        if (o.description) { const d = document.createElement('div'); d.textContent = String(o.description); el.appendChild(d); }
        r.appendChild(el);
      },
      close(id) { root?.querySelectorAll(`.f[data-id="${CSS.escape(String(id))}"]`).forEach((e) => e.remove()); },
    };
  })();
  const theme = async () => {
    const t = await call('enableTheming', null);
    for (const [k, v] of Object.entries(t.attrs)) document.documentElement.setAttribute(k, v);
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(t.css);
    document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
    return undefined;
  };
  const memoryHistory = () => {
    const listeners = new Set();
    const entries = [{ pathname: '/', search: '', hash: '', state: null, key: 'default' }];
    let index = 0;
    const parse = (to, state) => {
      const u = new URL(typeof to === 'string' ? to : `${to.pathname ?? '/'}${to.search ?? ''}${to.hash ?? ''}`, 'http://forge.local');
      return { pathname: u.pathname, search: u.search, hash: u.hash, state: state ?? to?.state ?? null, key: Math.random().toString(36).slice(2, 8) };
    };
    const h = {
      action: 'POP',
      get location() { return entries[index]; },
      get length() { return entries.length; },
      push(to, state) { entries.splice(index + 1); entries.push(parse(to, state)); index = entries.length - 1; h.action = 'PUSH'; notify(); },
      replace(to, state) { entries[index] = parse(to, state); h.action = 'REPLACE'; notify(); },
      go(n) { index = Math.max(0, Math.min(entries.length - 1, index + n)); h.action = 'POP'; notify(); },
      back() { h.go(-1); },
      forward() { h.go(1); },
      listen(fn) { listeners.add(fn); return () => listeners.delete(fn); },
      createHref(to) { return typeof to === 'string' ? to : `${to.pathname ?? ''}${to.search ?? ''}${to.hash ?? ''}`; },
    };
    const notify = () => { call('historyChange', { action: h.action, location: entries[index] }); for (const fn of listeners) fn({ action: h.action, location: entries[index] }); };
    return h;
  };
  const callBridge = async (op, payload) => {
    switch (op) {
      case 'enableTheming': return theme();
      case 'on': case 'onPublic': {
        const set = subs.get(payload.event) ?? new Set();
        set.add(payload.callback);
        subs.set(payload.event, set);
        await call(op, { event: payload.event });
        return { unsubscribe: () => set.delete(payload.callback) };
      }
      case 'getWidgetApi':
        await call('getWidgetApi', null);
        return { setPreviewConfig: (c) => call('setPreviewConfig', c) };
      case 'getWidgetEditApi':
        await call('getWidgetEditApi', null);
        return {
          updateConfig: async (c) => {
            lastUpdate = strip(c);
            hasUpdate = true;
            await call('updateConfig', lastUpdate);
            emitLocal('FORGE_DASHBOARDS_WIDGET_EDIT_CONFIG_CHANGED', { widgetId: cfg.widgetId });
          },
          onSave: (fn) => { editHandlers.onSave.push(fn); call('onSave', null); },
          onProductSave: (fn) => { editHandlers.onProductSave = fn; call('onProductSave', null); },
          onSaveError: (fn) => { editHandlers.onSaveError.push(fn); call('onSaveError', null); },
        };
      case 'subscribeRealtimeChannel': {
        const r = await call('subscribeRealtimeChannel', { channelName: payload.channelName, options: payload.options ?? null, isGlobal: Boolean(payload.isGlobal) });
        rtSubs.set(r.subscriptionId, payload.onEvent);
        return { unsubscribe: async () => { rtSubs.delete(r.subscriptionId); await call('unsubscribeRealtimeChannel', { subscriptionId: r.subscriptionId }); } };
      }
      case 'createHistory':
        await call('createHistory', null);
        return memoryHistory();
      case 'onClose':
        await call('onClose', null);
        return true;
      case 'showFlag': {
        const r = await call('showFlag', payload);
        flags.show(payload);
        return r;
      }
      case 'closeFlag': {
        const r = await call('closeFlag', payload);
        flags.close(payload?.id);
        return r;
      }
      default:
        return call(op, payload);
    }
  };
  globalThis.__bridge = { callBridge };

  // The dashboard's Save (DESIGN.md §6.4): with onProductSave registered only its return value is
  // stored (null stores nothing); with none registered the last updateConfig value is stored.
  const save = async (initiator) => {
    const current = await call('currentConfig', null);
    let toStore = null;
    let via = 'none';
    if (editHandlers.onProductSave) {
      via = 'onProductSave';
      const v = await editHandlers.onProductSave(hasUpdate ? lastUpdate : current);
      toStore = v === undefined || v === null ? null : strip(v);
    } else if (hasUpdate) {
      via = 'updateConfig';
      toStore = lastUpdate;
    }
    const r = await call('save', { config: toStore, via, initiator });
    for (const fn of editHandlers.onSave) {
      try { await fn(r.config ?? current, cfg.widgetId); } catch (e) { for (const h of editHandlers.onSaveError) h(e); }
    }
    emitLocal('FORGE_DASHBOARDS_WIDGET_CONFIG_CHANGED', { widgetId: cfg.widgetId });
    return r;
  };
  const hostApi = { emit: emitLocal, save: () => save(cfg.transport === 'binding' ? 'host' : 'dev') };
  Object.defineProperty(window, cfg.hostKey, { value: hostApi, enumerable: false });
  if (cfg.transport === 'http') {
    window.__forgeHost = { save: () => save('dev') };
    const es = new EventSource(cfg.eventsUrl);
    es.onmessage = (m) => { const e = JSON.parse(m.data); emitLocal(e.event, e.payload); };
  }
}

module.exports = { pageBridge };
