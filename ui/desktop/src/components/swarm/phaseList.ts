import type { PhaseKey, TurnLane } from './useSwarmRun';

/**
 * The phases that happen BEFORE a worker builds anything — the PLANNING zone's half of the pipeline; build,
 * integrate and repair are the work board. Kept as one list because the panel used to hand-type four keys
 * into a filter, and a phase added to the checklist (ASK, CONTRACTS) then rendered in neither zone.
 */
export const PLANNING_PHASE_KEYS: ReadonlyArray<PhaseKey> = [
  'open',
  'ask',
  'research',
  'synthesis',
  'review',
  'contracts',
];

export function isPlanningPhase(key: PhaseKey): boolean {
  return PLANNING_PHASE_KEYS.includes(key);
}

export interface PhaseLaneGroup {
  label: string;
  lanes: TurnLane[];
}

/**
 * The fleet fan that belongs to a planning phase, so its lanes render UNDER that phase's checklist rather than
 * in a trailing group that says nothing about when they ran. RESEARCH is the slice fan; CONTRACTS is the
 * contract-* fan — which had no home in the zone at all and surfaced only as three busy nodes parked under
 * a ribbon lit on Build.
 */
export function planningLanesFor(
  key: PhaseKey,
  lanes: { sliceLanes: TurnLane[]; contractLanes: TurnLane[] }
): PhaseLaneGroup | null {
  if (key === 'research') return { label: 'Slice specs', lanes: lanes.sliceLanes };
  if (key === 'contracts') return { label: 'Contract stubs', lanes: lanes.contractLanes };
  return null;
}
