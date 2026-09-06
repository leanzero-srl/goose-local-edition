import { useEffect, useState, type ReactElement } from 'react';
import { Bot, Eye, FileText, Search, Sparkles, X } from 'lucide-react';
import { Button, Chip, EmptyState, NODE_INDEXES, Panel, StatusDot, TNUM, TYPE, WEIGHT, cx, nodeClasses, type NodeIndex, type Tone } from '../lz';
import { fmtDuration, type DeskLane, type DeskModel } from './agentWorkModel';

const KIND_LABEL: Record<DeskLane['kind'], string> = {
  orient: 'Orchestrator',
  lane: 'Surgeon',
  lens: 'Reviewer',
  synthesis: 'Synthesis',
};

const KIND_ICON: Record<DeskLane['kind'], ReactElement> = {
  orient: <Sparkles />,
  lane: <Search />,
  lens: <Eye />,
  synthesis: <FileText />,
};

const STATUS_TONE: Record<DeskLane['status'], Tone> = {
  queued: 'warn',
  running: 'ok',
  done: 'accent',
  failed: 'err',
};

/** A node's hue is its position in the desk's device list — the same ramp everywhere in this view. */
export function nodeIndexOf(model: DeskModel, modelId: string): NodeIndex {
  const i = Math.max(0, model.nodes.findIndex((n) => n.model_id === modelId));
  return NODE_INDEXES[i % NODE_INDEXES.length];
}

/**
 * The lanes of the CURRENT tick: the orchestrator's orient call, every surgeon lane, every reviewer,
 * the synthesis — each with the node it runs on, its status and its live line (the words, not a
 * shape). Click one to open the inspector with the durable logs.
 */
export function LaneBoard({
  model,
  dir,
  selected,
  onSelect,
}: {
  model: DeskModel;
  dir: string;
  selected: string | null;
  onSelect: (key: string | null) => void;
}) {
  const running = model.lanes.filter((l) => l.status === 'running').length;
  return (
    <Panel
      title={`Tick ${model.tick} lanes`}
      count={model.lanes.length}
      headerRight={
        <span className={cx(TYPE.meta, TNUM)}>
          {running} running · {model.queue.length} queued · {model.nodes.reduce((n, d) => n + d.free, 0)} free slots
        </span>
      }
      padded={false}
      className="min-h-0 flex-1"
    >
      {model.lanes.length === 0 ? (
        <EmptyState
          icon={<Bot />}
          title={model.liveness === 'stopped' ? 'The desk is not running' : 'No lanes yet this tick'}
          body={
            model.liveness === 'stopped'
              ? 'Start the desk: it polls on its cadence and fans one lane per item across your nodes.'
              : 'Lanes appear the moment the orchestrator has read the poll and decided what needs a surgeon.'
          }
        />
      ) : (
        <ul className="divide-y divide-lz-border" data-testid="lane-list">
          {model.lanes.map((l) => (
            <LaneRow key={l.key} lane={l} node={nodeIndexOf(model, l.model)} active={selected === l.key} onClick={() => onSelect(selected === l.key ? null : l.key)} />
          ))}
        </ul>
      )}
      {selected && (
        <LaneInspector lane={model.lanes.find((l) => l.key === selected) ?? null} model={model} dir={dir} onClose={() => onSelect(null)} />
      )}
    </Panel>
  );
}

function LaneRow({ lane, node, active, onClick }: { lane: DeskLane; node: NodeIndex; active: boolean; onClick: () => void }) {
  return (
    <li>
      <button
        type="button"
        onClick={onClick}
        data-testid="lane-row"
        data-lane-key={lane.key}
        aria-pressed={active}
        className={cx(
          'flex w-full flex-col gap-1.5 px-4 py-3 text-left transition-colors',
          active ? 'bg-lz-accent text-lz-accent-ink' : 'hover:bg-lz-surface-2'
        )}
      >
        <div className="flex flex-wrap items-center gap-2">
          <Chip icon={KIND_ICON[lane.kind]} tone={active ? undefined : undefined}>
            {KIND_LABEL[lane.kind]}
            {lane.kind === 'lane' && lane.surgeon ? ` · ${lane.surgeon}` : ''}
            {lane.kind === 'lens' && lane.lens ? ` · ${lane.lens}` : ''}
          </Chip>
          <span className={cx(TYPE.body, WEIGHT.semibold, active && 'text-lz-accent-ink')}>{lane.kind === 'lens' ? lane.laneId : lane.item || lane.laneId}</span>
          <span className="ml-auto flex items-center gap-2">
            {lane.model && <Chip node={node}>{lane.model}</Chip>}
            <StatusDot tone={STATUS_TONE[lane.status]} live={lane.status === 'running'} label={lane.status} />
            {lane.secs != null && <span className={cx(TYPE.meta, TNUM, active && 'text-lz-accent-ink')}>{fmtDuration(lane.secs * 1000)}</span>}
          </span>
        </div>
        {lane.objective && <div className={cx(TYPE.bodyMuted, 'line-clamp-1', active && 'text-lz-accent-ink')}>{lane.objective}</div>}
        <div className={cx(TYPE.mono, 'line-clamp-2 break-words', active && 'text-lz-accent-ink')} data-testid="lane-live-line">
          {lane.status === 'queued'
            ? 'waiting for a free node'
            : lane.liveLine || (lane.status === 'done' ? summaryOf(lane) : '(no words yet)')}
        </div>
        {(lane.status === 'done' || lane.status === 'failed') && (
          <div className="flex flex-wrap gap-1.5">
            {lane.hasDraft && <Chip tone="accent">draft</Chip>}
            {lane.confidence != null && <Chip tone={lane.confidence >= 2 ? 'ok' : 'warn'}>confidence {lane.confidence}</Chip>}
            {lane.verdict && <Chip tone={lane.verdict === 'PASS' ? 'ok' : 'err'}>{lane.verdict}</Chip>}
            {lane.ask && <Chip tone="warn">asks the human</Chip>}
            {lane.route && <Chip tone="secondary">route → {lane.route}</Chip>}
            {lane.error && <Chip tone="err">failed</Chip>}
            <Chip>{lane.toolCalls} calls</Chip>
          </div>
        )}
      </button>
    </li>
  );
}

function summaryOf(l: DeskLane): string {
  if (l.error) return l.error;
  return l.answerTail || l.thinkingTail || 'done';
}

/**
 * The words of one lane, live: the reasoning channel and the answer channel as the engine's durable
 * logs (`<key>.think.log` / `<key>.log`, read whole on click through read-swarm-activity-log with the
 * agent dir as the run dir), the tool calls with their results, and what is forming right now.
 * Never the rolling window appended to the log — they are the same stream.
 */
export function LaneInspector({ lane, model, dir, onClose }: { lane: DeskLane | null; model: DeskModel; dir: string; onClose: () => void }) {
  const [channel, setChannel] = useState<'thinking' | 'answer' | 'calls'>('thinking');
  const [full, setFull] = useState<{ key: string; thinking: string | null; answer: string | null } | null>(null);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);
  useEffect(() => {
    setFull(null);
  }, [lane?.key]);
  if (!lane) return null;
  const node = nodeIndexOf(model, lane.model);
  const loadFull = async () => {
    const [t, a] = await Promise.all([
      window.electron.readSwarmActivityLog(dir, lane.key, 'thinking'),
      window.electron.readSwarmActivityLog(dir, lane.key, 'transcript'),
    ]);
    setFull({ key: lane.key, thinking: t?.text ?? null, answer: a?.text ?? null });
  };
  const thinking = full?.key === lane.key && full.thinking != null ? full.thinking : lane.fullThinking || lane.thinkingTail;
  const answer = full?.key === lane.key && full.answer != null ? full.answer : lane.fullTranscript || lane.answerTail;
  return (
    <div role="dialog" aria-modal="true" aria-label={`Lane ${lane.key}`} className="fixed inset-y-0 right-0 z-40 flex w-[min(720px,90vw)] flex-col border-l-0 bg-lz-surface shadow-lz-overlay dark:shadow-lz-overlay-dark" data-testid="lane-inspector">
      <div className="flex items-start justify-between gap-3 border-b border-lz-border px-5 py-4">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <Chip icon={KIND_ICON[lane.kind]}>{KIND_LABEL[lane.kind]}</Chip>
            {lane.model && <Chip node={node}>{lane.model}</Chip>}
            <StatusDot tone={STATUS_TONE[lane.status]} live={lane.status === 'running'} label={lane.status} />
          </div>
          <h2 className={cx(TYPE.h2, 'mt-2 truncate')}>{lane.item || lane.laneId}{lane.lens ? ` — ${lane.lens} lens` : ''}</h2>
          {lane.objective && <p className={cx(TYPE.bodyMuted, 'mt-1')}>{lane.objective}</p>}
          <p className={cx(TYPE.meta, TNUM, 'mt-1')}>
            {lane.toolCalls} tool calls · {lane.errors} errors · thinking {Math.round(lane.thinkingBytes / 1024)} KB · answer {Math.round(lane.transcriptBytes / 1024)} KB
            {lane.digestAgeMs != null ? ` · digest ${fmtDuration(lane.digestAgeMs)} ago` : ''}
          </p>
        </div>
        <Button variant="ghost" iconOnly icon={<X />} aria-label="Close" onClick={onClose} />
      </div>
      {lane.forming.length > 0 && (
        <div className={cx('mx-5 mt-3 rounded-lz-control px-3 py-2 text-[12px]', nodeClasses(node, 'fill'))}>
          forming {lane.forming[lane.forming.length - 1].name}
          {lane.forming[lane.forming.length - 1].args_bytes ? ` · ${lane.forming[lane.forming.length - 1].args_bytes} bytes` : ''}
          {lane.forming[lane.forming.length - 1].args_preview ? ` — ${lane.forming[lane.forming.length - 1].args_preview}` : ''}
        </div>
      )}
      <div className="flex items-center gap-1 px-5 pt-3">
        {(['thinking', 'answer', 'calls'] as const).map((c) => (
          <button
            key={c}
            type="button"
            onClick={() => setChannel(c)}
            aria-pressed={channel === c}
            className={cx('h-7 rounded-lz-control px-2.5 text-[12px]', WEIGHT.medium, channel === c ? 'bg-lz-accent text-lz-accent-ink' : 'bg-lz-surface-2 text-lz-ink-2 hover:bg-lz-surface-2')}
          >
            {c === 'calls' ? `calls (${lane.calls.length})` : c}
          </button>
        ))}
        <span className="ml-auto">
          <Button variant="ghost" size="sm" onClick={loadFull}>show whole log</Button>
        </span>
      </div>
      <div className="min-h-0 flex-1 overflow-auto px-5 py-3">
        {channel === 'calls' ? (
          lane.calls.length === 0 ? (
            <p className={TYPE.bodyMuted}>no tool calls yet</p>
          ) : (
            <ol className="flex flex-col gap-2">
              {lane.calls.map((c, i) => (
                <li key={i} className={cx('rounded-lz-control border border-lz-border p-2', c.ok === false && 'border-lz-err')}>
                  <div className={cx(TYPE.body, WEIGHT.semibold)}>{c.name ?? 'call'}{c.ok === false ? ' — error' : ''}</div>
                  {c.args && <pre className={cx(TYPE.mono, 'mt-1 whitespace-pre-wrap break-words')}>{c.args}</pre>}
                  {c.result && <pre className={cx(TYPE.mono, 'mt-1 whitespace-pre-wrap break-words text-lz-ink-2')}>{c.result}</pre>}
                </li>
              ))}
            </ol>
          )
        ) : (
          <pre className={cx(TYPE.mono, 'whitespace-pre-wrap break-words')} data-testid={`lane-${channel}`}>
            {(channel === 'thinking' ? thinking : answer) || `(nothing on the ${channel} channel yet)`}
          </pre>
        )}
      </div>
    </div>
  );
}
