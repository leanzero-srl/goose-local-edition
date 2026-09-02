import React from 'react';
import { Check, X, Loader2 } from 'lucide-react';
import { NODE_DOT, RADIUS, TNUM, TONE_TEXT, cx, type NodeIndex, type Tone } from '../lz';
import { Clipped } from './Clipped';

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

/** The node ramp and the status triad are the Studio token utilities — the ONE definition the run panel,
 *  the ribbon and the fleet strip read (bg-lz-node-N for identity, text-lz-{ok,warn,err} for status). This
 *  file used to carry its own copies, so a palette change landed here only if someone remembered it
 *  existed, and a node's hue could differ between this card and the fleet strip. */
const STATUS_TONE: Record<NodeStatus, Tone> = { running: 'warn', done: 'ok', error: 'err' };
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

/** A lane's slot on the six-hue ramp — IDENTITY ONLY, the same slot the panel's NodeDot uses. */
const nodeSlot = (i: number): NodeIndex => ((i % 6) + 1) as NodeIndex;
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
        <span>flock · {dispatch}</span>
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
              className={cx('inline-block size-2 shrink-0', RADIUS.pill, NODE_DOT[nodeSlot(i)])}
              data-testid="node-chip"
              role="img"
              aria-label={`node ${nodeLetter(i)}`}
            />
            <Clipped
              text={lane.device}
              mono
              label="Node"
              context={[{ label: 'node', value: nodeLetter(i) }]}
              className="w-24 shrink-0 text-lz-ink-2"
              testId="fan-in-device"
            />
            <Clipped
              text={lane.action}
              label="Action"
              context={[
                { label: 'node', value: nodeLetter(i) },
                { label: 'status', value: lane.status },
              ]}
              className="flex-1"
              testId="fan-in-action"
            />
            {(() => {
              const Icon = STATUS_ICON[lane.status];
              return (
                <Icon
                  size={16}
                  strokeWidth={3}
                  data-testid="node-status"
                  className={cx(lane.status === 'running' && 'animate-spin', TONE_TEXT[STATUS_TONE[lane.status]])}
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
