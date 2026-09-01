import type { ReactNode } from 'react';
import { ROW, SURFACE, TNUM, TONE_TEXT, TYPE, cx, type Tone } from './tokens';

export interface KeyValueItem {
  /** Stable identity for the row. */
  key: string;
  label: ReactNode;
  value: ReactNode;
  /** Colour the VALUE by meaning (a failing count in err, a live one in ok). */
  tone?: Tone;
  /** Paths, hashes, ids: the mono register. */
  mono?: boolean;
}

export interface KeyValueProps {
  items: readonly KeyValueItem[];
  dense?: boolean;
  'aria-label'?: string;
  className?: string;
}

/** Label / value rows for a status panel. Values are right-aligned tabular figures. */
export function KeyValue({
  items,
  dense = false,
  'aria-label': ariaLabel,
  className,
}: KeyValueProps) {
  return (
    <dl
      data-testid="lz-key-value"
      aria-label={ariaLabel}
      className={cx('flex flex-col', className)}
    >
      {items.map((it) => (
        <div
          key={it.key}
          data-testid="lz-key-value-row"
          className={cx(
            'flex items-center justify-between gap-4 border-t first:border-t-0',
            SURFACE.hairline,
            dense ? ROW.dense : ROW.default
          )}
        >
          <dt className={cx(TYPE.meta, 'truncate')}>{it.label}</dt>
          <dd
            className={cx(
              'shrink-0 text-right font-lz-medium',
              it.mono ? 'font-mono text-lz-mono' : 'text-lz-body',
              TNUM,
              it.tone != null ? TONE_TEXT[it.tone] : 'text-lz-ink'
            )}
          >
            {it.value}
          </dd>
        </div>
      ))}
    </dl>
  );
}
