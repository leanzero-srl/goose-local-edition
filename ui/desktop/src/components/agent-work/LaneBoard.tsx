import { AgentText } from './AgentText';
import { ActivityDisclosure } from '../activity/ActivityDisclosure';
import { useEffect, useState, type ReactElement } from 'react';
import { Eye, FileText, Search, Sparkles, X } from 'lucide-react';
import {
  Button,
  Chip,
  NODE_INDEXES,
  StatusDot,
  TNUM,
  TONE_FILL,
  TYPE,
  WEIGHT,
  cx,
  nodeClasses,
  type NodeIndex,
  type Tone,
} from '../lz';
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
  interrupted: 'warn',
};

/** A node's hue is its position in the desk's device list — the same ramp everywhere in this view. */
export function nodeIndexOf(model: DeskModel, modelId: string): NodeIndex {
  const i = Math.max(
    0,
    model.nodes.findIndex((n) => n.model_id === modelId)
  );
  return NODE_INDEXES[i % NODE_INDEXES.length];
}

const KIND_FILL: Record<DeskLane['kind'], string> = {
  orient: TONE_FILL.secondary,
  lane: TONE_FILL.accent,
  lens: TONE_FILL.stopped,
  synthesis: TONE_FILL.secondary,
};

/**
 * The lanes of the VIEWED tick, in the order the tick ran them: the orchestrator's orient call,
 * every surgeon lane, every reviewer, the synthesis — each a card with its role, the node it ran
 * on, its status and time, and its words (the live line while it runs, the finding once done).
 * Click one to open the inspector with the durable logs.
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
  const liveTick = model.viewTick === model.tick && model.liveness === 'running';
  return (
    <section data-testid="lane-board" aria-labelledby="lane-board-heading" className="min-w-0">
      <div className="mb-3 flex flex-wrap items-baseline gap-x-3 gap-y-1">
        <h2 className={TYPE.h2} id="lane-board-heading">
          Lanes
        </h2>
        <span className={cx(TYPE.meta, TNUM)}>
          {liveTick
            ? `${running} running · ${model.queue.length} queued · ${model.nodes.reduce((n, d) => n + d.free, 0)} free slots`
            : `${model.lanes.length} in tick ${model.viewTick}`}
        </span>
      </div>
      {model.lanes.length === 0 ? (
        <p className={cx(TYPE.bodyMuted, 'rounded-lz-card border border-lz-border px-4 py-3')}>
          {model.liveness === 'stopped'
            ? 'No lanes ran. Start the desk: it polls on its cadence and fans one lane per item across your nodes.'
            : 'No lanes yet this tick. They appear the moment the orchestrator has read the poll and decided what needs a surgeon.'}
        </p>
      ) : (
        <ol className="flex flex-col gap-2" data-testid="lane-list">
          {model.lanes.map((l) => (
            <LaneRow
              key={l.key}
              lane={l}
              node={nodeIndexOf(model, l.model)}
              active={selected === l.key}
              onClick={() => onSelect(selected === l.key ? null : l.key)}
            />
          ))}
        </ol>
      )}
      {selected && (
        <LaneInspector
          lane={model.lanes.find((l) => l.key === selected) ?? null}
          model={model}
          dir={dir}
          onClose={() => onSelect(null)}
        />
      )}
    </section>
  );
}

function LaneRow({
  lane,
  node,
  active,
  onClick,
}: {
  lane: DeskLane;
  node: NodeIndex;
  active: boolean;
  onClick: () => void;
}) {
  const role = `${KIND_LABEL[lane.kind]}${lane.kind === 'lane' && lane.surgeon ? ` · ${lane.surgeon}` : ''}${lane.kind === 'lens' && lane.lens ? ` · ${lane.lens}` : ''}`;
  const settled = lane.status === 'done' || lane.status === 'failed';
  return (
    <li>
      <button
        type="button"
        onClick={onClick}
        data-testid="lane-row"
        data-lane-key={lane.key}
        aria-pressed={active}
        className={cx(
          'grid w-full grid-cols-[32px_minmax(0,1fr)] gap-x-3 rounded-lz-card border p-3 text-left transition-colors',
          active
            ? 'border-lz-accent ring-2 ring-inset ring-lz-accent'
            : 'border-lz-border bg-lz-surface hover:bg-lz-surface-2'
        )}
      >
        <span
          aria-hidden
          className={cx(
            'flex size-8 items-center justify-center rounded-lz-control [&_svg]:size-4',
            KIND_FILL[lane.kind]
          )}
        >
          {KIND_ICON[lane.kind]}
        </span>
        <span className="flex min-w-0 flex-col gap-1">
          <span className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
            <span className={cx(TYPE.meta, 'text-lz-ink-2')}>{role}</span>
            <span className="ml-auto flex shrink-0 items-center gap-2">
              {lane.model && <Chip node={node}>{lane.model}</Chip>}
              <span className="flex items-center gap-1.5">
                <StatusDot
                  tone={STATUS_TONE[lane.status]}
                  live={lane.status === 'running'}
                  label={lane.status}
                />
                <span className={TYPE.meta}>{lane.status}</span>
              </span>
              {lane.secs != null && (
                <span className={cx(TYPE.meta, TNUM, 'text-lz-ink')}>
                  {fmtDuration(lane.secs * 1000)}
                </span>
              )}
            </span>
          </span>
          <span className={cx(TYPE.body, WEIGHT.semibold, 'truncate')}>
            {lane.kind === 'lens' ? lane.laneId : lane.item || lane.laneId}
          </span>
          {lane.objective && (
            <span className={cx(TYPE.bodyMuted, 'line-clamp-2')}>{lane.objective}</span>
          )}
          <span
            className={cx(TYPE.body, 'line-clamp-3 [overflow-wrap:anywhere]')}
            data-testid="lane-live-line"
          >
            {lane.status === 'interrupted'
              ? 'Stopped before completion · open activity to inspect the last recorded output'
              : lane.status === 'queued'
                ? 'waiting for a free node'
                : lane.liveLine || (lane.status === 'done' ? summaryOf(lane) : '(no words yet)')}
          </span>
          {settled && (
            <span className="mt-1 flex flex-wrap gap-1.5">
              {lane.confidence != null && (
                <Chip tone={lane.confidence >= 2 ? 'ok' : 'warn'}>
                  confidence {lane.confidence}
                </Chip>
              )}
              {lane.verdict && (
                <Chip tone={lane.verdict === 'PASS' ? 'ok' : 'err'}>{lane.verdict}</Chip>
              )}
              {lane.hasDraft && <Chip tone="accent">draft</Chip>}
              {lane.ask && <Chip tone="warn">asks the human</Chip>}
              {lane.route && <Chip tone="secondary">route → {lane.route}</Chip>}
              {lane.error && <Chip tone="err">failed</Chip>}
              <Chip>
                {lane.toolCalls} {lane.toolCalls === 1 ? 'call' : 'calls'}
              </Chip>
            </span>
          )}
        </span>
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
export function LaneInspector({
  lane,
  model,
  dir,
  onClose,
}: {
  lane: DeskLane | null;
  model: DeskModel;
  dir: string;
  onClose: () => void;
}) {
  const [channel, setChannel] = useState<'thinking' | 'answer' | 'calls'>('answer');
  const [full, setFull] = useState<{
    key: string;
    thinking: string | null;
    answer: string | null;
  } | null>(null);
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
  const thinking =
    full?.key === lane.key && full.thinking != null
      ? full.thinking
      : lane.fullThinking || lane.thinkingTail;
  const answer =
    full?.key === lane.key && full.answer != null
      ? full.answer
      : lane.fullTranscript || lane.answerTail;
  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label={`Lane ${lane.key}`}
      className="fixed inset-y-0 right-0 z-40 flex w-[min(720px,90vw)] flex-col bg-lz-surface shadow-lz-overlay dark:shadow-lz-overlay-dark"
      data-testid="lane-inspector"
    >
      <div className="flex items-start justify-between gap-3 border-b border-lz-border px-5 py-4">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <Chip icon={KIND_ICON[lane.kind]}>{KIND_LABEL[lane.kind]}</Chip>
            {lane.model && <Chip node={node}>{lane.model}</Chip>}
            <StatusDot
              tone={STATUS_TONE[lane.status]}
              live={lane.status === 'running'}
              label={lane.status}
            />
          </div>
          <h2 className={cx(TYPE.h2, 'mt-2 truncate')}>
            {lane.item || lane.laneId}
            {lane.lens ? ` — ${lane.lens} lens` : ''}
          </h2>
          {lane.objective && <p className={cx(TYPE.bodyMuted, 'mt-1')}>{lane.objective}</p>}
          <p className={cx(TYPE.meta, TNUM, 'mt-1')}>
            {lane.toolCalls} tool calls · {lane.errors} errors · thinking{' '}
            {Math.round(lane.thinkingBytes / 1024)} KB · answer{' '}
            {Math.round(lane.transcriptBytes / 1024)} KB
            {lane.digestAgeMs != null ? ` · digest ${fmtDuration(lane.digestAgeMs)} ago` : ''}
          </p>
        </div>
        <Button variant="ghost" iconOnly icon={<X />} aria-label="Close" onClick={onClose} />
      </div>
      {lane.status === 'running' && lane.forming.length > 0 && (
        <div
          className={cx(
            'mx-5 mt-3 rounded-lz-control px-3 py-2 text-[12px]',
            nodeClasses(node, 'fill')
          )}
        >
          forming {lane.forming[lane.forming.length - 1].name}
          {lane.forming[lane.forming.length - 1].args_bytes
            ? ` · ${lane.forming[lane.forming.length - 1].args_bytes} bytes`
            : ''}
          {lane.forming[lane.forming.length - 1].args_preview
            ? ` — ${lane.forming[lane.forming.length - 1].args_preview}`
            : ''}
        </div>
      )}
      <div className="flex items-center gap-1 px-5 pt-3">
        {(['thinking', 'answer', 'calls'] as const).map((c) => (
          <button
            key={c}
            type="button"
            onClick={() => setChannel(c)}
            aria-pressed={channel === c}
            className={cx(
              'h-7 rounded-lz-control px-2.5 text-[12px]',
              WEIGHT.medium,
              channel === c
                ? 'bg-lz-accent text-lz-accent-ink'
                : 'bg-lz-surface-2 text-lz-ink-2 hover:bg-lz-surface-2'
            )}
          >
            {c === 'calls' ? `calls (${lane.calls.length})` : c}
          </button>
        ))}
        <span className="ml-auto">
          <Button variant="ghost" size="sm" onClick={loadFull}>
            show whole log
          </Button>
        </span>
      </div>
      <div className="min-h-0 flex-1 overflow-auto px-5 py-3">
        {channel === 'calls' ? (
          lane.calls.length === 0 ? (
            <p className={TYPE.bodyMuted}>no tool calls yet</p>
          ) : (
            <ol className="flex flex-col gap-2">
              {lane.calls.map((c, i) => (
                <li
                  key={i}
                  className={cx(
                    'rounded-lz-control border border-lz-border p-2',
                    c.ok === false && 'border-lz-err'
                  )}
                >
                  <ActivityDisclosure
                    label={
                      <span className="flex justify-between gap-3">
                        <span>{c.name ?? 'Tool call'}</span>
                        <span className="text-xs">
                          {c.ok === false
                            ? 'Failed'
                            : c.ok === true
                              ? 'Completed'
                              : lane.status === 'interrupted'
                                ? 'No result received'
                                : 'Awaiting result'}
                        </span>
                      </span>
                    }
                    isStartExpanded={c.ok === false}
                  >
                    {c.args && (
                      <section className="p-3">
                        <h3 className="mb-2 text-sm font-semibold">Arguments</h3>
                        <AgentText text={c.args} />
                      </section>
                    )}
                    {c.result && (
                      <section className="p-3">
                        <h3 className="mb-2 text-sm font-semibold">Result</h3>
                        <AgentText text={c.result} />
                      </section>
                    )}
                  </ActivityDisclosure>
                </li>
              ))}
            </ol>
          )
        ) : (
          <div data-testid={`lane-${channel}`}>
            <AgentText
              text={
                (channel === 'thinking' ? thinking : answer) ||
                `(nothing on the ${channel} channel yet)`
              }
              raw
            />
          </div>
        )}
      </div>
    </div>
  );
}
