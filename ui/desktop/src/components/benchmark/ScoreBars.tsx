import { TNUM, WEIGHT, cx } from '../lz';
import { BenchmarkRow } from './baselines';

/**
 * Ranked score bars. Hand-rolled SVG on purpose — the desktop ships no charting library, and one
 * measured bar per row needs none.
 *
 * The user's own row is filled in the accent and labelled, so it is findable at a glance without
 * reading every label; baselines are a neutral slate so the comparison, not the decoration, is
 * what carries. No node hue anywhere — the ramp is node identity only (ui/desktop/DESIGN.md).
 */
export function ScoreBars({ rows }: { rows: BenchmarkRow[] }) {
  if (!rows.length) return null;
  const barHeight = 26;
  /** "YOUR FLEET" at 10px/800 with 0.06em tracking measured ~66px. The Studio sets it at 600, which
   *  is narrower, so the same floor still keeps the badge clear of the row label beside it. */
  const YOUR_FLEET_LABEL_WIDTH = 66;
  const gap = 10;
  const labelWidth = 190;
  const valueWidth = 62;
  const width = 860;
  const trackWidth = width - labelWidth - valueWidth;
  const height = rows.length * (barHeight + gap);

  return (
    <div className="mt-3 overflow-x-auto">
      <svg
        width={width}
        height={height}
        role="img"
        aria-label="Benchmark scores by entrant"
        className="min-w-[680px]"
      >
        {rows.map((row, i) => {
          const y = i * (barHeight + gap);
          const filled = Math.max(2, trackWidth * Math.min(1, Math.max(0, row.score)));
          return (
            <g key={row.mine ? `mine:${row.label}` : row.label}>
              <text
                x={0}
                y={y + barHeight * 0.68}
                className={cx('fill-lz-ink text-lz-body', row.mine && WEIGHT.semibold)}
              >
                {row.label}
                {row.nodes ? ` · ${row.nodes}n` : ''}
              </text>
              <rect
                x={labelWidth}
                y={y + 4}
                width={trackWidth}
                height={barHeight - 8}
                rx={2}
                className="fill-lz-surface-2"
              />
              <rect
                x={labelWidth}
                y={y + 4}
                width={filled}
                height={barHeight - 8}
                rx={2}
                className={row.mine ? 'fill-lz-accent' : 'fill-lz-ink-3'}
              />
              <text
                x={labelWidth + trackWidth + 10}
                y={y + barHeight * 0.68}
                className={cx('fill-lz-ink text-lz-body', WEIGHT.semibold, TNUM)}
              >
                {(row.score * 100).toFixed(1)}%
              </text>
              {/* ONLY INSIDE A BAR THAT CAN HOLD IT.
                  This was drawn unconditionally at `labelWidth + filled - 8`, end-anchored, so a low
                  score put it straight on top of the row's own name: at 1.6% the bar is a handful of
                  pixels wide and "YOUR FLEET" landed across "Your fleet · 3 nodes", two strings
                  overprinted into an unreadable smear. And the low scores are exactly the ones this
                  project spends its time looking at.

                  The row label already says whose bar it is, so when the bar is too narrow the badge is
                  simply dropped rather than moved somewhere it would collide with the percentage. */}
              {row.mine && filled > YOUR_FLEET_LABEL_WIDTH + 16 && (
                <text
                  x={labelWidth + filled - 8}
                  y={y + barHeight * 0.68}
                  textAnchor="end"
                  className="fill-lz-accent-ink"
                  style={{ fontSize: 10, fontWeight: 600, letterSpacing: '0.06em' }}
                >
                  YOUR FLEET
                </text>
              )}
            </g>
          );
        })}
      </svg>
    </div>
  );
}
