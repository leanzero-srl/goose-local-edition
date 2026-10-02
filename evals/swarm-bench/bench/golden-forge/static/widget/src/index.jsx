import React, { useCallback, useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { events, view } from '@forge/bridge';
import { boot, call } from '../../shared/bridge';
import '../../shared/base.css';
import './widget.css';

const SERIES = ['committed', 'added', 'removed'];
const LABELS = { committed: 'Committed', added: 'Added', removed: 'Removed', creep: 'Creep' };

// Every bar on one linear scale from 0. The SVG stretches to the widget's width
// (preserveAspectRatio="none") and keeps a fixed height, so a bar's rendered height stays
// proportional to its number at any width.
const CHART_HEIGHT = 160;
const BAR = 18;
const BAR_GAP = 4;
const GROUP_GAP = 24;

function Chart({ sprints }) {
  const max = Math.max(0, ...sprints.flatMap((s) => SERIES.map((k) => s.values[k])));
  const groupWidth = SERIES.length * BAR + (SERIES.length - 1) * BAR_GAP;
  const width = Math.max(1, sprints.length * groupWidth + (sprints.length + 1) * GROUP_GAP);
  return (
    <figure className="chart">
      <svg
        data-testid="chart"
        className="chart-svg"
        viewBox={`0 0 ${width} ${CHART_HEIGHT}`}
        preserveAspectRatio="none"
        role="img"
        aria-label="Committed, added and removed points per sprint"
      >
        <line className="chart-axis" x1="0" x2={width} y1={CHART_HEIGHT} y2={CHART_HEIGHT} vectorEffect="non-scaling-stroke" />
        {sprints.map((s, i) =>
          SERIES.map((k, j) => {
            const h = max > 0 ? (s.values[k] / max) * CHART_HEIGHT : 0;
            const x = GROUP_GAP + i * (groupWidth + GROUP_GAP) + j * (BAR + BAR_GAP);
            return (
              <rect key={`${s.id}-${k}`} data-sprint-id={s.id} data-series={k} className={`bar series-${k}`} x={x} y={CHART_HEIGHT - h} width={BAR} height={h}>
                <title>{`${s.name}: ${LABELS[k]} ${s.text[k]}`}</title>
              </rect>
            );
          }),
        )}
      </svg>
      <figcaption className="legend">
        {SERIES.map((k) => (
          <span key={k} className="legend-item">
            <span className={`swatch series-${k}`} aria-hidden="true" />
            {LABELS[k]}
          </span>
        ))}
      </figcaption>
      <p className="chart-caption">Bars, left to right: {sprints.map((s) => s.name).join(' · ')}</p>
    </figure>
  );
}

function Sprint({ sprint }) {
  return (
    <section className="sprint" data-testid="sprint" data-sprint-id={sprint.id}>
      <h3 className="sprint-name">{sprint.name}</h3>
      <div className="metrics-host">
      <dl className="metrics">
        {['committed', 'added', 'removed', 'creep'].map((k) => (
          <div key={k} className={`metric metric-${k}`}>
            <dt>{LABELS[k]}</dt>
            <dd data-metric={k}>{sprint.text[k]}</dd>
          </div>
        ))}
      </dl>
      </div>
    </section>
  );
}

function Widget({ initialContext }) {
  const [context, setContext] = useState(initialContext);
  const [state, setState] = useState({ phase: 'loading' });
  const boardId = context?.extension?.config?.boardId;

  const load = useCallback(async () => {
    if (boardId === undefined || boardId === null || boardId === '') return setState({ phase: 'needs-config' });
    setState((s) => (s.phase === 'ready' ? s : { phase: 'loading' }));
    try {
      const data = await call('widget', { boardId: String(boardId) });
      if (data.needsConfig) return setState({ phase: 'needs-config' });
      setState({ phase: 'ready', data });
    } catch (e) {
      setState({ phase: 'error', message: e?.message ?? String(e) });
    }
  }, [boardId]);

  useEffect(() => {
    load();
  }, [load]);

  // The dashboard tells an open widget when its saved config changes; re-read the context then.
  useEffect(() => {
    // A host without this event leaves the widget on the config it was opened with.
    let sub;
    try {
      sub = Promise.resolve(events.on('FORGE_DASHBOARDS_WIDGET_CONFIG_CHANGED', async () => setContext(await view.getContext()))).catch(() => null);
    } catch {
      sub = Promise.resolve(null);
    }
    return () => {
      sub.then((s) => s?.unsubscribe());
    };
  }, []);

  if (state.phase === 'needs-config') {
    return (
      <div data-testid="scope-widget" className="widget">
        <p data-testid="needs-config" className="message">
          Choose a scrum board in this widget's settings to see its sprint scope.
        </p>
      </div>
    );
  }
  return (
    <div data-testid="scope-widget" className="widget">
      {state.phase === 'loading' && <p className="subtle">Loading sprint scope…</p>}
      {state.phase === 'error' && (
        <p className="message error" role="alert">
          Could not load sprint scope: {state.message}
        </p>
      )}
      {state.phase === 'ready' && state.data.sprints.length === 0 && <p className="message">This board has no active sprint.</p>}
      {state.phase === 'ready' && state.data.sprints.length > 0 && (
        <>
          <div className="sprints">
            {state.data.sprints.map((s) => (
              <Sprint key={s.id} sprint={s} />
            ))}
          </div>
          <Chart sprints={state.data.sprints} />
        </>
      )}
    </div>
  );
}

boot((context) => createRoot(document.getElementById('root')).render(<Widget initialContext={context} />));
