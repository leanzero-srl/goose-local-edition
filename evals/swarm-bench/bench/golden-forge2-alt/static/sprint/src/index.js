import { router, showFlag, view } from '@forge/bridge';
import { boot, call, formatInstant } from '../../shared/bridge';
import { h, replace } from '../../shared/dom';
import '../../shared/base.css';
import './sprint.css';

const METRICS = [
  ['committed', 'Committed'],
  ['added', 'Added'],
  ['removed', 'Removed'],
  ['creep', 'Creep'],
];

const COLUMNS = [
  ['issue', 'Issue'],
  ['points', 'Points'],
  ['kind', 'Change'],
  ['by', 'By'],
  ['at', 'When'],
  ['source', 'Source'],
  ['deployed', 'Deployed'],
];

const isNumeric = (x) => /^\d+$/.test(x);
const compareIds = (a, b) => {
  if (isNumeric(a) && isNumeric(b)) return a.length - b.length || (a < b ? -1 : a > b ? 1 : 0);
  return a < b ? -1 : a > b ? 1 : 0;
};
const byTime = (a, b) => Date.parse(a.at) - Date.parse(b.at) || compareIds(a.changeId, b.changeId);

let flagSeq = 0;
const flag = (type, title, description) =>
  showFlag({ id: `scope-ledger-${Date.now()}-${(flagSeq += 1)}`, title, description, type, appearance: type, isAutoDismiss: true });

const root = document.getElementById('root');
const st = { context: null, data: null, dir: 'ascending', selected: null, posting: false, explaining: false, explanation: null };

const closeButton = () => h('button', { type: 'button', 'data-testid': 'close', class: 'button', onClick: () => view.close() }, 'Close');

function openIssue(e, key) {
  e.preventDefault();
  e.stopPropagation();
  router.open(`/browse/${key}`).catch((err) => flag('error', `Could not open ${key}`, err?.message ?? String(err)));
}

function select(changeId) {
  st.selected = changeId;
  for (const tr of root.querySelectorAll('tr[data-change-id]')) {
    const on = tr.getAttribute('data-change-id') === changeId;
    tr.setAttribute('aria-selected', on ? 'true' : 'false');
    tr.classList.toggle('selected', on);
  }
  const row = st.data.changes.find((c) => c.changeId === changeId);
  const note = root.querySelector('.selection');
  if (note && row) note.textContent = `Selected: ${row.issueKey} (${row.kind})`;
}

function rowFor(r) {
  return h(
    'tr',
    {
      'data-change-id': r.changeId,
      'aria-selected': st.selected === r.changeId ? 'true' : 'false',
      tabindex: 0,
      class: st.selected === r.changeId ? 'selected' : undefined,
      onClick: () => select(r.changeId),
      onKeydown: (e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          select(r.changeId);
        }
      },
    },
    h('td', { 'data-col': 'issue' }, h('a', { class: 'link', href: `/browse/${r.issueKey}`, onClick: (e) => openIssue(e, r.issueKey) }, r.issueKey)),
    h('td', { 'data-col': 'points', class: 'num' }, r.pointsText),
    h('td', { 'data-col': 'kind' }, h('span', { class: `kind kind-${r.kind}` }, r.kind)),
    h('td', { 'data-col': 'by' }, r.by),
    h('td', { 'data-col': 'at' }, h('time', { datetime: r.at }, formatInstant(r.at, st.context))),
    h('td', { 'data-col': 'source' }, r.source),
    h('td', { 'data-col': 'deployed' }, (r.deployed ?? []).map((env) => h('span', { class: 'env', 'data-env': env }, `Deployed to ${env}`))),
  );
}

function tbody() {
  const rows = [...st.data.changes].sort(byTime);
  if (st.dir === 'descending') rows.reverse();
  return h(
    'tbody',
    null,
    rows.length === 0 ? h('tr', null, h('td', { colspan: COLUMNS.length, class: 'empty' }, 'No visible changes since the sprint started.')) : rows.map(rowFor),
  );
}

// Clicking "When" toggles between time order and its exact reverse.
function toggleSort() {
  st.dir = st.dir === 'ascending' ? 'descending' : 'ascending';
  const th = root.querySelector('th[data-col="at"]');
  th.setAttribute('aria-sort', st.dir);
  th.querySelector('.sort-mark').textContent = st.dir === 'ascending' ? ' ▲' : ' ▼';
  root.querySelector('table[data-testid="ledger"] tbody').replaceWith(tbody());
}

function explanationSection() {
  const ex = st.explanation;
  if (!ex) return null;
  return h(
    'section',
    { 'data-testid': 'explanation', class: 'explanation', 'aria-live': 'polite' },
    h('h3', null, 'Why the scope changed'),
    h('p', null, ex.summary),
    ex.changes.length > 0 && h('ul', { class: 'explained' }, ex.changes.map((c) => h('li', { 'data-change-id': c.changeId }, h('span', { class: `kind kind-${c.kind}` }, c.kind), ` ${c.issueKey}`))),
  );
}

function setBusy(testId, busy) {
  const button = root.querySelector(`[data-testid="${testId}"]`);
  if (!button) return;
  button.classList.toggle('busy', busy);
  button.setAttribute('aria-busy', busy ? 'true' : 'false');
}

// One click (or a double click) is one comment: a click while a post is in flight is ignored.
async function post() {
  if (st.posting) return;
  const row = st.data.changes.find((c) => c.changeId === st.selected);
  if (!row) {
    flag('info', 'Select a change first', 'Click a row of the ledger, then post its summary.');
    return;
  }
  st.posting = true;
  setBusy('post-summary', true);
  try {
    const res = await call('postSummary', { changeId: row.changeId });
    if (res?.ok) {
      if (!res.duplicate) flag('success', `Summary posted on ${res.issueKey}`, `Scope creep ${st.data.text.creep} in ${st.data.sprint.name}.`);
    } else flag('error', 'The summary was not posted', res?.error ?? 'Unknown error.');
  } catch (e) {
    flag('error', 'The summary was not posted', e?.message ?? String(e));
  } finally {
    st.posting = false;
    setBusy('post-summary', false);
  }
}

async function explain() {
  if (st.explaining) return;
  st.explaining = true;
  setBusy('explain', true);
  try {
    const res = await call('explain');
    // A failed attempt clears the previous answer, so nothing on screen claims to explain it.
    st.explanation = res?.ok ? res : null;
    if (!res?.ok) flag('error', 'No explanation', res?.error ?? 'Forge LLM did not answer.');
  } catch (e) {
    st.explanation = null;
    flag('error', 'No explanation', e?.message ?? String(e));
  } finally {
    st.explaining = false;
    setBusy('explain', false);
    const old = root.querySelector('[data-testid="explanation"]');
    const next = explanationSection();
    if (old && next) old.replaceWith(next);
    else if (old) old.remove();
    else if (next) root.querySelector('.actions').before(next);
  }
}

function ledger() {
  const d = st.data;
  const head = h(
    'thead',
    null,
    h(
      'tr',
      null,
      COLUMNS.map(([col, label]) =>
        col === 'at'
          ? h(
              'th',
              { 'data-col': col, scope: 'col', 'aria-sort': st.dir, class: 'sortable', onClick: toggleSort },
              h('button', { type: 'button', class: 'sort-button' }, label, h('span', { class: 'sort-mark', 'aria-hidden': 'true' }, st.dir === 'ascending' ? ' ▲' : ' ▼')),
            )
          : h('th', { 'data-col': col, scope: 'col' }, label),
      ),
    ),
  );
  return [
    h('div', { class: 'metrics-host' }, h('dl', { class: 'metrics' }, METRICS.map(([k, label]) => h('div', { class: `metric metric-${k}` }, h('dt', null, label), h('dd', { 'data-metric': k }, d.text[k]))))),
    h(
      'p',
      { class: 'hidden-line' },
      h('span', { 'data-testid': 'hidden-count', class: 'hidden-count' }, String(d.hiddenCount)),
      ` ${d.hiddenCount === 1 ? 'change is' : 'changes are'} on issues you cannot browse and not listed.`,
    ),
    h('div', { class: 'table-wrap' }, h('table', { 'data-testid': 'ledger', class: 'ledger' }, h('caption', { class: 'visually-hidden' }, 'Changes to the sprint after it started'), head, tbody())),
    explanationSection(),
    h(
      'div',
      { class: 'actions' },
      h('span', { class: 'subtle selection' }, 'Select a change to comment on its issue.'),
      h('button', { type: 'button', 'data-testid': 'post-summary', class: 'button primary', 'aria-busy': 'false', onClick: post }, 'Post summary comment'),
      h('button', { type: 'button', 'data-testid': 'explain', class: 'button', 'aria-busy': 'false', onClick: explain }, 'Explain the creep'),
      closeButton(),
    ),
  ];
}

function render(phase, message) {
  const d = st.data;
  if (phase === 'not-started') {
    replace(root, h('div', { 'data-testid': 'sprint-ledger', class: 'sprint-action' }, h('p', { 'data-testid': 'not-started', class: 'message' }, `${d.sprint.name} has not started yet, so nothing has entered or left it.`), h('div', { class: 'actions' }, closeButton())));
    return;
  }
  replace(
    root,
    h(
      'div',
      { 'data-testid': 'sprint-ledger', class: 'sprint-action' },
      h('h2', null, phase === 'ready' ? `Scope ledger · ${d.sprint.name}${d.sprint.state === 'closed' ? ' (closed)' : ''}` : 'Scope ledger'),
      phase === 'loading' && h('p', { class: 'subtle' }, "Loading the sprint's scope changes…"),
      phase === 'error' && [h('p', { class: 'message error', role: 'alert' }, message), h('div', { class: 'actions' }, closeButton())],
      phase === 'ready' && ledger(),
    ),
  );
}

boot(async (context) => {
  st.context = context;
  render('loading');
  try {
    const data = await call('sprintLedger');
    if (!data || data.error) {
      render('error', data?.error ?? 'No answer from the app.');
      return;
    }
    st.data = data;
    render(data.notStarted ? 'not-started' : 'ready');
  } catch (e) {
    render('error', e?.message ?? String(e));
  }
});
