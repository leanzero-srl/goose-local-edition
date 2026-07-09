import React from 'react';
import { Check, X, Loader2 } from 'lucide-react';

/**
 * Desktop twin of the CLI swarm fan-in unit — Goose Local Edition's signature.
 *
 * A sharp full-border card (NEVER a left rail): a dispatch header, one lane per node with a SOLID
 * formation-hue identity chip (`⬢`+letter, an inline leading token), the device + action, and a goose
 * status glyph whose color comes from the status triad — deliberately DISJOINT from the node identity ramp
 * so a red status never reads as a node's identity — then a rolled-up fan-in footer. This is what a
 * single-model UI cannot show; the palette matches the CLI `theme::palette`.
 */

export type NodeStatus = 'running' | 'done' | 'error';

/** Node identity: a 6-hue formation ramp (cyan → rose), matching the CLI palette. */
const FORMATION_RAMP = ['#17c4c4', '#2e8bff', '#6a5cff', '#b14cff', '#ff3ea5', '#ff5c7a'];

/** Semantic status triad — orthogonal to (disjoint from) the formation ramp. */
const STATUS_COLOR: Record<NodeStatus, string> = {
  running: '#f5a623',
  done: '#2ecc71',
  error: '#ff3b30',
};
/**
 * SVG icons, not unicode glyphs. `✔`/`●`/`✕` get emoji-presentation on some platforms, which IGNORES the
 * CSS color and renders as a faint monochrome glyph in dark mode. Lucide SVGs always honor `color`.
 */
const STATUS_ICON: Record<
  NodeStatus,
  React.ComponentType<{
    size?: number;
    strokeWidth?: number;
    className?: string;
    style?: React.CSSProperties;
    'data-testid'?: string;
  }>
> = {
  running: Loader2,
  done: Check,
  error: X,
};

export interface NodeLane {
  device: string;
  action: string;
  status: NodeStatus;
}

interface FanInCardProps {
  dispatch: string;
  lanes: NodeLane[];
  className?: string;
}

const nodeHue = (i: number): string => FORMATION_RAMP[i % FORMATION_RAMP.length];
const nodeLetter = (i: number): string => String.fromCharCode(65 + (i % 26));

const FanInCard: React.FC<FanInCardProps> = ({ dispatch, lanes, className = '' }) => {
  const done = lanes.filter((l) => l.status === 'done').length;

  return (
    <div
      data-testid="fan-in-card"
      className={`border border-border-primary bg-background-secondary text-text-primary p-3 text-sm ${className}`}
      style={{ borderRadius: 3 }}
    >
      <div className="flex items-center justify-between text-text-secondary text-xs mb-2">
        <span>swarm · {dispatch}</span>
        <span>
          {lanes.length} nodes · {done}/{lanes.length} done
        </span>
      </div>

      <div className="space-y-1">
        {lanes.map((lane, i) => (
          <div key={i} className="flex items-center gap-2" data-testid="fan-in-lane">
            <span
              className="font-bold"
              data-testid="node-chip"
              style={{ color: nodeHue(i) }}
              aria-label={`node ${nodeLetter(i)}`}
            >
              ⬢{nodeLetter(i)}
            </span>
            <span className="w-24 shrink-0 truncate text-text-secondary">{lane.device}</span>
            <span className="flex-1 truncate">{lane.action}</span>
            {(() => {
              const Icon = STATUS_ICON[lane.status];
              return (
                <Icon
                  size={16}
                  strokeWidth={3}
                  data-testid="node-status"
                  className={lane.status === 'running' ? 'animate-spin' : ''}
                  style={{ color: STATUS_COLOR[lane.status] }}
                />
              );
            })()}
          </div>
        ))}
      </div>

      <div className="text-text-secondary text-xs mt-2">▾ fan-in · {lanes.length} lane(s)</div>
    </div>
  );
};

export default FanInCard;
