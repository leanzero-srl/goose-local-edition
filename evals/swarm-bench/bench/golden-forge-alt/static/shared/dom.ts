import { view } from '@forge/bridge';

type Attrs = Record<string, string | number | boolean | null | undefined>;
type Child = Node | string | null | undefined | false;

export function h<K extends keyof HTMLElementTagNameMap>(tag: K, attrs: Attrs = {}, ...children: Child[]): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  setAttrs(el, attrs);
  append(el, children);
  return el;
}

const SVG_NS = 'http://www.w3.org/2000/svg';
export function s(tag: string, attrs: Attrs = {}, ...children: Child[]): SVGElement {
  const el = document.createElementNS(SVG_NS, tag) as SVGElement;
  setAttrs(el, attrs);
  append(el, children);
  return el;
}

function setAttrs(el: Element, attrs: Attrs) {
  for (const [k, v] of Object.entries(attrs)) {
    if (v === null || v === undefined || v === false) continue;
    el.setAttribute(k, v === true ? '' : String(v));
  }
}

function append(el: Element, children: Child[]) {
  for (const c of children) {
    if (c === null || c === undefined || c === false) continue;
    el.append(typeof c === 'string' ? document.createTextNode(c) : c);
  }
}

export function mount(...nodes: Node[]) {
  const root = document.getElementById('root')!;
  root.replaceChildren(...nodes);
}

export async function boot<T>(run: () => Promise<T>) {
  await view.theme.enable();
  try {
    await run();
  } catch (err) {
    mount(h('p', { class: 'state state-error', role: 'alert' }, `Something went wrong: ${(err as Error).message}`));
  }
}

export function fmtPoints(x: number): string {
  const n = Number(Number(x || 0).toFixed(6));
  return Object.is(n, -0) ? '0' : String(n);
}

export interface Summary {
  committed: number;
  added: number;
  removed: number;
  creepPercent: number | null;
  creepText: string;
}

export function metricText(summary: Summary, metric: string): string {
  return metric === 'creep' ? summary.creepText : fmtPoints(summary[metric as 'committed']);
}

export function metricList(summary: Summary) {
  return h(
    'dl',
    { class: 'metrics' },
    ...(['committed', 'added', 'removed', 'creep'] as const).map((m) =>
      h('div', { class: `metric metric-${m}` }, h('dt', {}, m[0].toUpperCase() + m.slice(1)), h('dd', { 'data-metric': m }, metricText(summary, m))),
    ),
  );
}
