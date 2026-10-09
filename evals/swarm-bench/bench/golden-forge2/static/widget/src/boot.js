import { bootFirstPaint, el, metrics, METRIC_LABELS } from '../../shared/boot';
import '../../shared/base.css';
import './widget.css';

// The first paint is the whole static view (sprints, numbers, chart) — the same markup app.js renders.
const SERIES = ['committed', 'added', 'removed'];
const CHART_HEIGHT = 160;
const BAR = 18;
const BAR_GAP = 4;
const GROUP_GAP = 24;
const SVG = 'http://www.w3.org/2000/svg';

function svg(tag, attrs, ...children) {
  const node = document.createElementNS(SVG, tag);
  for (const [k, v] of Object.entries(attrs)) node.setAttribute(k, String(v));
  for (const c of children) node.append(c);
  return node;
}

function chart(sprints) {
  const max = Math.max(0, ...sprints.flatMap((s) => SERIES.map((k) => s.values[k])));
  const groupWidth = SERIES.length * BAR + (SERIES.length - 1) * BAR_GAP;
  const width = Math.max(1, sprints.length * groupWidth + (sprints.length + 1) * GROUP_GAP);
  const bars = sprints.flatMap((s, i) =>
    SERIES.map((k, j) => {
      const h = max > 0 ? (s.values[k] / max) * CHART_HEIGHT : 0;
      const x = GROUP_GAP + i * (groupWidth + GROUP_GAP) + j * (BAR + BAR_GAP);
      return svg('rect', { 'data-sprint-id': s.id, 'data-series': k, class: `bar series-${k}`, x, y: CHART_HEIGHT - h, width: BAR, height: h }, svg('title', {}, `${s.name}: ${METRIC_LABELS[k]} ${s.text[k]}`));
    }),
  );
  const axis = svg('line', { class: 'chart-axis', x1: 0, x2: width, y1: CHART_HEIGHT, y2: CHART_HEIGHT, 'vector-effect': 'non-scaling-stroke' });
  const legend = el('figcaption', { class: 'legend' }, ...SERIES.map((k) => el('span', { class: 'legend-item' }, el('span', { class: `swatch series-${k}`, 'aria-hidden': 'true' }), METRIC_LABELS[k])));
  return el(
    'figure',
    { class: 'chart' },
    svg('svg', { 'data-testid': 'chart', class: 'chart-svg', viewBox: `0 0 ${width} ${CHART_HEIGHT}`, preserveAspectRatio: 'none', role: 'img', 'aria-label': 'Committed, added and removed points per sprint' }, axis, ...bars),
    legend,
    el('p', { class: 'chart-caption' }, `Bars, left to right: ${sprints.map((s) => s.name).join(' · ')}`),
  );
}

const boardOf = (context) => context?.extension?.config?.boardId;

bootFirstPaint({
  request: (context) => {
    const boardId = boardOf(context);
    return boardId === undefined || boardId === null || boardId === '' ? null : { key: 'widget', payload: { boardId: String(boardId) } };
  },
  paint: (root, data) => {
    const widget = el('div', { 'data-testid': 'scope-widget', class: 'widget' });
    if (!data || data.needsConfig) widget.append(el('p', { 'data-testid': 'needs-config', class: 'message' }, "Choose a scrum board in this widget's settings to see its sprint scope."));
    else if (data.ok === false) widget.append(el('p', { class: 'message error', role: 'alert' }, `Could not load sprint scope: ${data.error}`));
    else if (!data.sprints.length) widget.append(el('p', { class: 'message' }, 'This board has no active sprint.'));
    else {
      const sprints = data.sprints.map((s) => el('section', { class: 'sprint', 'data-testid': 'sprint', 'data-sprint-id': s.id }, el('h3', { class: 'sprint-name' }, s.name), metrics(s.text)));
      widget.append(el('div', { class: 'sprints' }, ...sprints), chart(data.sprints));
    }
    root.replaceChildren(widget);
  },
});
