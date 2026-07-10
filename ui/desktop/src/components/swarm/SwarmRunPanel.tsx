import React from 'react';
import { Check, X, Loader2 } from 'lucide-react';
import { useSwarmRun, type TurnStatus } from './useSwarmRun';

/**
 * Goose Local Edition — LIVE swarm run turn loops. One row per task: the node identity chip, the node's
 * device, the model, the worker's latest reasoning + recent tool calls, and a status glyph. Fed by
 * useSwarmRun (reads <cwd>/.swarm). Sharp full-border card (never a left rail), solid saturated status
 * colors, matching the FanInCard fleet view. Renders nothing when there is no run to show.
 */

// Node identity ramp — matches FanInCard so a node reads the same across the fleet + run views.
const FORMATION_RAMP = ['#17c4c4', '#2e8bff', '#6a5cff', '#b14cff', '#ff3ea5', '#ff5c7a'];
const STATUS_COLOR: Record<TurnStatus, string> = {
  running: '#f5a623',
  done: '#2ecc71',
  error: '#ff3b30',
};
const STATUS_ICON: Record<TurnStatus, React.ComponentType<{ size?: number; strokeWidth?: number; className?: string; style?: React.CSSProperties }>> = {
  running: Loader2,
  done: Check,
  error: X,
};

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

export const SwarmRunPanel: React.FC<{ workingDir: string | undefined; className?: string }> = ({
  workingDir,
  className = '',
}) => {
  const run = useSwarmRun(workingDir);

  if (!run.present || run.lanes.length === 0) return null;

  // Deterministic node ordering (first-seen device order) for stable letters/hues.
  const deviceOrder: string[] = [];
  for (const l of run.lanes) if (!deviceOrder.includes(l.device)) deviceOrder.push(l.device);

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
            <span style={{ color: STATUS_COLOR.running }} className="font-semibold">
              {running} running
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

      <div className="max-h-72 overflow-y-auto divide-y divide-border-primary">
        {run.lanes.map((lane) => {
          const idx = deviceIndex(lane.device, deviceOrder);
          const hue = FORMATION_RAMP[idx % FORMATION_RAMP.length];
          const letter = String.fromCharCode(65 + (idx % 26));
          const Icon = STATUS_ICON[lane.status];
          // Prefer a meaningful reasoning snippet; the activity digest's last_text is often a stray
          // fragment (".", "2"), so fall back to the recent tool calls when it is too short to be useful.
          const text = lane.lastText?.trim() ?? '';
          const detail =
            (text.length > 4 ? text : '') ||
            (lane.recent && lane.recent.length > 0 ? lane.recent.join(' · ') : '') ||
            (lane.status === 'running' ? 'working…' : lane.status);
          return (
            <div key={lane.taskId} className="px-3 py-2" data-testid="turn-lane">
              <div className="flex items-center gap-2">
                <span className="font-bold shrink-0" style={{ color: hue }} aria-label={`node ${letter}`}>
                  ⬢{letter}
                </span>
                <span className="w-20 shrink-0 truncate text-text-secondary text-xs">{lane.device}</span>
                <span className="flex-1 truncate text-xs font-mono text-text-primary">{lane.taskId}</span>
                <span className="text-xs text-text-secondary tabular-nums shrink-0">
                  {typeof lane.toolCalls === 'number' ? `${lane.toolCalls}⚙` : ''}
                  {lane.errors ? (
                    <span style={{ color: STATUS_COLOR.error }}> {lane.errors}✕</span>
                  ) : null}
                  {typeof lane.elapsedMs === 'number' ? ` ${Math.round(lane.elapsedMs / 1000)}s` : ''}
                </span>
                <Icon
                  size={15}
                  strokeWidth={3}
                  className={`shrink-0 ${lane.status === 'running' ? 'animate-spin' : ''}`}
                  style={{ color: STATUS_COLOR[lane.status] }}
                />
              </div>
              {detail && (
                <div className="pl-6 pr-6 mt-0.5 text-xs text-text-secondary line-clamp-2 break-words">
                  {detail}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
};

export default SwarmRunPanel;
