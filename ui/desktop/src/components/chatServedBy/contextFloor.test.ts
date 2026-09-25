import { describe, expect, it } from 'vitest';
import { nextMeasuredPrompt, shownContextTokens } from './contextFloor';

describe('the context counter’s measured floor (Q-60)', () => {
  it('a first turn dropped mid-answer counts the 49k-token prompt the Studio read — not 0', () => {
    const measured = nextMeasuredPrompt(null, 's1', 49293, 0);
    expect(shownContextTokens(measured, 's1', 0)).toBe(49293);
    // The read ending (the drop) does not take the floor away.
    expect(shownContextTokens(nextMeasuredPrompt(measured, 's1', null, 0), 's1', 0)).toBe(49293);
  });

  it('goose reporting usage again wins; another chat never inherits the floor', () => {
    const measured = nextMeasuredPrompt(null, 's1', 49293, 0);
    expect(shownContextTokens(measured, 's1', 51200)).toBe(51200);
    expect(shownContextTokens(measured, 's2', 0)).toBe(0);
  });

  it('keeps the largest prompt read for the same usage report (the turn, not a background call)', () => {
    const turn = nextMeasuredPrompt(null, 's1', 49293, 0);
    expect(nextMeasuredPrompt(turn, 's1', 140, 0)).toBe(turn);
  });
});
