import { Chip, RADIUS, SURFACE, TNUM, TONE_DOT, TYPE, WEIGHT, cx } from '../lz';
import { BenchmarkRow, Tier } from './baselines';

const TIERS: Tier[] = ['A', 'B', 'C', 'D'];

/**
 * Per-tier bars, one row per entrant. This is the part that turns a score into a diagnosis: a build
 * can sit at A 100 / B 0 — perfectly structured with nothing flowing through it — and only the split
 * shows it. A single number would call that "9%" and tell you nothing about what to fix.
 *
 * Every bar is the accent. The four tiers are told apart by their COLUMN and the "A 88%" label under
 * each bar, never by a hue — the node ramp is node identity only (ui/desktop/DESIGN.md), and a
 * legend of coloured squares was the thing that made tiers look like nodes.
 */
export function TierBreakdown({ rows }: { rows: BenchmarkRow[] }) {
  if (!rows.length) return null;

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
              {TIERS.map((tier) => {
                const value = Math.min(1, Math.max(0, row.tiers[tier] ?? 0));
                return (
                  <div key={tier} className="flex-1">
                    <div className={cx('h-2 w-full', RADIUS.pill, SURFACE.inset)}>
                      <div
                        className={cx('h-2', RADIUS.pill, TONE_DOT.accent)}
                        style={{ width: `${Math.max(2, value * 100)}%` }}
                      />
                    </div>
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
