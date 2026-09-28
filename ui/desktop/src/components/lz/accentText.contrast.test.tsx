import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { CandidateFigures } from '../leanzero-swarm/PlacementCandidates';
import { PLAN_27B } from '../leanzero-swarm/placement.fixtures';
import { contrast, resolvedPaint, studioToken } from './resolvedPaint';

const THEMES = ['light', 'dark'] as const;

/**
 * Q-320, live critic on 3.0.68 (52-dark-engine.png): the blue "~11.5 tok/s writing" on the dark
 * Run it rows was the accent FILL (#1d4ed8, kept dark enough to carry white ink) used as text on
 * the row's surface-2 — 2.18:1. Accent text now paints its own token; the fill is unchanged.
 */
describe('accent as text clears 4.5:1 where it sits, in both themes (Q-320)', () => {
  it('the Run it figure on its row (surface-2)', async () => {
    const candidate = (PLAN_27B.candidates ?? []).find((c) => c.id === 'single:local');
    expect(candidate).toBeDefined();
    render(
      <IntlTestWrapper>
        <CandidateFigures candidate={candidate!} goal="chat" />
      </IntlTestWrapper>
    );
    const figure = screen.getByText(/^~[\d.]+ tok\/s writing$/);
    for (const theme of THEMES) {
      const row = studioToken('--color-lz-surface-2', theme);
      const paint = await resolvedPaint(figure, theme, { inherit: { bg: row } });
      expect(paint.missing, theme).toEqual([]);
      expect(paint.text, theme).toBe(studioToken('--color-lz-accent-text', theme));
      expect(contrast(paint.text, row), `${theme} figure on its row`).toBeGreaterThanOrEqual(4.5);
    }
  }, 30_000);

  it('text-lz-accent on every page surface; the accent fill still carries white ink', async () => {
    render(
      <div>
        <span data-testid="text">link</span>
        <span data-testid="fill">selected</span>
      </div>
    );
    const text = screen.getByTestId('text');
    text.className = 'text-lz-accent';
    const fill = screen.getByTestId('fill');
    fill.className = 'bg-lz-accent text-lz-accent-ink';
    for (const theme of THEMES) {
      for (const ground of ['--color-lz-bg', '--color-lz-surface', '--color-lz-surface-2']) {
        const bg = studioToken(ground, theme);
        const paint = await resolvedPaint(text, theme, { inherit: { bg } });
        expect(contrast(paint.text, bg), `${theme} ${ground}`).toBeGreaterThanOrEqual(4.5);
      }
      const f = await resolvedPaint(fill, theme);
      expect(f.bg, theme).toBe('#1d4ed8');
      expect(contrast(f.bg, f.text), `${theme} fill ink`).toBeGreaterThan(4.5);
    }
  }, 30_000);
});
