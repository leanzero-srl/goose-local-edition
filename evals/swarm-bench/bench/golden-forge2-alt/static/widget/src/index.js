import { events, realtime, view } from '@forge/bridge';
import { boot, call } from '../../shared/bridge';
import { h, s, replace } from '../../shared/dom';
import '../../shared/base.css';
import './widget.css';

const SERIES = ['committed', 'added', 'removed'];
const METRICS = ['committed', 'added', 'removed', 'creep'];
const LABELS = { committed: 'Committed', added: 'Added', removed: 'Removed', creep: 'Creep' };

// Every bar on one linear scale from 0. The SVG stretches to the widget's width (preserveAspectRatio="none") and
// keeps a fixed height equal to its viewBox height, so a bar's rendered height is its number's share of the scale.
const CHART_HEIGHT = 160;
const BAR = 18;
const BAR_GAP = 4;
const GROUP_GAP = 24;

const root = document.getElementById('root');
const state = { context: null, shown: new Set(), subscribed: false, live: null };

function chart(sprints) {
  const max = Math.max(0, ...sprints.flatMap((sp) => SERIES.map((k) => sp.values[k])));
  const groupWidth = SERIES.length * BAR + (SERIES.length - 1) * BAR_GAP;
  const width = Math.max(1, sprints.length * groupWidth + (sprints.length + 1) * GROUP_GAP);
  const bars = sprints.flatMap((sp, i) =>
    SERIES.map((k, j) => {
      const height = max > 0 ? (sp.values[k] / max) * CHART_HEIGHT : 0;
      const x = GROUP_GAP + i * (groupWidth + GROUP_GAP) + j * (BAR + BAR_GAP);
      return s(
        'rect',
        { 'data-sprint-id': sp.id, 'data-series': k, class: `bar series-${k}`, x, y: CHART_HEIGHT - height, width: BAR, height },
        s('title', null, `${sp.name}: ${LABELS[k]} ${sp.text[k]}`),
      );
    }),
  );
  return h(
    'figure',
    { class: 'chart' },
    s(
      'svg',
      { 'data-testid': 'chart', class: 'chart-svg', viewBox: `0 0 ${width} ${CHART_HEIGHT}`, preserveAspectRatio: 'none', role: 'img', 'aria-label': 'Committed, added and removed points per sprint' },
      s('line', { class: 'chart-axis', x1: 0, x2: width, y1: CHART_HEIGHT, y2: CHART_HEIGHT, 'vector-effect': 'non-scaling-stroke' }),
      bars,
    ),
    h(
      'figcaption',
      { class: 'legend' },
      SERIES.map((k) => h('span', { class: 'legend-item' }, h('span', { class: `swatch series-${k}`, 'aria-hidden': 'true' }), LABELS[k])),
    ),
    h('p', { class: 'chart-caption' }, `Bars, left to right: ${sprints.map((sp) => sp.name).join(' · ')}`),
  );
}

const sprintCard = (sp) =>
  h(
    'section',
    { class: 'sprint', 'data-testid': 'sprint', 'data-sprint-id': sp.id },
    h('h3', { class: 'sprint-name', title: sp.name }, sp.name),
    h(
      'div',
      { class: 'metrics-host' },
      h('dl', { class: 'metrics' }, METRICS.map((k) => h('div', { class: `metric metric-${k}` }, h('dt', null, LABELS[k]), h('dd', { 'data-metric': k }, sp.text[k])))),
    ),
  );

function paint(content) {
  replace(root, h('div', { 'data-testid': 'scope-widget', class: 'widget' }, content));
}

const boardOf = (context) => context?.extension?.config?.boardId;

async function load() {
  const boardId = boardOf(state.context);
  if (boardId === undefined || boardId === null || boardId === '') {
    paint(h('p', { 'data-testid': 'needs-config', class: 'message' }, "Choose a scrum board in this widget's settings to see its sprint scope."));
    return;
  }
  if (!state.shown.size) paint(h('p', { class: 'subtle' }, 'Loading sprint scope…'));
  let data;
  try {
    data = await call('widget', { boardId: String(boardId) });
  } catch (e) {
    paint(h('p', { class: 'message error', role: 'alert' }, `Could not load sprint scope: ${e?.message ?? e}`));
    return;
  }
  if (data?.needsConfig) {
    paint(h('p', { 'data-testid': 'needs-config', class: 'message' }, "Choose a scrum board in this widget's settings to see its sprint scope."));
    return;
  }
  if (!data || data.ok === false || !Array.isArray(data.sprints)) {
    paint(h('p', { class: 'message error', role: 'alert' }, `Could not load sprint scope: ${data?.error ?? 'no answer'}`));
    return;
  }
  state.shown = new Set(data.sprints.map((sp) => sp.id));
  paint(
    data.sprints.length === 0
      ? h('p', { class: 'message' }, 'This board has no active sprint.')
      : [h('div', { class: 'sprints' }, data.sprints.map(sprintCard)), chart(data.sprints), state.live && h('p', { class: 'live subtle' }, state.live)],
  );
  if (data.realtime && !state.subscribed) subscribe(data.realtime.channel);
}

// Forge Realtime: the backend announces the sprint ids whose ledger changed; the widget re-reads its numbers when one
// of its sprints is named. No polling.
async function subscribe(channel) {
  state.subscribed = true;
  try {
    await realtime.subscribeGlobal(channel, (payload) => {
      let body = payload;
      if (typeof payload === 'string') {
        try {
          body = JSON.parse(payload);
        } catch {
          body = null;
        }
      }
      const ids = Array.isArray(body?.sprintIds) ? body.sprintIds.map(String) : [];
      if (ids.some((id) => state.shown.has(id))) load();
    });
    state.live = 'Live — updates as the ledger changes';
  } catch (e) {
    state.subscribed = false;
    state.live = `Live updates are off (${e?.message ?? e}); reopen the dashboard to refresh.`;
  }
  const note = root.querySelector('.live');
  if (note) note.textContent = state.live;
  else root.querySelector('.widget')?.append(h('p', { class: 'live subtle' }, state.live));
}

boot(async (context) => {
  state.context = context;
  await load();
  // The dashboard tells an open widget when its saved config changes; re-read the context then.
  try {
    await events.on('FORGE_DASHBOARDS_WIDGET_CONFIG_CHANGED', async () => {
      state.context = await view.getContext();
      state.shown = new Set();
      load();
    });
  } catch {
    // A host without this event leaves the widget on the config it was opened with.
  }
});
