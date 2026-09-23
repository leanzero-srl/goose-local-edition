import { AgentResults } from './AgentResults';
import { useCallback, useEffect, useState } from 'react';
import { Bot, FolderPlus, Plus, RefreshCw, Send } from 'lucide-react';
import { MainPanelLayout } from '../Layout/MainPanelLayout';
import { ActivityDisclosure } from '../activity/ActivityDisclosure';
import {
  Button,
  Chip,
  EmptyState,
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
import { DeskHero } from './DeskHero';
import { TickAnatomy } from './TickAnatomy';
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
  const queryTick = parseTick(query.get('tick'));
  const wantsNew = query.get('new') === '1';
  const [selected, setSelected] = useState<string | null>(queryDesk);
  const [creating, setCreating] = useState(false);
  const [removing, setRemoving] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  // `?tick=N` names a tick of the desk the URL names; on the fallback desk it means nothing.
  const viewTick = queryDesk && queryDesk === selected ? queryTick : null;
  const desk = useDesk(selected, viewTick);

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
        <div className={cx('border-b px-lz-page pb-4 pt-5', SURFACE.hairline)}>
          <PageHeader
            className="page-transition"
            title="Agent Work"
            subtitle={
              <span>Recurring assignments: what each agent found, and what it needs from you.</span>
            }
            actions={
              <div className="flex flex-wrap items-center justify-end gap-2">
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
        {/* The scroller is a BLOCK, not a flex column: a flex column shrinks its overflow-hidden
            children (every Panel) to fit, and the desk's top card collapsed to its border. */}
        <main className="min-h-0 flex-1 overflow-auto" data-testid="agent-work-scroll">
          <div className="mx-auto flex w-full min-w-0 max-w-[1120px] flex-col gap-8 px-lz-page py-6">
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
                onOpenTick={(t) => openTick(selected, t)}
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
          </div>
        </main>
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

function parseTick(raw: string | null): number | null {
  return raw != null && /^\d+$/.test(raw) ? Number(raw) : null;
}

/** Opening a tick is a URL change (`?desk=…&tick=N`) — the sidebar follows it; null is "latest". */
function openTick(dir: string, tick: number | null) {
  const params = new URLSearchParams({ desk: dir });
  if (tick != null) params.set('tick', String(tick));
  window.location.hash = `#/agent-work?${params.toString()}`;
}

function Desk({
  dir,
  model,
  read,
  busy,
  onOpenTick,
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
  onOpenTick: (tick: number | null) => void;
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
  const m = read.manifest;
  const title = m?.title || m?.name || dir;
  const hasTicks = model.tick > 0 || model.ticks.length > 0;
  return (
    <>
      <DeskHero
        model={model}
        title={title}
        schedule={scheduleLine(m)}
        dir={dir}
        plannerModel={read.state?.planner_model ?? ''}
        controls={{ busy, onStart, onRunOnce, onStop, onTickNow, onPause }}
        onRemove={onRemove}
        onReviewNeeds={() =>
          document
            .getElementById('agent-work-needs-you')
            ?.scrollIntoView({ behavior: 'smooth', block: 'start' })
        }
      />
      <NeedsYou
        model={model}
        onDecide={onDecide}
        requiresApproval={(m?.post?.approval ?? 'human') === 'human'}
        hasPostCommand={Boolean(m?.post?.command)}
      />
      {hasTicks && <TickAnatomy model={model} onOpenTick={onOpenTick} />}
      <AgentResults model={model} />
      <LaneBoard model={model} dir={dir} selected={lane} onSelect={setLane} />
      <LedgerPanel model={model} read={read} onOpenTick={(t) => onOpenTick(t)} />
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
        <NoteBox busy={busy} onNote={onNote} />
        <NodesPanel model={model} />
      </div>
      {read.engineLog.trim() && (
        <div className="rounded-lz-card border border-lz-border" data-testid="engine-log">
          <ActivityDisclosure label="Engine log">
            <pre
              className={cx(
                TYPE.mono,
                'max-h-64 overflow-auto whitespace-pre-wrap break-words px-4 pb-3'
              )}
            >
              {read.engineLog.trim().split('\n').slice(-40).join('\n')}
            </pre>
          </ActivityDisclosure>
        </div>
      )}
    </>
  );
}

function NoteBox({ busy, onNote }: { busy: boolean; onNote: (t: string) => Promise<void> }) {
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
  return (
    <Panel title="Tell the desk" padded>
      <p className={cx(TYPE.bodyMuted, 'mb-2')}>
        A note the orchestrator reads at its next tick: a hold, a steer, a fact it lacks.
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
          variant="secondary"
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
  );
}

function NodesPanel({ model }: { model: DeskModel }) {
  return (
    <Panel title="Nodes" count={model.nodes.length} padded={false}>
      {model.nodes.length === 0 ? (
        <p className={cx(TYPE.bodyMuted, 'p-4')}>
          {model.liveness === 'stopped'
            ? 'The fleet is resolved when the desk starts.'
            : 'No node resolved. Read the engine log below.'}
        </p>
      ) : (
        <ul className="divide-y divide-lz-border" data-testid="node-list">
          {model.nodes.map((n) => (
            <li key={n.id} className="flex min-w-0 flex-col gap-1.5 px-4 py-2.5">
              <div className="flex min-w-0 flex-wrap items-center gap-2">
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
                  <span className={WEIGHT.medium}>
                    {l.kind === 'lens' ? `${l.laneId} · ${l.lens} lens` : l.item || l.laneId}
                  </span>{' '}
                  — {l.liveLine || '…'}
                </div>
              ))}
            </li>
          ))}
        </ul>
      )}
    </Panel>
  );
}
