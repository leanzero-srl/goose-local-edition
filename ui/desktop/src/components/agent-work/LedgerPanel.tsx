import { useState } from 'react';
import { DataTable, EmptyState, Panel, Segmented, TNUM, TYPE, WEIGHT, cx, type DataTableColumn, type SegmentedOption } from '../lz';
import { fmtDuration, type AgentWorkRead, type DeskModel, type PreparedRow, type TickRecord } from './agentWorkModel';

type Tab = 'ticks' | 'facts' | 'drafts' | 'scratchpad' | 'pending' | 'log';

/**
 * The snowball, readable: what every tick delivered (cost in lane-minutes beside it — the value
 * gate's row), the facts the orchestrator carries forward, every draft's fate, and the three files
 * the desk keeps for the human (scratchpad, pending, daily log).
 */
export function LedgerPanel({ model, read }: { model: DeskModel; read: AgentWorkRead }) {
  const [tab, setTab] = useState<Tab>('ticks');
  const tabs: SegmentedOption<Tab>[] = [
    { value: 'ticks', label: `Ticks (${model.totals.ticks})` },
    { value: 'facts', label: `Facts (${model.facts.length})` },
    { value: 'drafts', label: `Drafts (${read.prepared.length})` },
    { value: 'scratchpad', label: 'Scratchpad' },
    { value: 'pending', label: 'Pending' },
    { value: 'log', label: 'Daily log' },
  ];
  return (
    <Panel
      title="Ledger"
      headerRight={<Segmented size="sm" options={tabs} value={tab} onChange={setTab} aria-label="Ledger sections" />}
      padded={tab !== 'ticks' && tab !== 'drafts'}
    >
      {tab === 'ticks' && <TicksTable ticks={model.ticks} />}
      {tab === 'facts' && (
        model.facts.length === 0 ? (
          <EmptyState title="No facts recorded yet" body="Synthesis writes what the desk must remember: measured things with identifiers, decisions, dead ends." />
        ) : (
          <ul className="flex flex-col gap-1.5" data-testid="facts-list">
            {model.facts.map((f, i) => (
              <li key={i} className={cx(TYPE.body, 'flex gap-3')}>
                <span className={cx(TYPE.meta, TNUM, 'w-10 shrink-0')}>t{f.tick}</span>
                <span>{f.fact}</span>
              </li>
            ))}
          </ul>
        )
      )}
      {tab === 'drafts' && <DraftsTable rows={read.prepared} />}
      {tab === 'scratchpad' && <FileText text={read.scratchpad} empty="The scratchpad is empty — the orchestrator rewrites it every tick." />}
      {tab === 'pending' && <FileText text={read.pending} empty="Nothing pending." />}
      {tab === 'log' && <FileText text={read.dailyLog} empty="No daily log lines yet." />}
    </Panel>
  );
}

function FileText({ text, empty }: { text: string; empty: string }) {
  return text.trim() ? <pre className={cx(TYPE.mono, 'max-h-[40vh] overflow-auto whitespace-pre-wrap break-words')}>{text}</pre> : <p className={TYPE.bodyMuted}>{empty}</p>;
}

function TicksTable({ ticks }: { ticks: TickRecord[] }) {
  const columns: DataTableColumn<TickRecord>[] = [
    { key: 'tick', header: '#', cell: (t) => t.tick, numeric: true, width: 48 },
    { key: 'outcome', header: 'Outcome', cell: (t) => <span className={cx(WEIGHT.semibold, t.outcome === 'done' ? 'text-lz-ok' : t.outcome === 'held' ? 'text-lz-warn' : 'text-lz-err')}>{t.outcome ?? '—'}</span>, width: 80 },
    { key: 'summary', header: 'What happened', cell: (t) => <span className="line-clamp-2">{t.summary ?? ''}</span> },
    { key: 'lanes', header: 'Lanes', cell: (t) => t.lanes?.length ?? 0, numeric: true, width: 64 },
    { key: 'staged', header: 'Staged', cell: (t) => t.synthesis?.staged.length ?? 0, numeric: true, width: 64 },
    { key: 'posted', header: 'Posted', cell: (t) => t.posted?.length ?? 0, numeric: true, width: 64 },
    { key: 'cost', header: 'Lane-min', cell: (t) => (t.lane_secs != null ? (t.lane_secs / 60).toFixed(1) : '—'), numeric: true, width: 80 },
    { key: 'wall', header: 'Wall', cell: (t) => (t.wall_secs != null ? fmtDuration(t.wall_secs * 1000) : '—'), numeric: true, width: 80 },
  ];
  if (ticks.length === 0) return <div className="p-4"><EmptyState title="No ticks yet" body="Each finished tick lands here with its cost beside what it delivered." /></div>;
  return <DataTable columns={columns} rows={ticks} rowKey={(t) => String(t.tick)} dense />;
}

function DraftsTable({ rows }: { rows: PreparedRow[] }) {
  const columns: DataTableColumn<PreparedRow>[] = [
    { key: 'id', header: 'Draft', cell: (r) => <span className={TNUM}>{r.id}</span> },
    { key: 'target', header: 'Target', cell: (r) => `${r.kind} → ${r.target}` },
    { key: 'status', header: 'Status', cell: (r) => <span className={cx(WEIGHT.semibold, r.status === 'posted' ? 'text-lz-ok' : r.status === 'failed' || r.status === 'declined' ? 'text-lz-err' : 'text-lz-accent')}>{r.status}</span>, width: 90 },
    { key: 'tick', header: 'Tick', cell: (r) => r.tick, numeric: true, width: 56 },
    { key: 'posted', header: 'Posted tick', cell: (r) => r.posted_tick ?? '—', numeric: true, width: 90 },
    { key: 'chars', header: 'Chars', cell: (r) => r.body.length, numeric: true, width: 64 },
  ];
  if (rows.length === 0) return <div className="p-4"><EmptyState title="No drafts yet" body="A draft the review let through is staged here, then posted on a later tick." /></div>;
  return <DataTable columns={columns} rows={[...rows].sort((a, b) => b.tick - a.tick)} rowKey={(r) => r.id} dense />;
}
