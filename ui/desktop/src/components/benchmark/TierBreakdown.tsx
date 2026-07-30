import { BenchmarkRow, Tier, TIER_COLORS, TIER_LABELS } from './baselines';

const TIERS: Tier[] = ['A', 'B', 'C', 'D'];

/**
 * Per-tier bars, one row per entrant. This is the part that turns a score into a diagnosis: a build
 * can sit at A 100 / B 0 — perfectly structured with nothing flowing through it — and only the split
 * shows it. A single number would call that "9%" and tell you nothing about what to fix.
 */
export function TierBreakdown({ rows }: { rows: BenchmarkRow[] }) {
  if (!rows.length) return null;

  return (
    <div className="overflow-x-auto">
      <div className="min-w-[680px]">
        <div className="mb-3 flex flex-wrap gap-4">
          {TIERS.map((tier) => (
            <span key={tier} className="flex items-center gap-2 text-xs text-text-secondary">
              <span
                className="inline-block h-3 w-3 rounded-[2px]"
                style={{ backgroundColor: TIER_COLORS[tier] }}
              />
              {TIER_LABELS[tier]}
            </span>
          ))}
        </div>

        {rows.map((row, i) => (
          <div
            key={`${row.label}-${i}`}
            className="flex items-center gap-3 border-t border-border-primary py-2.5"
          >
            <div
              className="w-[190px] shrink-0 text-[13px] text-text-primary"
              style={{ fontWeight: row.mine ? 700 : 500 }}
            >
              {row.label}
              {row.mine && (
                <span className="ml-2 rounded-[2px] bg-[var(--color-block-teal)] px-1.5 py-0.5 text-[9px] font-extrabold tracking-wider text-white">
                  YOURS
                </span>
              )}
            </div>
            <div className="flex flex-1 gap-2">
              {TIERS.map((tier) => {
                const value = Math.min(1, Math.max(0, row.tiers[tier] ?? 0));
                return (
                  <div key={tier} className="flex-1">
                    <div
                      className="h-2 w-full rounded-[2px]"
                      style={{ backgroundColor: 'var(--color-background-secondary)' }}
                    >
                      <div
                        className="h-2 rounded-[2px]"
                        style={{
                          width: `${Math.max(2, value * 100)}%`,
                          backgroundColor: TIER_COLORS[tier],
                        }}
                      />
                    </div>
                    <div
                      className="mt-1 text-[10px] text-text-secondary"
                      style={{ fontVariantNumeric: 'tabular-nums' }}
                    >
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
