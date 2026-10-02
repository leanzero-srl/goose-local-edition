import { invoke, view, realtime } from '@forge/bridge';
import { boot, h, s, mount, metricList, fmtPoints, Summary } from '../../shared/dom';

interface SprintRow {
  id: string;
  name: string;
  startDate: string;
  summary: Summary;
}

const SERIES = ['committed', 'added', 'removed'] as const;
const PLOT_HEIGHT = 140;

function chart(sprints: SprintRow[]) {
  const max = Math.max(0, ...sprints.flatMap((sp) => SERIES.map((k) => sp.summary[k])));
  const slot = 100 / Math.max(sprints.length, 1);
  const bar = slot / (SERIES.length + 1.5);
  const bars: SVGElement[] = [];
  sprints.forEach((sp, i) => {
    SERIES.forEach((k, j) => {
      const value = sp.summary[k];
      const height = max > 0 ? (value / max) * PLOT_HEIGHT : 0;
      bars.push(
        s(
          'rect',
          {
            class: `bar bar-${k}`,
            'data-sprint-id': sp.id,
            'data-series': k,
            x: `${i * slot + bar * (0.75 + j)}%`,
            width: `${bar}%`,
            y: PLOT_HEIGHT - height,
            height,
          },
          s('title', {}, `${sp.name} — ${k}: ${fmtPoints(value)}`),
        ),
      );
    });
  });
  return h(
    'figure',
    { class: 'chart' },
    s(
      'svg',
      { 'data-testid': 'chart', width: '100%', height: PLOT_HEIGHT + 1, role: 'img', 'aria-label': 'Story points per sprint: committed, added and removed' },
      ...bars,
      s('line', { class: 'axis', x1: '0', x2: '100%', y1: PLOT_HEIGHT + 0.5, y2: PLOT_HEIGHT + 0.5 }),
    ),
    h('div', { class: 'chart-labels' }, ...sprints.map((sp) => h('span', { class: 'chart-label', title: sp.name }, sp.name))),
    h('figcaption', { class: 'legend' }, ...SERIES.map((k) => h('span', { class: `key key-${k}` }, k[0].toUpperCase() + k.slice(1)))),
  );
}

boot(async () => {
  const ctx = (await view.getContext()) as { extension?: { config?: { boardId?: string | number } } };
  const boardId = ctx.extension?.config?.boardId;
  if (boardId === undefined || boardId === null || boardId === '') {
    mount(h('main', { 'data-testid': 'scope-widget', class: 'widget' }, h('p', { 'data-testid': 'needs-config', class: 'state' }, 'Edit this widget and choose a scrum board.')));
    return;
  }
  const load = async () => {
    const data = (await invoke('board-scope', { boardId: String(boardId) })) as { sprints?: SprintRow[]; error?: string };
    if (data.error) throw new Error(data.error);
    return data.sprints || [];
  };
  const render = (sprints: SprintRow[]) =>
    mount(
      h(
        'main',
        { 'data-testid': 'scope-widget', class: 'widget' },
        sprints.length
          ? h(
              'div',
              { class: 'sprints' },
              ...sprints.map((sp) =>
                h('section', { 'data-testid': 'sprint', 'data-sprint-id': sp.id, class: 'sprint' }, h('h3', { class: 'sprint-name', title: sp.name }, sp.name), metricList(sp.summary)),
              ),
            )
          : h('p', { class: 'state' }, 'This board has no active sprint.'),
        sprints.length ? chart(sprints) : null,
      ),
    );

  let sprints = await load();
  render(sprints);

  // Live: one global channel per shown sprint (the backend publishes the sprint id after a
  // ledger write). A message re-reads the board once; messages that arrive meanwhile coalesce.
  let reloading: Promise<void> | null = null;
  let again = false;
  const refresh = () => {
    if (reloading) {
      again = true;
      return;
    }
    reloading = (async () => {
      do {
        again = false;
        sprints = await load();
        render(sprints);
      } while (again);
    })().finally(() => {
      reloading = null;
    });
  };
  const shown = new Set(sprints.map((sp) => sp.id));
  // A channel that cannot be subscribed leaves the rendered numbers in place and says so.
  const results = await Promise.allSettled(
    [...shown].map((id) =>
      realtime.subscribeGlobal(`scope-ledger-sprint-${id}`, (payload) => {
        const msg = typeof payload === 'string' ? safeJson(payload) : payload;
        if (msg && shown.has(String((msg as { sprintId?: unknown }).sprintId))) refresh();
      }),
    ),
  );
  const failed = results.filter((r) => r.status === 'rejected') as PromiseRejectedResult[];
  if (failed.length) console.error(`Live updates unavailable for ${failed.length} sprint(s): ${String(failed[0].reason)}`);
});

function safeJson(text: string): Record<string, unknown> | null {
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}
