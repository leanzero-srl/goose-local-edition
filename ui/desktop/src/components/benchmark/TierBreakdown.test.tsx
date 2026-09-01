import { describe, expect, it } from 'vitest';
import { render } from '@testing-library/react';
import { TierBreakdown } from './TierBreakdown';
import type { BenchmarkRow } from './baselines';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';

/**
 * Tiers are not nodes. The per-tier bars used to take four hues from the node ramp with a legend
 * of coloured squares; now every bar is the accent and the tier is its column + "A 88%" label.
 */
const rows: BenchmarkRow[] = [
  { label: 'Claude Opus 5', score: 0.9, tiers: { A: 1, B: 0.9, C: 0.85, D: 0.8 }, scorerVersion: 'sb-5.3' },
  { label: 'Your fleet · 3 nodes', score: 0.6, tiers: { A: 0.7, B: 0.5, C: 0.6, D: 0.6 }, mine: true, scorerVersion: 'sb-5.3' },
];

describe('TierBreakdown', () => {
  it('draws every tier bar in the accent, labels each by letter, and marks the user row with a chip — no node hue, no legend squares', async () => {
    const { container, getByText, getAllByText } = render(<TierBreakdown rows={rows} />);
    const fills = container.querySelectorAll('.bg-lz-accent');
    // 2 rows × 4 tiers of bar fill, plus the "yours" chip.
    expect(fills).toHaveLength(9);
    expect(getByText('A 70%')).toBeTruthy();
    expect(getByText('D 80%')).toBeTruthy();
    expect(getAllByText('yours')).toHaveLength(1);
    expect(container.innerHTML).not.toMatch(/color-node-|color-block|#[0-9a-f]{6}/i);
    assertStudioClean(container);
    expect(await missingUtilities(allClasses(container))).toEqual([]);
  }, 30_000);
});
