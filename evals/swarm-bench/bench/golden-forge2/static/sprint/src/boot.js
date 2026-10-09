import { bootFirstPaint, el, metrics } from '../../shared/boot';
import { formatInstant } from '../../shared/bridge';
import '../../shared/base.css';
import './sprint.css';

// The first paint is the whole static view (totals, hidden count, the ledger in table order) — the same
// markup app.js renders; app.js adds sorting, selection and the actions.
const COLUMNS = [
  ['issue', 'Issue'],
  ['points', 'Points'],
  ['kind', 'Change'],
  ['by', 'By'],
  ['at', 'When'],
  ['source', 'Source'],
  ['deployed', 'Deployed'],
];

function ledger(changes, context) {
  const head = el('tr', {}, ...COLUMNS.map(([col, label]) => el('th', { 'data-col': col, scope: 'col', ...(col === 'at' ? { 'aria-sort': 'ascending' } : {}) }, label)));
  const rows = changes.length
    ? changes.map((r) =>
        el(
          'tr',
          { 'data-change-id': r.changeId, 'aria-selected': 'false' },
          el('td', { 'data-col': 'issue' }, el('a', { class: 'link', href: `/browse/${r.issueKey}` }, r.issueKey)),
          el('td', { 'data-col': 'points', class: 'num' }, r.pointsText),
          el('td', { 'data-col': 'kind' }, el('span', { class: `kind kind-${r.kind}` }, r.kind)),
          el('td', { 'data-col': 'by' }, r.by),
          el('td', { 'data-col': 'at' }, el('time', { datetime: r.at }, formatInstant(r.at, context))),
          el('td', { 'data-col': 'source' }, r.source),
          el('td', { 'data-col': 'deployed' }, r.deployedTo.map((env) => `Deployed to ${env}`).join(', ')),
        ),
      )
    : [el('tr', {}, el('td', { colspan: String(COLUMNS.length), class: 'empty' }, 'No visible changes since the sprint started.'))];
  return el('div', { class: 'table-wrap' }, el('table', { 'data-testid': 'ledger', class: 'ledger' }, el('thead', {}, head), el('tbody', {}, ...rows)));
}

bootFirstPaint({
  request: () => ({ key: 'sprintLedger', payload: undefined }),
  paint: (root, data, context) => {
    const box = el('div', { 'data-testid': 'sprint-ledger', class: 'sprint-action' });
    if (data?.notStarted) box.append(el('p', { 'data-testid': 'not-started', class: 'message' }, `${data.sprint.name} has not started yet, so nothing has entered or left it.`));
    else if (!data || data.error) box.append(el('h2', {}, 'Scope ledger'), el('p', { class: 'message error', role: 'alert' }, data?.error ?? 'The sprint could not be loaded.'));
    else {
      const hidden = el('p', { class: 'hidden-line' }, el('span', { 'data-testid': 'hidden-count', class: 'hidden-count' }, String(data.hiddenCount)), ` ${data.hiddenCount === 1 ? 'change is' : 'changes are'} on issues you cannot browse and not listed.`);
      box.append(el('h2', {}, `Scope ledger · ${data.sprint.name}`), metrics(data.text), hidden, ledger(data.changes, context));
    }
    root.replaceChildren(box);
  },
});
