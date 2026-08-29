import { render, screen, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import FormationRibbon from './FormationRibbon';
import { FORMATION_PHASES } from './formationVisualState';
import { isPlanningPhase, planningLanesFor } from './phaseList';
import { buildPhaseTodo, foldEvents, foldRunPhase } from './useSwarmRun';
import fixture from './__fixtures__/r0PhaseSequence.json';

/**
 * THE BUG THIS PINS — the owner, 2026-08-29 22:35: "If contracts is a phase why is it not in the visuals
 * of the desktop app. So all phases should be clearly displayed in the desktop app. Contracts is somehow
 * in build.... shouldn't be."
 *
 * The fixture is the real run (benchmark/runs/build/swarm-3node-r0) cut to its phase / ask / contracts /
 * plan / dispatch lines, plus the eight contract-* digests it wrote. At 19:08:17 the engine announced
 * CONTRACTS; the ribbon had no such step, mapped it onto Build, and parked three busy nodes under a lit
 * Build chip for six minutes. ASK was folded into Open the same way.
 */
type Ev = Record<string, unknown>;
const EVENTS = fixture.events as Ev[];
const ACTIVITY = fixture.activity as Record<string, Record<string, unknown>>;
const CONTRACT_IDS = Object.keys(ACTIVITY).sort();

const upTo = (event: string, phase?: string): Ev[] => {
  const at = EVENTS.findIndex((e) => e.event === event && (phase == null || e.phase === phase));
  expect(at, `${event} ${phase ?? ''}`).toBeGreaterThanOrEqual(0);
  return EVENTS.slice(0, at + 1);
};
const stillWriting = Object.fromEntries(
  Object.entries(ACTIVITY).map(([k, d]) => [k, { ...d, phase: 'processing' }])
);

describe('every engine phase is a phase in the desktop app — the real r0 run', () => {
  it('walks the ribbon through OPEN, ASK, RESEARCH, SYNTHESIS, REVIEW, CONTRACTS, BUILD in engine order', () => {
    const visited: string[] = [];
    for (let i = 1; i <= EVENTS.length; i += 1) {
      const { phase } = foldRunPhase(EVENTS.slice(0, i));
      if (phase && visited[visited.length - 1] !== phase) visited.push(phase);
    }
    expect(visited).toEqual([
      'open',
      'ask',
      'research',
      'synthesize',
      'review',
      'contracts',
      'build',
    ]);
  });

  it('draws every phase as a chip, CONTRACTS done and BUILD current, with the fleet under BUILD', () => {
    const { phase, observed } = foldRunPhase(EVENTS);
    render(
      <FormationRibbon
        phase={phase}
        evidence={observed}
        nodes={[
          { device: 'gabee', working: true },
          { device: 'mihai', working: true },
          { device: 'workhorse', working: true },
        ]}
      />
    );
    const chips = within(screen.getByRole('list', { name: 'Run phases' })).getAllByRole('listitem');
    expect(chips.map((li) => `${li.textContent?.trim()}:${li.dataset.state}`)).toEqual([
      'Open:complete',
      'Ask:complete',
      'Research:complete',
      'Synthesize:complete',
      'Review:complete',
      'Contracts:complete',
      'Build:active',
      'Integrate:upcoming',
      'Repair:upcoming',
      'Done:upcoming',
    ]);
    expect(chips).toHaveLength(FORMATION_PHASES.length);
    const buildColumn = document.querySelector('[data-formation-phase="build"]')!;
    expect(within(buildColumn as HTMLElement).getAllByTestId('formation-node')).toHaveLength(3);
    expect(
      document.querySelector('[data-formation-phase="contracts"] [data-testid="formation-node"]')
    ).toBeNull();
  });

  it('gives ASK and CONTRACTS their own checklist phases, in engine order', () => {
    const todo = buildPhaseTodo(EVENTS, ACTIVITY, { clarifyPending: false });
    expect(todo.map((p) => p.key)).toEqual([
      'open',
      'ask',
      'research',
      'synthesis',
      'review',
      'contracts',
      'build',
      'integrate',
      'repair',
      'done',
    ]);
    const byKey = (key: string) => todo.find((p) => p.key === key)!;
    const ask = byKey('ask');
    expect(ask.state).toBe('done');
    // WHO answered is not asserted: this run armed the proxy in immediate mode and emitted no
    // clarify_proxy_answered, so the reducer attributes the answer to you. That is an engine-side gap in
    // the event stream, not the phase list, and pinning the wrong attribution here would bless it.
    expect(ask.items.map((i) => `${i.id}:${i.label}:${i.state}`)).toEqual([
      'a-ask:3 open decisions:done',
    ]);
    const contracts = byKey('contracts');
    expect(contracts.state).toBe('done');
    expect(contracts.items.map((i) => i.label)).toEqual(['Frozen interfaces — 8 modules']);
    // The frozen-interfaces row used to be filed under Synthesize, which is where the owner could not find it.
    expect(byKey('synthesis').items.map((i) => i.id)).not.toContain('s-contracts');
    expect(byKey('open').items.map((i) => i.id)).not.toContain('o-ask');
    expect(byKey('build').active).toBe(true);
    expect(contracts.active).toBe(false);
  });

  it('while the fleet is freezing interfaces, the ribbon and the checklist both say CONTRACTS is now', () => {
    const midContracts = upTo('phase', 'contracts');
    expect(foldRunPhase(midContracts).phase).toBe('contracts');

    const todo = buildPhaseTodo(midContracts, stillWriting, { clarifyPending: false });
    const contracts = todo.find((p) => p.key === 'contracts')!;
    expect(contracts.active).toBe(true);
    expect(contracts.items.map((i) => `${i.id}:${i.state}`)).toEqual(['c-run:running']);
    expect(todo.find((p) => p.key === 'build')!.items).toHaveLength(0);

    const folded = foldEvents(midContracts, stillWriting);
    expect(folded.contractLanes.map((l) => l.taskId)).toEqual(CONTRACT_IDS);
    expect(folded.contractLanes.every((l) => l.status === 'running')).toBe(true);
  });

  it('files the contract-* lanes under CONTRACTS, never under BUILD', () => {
    const folded = foldEvents(EVENTS, ACTIVITY);
    expect(folded.contractLanes.map((l) => l.taskId)).toEqual(CONTRACT_IDS);
    expect(folded.contractLanes.map((l) => l.description)).toContain('Contract · api-endpoints');
    // The BUILD phase event ended the freeze even for a digest that never stamped itself done.
    const neverStamped = Object.fromEntries(
      Object.entries(ACTIVITY).map(([k, d]) => [k, { ...d, phase: undefined }])
    );
    expect(foldEvents(EVENTS, neverStamped).contractLanes.every((l) => l.status === 'done')).toBe(
      true
    );

    const under = planningLanesFor('contracts', folded);
    expect(under?.label).toBe('Contract stubs');
    expect(under?.lanes).toBe(folded.contractLanes);
    expect(planningLanesFor('build', folded)).toBeNull();
    expect(isPlanningPhase('contracts')).toBe(true);
    expect(isPlanningPhase('build')).toBe(false);
    // The work board's lanes are the dispatched tasks only — no contract lane leaks in.
    expect(folded.lanes.map((l) => l.taskId).filter((id) => id.startsWith('contract-'))).toEqual(
      []
    );
  });
});
