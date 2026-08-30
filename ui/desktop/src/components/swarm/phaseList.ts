import type { PhaseKey, TurnLane } from './useSwarmRun';

/**
 * The phases that happen BEFORE a worker builds anything — the PLANNING zone's half of the pipeline; build,
 * integrate and repair are the work board. Kept as one list because the panel used to hand-type four keys
 * into a filter, and a phase added to the checklist (ASK, CONTRACTS) then rendered in neither zone.
 * `research` is LIVE again (the v2 fan: the opener's own questions answered read-only across the fleet
 * between ASK and SYNTHESIS); `contracts` is RETIRED (deleted by P1-4) but stays in this filter so an
 * ARCHIVED run's historical rows and lanes still land in the planning zone — the key costs a new run
 * nothing because its checklist never creates that phase's items.
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
 * in a trailing group that says nothing about when they ran. RESEARCH owns TWO fans across engine versions:
 * v2's research-* question lanes (the live engine — their own fold group since panel #5) and v1's slice fan
 * (archived runs only); a run produces one or the other, never both, so the live group wins when it exists.
 * CONTRACTS is the contract-* fan — which had no home in the zone at all and surfaced only as three busy
 * nodes parked under a ribbon lit on Build. This mapping is how an ARCHIVED run's fans keep their home.
 */
export function planningLanesFor(
  key: PhaseKey,
  lanes: { sliceLanes: TurnLane[]; contractLanes: TurnLane[]; researchLanes: TurnLane[] }
): PhaseLaneGroup | null {
  if (key === 'research') {
    return lanes.researchLanes.length > 0
      ? { label: 'Research answers', lanes: lanes.researchLanes }
      : { label: 'Slice specs', lanes: lanes.sliceLanes };
  }
  if (key === 'contracts') return { label: 'Contract stubs', lanes: lanes.contractLanes };
  return null;
}
