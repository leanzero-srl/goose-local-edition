import { describe, it, expect } from 'vitest';
import { render } from '@testing-library/react';
import { ScoreBars } from './ScoreBars';
import type { BenchmarkRow } from './baselines';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';

/**
 * THE "YOUR FLEET" BADGE MUST NOT OVERPRINT THE ROW LABEL.
 *
 * It was drawn unconditionally inside the bar, end-anchored at its right edge. At a low score the bar
 * is a few pixels wide, so the badge landed on top of the row's own name — Mihai's screenshot shows
 * "Your fleet · 3 nodes" and "YOUR FLEET" smeared over each other at 1.6%. Low scores are precisely the
 * ones this project spends its time looking at.
 */
const row = (score: number): BenchmarkRow => ({
  label: 'Your fleet · 3 nodes',
  score,
  tiers: {} as BenchmarkRow['tiers'],
  mine: true,
  scorerVersion: 'sb-7.0-rc',
});

const badges = (c: HTMLElement) =>
  [...c.querySelectorAll('text')].filter((t) => t.textContent === 'YOUR FLEET');

describe('ScoreBars', () => {
  it('drops the in-bar badge when the bar is too narrow to hold it', () => {
    const { container } = render(<ScoreBars rows={[row(0.016)]} />);
    expect(badges(container)).toHaveLength(0);
  });

  it('keeps the badge when the bar is wide enough', () => {
    const { container } = render(<ScoreBars rows={[row(0.68)]} />);
    expect(badges(container)).toHaveLength(1);
  });

  it('still shows the row label and the percentage at a low score', () => {
    const { container } = render(<ScoreBars rows={[row(0.016)]} />);
    const texts = [...container.querySelectorAll('text')].map((t) => t.textContent);
    expect(texts).toContain('Your fleet · 3 nodes');
    expect(texts).toContain('1.6%');
  });

  it('fills the user row in the accent and baselines in a neutral slate — never a node hue', async () => {
    const baseline: BenchmarkRow = {
      label: 'Claude Opus 5',
      score: 0.9,
      tiers: {} as BenchmarkRow['tiers'],
      scorerVersion: 'sb-7.0-rc',
    };
    const { container } = render(<ScoreBars rows={[baseline, row(0.68)]} />);
    const bars = [...container.querySelectorAll('rect')].filter((r) => r.getAttribute('rx') === '2');
    // Two tracks (surface-2) and two fills: the baseline's neutral, the user's accent.
    expect(bars.filter((r) => r.classList.contains('fill-lz-surface-2'))).toHaveLength(2);
    expect(bars.filter((r) => r.classList.contains('fill-lz-ink-3'))).toHaveLength(1);
    expect(bars.filter((r) => r.classList.contains('fill-lz-accent'))).toHaveLength(1);
    expect(container.innerHTML).not.toMatch(/color-node-|color-block|#[0-9a-f]{6}/i);
    assertStudioClean(container);
    expect(await missingUtilities(allClasses(container))).toEqual([]);
  }, 30_000);
});
