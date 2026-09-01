import type { ReactNode } from 'react';
import { RADIUS, SURFACE, TNUM, TYPE, cx } from './tokens';

export interface SectionHeaderProps {
  title: ReactNode;
  /** A count pill. It MUST be the length of the list the section body renders — a header counts what the body shows. */
  count?: number;
  /** Right-aligned slot: a Segmented, a Button, a Chip. */
  right?: ReactNode;
  as?: 'h2' | 'h3' | 'div';
  className?: string;
}

/** The zone register — 11px semibold uppercase 0.08em. No coloured square, no rail: hierarchy is type alone. */
export function SectionHeader({ title, count, right, as = 'h2', className }: SectionHeaderProps) {
  const Tag = as;
  return (
    <div data-testid="lz-section-header" className={cx('flex h-8 items-center gap-2', className)}>
      <Tag className={TYPE.zone}>{title}</Tag>
      {count != null && (
        <span
          data-testid="lz-section-count"
          className={cx(
            'inline-flex h-5 min-w-5 items-center justify-center px-1.5 text-lz-meta text-lz-ink-2',
            TNUM,
            RADIUS.pill,
            SURFACE.outline
          )}
        >
          {count}
        </span>
      )}
      {right != null && <div className="ml-auto flex items-center gap-2">{right}</div>}
    </div>
  );
}
