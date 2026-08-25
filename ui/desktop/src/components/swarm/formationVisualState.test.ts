import { describe, expect, it } from 'vitest';
import {
  FORMATION_FALLBACKS,
  FORMATION_PHASES,
  contrastRatio,
  formationPhaseState,
  nextRevealedText,
  phaseStepIndex,
  reducedMotionPreference,
} from './formationVisualState';

describe('formation phase truth', () => {
  it('maps engine phase names onto the fixed run pipeline', () => {
    expect(FORMATION_PHASES.map((phase) => phase.label)).toEqual([
      'Research',
      'Plan',
      'Build',
      'Integrate',
      'Repair',
      'Done',
    ]);
    expect(phaseStepIndex('researching')).toBe(0);
    expect(phaseStepIndex('planning')).toBe(1);
    expect(phaseStepIndex('legacy contract generation')).toBe(1);
    expect(phaseStepIndex('dispatching workers')).toBe(2);
    expect(phaseStepIndex('integrate and verify')).toBe(3);
    expect(phaseStepIndex('repair wave')).toBe(4);
    expect(phaseStepIndex('finished')).toBe(5);
  });

  it('marks only earlier phases complete and the engine phase active', () => {
    expect(formationPhaseState('build', 1)).toBe('complete');
    expect(formationPhaseState('build', 2)).toBe('active');
    expect(formationPhaseState('build', 3)).toBe('upcoming');
  });

  it('marks conditional stages skipped at Done unless the engine emitted evidence', () => {
    const noConditionalStages = {
      researchObserved: true,
      planObserved: true,
      integrationObserved: false,
      repairObserved: false,
    };
    expect(formationPhaseState('done', 3, noConditionalStages)).toBe('skipped');
    expect(formationPhaseState('done', 4, noConditionalStages)).toBe('skipped');
    expect(
      formationPhaseState('done', 3, {
        researchObserved: true,
        planObserved: true,
        integrationObserved: true,
        repairObserved: false,
      })
    ).toBe('complete');
  });

  it('does not backfill fictional Research or Plan completion for an immediate build', () => {
    const immediateBuild = {
      researchObserved: false,
      planObserved: false,
      integrationObserved: false,
      repairObserved: false,
    };
    expect(formationPhaseState('build', 0, immediateBuild)).toBe('skipped');
    expect(formationPhaseState('build', 1, immediateBuild)).toBe('skipped');
    expect(formationPhaseState('build', 2, immediateBuild)).toBe('active');
  });

  it('keeps the 12px node glyph ramp at normal-text contrast', () => {
    for (const background of FORMATION_FALLBACKS) {
      expect(contrastRatio('#0b0b0b', background)).toBeGreaterThanOrEqual(4.5);
    }
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
