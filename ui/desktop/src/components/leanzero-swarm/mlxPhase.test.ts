import { describe, expect, it } from 'vitest';
import { PHASE_FILL, PHASE_HEX, type EnginePhase } from '../lz/tokens';
import { activityPhase, hostingPhase, nodePhase, runPhase, singlePhase } from './mlxPhase';

/**
 * Every backend state → the ONE engine-phase palette (lz/tokens.ts). The owner's spec, 2026-09-24:
 * not loaded dark + outlined, idle grey, loading amber, reading blue, writing green, queued/held
 * orange, failed red — each distinct at a glance.
 */
describe('the engine-phase palette: every state has its colour', () => {
  it('the palette itself: seven hues, the spec colour families, unloaded outlined', () => {
    expect(PHASE_HEX).toEqual({
      unloaded: '#27272a',
      idle: '#71717a',
      loading: '#f59e0b',
      reading: '#2563eb',
      writing: '#15803d',
      held: '#c2410c',
      failed: '#dc2626',
    });
    expect(PHASE_FILL.unloaded).toContain('border-2');
    for (const phase of Object.keys(PHASE_FILL) as EnginePhase[]) {
      expect(PHASE_FILL[phase]).toContain(`bg-lz-phase-${phase}`);
      expect(PHASE_FILL[phase]).toContain(`text-lz-phase-${phase}-ink`);
    }
  });

  it.each<[string | null, boolean, Parameters<typeof singlePhase>[2], EnginePhase]>([
    ['stopped', false, null, 'unloaded'],
    [null, false, null, 'unloaded'],
    [null, true, null, 'failed'],
    ['mounting', false, null, 'loading'],
    ['failed', false, null, 'failed'],
    ['running', false, null, 'idle'],
    ['running', false, 'idle', 'idle'],
    ['running', false, 'not_loaded', 'unloaded'],
    ['running', false, 'prefill', 'reading'],
    ['running', false, 'generating', 'writing'],
    ['running', false, 'queued', 'held'],
  ])('single %s (unreachable %s, activity %s) → %s', (state, unreachable, activity, phase) => {
    expect(singlePhase(state, unreachable, activity)).toBe(phase);
  });

  it('activity → phase is the same mapping the tile and the tray use', () => {
    expect(activityPhase('generating')).toBe('writing');
    expect(activityPhase('prefill')).toBe('reading');
    expect(activityPhase('queued')).toBe('held');
    expect(activityPhase('idle')).toBe('idle');
    expect(activityPhase('not_loaded')).toBe('unloaded');
  });

  it.each<[string, boolean, EnginePhase]>([
    ['preflight', true, 'loading'],
    ['starting', true, 'loading'],
    ['stopping', true, 'loading'],
    ['ready', true, 'idle'],
    ['serving', true, 'writing'],
    ['ready', false, 'held'],
    ['serving', false, 'held'],
    ['failed', true, 'failed'],
    ['failed', false, 'failed'],
    ['stopped', true, 'unloaded'],
  ])('distributed run %s (admission open %s) → %s', (state, open, phase) => {
    expect(runPhase(state, open)).toBe(phase);
  });

  it.each<[string, EnginePhase]>([
    ['preflight', 'loading'],
    ['loading', 'loading'],
    ['ready', 'idle'],
    ['serving', 'writing'],
    ['failed', 'failed'],
    ['stopped', 'unloaded'],
  ])('a distributed rank %s → %s', (state, phase) => {
    expect(nodePhase(state)).toBe(phase);
  });

  it('the rank this Mac hosts: amber while it loads, grey once joined, red when failed', () => {
    expect(hostingPhase('loading')).toBe('loading');
    expect(hostingPhase('serving')).toBe('idle');
    expect(hostingPhase('failed')).toBe('failed');
  });

  it('a state a newer backend adds claims nothing coloured: idle grey', () => {
    expect(singlePhase('warming', false, null)).toBe('idle');
    expect(runPhase('draining', true)).toBe('idle');
    expect(nodePhase('compiling')).toBe('idle');
  });
});
