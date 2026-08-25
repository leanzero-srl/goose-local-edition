import { describe, expect, it } from 'vitest';
import {
  formationPhaseState,
  nextRevealedText,
  phaseStepIndex,
  reducedMotionPreference,
} from './formationVisualState';

describe('formation phase truth', () => {
  it('maps engine phase names onto the fixed run pipeline', () => {
    expect(phaseStepIndex('researching')).toBe(0);
    expect(phaseStepIndex('planning')).toBe(1);
    expect(phaseStepIndex('contract generation')).toBe(2);
    expect(phaseStepIndex('dispatching workers')).toBe(3);
    expect(phaseStepIndex('integrate and verify')).toBe(4);
    expect(phaseStepIndex('finished')).toBe(5);
  });

  it('marks only earlier phases complete and the engine phase active', () => {
    expect(formationPhaseState('build', 1)).toBe('complete');
    expect(formationPhaseState('build', 3)).toBe('active');
    expect(formationPhaseState('build', 4)).toBe('upcoming');
  });
});

describe('reduced motion visual state', () => {
  it('reveals live text immediately when reduced motion is requested', () => {
    expect(
      nextRevealedText({
        target: 'engine truth',
        current: 'eng',
        charsPerSec: 110,
        deltaSeconds: 0.016,
        reduceMotion: true,
      })
    ).toBe('engine truth');
    expect(reducedMotionPreference(() => ({ matches: true }))).toBe(true);
  });

  it('advances append-only text incrementally otherwise', () => {
    const next = nextRevealedText({
      target: 'engine truth',
      current: 'eng',
      charsPerSec: 100,
      deltaSeconds: 0.02,
      reduceMotion: false,
    });
    expect(next).toBe('engin');
    expect(reducedMotionPreference(() => ({ matches: false }))).toBe(false);
  });
});
