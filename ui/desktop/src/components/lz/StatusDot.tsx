import {
  NODE_DOT,
  PHASE_DOT,
  RADIUS,
  TONE_DOT,
  cx,
  type EnginePhase,
  type NodeIndex,
  type Tone,
} from './tokens';

export interface StatusDotProps {
  tone?: Tone;
  /** Node identity hue instead of a tone. */
  node?: NodeIndex;
  /** An engine phase (lz tokens PHASE_DOT) instead of a tone. */
  phase?: EnginePhase;
  /** "live": the dot SCALES on the motion token (never fades). */
  live?: boolean;
  /** What the colour means, for the reader who cannot see it. */
  label: string;
  size?: 8 | 10;
  className?: string;
}

/** An 8px solid dot in a tone or a node hue. The mark is the colour; the label is the meaning. */
export function StatusDot({
  tone = 'stopped',
  node,
  phase,
  live = false,
  label,
  size = 8,
  className,
}: StatusDotProps) {
  return (
    <span
      role="img"
      aria-label={label}
      data-testid="lz-status-dot"
      data-live={live || undefined}
      data-phase={phase}
      className={cx(
        'inline-block shrink-0',
        size === 10 ? 'size-2.5' : 'size-2',
        RADIUS.pill,
        node != null ? NODE_DOT[node] : phase != null ? PHASE_DOT[phase] : TONE_DOT[tone],
        live && 'animate-lz-live',
        className
      )}
    />
  );
}
