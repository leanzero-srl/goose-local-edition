import { describe, expect, it } from 'vitest';
import {
  FORMATION_FALLBACKS,
  FORMATION_INK_FALLBACKS,
  FORMATION_PHASES,
  contrastRatio,
  formationPhaseIndex,
  formationPhaseState,
  formationPhasesFor,
  nextRevealedText,
  reducedMotionPreference,
} from './formationVisualState';

describe('formation phase truth', () => {
  it('carries every phase any run ever emitted — the SUPERSET the per-run filter draws from', () => {
    expect(FORMATION_PHASES.map((phase) => phase.label)).toEqual([
      'Open',
      'Ask',
      'Research',
      'Synthesize',
      'Review',
      'Contracts',
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

  // FINDING 16 of the frontend truth review: with NO active step, history must stay lit off the
  // evidence map. Returning 'upcoming' for everything meant a held run (whose phase was nulled)
  // erased every completed checkmark the moment Pause landed — a run four phases in read as not run.
  it('keeps observed history complete when no step is active, and never mints a check for the furthest', () => {
    const evidence = { open: true, synthesize: true, build: true } as const;
    const steps = formationPhasesFor(evidence);
    const at = (key: string) => steps.findIndex((s) => s.key === key);
    expect(formationPhaseState(null, at('open'), evidence, steps)).toBe('complete');
    expect(formationPhaseState(null, at('synthesize'), evidence, steps)).toBe('complete');
    // ask sits below the furthest observed step and was never observed — skipped, exactly as it
    // would read behind an active step.
    expect(formationPhaseState(null, at('ask'), evidence, steps)).toBe('skipped');
    // build is the FURTHEST observed: entered, not finished. Evidence lands on phase ENTRY, so a
    // green check here would be unearned — it asserts neither work nor completion.
    expect(formationPhaseState(null, at('build'), evidence, steps)).toBe('upcoming');
    expect(formationPhaseState(null, at('integrate'), evidence, steps)).toBe('upcoming');
  });

  it('maps every engine phase onto its own step', () => {
    expect(formationPhaseIndex('open')).toBe(0);
    expect(formationPhaseIndex('ask')).toBe(1);
    expect(formationPhaseIndex('research')).toBe(2);
    expect(formationPhaseIndex('synthesize')).toBe(3);
    expect(formationPhaseIndex('review')).toBe(4);
    expect(formationPhaseIndex('contracts')).toBe(5);
    expect(formationPhaseIndex('build')).toBe(6);
    expect(formationPhaseIndex('integrate')).toBe(7);
    expect(formationPhaseIndex('repair')).toBe(8);
    expect(formationPhaseIndex('done')).toBe(9);
  });

  // RESEARCH and CONTRACTS are deleted from the engine (P1-5/P1-4). A new run must not be offered
  // either as a stage — it would sit forever "skipped", claiming a route that no longer exists — but
  // an ARCHIVED run whose events prove it ran them keeps its historical chips.
  it('offers no retired phase without evidence, and keeps it for an archived run that ran it', () => {
    const live = ['open', 'ask', 'synthesize', 'review', 'build', 'integrate', 'repair', 'done'];
    expect(formationPhasesFor(undefined).map((s) => s.key)).toEqual(live);
    expect(formationPhasesFor({ open: true, build: true }).map((s) => s.key)).toEqual(live);
    expect(
      formationPhasesFor({ open: true, research: true, contracts: true }).map((s) => s.key)
    ).toEqual(['open', 'ask', 'research', 'synthesize', 'review', 'contracts', 'build', 'integrate', 'repair', 'done']);
    // Index and state hold against the run's OWN list, where 'build' is no longer at 6.
    const steps = formationPhasesFor(undefined);
    expect(formationPhaseIndex('build', steps)).toBe(4);
    expect(formationPhaseState('build', 3, undefined, steps)).toBe('complete');
    expect(formationPhaseState('build', 4, undefined, steps)).toBe('active');
    expect(formationPhaseState('build', 5, undefined, steps)).toBe('upcoming');
  });

  it('marks only earlier phases complete and the engine phase active', () => {
    expect(formationPhaseState('build', 5)).toBe('complete');
    expect(formationPhaseState('build', 6)).toBe('active');
    expect(formationPhaseState('build', 7)).toBe('upcoming');
  });

  it('never back-fills a stage the engine did not emit', () => {
    const straightToBuild = { open: true, research: true, synthesize: true, review: false };
    expect(formationPhaseState('build', 4, straightToBuild)).toBe('skipped');
    expect(formationPhaseState('build', 3, straightToBuild)).toBe('complete');
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
    expect(formationPhaseState('done', 7, noSink)).toBe('skipped');
    expect(formationPhaseState('done', 8, noSink)).toBe('skipped');
    expect(formationPhaseState('done', 6, noSink)).toBe('complete');
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
