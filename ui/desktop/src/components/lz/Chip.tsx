import type { ReactNode } from 'react';
import {
  NODE_FILL,
  RADIUS,
  SURFACE,
  TNUM,
  TONE_FILL,
  cx,
  type NodeIndex,
  type Tone,
} from './tokens';

export interface ChipProps {
  /** Semantic state — the status triad or the accent. Solid fill, white/ink per token. */
  tone?: Tone;
  /** Node identity — the 6-hue ramp with its measured ink. Identity ONLY; never for state. */
  node?: NodeIndex;
  icon?: ReactNode;
  children: ReactNode;
  title?: string;
  className?: string;
}

/**
 * Two registers. QUIET (the default): a 1px outline in ink-3, no fill — metadata reads as text,
 * not as a pile of stickers. FILLED (`tone` or `node`): a solid colour that MEANS something.
 * 11px, normal case, tabular figures; uppercase belongs to zone headers only.
 */
export function Chip({ tone, node, icon, children, title, className }: ChipProps) {
  const filled = node != null || tone != null;
  const register =
    node != null
      ? NODE_FILL[node]
      : tone != null
        ? TONE_FILL[tone]
        : cx(SURFACE.outline, 'text-lz-ink-3');
  return (
    <span
      title={title}
      data-testid="lz-chip"
      data-tone={tone}
      data-node={node}
      className={cx(
        'inline-flex h-5 shrink-0 items-center gap-1 whitespace-nowrap px-1.5 text-lz-meta [&_svg]:size-3',
        filled && 'font-lz-semibold',
        TNUM,
        RADIUS.control,
        register,
        className
      )}
    >
      {icon != null && <span aria-hidden>{icon}</span>}
      {children}
    </span>
  );
}
