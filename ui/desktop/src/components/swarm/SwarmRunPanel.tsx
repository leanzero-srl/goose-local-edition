import React, { useState } from 'react';
import { Check, X, Loader2, CircleSlash, ChevronRight, ChevronDown, Wrench } from 'lucide-react';
import { useSwarmRun, type TurnStatus, type TurnLane, type SwarmCall } from './useSwarmRun';

/**
 * Goose Local Edition — LIVE swarm run turn loops, verbose. One expandable lane per task: the node
 * identity chip, device, model, and status in the header; on expand, the worker's full reasoning and
 * the actual TOOL CALLS it made (the shell line, edited path, query…) with per-call ok/err. Fed by
 * useSwarmRun (reads <cwd>/.swarm). Running lanes auto-expand so live activity is visible at a glance.
 * A run whose files have gone quiet for STALE_MS is shown as interrupted rather than a live spinner.
 * Sharp full-border card (never a left rail), solid saturated status colors. Renders nothing when idle.
 */

// Node identity ramp — matches FanInCard so a node reads the same across the fleet + run views.
const FORMATION_RAMP = ['#17c4c4', '#2e8bff', '#6a5cff', '#b14cff', '#ff3ea5', '#ff5c7a'];
const STATUS_COLOR: Record<TurnStatus, string> = {
  running: '#f5a623',
  done: '#2ecc71',
  error: '#ff3b30',
};
// A worker rewrites its digest each turn, but a single long tool call (cargo build, a big pytest run)
// produces no write while it runs. Keep this above realistic single-tool durations so a live worker isn't
// mislabelled "interrupted"; a genuinely dead run still goes stale within this window.
const STALE_MS = 300_000;
const CALL_OK = '#2ecc71';
const CALL_ERR = '#ff3b30';
const CALL_PENDING = '#8a8a8a';

/** Stable node letter/hue per device so the same node keeps its identity across rows and polls. */
function deviceIndex(device: string, order: string[]): number {
  const i = order.indexOf(device);
  return i < 0 ? order.length : i;
}

function ago(mtime: number | null): string {
  if (!mtime) return '';
  const s = Math.max(0, Math.round((Date.now() - mtime) / 1000));
  if (s < 60) return `${s}s ago`;
  const m = Math.round(s / 60);
  return m < 60 ? `${m}m ago` : `${Math.round(m / 60)}h ago`;
}

function callColor(ok: boolean | null): string {
  if (ok === true) return CALL_OK;
  if (ok === false) return CALL_ERR;
  return CALL_PENDING;
}

const CallRow: React.FC<{ call: SwarmCall }> = ({ call }) => (
  <div className="py-0.5">
    <div className="flex items-start gap-2">
      <span
        className="mt-1 h-1.5 w-1.5 shrink-0"
        style={{ backgroundColor: callColor(call.ok), borderRadius: 1 }}
        aria-hidden
      />
      <span
        className="shrink-0 font-mono text-[10px] uppercase tracking-wide px-1 py-px text-text-secondary border border-border-primary"
        style={{ borderRadius: 2 }}
      >
        {call.name.replace(/^developer__/, '')}
      </span>
      <span
        className="flex-1 font-mono text-xs text-text-primary break-words"
        style={call.ok === false ? { color: CALL_ERR } : undefined}
        title={call.summary}
      >
        {call.summary || '—'}
      </span>
    </div>
    {call.result && call.result.trim().length > 0 && (
      <div
        className="ml-[1.15rem] mt-0.5 font-mono text-[11px] whitespace-pre-wrap break-words max-h-28 overflow-y-auto px-2 py-1 bg-background-secondary border border-border-primary text-text-secondary"
        style={{ borderRadius: 2 }}
      >
        {call.result.trim()}
      </div>
    )}
  </div>
);

const LaneRow: React.FC<{
  lane: TurnLane;
  deviceOrder: string[];
  stale: boolean;
  open: boolean;
  onToggle: () => void;
}> = ({ lane, deviceOrder, stale, open, onToggle }) => {
  const idx = deviceIndex(lane.device, deviceOrder);
  const hue = FORMATION_RAMP[idx % FORMATION_RAMP.length];
  const letter = String.fromCharCode(65 + (idx % 26));

  const live = lane.status === 'running' && !stale;
  const interrupted = lane.status === 'running' && stale;
  const Icon = interrupted ? CircleSlash : lane.status === 'done' ? Check : lane.status === 'error' ? X : Loader2;
  const iconColor = interrupted ? CALL_PENDING : STATUS_COLOR[lane.status];

  const calls = lane.calls ?? [];
  // Only surface reasoning that is real narration. Old-schema runs fall back to last_text, which for
  // these coder models is often a stray "." / "`" / one-word fragment — never show a box holding that.
  const rawReasoning = lane.reasoning?.trim() || lane.lastText?.trim() || '';
  const reasoning = rawReasoning.length >= 12 && /[a-zA-Z]{3,}/.test(rawReasoning) ? rawReasoning : '';
  const hasBody = reasoning.length > 0 || calls.length > 0 || (lane.recent?.length ?? 0) > 0;

  return (
    <div data-testid="turn-lane">
      <button
        type="button"
        onClick={onToggle}
        className="w-full flex items-center gap-2 px-3 py-2 text-left hover:bg-background-primary/40 transition-colors"
      >
        {hasBody ? (
          open ? (
            <ChevronDown className="h-3.5 w-3.5 shrink-0 text-text-secondary" />
          ) : (
            <ChevronRight className="h-3.5 w-3.5 shrink-0 text-text-secondary" />
          )
        ) : (
          <span className="w-3.5 shrink-0" />
        )}
        <span className="font-bold shrink-0" style={{ color: hue }} aria-label={`node ${letter}`}>
          ⬢{letter}
        </span>
        <span className="w-16 shrink-0 truncate text-text-secondary text-xs">{lane.device}</span>
        <span className="flex-1 truncate text-xs font-mono text-text-primary">{lane.taskId}</span>
        {lane.model && (
          <span className="hidden sm:inline shrink-0 max-w-[9rem] truncate text-[10px] font-mono text-text-secondary">
            {lane.model}
          </span>
        )}
        <span className="text-xs text-text-secondary tabular-nums shrink-0 flex items-center gap-1">
          {typeof lane.toolCalls === 'number' && lane.toolCalls > 0 ? (
            <span className="flex items-center gap-0.5">
              <Wrench className="h-3 w-3" />
              {lane.toolCalls}
            </span>
          ) : null}
          {lane.errors ? <span style={{ color: STATUS_COLOR.error }}>{lane.errors}✕</span> : null}
          {typeof lane.elapsedMs === 'number' ? <span>{Math.round(lane.elapsedMs / 1000)}s</span> : null}
        </span>
        <Icon
          size={15}
          strokeWidth={3}
          className={`shrink-0 ${live ? 'animate-spin' : ''}`}
          style={{ color: iconColor }}
        />
      </button>

      {open && hasBody && (
        <div className="px-3 pb-3 pl-9 space-y-2">
          {reasoning && (
            <div>
              <div className="text-[10px] uppercase tracking-wide text-text-secondary mb-1">Reasoning</div>
              <div
                className="text-xs text-text-primary whitespace-pre-wrap break-words max-h-40 overflow-y-auto bg-background-primary border border-border-primary px-2 py-1.5"
                style={{ borderRadius: 3 }}
              >
                {reasoning}
              </div>
            </div>
          )}

          {calls.length > 0 ? (
            <div>
              <div className="text-[10px] uppercase tracking-wide text-text-secondary mb-1">
                Tool calls · {lane.toolCalls ?? calls.length}
              </div>
              <div
                className="max-h-56 overflow-y-auto bg-background-primary border border-border-primary px-2 py-1"
                style={{ borderRadius: 3 }}
              >
                {calls.map((c, i) => (
                  <CallRow key={i} call={c} />
                ))}
              </div>
            </div>
          ) : lane.recent && lane.recent.length > 0 ? (
            <div className="text-xs text-text-secondary font-mono break-words">{lane.recent.join(' · ')}</div>
          ) : null}
        </div>
      )}
    </div>
  );
};

export const SwarmRunPanel: React.FC<{ workingDir: string | undefined; className?: string }> = ({
  workingDir,
  className = '',
}) => {
  const run = useSwarmRun(workingDir);
  const [overrides, setOverrides] = useState<Record<string, boolean>>({});

  if (!run.present || run.lanes.length === 0) return null;

  // Deterministic node ordering (first-seen device order) for stable letters/hues.
  const deviceOrder: string[] = [];
  for (const l of run.lanes) if (!deviceOrder.includes(l.device)) deviceOrder.push(l.device);

  const stale = run.mtime != null && Date.now() - run.mtime > STALE_MS;
  const { running, done, failed, tasks } = run.totals;

  return (
    <div
      data-testid="swarm-run-panel"
      className={`border border-border-primary bg-background-secondary text-text-primary text-sm ${className}`}
      style={{ borderRadius: 3 }}
    >
      <div className="flex items-center justify-between px-3 py-2 border-b border-border-primary">
        <span className="text-xs font-semibold">Swarm LeanZero — turn loops</span>
        <span className="text-xs text-text-secondary tabular-nums">
          {running > 0 && (
            <span
              style={{ color: stale ? CALL_PENDING : STATUS_COLOR.running }}
              className="font-semibold"
            >
              {running} {stale ? 'interrupted' : 'running'}
            </span>
          )}
          {running > 0 && ' · '}
          <span style={{ color: STATUS_COLOR.done }}>{done} done</span>
          {failed > 0 && (
            <>
              {' · '}
              <span style={{ color: STATUS_COLOR.error }} className="font-semibold">
                {failed} failed
              </span>
            </>
          )}
          {' · '}
          {tasks} tasks · {ago(run.mtime)}
        </span>
      </div>

      <div className="max-h-[28rem] overflow-y-auto divide-y divide-border-primary">
        {run.lanes.map((lane) => {
          const open = overrides[lane.taskId] ?? (lane.status === 'running');
          return (
            <LaneRow
              key={lane.taskId}
              lane={lane}
              deviceOrder={deviceOrder}
              stale={stale}
              open={open}
              onToggle={() =>
                setOverrides((o) => ({ ...o, [lane.taskId]: !(o[lane.taskId] ?? lane.status === 'running') }))
              }
            />
          );
        })}
      </div>
    </div>
  );
};

export default SwarmRunPanel;
