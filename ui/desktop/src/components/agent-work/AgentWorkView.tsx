import { AgentResults } from './AgentResults';
import { useCallback, useEffect, useState } from 'react';
import { Bot, FolderPlus, Plus, RefreshCw, Send, Trash2 } from 'lucide-react';
import { MainPanelLayout } from '../Layout/MainPanelLayout';
import {
  Button,
  Chip,
  EmptyState,
  KeyValue,
  PageHeader,
  Panel,
  SURFACE,
  TNUM,
  TYPE,
  WEIGHT,
  cx,
  nodeClasses,
} from '../lz';
import { ConfirmationModal } from '../ui/ConfirmationModal';
import { scheduleLine, type DeskModel } from './agentWorkModel';
import { useAgentRoster, useDesk } from './useAgentWork';
import { TickClock } from './TickClock';
import { LaneBoard, nodeIndexOf } from './LaneBoard';
import { NeedsYou } from './NeedsYou';
import { LedgerPanel } from './LedgerPanel';
import { NewAgentDialog } from './NewAgentDialog';
import { useHashQuery } from '../Layout/useHashQuery';

/**
 * AGENT WORK — the second swarm operation. Not a build: a desk that ticks. The values here are the
 * desk's values: when the next tick is, what each agent is doing right now (its words), what waits
 * on the human, what the ledger has snowballed, and what every tick cost in lane-minutes. The nodes
 * are shown for occupancy, not celebrated.
 */
export default function AgentWorkView() {
  const roster = useAgentRoster();
  // The sidebar's Agent Work tree is THE roster; this view opens the desk the URL names
  // (`?desk=…`, `?tick=…`, `?new=1`) and falls back to the first desk when it names none.
  const query = useHashQuery();
  const queryDesk = query.get('desk');
  const wantsNew = query.get('new') === '1';
  const [selected, setSelected] = useState<string | null>(queryDesk);
  const [creating, setCreating] = useState(false);
  const [removing, setRemoving] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const desk = useDesk(selected);

  useEffect(() => {
    if (queryDesk) setSelected(queryDesk);
  }, [queryDesk]);
  useEffect(() => {
    if (wantsNew) setCreating(true);
  }, [wantsNew]);
  useEffect(() => {
    if (!selected && roster.rows.length > 0) setSelected(roster.rows[0].dir);
    if (selected && roster.loaded && !roster.rows.some((r) => r.dir === selected))
      setSelected(roster.rows[0]?.dir ?? null);
  }, [roster.rows, roster.loaded, selected]);

  const act = useCallback(
    async (fn: () => Promise<{ ok: boolean; error?: string } | boolean>) => {
      setBusy(true);
      setNotice(null);
      try {
        const r = await fn();
        if (r === false || (typeof r === 'object' && !r.ok))
          throw new Error(
            typeof r === 'object'
              ? (r.error ?? 'The action could not be completed.')
              : 'The action could not be completed.'
          );
        await desk.refresh();
        await roster.refresh();
      } catch (e) {
        setNotice(e instanceof Error ? e.message : String(e));
        throw e;
      } finally {
        setBusy(false);
      }
    },
    [desk, roster]
  );

  const addExisting = async () => {
    const d = await window.electron.agentWorkPickDir();
    if (!d) return;
    await act(() => window.electron.agentWorkAdd(d));
    setSelected(d);
  };

  return (
    <MainPanelLayout>
      <div className={cx('flex min-h-0 flex-1 flex-col', SURFACE.page)}>
        <div className={cx('border-b px-lz-page pb-5 pt-6', SURFACE.hairline)}>
          <PageHeader
            className="page-transition"
            title="Agent Work"
            subtitle={
              <span>
                Give agents recurring assignments. Follow their research, read the results, and
                review decisions in one workspace.
              </span>
            }
            actions={
              <div className="flex items-center gap-2">
                <Button
                  variant="ghost"
                  icon={<RefreshCw />}
                  onClick={async () => {
                    await Promise.all([roster.refresh(), desk.refresh()]);
                  }}
                  disabled={busy}
                >
                  Refresh
                </Button>
                <Button
                  variant="secondary"
                  icon={<FolderPlus />}
                  onClick={addExisting}
                  disabled={busy}
                >
                  Add existing
                </Button>
                <Button
                  variant="primary"
                  icon={<Plus />}
                  onClick={() => setCreating(true)}
                  disabled={busy}
                >
                  New agent
                </Button>
              </div>
            }
          />
        </div>
        <div className="flex min-h-0 flex-1 flex-col">
          <main className="flex min-h-0 flex-col gap-4 overflow-auto p-lz-page">
            {roster.error && (
              <p role="alert" className="text-lz-err">
                {roster.error}
              </p>
            )}
            {notice && (
              <div
                className={cx(
                  'rounded-lz-control bg-lz-err-solid px-3 py-2 text-lz-body text-white',
                  WEIGHT.medium
                )}
                role="alert"
              >
                {notice}
              </div>
            )}
            {!selected || !desk.model || !desk.read ? (
              selected ? (
                <p className={TYPE.bodyMuted}>{desk.error ?? 'reading the desk…'}</p>
              ) : roster.loaded ? (
                <EmptyState
                  icon={<Bot />}
                  title="No agents yet"
                  body="Create one, or add a directory that already has an agent.yaml. Your agents and their ticks are listed in the sidebar."
                />
              ) : null
            ) : (
              <Desk
                key={selected}
                dir={selected}
                model={desk.model}
                read={desk.read}
                busy={busy}
                onStart={() => act(() => window.electron.agentWorkStart(selected, false))}
                onRunOnce={() => act(() => window.electron.agentWorkStart(selected, true))}
                onStop={() => act(() => window.electron.agentWorkStop(selected, true))}
                onTickNow={() => act(() => window.electron.agentWorkTickNow(selected))}
                onPause={(p) => act(() => window.electron.agentWorkSetPaused(selected, p))}
                onNote={(t) => act(() => window.electron.agentWorkNote(selected, t))}
                onDecide={async (id, decision, text) => {
                  await act(() => window.electron.agentWorkDecide(selected, id, decision, text));
                }}
                onRemove={() => setRemoving(selected)}
              />
            )}
          </main>
        </div>
      </div>
      {creating && (
        <NewAgentDialog
          onClose={() => setCreating(false)}
          onCreated={async (d) => {
            setCreating(false);
            await roster.refresh();
            setSelected(d);
          }}
        />
      )}
      <ConfirmationModal
        isOpen={removing != null}
        title="Remove this agent from the roster?"
        message="The directory and everything in it stay on disk. Only the roster entry goes."
        confirmLabel="Remove"
        onCancel={() => setRemoving(null)}
        onConfirm={async () => {
          const d = removing;
          setRemoving(null);
          if (d) await act(() => window.electron.agentWorkRemove(d));
        }}
      />
    </MainPanelLayout>
  );
}

function Desk({
  dir,
  model,
  read,
  busy,
  onStart,
  onRunOnce,
  onStop,
  onTickNow,
  onPause,
  onNote,
  onDecide,
  onRemove,
}: {
  dir: string;
  model: DeskModel;
  read: NonNullable<ReturnType<typeof useDesk>['read']>;
  busy: boolean;
  onStart: () => void;
  onRunOnce: () => void;
  onStop: () => void;
  onTickNow: () => void;
  onPause: (p: boolean) => void;
  onNote: (t: string) => Promise<void>;
  onDecide: (id: string, decision: string, text: string) => Promise<void>;
  onRemove: () => void;
}) {
  const [lane, setLane] = useState<string | null>(null);
  const [noteError, setNoteError] = useState('');
  const [note, setNote] = useState('');
  const sendNote = async () => {
    if (busy || !note.trim()) return;
    setNoteError('');
    try {
      await onNote(note.trim());
      setNote('');
    } catch (e) {
      setNoteError(e instanceof Error ? e.message : String(e));
    }
  };
  const m = read.manifest;
  const title = m?.title || m?.name || dir;
  return (
    <>
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <h2 className={TYPE.h1}>{title}</h2>
          <p className={cx(TYPE.meta, 'mt-1')}>
            {scheduleLine(m)} · {dir}
          </p>
        </div>
        <Button variant="ghost" size="sm" icon={<Trash2 />} onClick={onRemove} disabled={busy}>
          Remove from roster
        </Button>
      </div>
      <Panel padded>
        <TickClock
          model={model}
          busy={busy}
          onStart={onStart}
          onRunOnce={onRunOnce}
          onStop={onStop}
          onTickNow={onTickNow}
          onPause={onPause}
        />
      </Panel>
      <div className="grid grid-cols-1 2xl:grid-cols-[minmax(0,1fr)_340px] gap-4">
        <div className="flex min-h-0 flex-col gap-4">
          <AgentResults model={model} />
          <LaneBoard model={model} dir={dir} selected={lane} onSelect={setLane} />
          <LedgerPanel model={model} read={read} />
        </div>
        <div className="flex flex-col gap-4">
          <NeedsYou
            model={model}
            onDecide={onDecide}
            requiresApproval={(m?.post?.approval ?? 'human') === 'human'}
            hasPostCommand={Boolean(m?.post?.command)}
          />
          <Panel title="Nodes" count={model.nodes.length} padded={false}>
            {model.nodes.length === 0 ? (
              <p className={cx(TYPE.bodyMuted, 'p-4')}>
                {model.liveness === 'stopped'
                  ? 'The fleet is resolved when the desk starts.'
                  : 'No node resolved — read the engine log below.'}
              </p>
            ) : (
              <ul className="divide-y divide-lz-border" data-testid="node-list">
                {model.nodes.map((n) => (
                  <li key={n.id} className="flex flex-col gap-1 px-4 py-2.5">
                    <div className="flex items-center gap-2">
                      <Chip node={nodeIndexOf(model, n.model_id)}>{n.model_id}</Chip>
                      {n.supervision && <Chip tone="secondary">orchestrator</Chip>}
                      <span className={cx(TYPE.meta, TNUM, 'ml-auto')}>
                        {n.running.length}/{n.weight} busy
                      </span>
                    </div>
                    <div className="flex gap-1">
                      {Array.from({ length: n.weight }).map((_, i) => (
                        <span
                          key={i}
                          className={cx(
                            'h-2 flex-1 rounded-lz-pill',
                            i < n.running.length
                              ? nodeClasses(nodeIndexOf(model, n.model_id), 'dot')
                              : 'bg-lz-surface-2'
                          )}
                        />
                      ))}
                    </div>
                    {n.running.map((l) => (
                      <div key={l.key} className={cx(TYPE.meta, 'truncate')}>
                        {l.kind === 'lens' ? `${l.laneId} · ${l.lens} lens` : l.item || l.laneId} —{' '}
                        {l.liveLine || '…'}
                      </div>
                    ))}
                  </li>
                ))}
              </ul>
            )}
          </Panel>
          <Panel title="Tell the desk" padded>
            <p className={cx(TYPE.bodyMuted, 'mb-2')}>
              A note the orchestrator reads at its next tick — a hold, a steer, a fact it lacks.
            </p>
            <div className="flex gap-2">
              <input
                value={note}
                onChange={(e) => setNote(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') void sendNote();
                }}
                aria-label="Note to the desk"
                placeholder="Focus the next run on the sources I shared"
                className="h-8 min-w-0 flex-1 rounded-lz-control border border-lz-border-strong bg-lz-surface px-2 text-lz-body text-lz-ink"
              />
              <Button
                variant="primary"
                size="sm"
                icon={<Send />}
                disabled={busy || !note.trim()}
                onClick={sendNote}
              >
                Send
              </Button>
            </div>
            {noteError && (
              <p role="alert" className="mt-2 text-sm text-lz-err">
                {noteError}
              </p>
            )}
          </Panel>
          <Panel title="This desk" padded>
            <KeyValue
              dense
              items={[
                { key: 'ticks', label: 'Ticks', value: String(model.totals.ticks) },
                { key: 'lanes', label: 'Lanes run', value: String(model.totals.lanes) },
                { key: 'staged', label: 'Drafts staged', value: String(model.totals.staged) },
                {
                  key: 'posted',
                  label: 'Posted',
                  value: String(model.totals.posted),
                  tone: model.totals.posted > 0 ? 'ok' : undefined,
                },
                { key: 'asks', label: 'Asks raised', value: String(model.totals.asks) },
                { key: 'cost', label: 'Lane-minutes', value: model.totals.laneMinutes.toFixed(1) },
                {
                  key: 'planner',
                  label: 'Orchestrator model',
                  value: read.state?.planner_model || '—',
                  mono: true,
                },
                {
                  key: 'last',
                  label: 'Last tick',
                  value: model.lastTick
                    ? `#${model.lastTick.tick} ${model.lastTick.outcome} — ${model.lastTick.summary}`
                    : '—',
                },
              ]}
            />
          </Panel>
          {read.engineLog.trim() && (
            <Panel title="Engine log" padded>
              <pre
                className={cx(TYPE.mono, 'max-h-48 overflow-auto whitespace-pre-wrap break-words')}
              >
                {read.engineLog.trim().split('\n').slice(-40).join('\n')}
              </pre>
            </Panel>
          )}
        </div>
      </div>
    </>
  );
}
