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
      'Split',
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
    // ask is CONDITIONAL (VA-138): never observed on this run, so it has no chip at all — nothing to
    // read as skipped. research is unconditional, sits below the furthest step unobserved: skipped.
    expect(at('ask')).toBe(-1);
    expect(formationPhaseState(null, at('research'), evidence, steps)).toBe('skipped');
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
    expect(formationPhaseIndex('split')).toBe(6);
    expect(formationPhaseIndex('build')).toBe(7);
    expect(formationPhaseIndex('integrate')).toBe(8);
    expect(formationPhaseIndex('repair')).toBe(9);
    expect(formationPhaseIndex('done')).toBe(10);
  });

  // CONTRACTS (P1-4) and REVIEW (2447d145c) are deleted from the engine. A new run must not be offered
  // either as a stage — it would sit forever "skipped", claiming a route that no longer exists — but an
  // ARCHIVED run whose events prove it ran keeps its historical chip. RESEARCH is LIVE (the v2 fan, one
  // lane per slice on every run), so it is always offered. ASK and SPLIT are CONDITIONAL (VA-138): the
  // engine walks them only on a run with an open decision / a fat task, so they too appear only on
  // evidence — the step list is derived from the events seen, never a fixed array.
  it('offers no retired or conditional phase without evidence, and keeps it for a run that walked it', () => {
    const live = ['open', 'research', 'synthesize', 'build', 'integrate', 'repair', 'done'];
    expect(formationPhasesFor(undefined).map((s) => s.key)).toEqual(live);
    expect(formationPhasesFor({ open: true, build: true }).map((s) => s.key)).toEqual(live);
    // Evidence of ONE retired/conditional phase restores only that one.
    expect(
      formationPhasesFor({ open: true, research: true, contracts: true }).map((s) => s.key)
    ).toEqual(['open', 'research', 'synthesize', 'contracts', 'build', 'integrate', 'repair', 'done']);
    expect(
      formationPhasesFor({ open: true, synthesize: true, review: true }).map((s) => s.key)
    ).toEqual(['open', 'research', 'synthesize', 'review', 'build', 'integrate', 'repair', 'done']);
    expect(formationPhasesFor({ open: true, ask: true }).map((s) => s.key)).toEqual([
      'open',
      'ask',
      'research',
      'synthesize',
      'build',
      'integrate',
      'repair',
      'done',
    ]);
    expect(
      formationPhasesFor({ open: true, synthesize: true, split: true }).map((s) => s.key)
    ).toEqual(['open', 'research', 'synthesize', 'split', 'build', 'integrate', 'repair', 'done']);
    // Index and state hold against the run's OWN list, where 'build' is no longer at 7.
    const steps = formationPhasesFor(undefined);
    expect(formationPhaseIndex('build', steps)).toBe(3);
    expect(formationPhaseState('build', 2, undefined, steps)).toBe('complete');
    expect(formationPhaseState('build', 3, undefined, steps)).toBe('active');
    expect(formationPhaseState('build', 4, undefined, steps)).toBe('upcoming');
  });

  // THE DEFECT: formationPhaseState reads any step behind the active one without evidence as 'skipped'.
  // That is right for a conditional live stage and wrong for a deleted one — after 2447d145c every new
  // run would have shown "Review — skipped" once Build lit. The fix is structural: a retired step is not
  // in the run's list at all unless its own events put it there, so there is no index to read skipped.
  it('a retired phase without evidence has no index to read as skipped', () => {
    const newRun = { open: true, ask: true, research: true, synthesize: true, build: true };
    const steps = formationPhasesFor(newRun);
    expect(steps.some((s) => s.key === 'review')).toBe(false);
    expect(formationPhaseIndex('review', steps)).toBe(-1);
    const states = steps.map((_, i) => formationPhaseState('build', i, newRun, steps));
    expect(states).not.toContain('skipped');
  });

  it('marks only earlier phases complete and the engine phase active', () => {
    expect(formationPhaseState('build', 6)).toBe('complete');
    expect(formationPhaseState('build', 7)).toBe('active');
    expect(formationPhaseState('build', 8)).toBe('upcoming');
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
    expect(formationPhaseState('done', 8, noSink)).toBe('skipped');
    expect(formationPhaseState('done', 9, noSink)).toBe('skipped');
    expect(formationPhaseState('done', 7, noSink)).toBe('complete');
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
