import { describe, it, expect } from 'vitest';
import { confColorVsFloor } from './SwarmRunPanel';
import { SWARM_STATUS } from './formationVisualState';

// Companion to confVerdict.test.ts. The WORDS were fixed for the 73-vs-80 case and the COLOUR was not, so
// the pill stayed GREEN next to text reading "Below your bar of 80" — saying "good" in the one channel a
// user reads before any words, about a run that had stopped and asked. Colour is a verdict too.
//
// Asserted against the palette TOKENS, not hard-coded hexes: the point of the test is WHICH VERDICT the
// colour states, and a hex literal here just re-breaks the suite every time the theme is retuned.
const GREEN = SWARM_STATUS.done;
const AMBER = SWARM_STATUS.running;
const RED = SWARM_STATUS.error;

describe('confColorVsFloor', () => {
  it('is NOT green below the engine bar — the exact 73-vs-80 case', () => {
    expect(confColorVsFloor(73, 80)).not.toBe(GREEN);
    expect(confColorVsFloor(73, 80)).toBe(AMBER);
  });

  it('is green only at or above the bar', () => {
    expect(confColorVsFloor(80, 80)).toBe(GREEN);
    expect(confColorVsFloor(96, 80)).toBe(GREEN);
    expect(confColorVsFloor(79, 80)).not.toBe(GREEN);
  });

  it('a high number under a HIGH bar is still not green', () => {
    // The old band painted anything >=70 green regardless of the bar it was measured against.
    expect(confColorVsFloor(90, 95)).not.toBe(GREEN);
  });

  it('goes red only when it is not close to the bar', () => {
    expect(confColorVsFloor(61, 80)).toBe(AMBER); // asked, but near
    expect(confColorVsFloor(59, 80)).toBe(RED); // not close
  });

  it('falls back to the band ONLY with no floor (that run never asks, so there is no bar)', () => {
    expect(confColorVsFloor(73, null)).toBe(GREEN);
    expect(confColorVsFloor(50, null)).toBe(AMBER);
    expect(confColorVsFloor(20, null)).toBe(RED);
  });
});
