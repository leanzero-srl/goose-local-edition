import { Chip, RADIUS, SURFACE, TNUM, TONE_DOT, TYPE, WEIGHT, cx } from '../lz';
import { BenchmarkRow, Tier } from './baselines';

const TIERS: Tier[] = ['A', 'B', 'C', 'D', 'E', 'F'];

/** One tier the scorer recorded, with the weight it carried in THAT result (absent when unknown). */
export interface TierColumn {
  tier: string;
  name?: string;
  weight?: number;
  /** Scored as an admission gate: it caps the score and adds no points (SB7.1's S/Q/M). */
  admissionOnly?: boolean;
}

function TierBar({ value }: { value: number }) {
  return (
    <div className={cx('h-2 w-full', RADIUS.pill, SURFACE.inset)}>
      <div
        className={cx('h-2', RADIUS.pill, TONE_DOT.accent)}
        style={{ width: `${Math.max(2, value * 100)}%` }}
      />
    </div>
  );
}

const clamp = (value: number | undefined) => Math.min(1, Math.max(0, value ?? 0));

/**
 * Per-tier bars, one row per entrant. This is the part that turns a score into a diagnosis: a build
 * can sit at A 100 / B 0 — perfectly structured with nothing flowing through it — and only the split
 * shows it. A single number would call that "9%" and tell you nothing about what to fix.
 *
 * Every bar is the accent. The four tiers are told apart by their COLUMN and the "A 88%" label under
 * each bar, never by a hue — the node ramp is node identity only (ui/desktop/DESIGN.md), and a
 * legend of coloured squares was the thing that made tiers look like nodes.
 *
 * With `tiers` (the sb-7 family's J…M tiers, read from the result), every recorded tier gets a cell
 * in a wrapping grid that names its weight — SB7.2 weights its visual tiers S/Q/M, SB7.1 scored them
 * as admission gates, and a bar alone cannot say which. Without it, the A–F columns as before.
 */
export function TierBreakdown({ rows, tiers }: { rows: BenchmarkRow[]; tiers?: TierColumn[] }) {
  if (!rows.length) return null;

  if (tiers)
    return (
      <div className="flex flex-col">
        {rows.map((row) => (
          <div
            key={row.mine ? `mine:${row.label}` : row.label}
            className={cx('flex flex-col gap-2 border-t py-2.5 first:border-t-0', SURFACE.hairline)}
          >
            <div
              className={cx(
                'flex min-w-0 items-center gap-2',
                TYPE.body,
                row.mine && WEIGHT.semibold
              )}
            >
              <span className="truncate">{row.label}</span>
              {row.mine && <Chip tone="accent">yours</Chip>}
            </div>
            <div className="grid grid-cols-[repeat(auto-fill,minmax(128px,1fr))] gap-x-4 gap-y-3">
              {tiers
                .filter((column) => row.tiers?.[column.tier] !== undefined)
                .map((column) => {
                  const value = clamp(row.tiers?.[column.tier]);
                  return (
                    <div key={column.tier} data-testid={`tier-cell-${column.tier}`}>
                      <div className={cx('mb-1 truncate', TYPE.meta)} title={column.name}>
                        {column.name ?? column.tier}
                      </div>
                      <TierBar value={value} />
                      <div className={cx('mt-1 flex items-baseline gap-2', TYPE.meta, TNUM)}>
                        <span className={cx('text-lz-ink', WEIGHT.semibold)}>
                          {column.tier} {(value * 100).toFixed(0)}%
                        </span>
                        {column.admissionOnly ? (
                          <span>admission gate</span>
                        ) : column.weight != null ? (
                          <span>weight {(column.weight * 100).toFixed(0)}%</span>
                        ) : null}
                      </div>
                    </div>
                  );
                })}
            </div>
          </div>
        ))}
      </div>
    );

  return (
    <div className="overflow-x-auto">
      <div className="min-w-[680px]">
        {rows.map((row) => (
          <div
            key={row.mine ? `mine:${row.label}` : row.label}
            className={cx(
              'flex items-center gap-3 border-t py-2.5 first:border-t-0',
              SURFACE.hairline
            )}
          >
            <div
              className={cx(
                'flex w-[190px] shrink-0 items-center gap-2',
                TYPE.body,
                row.mine && WEIGHT.semibold
              )}
            >
              <span className="truncate">{row.label}</span>
              {row.mine && <Chip tone="accent">yours</Chip>}
            </div>
            <div className="flex flex-1 gap-2">
              {TIERS.filter((tier) => row.tiers?.[tier] !== undefined).map((tier) => {
                const value = clamp(row.tiers?.[tier]);
                return (
                  <div key={tier} className="flex-1">
                    <TierBar value={value} />
                    <div className={cx('mt-1', TYPE.meta, TNUM)}>
                      {tier} {(value * 100).toFixed(0)}%
                    </div>
                  </div>
                );
              })}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
