import { describe, expect, it } from 'vitest';
import {
  FORMATION_FALLBACKS,
  FORMATION_INK_FALLBACKS,
  FORMATION_PHASES,
  contrastRatio,
  formationPhaseIndex,
  formationPhaseState,
  nextRevealedText,
  reducedMotionPreference,
} from './formationVisualState';

describe('formation phase truth', () => {
  it('draws the pipeline the rewritten engine actually runs', () => {
    expect(FORMATION_PHASES.map((phase) => phase.label)).toEqual([
      'Open',
      'Research',
      'Synthesize',
      'Review',
      'Build',
      'Integrate',
      'Repair',
      'Done',
    ]);
  });

  // THE BUG THIS PINS: the previous ribbon regex-matched a human phase LABEL and returned Build for
  // anything it did not recognise, so a held run rendered "Build active" while every node sat idle.
  // There is no unknown case left — the index comes from the engine's own phase key, and "no phase" is
  // a first-class answer.
  it('has no default-to-Build: an absent phase lights nothing', () => {
    expect(formationPhaseIndex(null)).toBe(-1);
    for (let i = 0; i < FORMATION_PHASES.length; i += 1) {
      expect(formationPhaseState(null, i)).toBe('upcoming');
    }
  });

  it('maps every engine phase onto its own step', () => {
    expect(formationPhaseIndex('open')).toBe(0);
    expect(formationPhaseIndex('research')).toBe(1);
    expect(formationPhaseIndex('synthesize')).toBe(2);
    expect(formationPhaseIndex('review')).toBe(3);
    expect(formationPhaseIndex('build')).toBe(4);
    expect(formationPhaseIndex('integrate')).toBe(5);
    expect(formationPhaseIndex('repair')).toBe(6);
    expect(formationPhaseIndex('done')).toBe(7);
  });

  it('marks only earlier phases complete and the engine phase active', () => {
    expect(formationPhaseState('build', 3)).toBe('complete');
    expect(formationPhaseState('build', 4)).toBe('active');
    expect(formationPhaseState('build', 5)).toBe('upcoming');
  });

  it('never back-fills a stage the engine did not emit', () => {
    const straightToBuild = { open: true, research: true, synthesize: true, review: false };
    expect(formationPhaseState('build', 3, straightToBuild)).toBe('skipped');
    expect(formationPhaseState('build', 2, straightToBuild)).toBe('complete');
  });

  it('marks the conditional Integrate and Repair skipped at Done without evidence', () => {
    const noSink = {
      open: true,
      research: true,
      synthesize: true,
      review: true,
      build: true,
      integrate: false,
      repair: false,
    };
    expect(formationPhaseState('done', 5, noSink)).toBe('skipped');
    expect(formationPhaseState('done', 6, noSink)).toBe('skipped');
    expect(formationPhaseState('done', 4, noSink)).toBe('complete');
  });

  // Each ramp hue carries its OWN ink for exactly this reason: white clears AA on the blue (6.7:1) and
  // fails on the green (3.3:1). Pinning the PAIR keeps the fix from being quietly undone by dropping the
  // ink table, and keeps anyone from "fixing" the contrast by washing a hue out.
  it('keeps every node glyph at normal-text contrast on its own hue', () => {
    expect(FORMATION_INK_FALLBACKS).toHaveLength(FORMATION_FALLBACKS.length);
    FORMATION_FALLBACKS.forEach((background, i) => {
      expect(contrastRatio(FORMATION_INK_FALLBACKS[i], background)).toBeGreaterThanOrEqual(4.5);
    });
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
    expect(
      nextRevealedText({
        target: 'engine truth',
        current: 'eng',
        charsPerSec: 100,
        deltaSeconds: 0.02,
        reduceMotion: false,
      })
    ).toBe('engin');
    expect(reducedMotionPreference(() => ({ matches: false }))).toBe(false);
  });

  it('snaps instead of retyping when the tail window slides past what is shown', () => {
    expect(
      nextRevealedText({
        target: 'a different tail',
        current: 'engine tr',
        charsPerSec: 100,
        deltaSeconds: 0.02,
        reduceMotion: false,
      })
    ).toBe('a different tail');
  });
});
