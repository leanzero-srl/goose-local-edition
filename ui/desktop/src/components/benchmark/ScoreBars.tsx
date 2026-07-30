import { BenchmarkRow } from './baselines';

/**
 * Ranked score bars. Hand-rolled SVG on purpose — the desktop ships no charting library, and one
 * measured bar per row needs none.
 *
 * The user's own row is filled in the accent and labelled, so it is findable at a glance without
 * reading every label; baselines stay in a quieter blue so the comparison, not the decoration, is
 * what carries.
 */
export function ScoreBars({ rows }: { rows: BenchmarkRow[] }) {
  if (!rows.length) return null;
  const barHeight = 26;
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
          const fill = row.mine ? 'var(--color-block-teal)' : 'var(--color-node-2)';
          return (
            <g key={`${row.label}-${i}`}>
              <text
                x={0}
                y={y + barHeight * 0.68}
                className="fill-[var(--color-text-primary)]"
                style={{ fontSize: 13, fontWeight: row.mine ? 700 : 500 }}
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
                fill="var(--color-background-secondary)"
              />
              <rect
                x={labelWidth}
                y={y + 4}
                width={filled}
                height={barHeight - 8}
                rx={2}
                fill={fill}
              />
              <text
                x={labelWidth + trackWidth + 10}
                y={y + barHeight * 0.68}
                className="fill-[var(--color-text-primary)]"
                style={{ fontSize: 13, fontWeight: 700, fontVariantNumeric: 'tabular-nums' }}
              >
                {(row.score * 100).toFixed(1)}%
              </text>
              {row.mine && (
                <text
                  x={labelWidth + filled - 8}
                  y={y + barHeight * 0.68}
                  textAnchor="end"
                  style={{ fontSize: 10, fontWeight: 800, letterSpacing: '0.06em', fill: '#fff' }}
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
