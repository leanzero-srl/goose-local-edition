import React from 'react';
import { Check, X, Loader2 } from 'lucide-react';
import { RADIUS, TNUM, cx } from '../lz';
import { FORMATION_RAMP, SWARM_STATUS } from './formationVisualState';

/**
 * Desktop twin of the CLI swarm fan-in unit — Goose Local Edition's signature.
 *
 * A Studio surface card (1px hairline, radius 6, NEVER a left rail): a dispatch header in the meta
 * register, one lane per node with a SOLID formation-hue identity DOT (the letter lives in its
 * aria-label), the device + action, and a status glyph whose colour comes from the status triad —
 * deliberately DISJOINT from the node identity ramp so a red status never reads as a node's identity.
 * This is what a single-model UI cannot show; the palette matches the CLI `theme::palette`.
 */

export type NodeStatus = 'running' | 'done' | 'error';

/** The node ramp and the status triad both come from formationVisualState — the ONE definition the run
 *  panel and the ribbon read. This file used to carry its own copies, so a palette change landed here only
 *  if someone remembered it existed, and a node's hue could differ between this card and the fleet strip. */
const STATUS_COLOR: Record<NodeStatus, string> = {
  running: SWARM_STATUS.running,
  done: SWARM_STATUS.done,
  error: SWARM_STATUS.error,
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
      className={cx(
        'border border-lz-border bg-lz-surface p-3 text-lz-body text-lz-ink',
        RADIUS.control,
        className
      )}
    >
      <div className={cx('mb-2 flex items-center justify-between text-lz-meta text-lz-ink-3', TNUM)}>
        <span>swarm · {dispatch}</span>
        <span>
          {/* LANES, not nodes: a 3-node fleet running 8 slice specs read "8 NODES". And the count was
              rendered again verbatim at the foot of the same card. */}
          {lanes.length} lane{lanes.length === 1 ? '' : 's'} · {done}/{lanes.length} done
        </span>
      </div>

      <div className="space-y-1">
        {lanes.map((lane, i) => (
          <div key={i} className="flex items-center gap-2" data-testid="fan-in-lane">
            <span
              className={cx('inline-block size-2 shrink-0', RADIUS.pill)}
              data-testid="node-chip"
              role="img"
              style={{ backgroundColor: nodeHue(i) }}
              aria-label={`node ${nodeLetter(i)}`}
            />
            <span className="w-24 shrink-0 truncate text-lz-ink-2">{lane.device}</span>
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
    </div>
  );
};

export default FanInCard;
