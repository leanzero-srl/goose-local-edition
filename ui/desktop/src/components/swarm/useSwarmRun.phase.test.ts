import { describe, expect, it } from 'vitest';
import { foldRunPhase } from './useSwarmRun';

describe('V21 phase folding', () => {
  it('folds pillar opening and research into Research', () => {
    expect(foldRunPhase([{ event: 'pillar_opening_started' }])).toBe('Researching');
    expect(
      foldRunPhase([
        { event: 'pillar_opening_started' },
        { event: 'pillar_research_started', pillars: 3 },
      ])
    ).toBe('Researching');
  });

  it('folds pillar synthesis into Plan and an immediate dispatch into Build', () => {
    const synthesis = [
      { event: 'pillar_research_completed', pillars: 3 },
      { event: 'pillar_synthesis_plan_started' },
      { event: 'pillar_synthesis_plan_completed', tasks: 4 },
    ];
    expect(foldRunPhase(synthesis)).toBe('Planning');
    expect(foldRunPhase([...synthesis, { event: 'task_dispatched', task_id: 'ui' }])).toBe(
      'Building'
    );
  });

  it('distinguishes the optional integration sink and repair wave', () => {
    expect(foldRunPhase([{ event: 'task_dispatched', task_id: 'integrate-verify' }])).toBe(
      'Integrating'
    );
    expect(
      foldRunPhase([
        { event: 'task_dispatched', task_id: 'integrate-verify' },
        { event: 'complete_fix_dispatched', task_id: 'complete-fix::twin0' },
      ])
    ).toBe('Repairing');
  });

  it('preserves legacy research and plan event truth without inventing Contracts', () => {
    expect(
      foldRunPhase([
        { event: 'scouts_planned', lenses: ['architecture'] },
        { event: 'research_completed', findings: 2 },
      ])
    ).toBe('Planning');
  });
});
