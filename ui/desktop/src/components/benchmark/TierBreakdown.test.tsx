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
  {
    label: 'Claude Opus 5',
    score: 0.9,
    tiers: { A: 1, B: 0.9, C: 0.85, D: 0.8 },
    scorerVersion: 'sb-5.3',
  },
  {
    label: 'Your fleet · 3 nodes',
    score: 0.6,
    tiers: { A: 0.7, B: 0.5, C: 0.6, D: 0.6 },
    mine: true,
    scorerVersion: 'sb-5.3',
  },
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

it('shows F only when the result actually recorded route planning', () => {
  const { getByText, rerender, queryByText } = render(
    <TierBreakdown rows={[{ ...rows[0], tiers: { A: 1, B: 1, C: 1, D: 1, E: 1, F: 0 } }]} />
  );
  getByText('F 0%');
  rerender(<TierBreakdown rows={[rows[0]]} />);
  expect(queryByText('F 0%')).toBeNull();
});

it('shows every recorded sb-7 family tier with its weight — SB7.2 S/Q/M as weighted tiers, SB7.1 as gates', async () => {
  const row: BenchmarkRow = {
    label: 'Your run',
    score: 0.7,
    tiers: { A: 1, X: 0.5, S: 0.8, Q: 0.6, M: 0 },
    mine: true,
    scorerVersion: 'sb-7.2',
  };
  const { container, getByTestId, getByText, rerender } = render(
    <TierBreakdown
      rows={[row]}
      tiers={[
        { tier: 'A', name: 'Structure', weight: 0.04 },
        { tier: 'X', name: 'Concurrency', weight: 0.16 },
        { tier: 'S', name: '3D structure', weight: 0.15 },
        { tier: 'Q', name: 'Presentation', weight: 0.1 },
        { tier: 'M', name: 'Animation', weight: 0.05 },
        { tier: 'R', name: 'Recovery', weight: 0.16 },
      ]}
    />
  );
  expect(getByTestId('tier-cell-S')).toHaveTextContent('3D structureS 80%weight 15%');
  expect(getByTestId('tier-cell-M')).toHaveTextContent('AnimationM 0%weight 5%');
  getByText('X 50%');
  // A tier the result did not record gets no cell.
  expect(container.querySelector('[data-testid="tier-cell-R"]')).toBeNull();
  assertStudioClean(container);
  expect(await missingUtilities(allClasses(container))).toEqual([]);

  rerender(
    <TierBreakdown
      rows={[{ ...row, scorerVersion: 'sb-7.1' }]}
      tiers={[{ tier: 'S', name: '3D structure', admissionOnly: true }]}
    />
  );
  expect(getByTestId('tier-cell-S')).toHaveTextContent('3D structureS 80%admission gate');

  // Forge 2.0's Forge 1.0 tiers weigh 1.76%–3.52% each: printed exactly, they add to 100% with R1–R9;
  // rounded to whole percents they read 2, 2, 4… and add to 102%. A whole-percent weight prints as before.
  rerender(
    <TierBreakdown
      rows={[{ ...row, tiers: { L: 1, K: 1, E: 0.5, R2: 1 }, scorerVersion: 'forge-2.0' }]}
      tiers={[
        { tier: 'L', name: 'Lint and bundles', weight: 0.0176 },
        { tier: 'K', name: 'Platform currency', weight: 0.022 },
        { tier: 'E', name: 'Excellence', weight: 0.03 },
        { tier: 'R2', name: 'Dosing', weight: 0.13 },
      ]}
    />
  );
  // …and on a Forge 2.0 result a group's mean is four decimals, the app's one format for a score there.
  expect(getByTestId('tier-cell-L')).toHaveTextContent('Lint and bundlesL 1.0000weight 1.76%');
  expect(getByTestId('tier-cell-E')).toHaveTextContent('E 0.5000');
  expect(getByTestId('tier-cell-K')).toHaveTextContent('weight 2.2%');
  expect(getByTestId('tier-cell-E')).toHaveTextContent('weight 3%');
  expect(getByTestId('tier-cell-R2')).toHaveTextContent('weight 13%');
}, 30_000);
