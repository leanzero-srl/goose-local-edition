import React, { useEffect, useMemo, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { router, showFlag, view } from '@forge/bridge';
import { boot, call, formatInstant } from '../../shared/bridge';
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
];

const isNumeric = (s) => /^\d+$/.test(s);
const compareIds = (a, b) => {
  if (isNumeric(a) && isNumeric(b)) return a.length - b.length || (a < b ? -1 : a > b ? 1 : 0);
  return a < b ? -1 : a > b ? 1 : 0;
};
const byTime = (a, b) => Date.parse(a.at) - Date.parse(b.at) || compareIds(a.changeId, b.changeId);

function sortRows(rows, sort) {
  const list = [...rows];
  if (sort.col === 'points') return list.sort((a, b) => b.points - a.points || byTime(a, b));
  list.sort(byTime);
  return sort.dir === 'descending' ? list.reverse() : list;
}

let flagSeq = 0;
const flag = (type, title, description) =>
  showFlag({ id: `scope-ledger-${Date.now()}-${(flagSeq += 1)}`, title, description, type, appearance: type, isAutoDismiss: true });

function Ledger({ data, context }) {
  const [sort, setSort] = useState({ col: 'at', dir: 'ascending' });
  const [selected, setSelected] = useState(null);
  const [posting, setPosting] = useState(false);
  const inFlight = useRef(false);
  const rows = useMemo(() => sortRows(data.changes, sort), [data.changes, sort]);
  const selectedRow = data.changes.find((c) => c.changeId === selected) ?? null;

  const sortBy = (col) => {
    if (col === 'at') setSort((s) => ({ col: 'at', dir: s.col === 'at' && s.dir === 'ascending' ? 'descending' : 'ascending' }));
    if (col === 'points') setSort({ col: 'points', dir: 'descending' });
  };

  const openIssue = (e, key) => {
    e.preventDefault();
    e.stopPropagation();
    router.open(`/browse/${key}`).catch((err) => flag('error', `Could not open ${key}`, err.message));
  };

  // One click (or a double click) is one comment: a second click while the first is in flight is ignored.
  const post = async () => {
    if (inFlight.current) return;
    if (!selectedRow) {
      flag('info', 'Select a change first', 'Click a row of the ledger, then post its summary.');
      return;
    }
    inFlight.current = true;
    setPosting(true);
    try {
      const res = await call('postSummary', { changeId: selectedRow.changeId });
      if (res?.ok) flag('success', `Summary posted on ${res.issueKey}`, `Scope creep ${data.text.creep} in ${data.sprint.name}.`);
      else flag('error', 'The summary was not posted', res?.error ?? 'Unknown error.');
    } catch (e) {
      flag('error', 'The summary was not posted', e?.message ?? String(e));
    } finally {
      inFlight.current = false;
      setPosting(false);
    }
  };

  const ariaSort = (col) => (sort.col === col ? sort.dir : undefined);

  return (
    <>
      <div className="metrics-host">
      <dl className="metrics">
        {METRICS.map(([k, label]) => (
          <div key={k} className={`metric metric-${k}`}>
            <dt>{label}</dt>
            <dd data-metric={k}>{data.text[k]}</dd>
          </div>
        ))}
      </dl>
      </div>
      <p className="hidden-line">
        <span data-testid="hidden-count" className="hidden-count">
          {data.hiddenCount}
        </span>{' '}
        {data.hiddenCount === 1 ? 'change is' : 'changes are'} on issues you cannot browse and not listed.
      </p>
      <div className="table-wrap">
        <table data-testid="ledger" className="ledger">
          <caption className="visually-hidden">Changes to the sprint after it started</caption>
          <thead>
            <tr>
              {COLUMNS.map(([col, label]) => {
                const sortable = col === 'at' || col === 'points';
                return (
                  <th key={col} data-col={col} scope="col" aria-sort={ariaSort(col)} className={sortable ? 'sortable' : undefined} onClick={sortable ? () => sortBy(col) : undefined}>
                    {sortable ? (
                      <button type="button" className="sort-button">
                        {label}
                        <span className="sort-mark" aria-hidden="true">
                          {sort.col === col ? (sort.dir === 'ascending' ? ' ▲' : ' ▼') : ''}
                        </span>
                      </button>
                    ) : (
                      label
                    )}
                  </th>
                );
              })}
            </tr>
          </thead>
          <tbody>
            {rows.length === 0 && (
              <tr>
                <td colSpan={COLUMNS.length} className="empty">
                  No visible changes since the sprint started.
                </td>
              </tr>
            )}
            {rows.map((r) => (
              <tr
                key={r.changeId}
                data-change-id={r.changeId}
                aria-selected={selected === r.changeId ? 'true' : 'false'}
                tabIndex={0}
                className={selected === r.changeId ? 'selected' : undefined}
                onClick={() => setSelected(r.changeId)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' || e.key === ' ') {
                    e.preventDefault();
                    setSelected(r.changeId);
                  }
                }}
              >
                <td data-col="issue">
                  <a className="link" href={`/browse/${r.issueKey}`} onClick={(e) => openIssue(e, r.issueKey)}>
                    {r.issueKey}
                  </a>
                </td>
                <td data-col="points" className="num">
                  {r.pointsText}
                </td>
                <td data-col="kind">
                  <span className={`kind kind-${r.kind}`}>{r.kind}</span>
                </td>
                <td data-col="by">{r.by}</td>
                <td data-col="at">
                  <time dateTime={r.at}>{formatInstant(r.at, context)}</time>
                </td>
                <td data-col="source">{r.source}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <div className="actions">
        <span className="subtle selection">{selectedRow ? `Selected: ${selectedRow.issueKey} (${selectedRow.kind})` : 'Select a change to comment on its issue.'}</span>
        <button type="button" data-testid="post-summary" className="button primary" aria-busy={posting ? 'true' : 'false'} onClick={post}>
          {posting ? 'Posting…' : 'Post summary comment'}
        </button>
        <button type="button" data-testid="close" className="button" onClick={() => view.close()}>
          Close
        </button>
      </div>
    </>
  );
}

function SprintAction({ context }) {
  const [state, setState] = useState({ phase: 'loading' });
  useEffect(() => {
    call('sprintLedger')
      .then((data) => setState(data.error ? { phase: 'error', message: data.error } : { phase: data.notStarted ? 'not-started' : 'ready', data }))
      .catch((e) => setState({ phase: 'error', message: e?.message ?? String(e) }));
  }, []);

  if (state.phase === 'not-started') {
    return (
      <div data-testid="sprint-ledger" className="sprint-action">
        <p data-testid="not-started" className="message">
          {state.data.sprint.name} has not started yet, so nothing has entered or left it.
        </p>
      </div>
    );
  }
  return (
    <div data-testid="sprint-ledger" className="sprint-action">
      <h2>{state.phase === 'ready' ? `Scope ledger · ${state.data.sprint.name}` : 'Scope ledger'}</h2>
      {state.phase === 'loading' && <p className="subtle">Loading the sprint's scope changes…</p>}
      {state.phase === 'error' && (
        <>
          <p className="message error" role="alert">
            {state.message}
          </p>
          <div className="actions">
            <button type="button" data-testid="close" className="button" onClick={() => view.close()}>
              Close
            </button>
          </div>
        </>
      )}
      {state.phase === 'ready' && <Ledger data={state.data} context={context} />}
    </div>
  );
}

boot((context) => createRoot(document.getElementById('root')).render(<SprintAction context={context} />));
