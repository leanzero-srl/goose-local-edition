import { invoke, view, router, showFlag, requestJira } from '@forge/bridge';
import { boot, h, mount, metricList, fmtPoints, Summary } from '../../shared/dom';

interface Change {
  changeId: string;
  issueKey: string;
  points: number;
  kind: 'added' | 'removed';
  by: string;
  at: string;
  source: 'event' | 'reconcile';
}
interface Report {
  error?: string;
  notStarted?: boolean;
  sprint: { id: string; name: string };
  summary: Summary;
  hiddenChanges: number;
  changes: Change[];
}

interface Explanation {
  error?: 'refused' | 'malformed' | 'error';
  message?: string;
  summary: string;
  changes: Array<{ changeId: string; issueKey: string; kind: 'added' | 'removed' }>;
}
const EXPLAIN_ERRORS: Record<string, string> = {
  refused: 'The model did not give an explanation',
  malformed: 'The model’s explanation was unusable',
  error: 'Could not explain this sprint',
};

type Sort = { col: 'at'; dir: 'ascending' | 'descending' };
const COLUMNS: Array<[string, string]> = [
  ['issue', 'Issue'],
  ['points', 'Points'],
  ['kind', 'Change'],
  ['by', 'By'],
  ['at', 'When'],
  ['source', 'Recorded by'],
];

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const defaultOrder = (a: Change, b: Change) => Date.parse(a.at) - Date.parse(b.at) || Number(a.changeId) - Number(b.changeId);

function ordered(changes: Change[], sort: Sort): Change[] {
  const base = [...changes].sort(defaultOrder);
  return sort.dir === 'ascending' ? base : base.reverse();
}

function closeButton() {
  const b = h('button', { type: 'button', class: 'btn btn-subtle', 'data-testid': 'close' }, 'Close');
  b.addEventListener('click', () => view.close());
  return b;
}

function adfSummary(c: Change, report: Report) {
  const prep = c.kind === 'added' ? 'to' : 'from';
  return {
    version: 1,
    type: 'doc',
    content: [
      {
        type: 'paragraph',
        content: [
          { type: 'text', text: `Scope ledger: ${c.issueKey} was ${c.kind} ${prep} sprint "${report.sprint.name}" after it started (${fmtPoints(c.points)} points). ` },
          { type: 'text', text: `Sprint scope creep: ${report.summary.creepText}`, marks: [{ type: 'strong' }] },
          { type: 'text', text: '.' },
        ],
      },
    ],
  };
}

// Posted from the browser with the bridge, so Jira records the viewer as the author.
async function postComment(c: Change, report: Report): Promise<void> {
  const body = JSON.stringify({ body: adfSummary(c, report) });
  for (let attempt = 0; attempt < 8; attempt++) {
    const res = await requestJira(`/rest/api/3/issue/${encodeURIComponent(c.issueKey)}/comment`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
      body,
    });
    if (res.status === 429) {
      const ra = Number(res.headers.get('Retry-After'));
      await sleep((Number.isFinite(ra) && ra > 0 ? ra : 1) * 1000 + 100);
      continue;
    }
    if (!res.ok) throw new Error(`Jira answered ${res.status}`);
    return;
  }
  throw new Error('Jira kept rate-limiting the request');
}

function renderLedger(report: Report) {
  let sort: Sort = { col: 'at', dir: 'ascending' };
  let selectedId: string | null = null;
  let posting = false;

  const headers = COLUMNS.map(([col, label]) => {
    const sortable = col === 'at';
    const th = h('th', { 'data-col': col, scope: 'col', class: sortable ? 'sortable' : null, tabindex: sortable ? 0 : null }, label);
    if (sortable) {
      const activate = () => {
        sort = { col: 'at', dir: sort.dir === 'ascending' ? 'descending' : 'ascending' };
        renderBody();
      };
      th.addEventListener('click', activate);
      th.addEventListener('keydown', (e) => (e.key === 'Enter' || e.key === ' ') && (e.preventDefault(), activate()));
    }
    return th;
  });
  const tbody = h('tbody');
  const table = h('table', { 'data-testid': 'ledger', class: 'ledger-table' }, h('thead', {}, h('tr', {}, ...headers)), tbody);

  const post = h('button', { type: 'button', class: 'btn btn-primary', 'data-testid': 'post-summary', disabled: true }, 'Post summary comment');

  function renderBody() {
    for (const th of headers) {
      if (th.dataset.col === sort.col) th.setAttribute('aria-sort', sort.dir);
      else th.removeAttribute('aria-sort');
    }
    const rows = ordered(report.changes, sort).map((c) => {
      const link = h('a', { href: `/browse/${c.issueKey}`, class: 'issue-link' }, c.issueKey);
      link.addEventListener('click', (e) => {
        e.preventDefault();
        router.navigate({ target: 'issue', issueKey: c.issueKey });
      });
      return h(
        'tr',
        { 'data-change-id': c.changeId, 'aria-selected': String(c.changeId === selectedId), tabindex: 0 },
        h('td', { 'data-col': 'issue' }, link),
        h('td', { 'data-col': 'points', class: 'num' }, fmtPoints(c.points)),
        h('td', { 'data-col': 'kind' }, h('span', { class: `kind kind-${c.kind}` }, c.kind)),
        h('td', { 'data-col': 'by' }, c.by),
        h('td', { 'data-col': 'at' }, h('time', { datetime: c.at, title: c.at }, new Date(c.at).toLocaleString())),
        h('td', { 'data-col': 'source' }, c.source),
      );
    });
    tbody.replaceChildren(...(rows.length ? rows : [h('tr', { class: 'empty' }, h('td', { colspan: COLUMNS.length }, 'No changes you can see.'))]));
  }

  const select = (tr: HTMLElement | null) => {
    if (!tr || !tr.dataset.changeId) return;
    selectedId = tr.dataset.changeId;
    for (const r of Array.from(tbody.querySelectorAll('tr'))) r.setAttribute('aria-selected', String(r === tr));
    post.disabled = posting;
  };
  tbody.addEventListener('click', (e) => select((e.target as Element).closest('tr')));
  tbody.addEventListener('keydown', (e) => e.key === 'Enter' && select((e.target as Element).closest('tr')));

  post.addEventListener('click', async () => {
    const change = report.changes.find((c) => c.changeId === selectedId);
    if (!change || posting) return;
    posting = true;
    post.disabled = true;
    try {
      await postComment(change, report);
      showFlag({ id: `posted-${change.changeId}-${Date.now()}`, type: 'success', title: 'Summary posted', description: `Comment added to ${change.issueKey}.`, isAutoDismiss: true });
    } catch (err) {
      showFlag({ id: `failed-${Date.now()}`, type: 'error', title: 'Could not post the summary', description: (err as Error).message, isAutoDismiss: true });
    } finally {
      posting = false;
      post.disabled = selectedId === null;
    }
  });

  const explanation = h('section', { 'data-testid': 'explanation', class: 'explanation', 'aria-live': 'polite', hidden: true });
  const explain = h('button', { type: 'button', class: 'btn btn-subtle', 'data-testid': 'explain' }, 'Explain the creep');
  let explaining = false;
  explain.addEventListener('click', async () => {
    if (explaining) return;
    explaining = true;
    explain.disabled = true;
    try {
      const r = (await invoke('explain', { sprintId: report.sprint.id })) as Explanation;
      if (r.error) {
        showFlag({ id: `explain-${Date.now()}`, type: 'error', title: EXPLAIN_ERRORS[r.error] || 'Could not explain this sprint', description: r.message, isAutoDismiss: true });
        return;
      }
      explanation.replaceChildren(
        h('h3', { class: 'explanation-title' }, 'Why the scope moved'),
        h('p', { class: 'explanation-text' }, r.summary),
        r.changes.length
          ? h('ul', { class: 'explanation-changes' }, ...r.changes.map((c) => h('li', { 'data-change-id': c.changeId }, h('span', { class: `kind kind-${c.kind}` }, c.kind), ' ', c.issueKey)))
          : h('p', { class: 'explanation-none' }, 'No single change stood out.'),
      );
      explanation.hidden = false;
    } catch (err) {
      showFlag({ id: `explain-${Date.now()}`, type: 'error', title: 'Could not explain this sprint', description: (err as Error).message, isAutoDismiss: true });
    } finally {
      explaining = false;
      explain.disabled = false;
    }
  });

  renderBody();
  mount(
    h(
      'main',
      { class: 'ledger' },
      h('header', { class: 'ledger-head' }, h('h2', { class: 'ledger-title', title: report.sprint.name }, report.sprint.name)),
      metricList(report.summary),
      h('p', { class: 'hidden-note' }, 'Changes hidden from you: ', h('strong', { 'data-testid': 'hidden-count' }, String(report.hiddenChanges))),
      h('div', { class: 'table-wrap' }, table),
      explanation,
      h('footer', { class: 'ledger-foot' }, explain, post, closeButton()),
    ),
  );
}

boot(async () => {
  const ctx = (await view.getContext()) as { extension?: { sprint?: { id?: string | number } } };
  const sprintId = ctx.extension?.sprint?.id;
  const report = (await invoke('sprint-scope', sprintId != null ? { sprintId: String(sprintId) } : {})) as Report;
  if (report.error) throw new Error(report.error);
  if (report.notStarted) {
    mount(h('main', { class: 'ledger' }, h('p', { 'data-testid': 'not-started', class: 'state' }, `${report.sprint.name} has not started yet.`), h('footer', { class: 'ledger-foot' }, closeButton())));
    return;
  }
  renderLedger(report);
});
