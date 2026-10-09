import { invoke, view } from '@forge/bridge';

// The boot budget: the first data paint comes from this small script (the bridge and plain DOM, no React)
// after exactly one invoke; the full app (app.js, React) loads after it and renders from the same answer,
// so it makes no second call to get started.

const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

export function el(tag, attrs = {}, ...children) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) node.setAttribute(k, v);
  for (const c of children) node.append(c);
  return node;
}

export const METRIC_LABELS = { committed: 'Committed', added: 'Added', removed: 'Removed', creep: 'Creep' };

export function metrics(text) {
  return el(
    'div',
    { class: 'metrics-host' },
    el('dl', { class: 'metrics' }, ...['committed', 'added', 'removed', 'creep'].map((k) => el('div', { class: `metric metric-${k}` }, el('dt', {}, METRIC_LABELS[k]), el('dd', { 'data-metric': k }, text[k])))),
  );
}

// request(context) names the one resolver call ({ key, payload }), or null when there is nothing to ask
// (a widget with no board yet). paint(root, data, context) draws the first data; then app.js takes over
// from window.__scopeBoot.
export async function bootFirstPaint({ request: requestOf, paint }) {
  await view.theme.enable();
  const context = await view.getContext();
  let data = null;
  const request = requestOf(context);
  if (request) {
    try {
      for (;;) {
        data = await invoke(request.key, request.payload);
        if (!data?.rateLimited) break;
        await wait(Math.max(1, Number(data.retryAfter) || 1) * 1000 + 50);
      }
    } catch (e) {
      data = { ok: false, error: e?.message ?? String(e) };
    }
  }
  paint(document.getElementById('root'), data, context);
  window.__scopeBoot = { context, data };
  // The app bundle is requested only once the first data has been painted (the next frame).
  requestAnimationFrame(() => setTimeout(() => document.body.append(el('script', { src: './app.js' })), 0));
}
